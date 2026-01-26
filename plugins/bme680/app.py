"""
==============================================================================
bme680_plugin.py - BME680 Environmental Sensor Plugin
==============================================================================

BME680 is an I2C sensor measuring:
- Temperature (°C)
- Humidity (%)
- Pressure (hPa)  
- Gas Resistance (KΩ) -> used for IAQ calculation

This plugin handles:
1. Reading raw sensor data via generic I2C HAL
2. Calibrating gas baseline (3 minute warmup)
3. Calculating Indoor Air Quality (IAQ) score
4. LED feedback based on air quality

Build:
    componentize-py -d ../../wit -w bme680-plugin componentize app -o bme680.wasm
"""

import os
from wit_world.exports import Bme680Logic
from wit_world.imports import i2c, gpio_provider, led_controller, buzzer_controller

# ==============================================================================
# BME680 Raw I2C Driver (No external library needed!)
# ==============================================================================
class BME680Driver:
    def __init__(self, addr=0x77):
        self.addr = addr
        self.cal = {}
        self._initialized = False
        self.passive = False
    
    def _read_reg(self, reg, length):
        result = i2c.transfer(self.addr, bytes([reg]).hex(), length)
        if result:
            return bytes.fromhex(result)
        return None

    def _write_reg(self, reg, value):
        data_hex = bytes([reg, value]).hex()
        i2c.transfer(self.addr, data_hex, 0)

    def init_sensor(self):
        if self._initialized: return True
        
        # Check passive mode here (not in __init__) because env var isn't
        # available at module load time in WASM - only at first poll
        passive_env = os.getenv("HARVESTER_PASSIVE")
        self.passive = passive_env == "1"
        
        try:
            # Check ID
            chip_id = self._read_reg(0xD0, 1)
            if not chip_id or chip_id[0] != 0x61:
                print(f"⚠️ BME680: Unexpected Chip ID {chip_id}")
                return False
            
            # Read Temperature Calibration (T1, T2, T3)
            t1_data = self._read_reg(0xE9, 2)
            self.cal['t1'] = (t1_data[1] << 8) | t1_data[0]
            
            t2_data = self._read_reg(0x8A, 2)
            t2 = (t2_data[1] << 8) | t2_data[0]
            self.cal['t2'] = t2 if t2 < 32768 else t2 - 65536
            
            t3_data = self._read_reg(0x8C, 1)
            t3 = t3_data[0]
            self.cal['t3'] = t3 if t3 < 128 else t3 - 256

            print(f"🟢 BME680: Initialized ({'PASSIVE' if self.passive else 'MASTER'}) | Cal: T1={self.cal['t1']} T2={self.cal['t2']} T3={self.cal['t3']}")
            self._initialized = True
            return True
        except Exception as e:
            print(f"❌ BME680 Driver Error: {e}")
            return False

    def get_readings(self):
        if not self._initialized: 
            if not self.init_sensor(): return None

        try:
            if not self.passive:
                # Gas heater configuration
                self._write_reg(0x5A, 0x73)  # Heater resistance target
                self._write_reg(0x64, 0x59)  # Heater duration (100ms)
                
                # Enable gas measurement + heater profile 0
                self._write_reg(0x71, 0x10)  # run_gas = 1, nb_conv = 0
                
                # Set oversampling: temp x2, pressure x16, humidity x1
                self._write_reg(0x72, 0x01)  # osrs_h = 1
                self._write_reg(0x74, 0x54)  # osrs_t = 2, osrs_p = 5
                
                # Trigger forced mode
                ctrl_meas = self._read_reg(0x74, 1)[0]
                self._write_reg(0x74, (ctrl_meas & 0xFC) | 0x01)
                
                # Wait for measurement
                import time
                time.sleep(0.2)
            
            # Read all data registers
            data = self._read_reg(0x1D, 15)
            if not data:
                return None
            
            # Check measurement status
            new_data = (data[0] & 0x80) != 0
            gas_valid = (data[14] & 0x20) != 0
            heater_stab = (data[14] & 0x10) != 0
            
            # Temperature (Registers 0x22-0x24)
            raw_temp = ((data[5] << 12) | (data[6] << 4) | (data[7] >> 4))
            
            # Bosch Compensation Formula
            var1 = (raw_temp / 16384.0) - (self.cal['t1'] / 1024.0)
            var1 = var1 * self.cal['t2']
            var2 = (raw_temp / 131072.0) - (self.cal['t1'] / 8192.0)
            var2 = (var2 * var2) * (self.cal['t3'] * 16.0)
            t_fine = var1 + var2
            temp = t_fine / 5120.0
            
            # Humidity
            raw_hum = (data[8] << 8) | data[9]
            hum = raw_hum / 1000.0
            
            # Pressure
            raw_pres = (data[2] << 12) | (data[3] << 4) | (data[4] >> 4)
            pres = raw_pres / 100.0
            
            if gas_valid and heater_stab:
                # Gas Resistance
                raw_gas = (data[13] << 2) | ((data[14] & 0xC0) >> 6)
                gas_range = data[14] & 0x0F
                
                gas_range_table = [
                    1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0,
                    256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0, 32768.0
                ]
                
                if raw_gas > 0:
                    gas_res_ohms = (1340.0 * 1000000.0) / (raw_gas * gas_range_table[gas_range])
                    gas = gas_res_ohms / 1000.0  # KOhms
                else:
                    gas = 0.0
                
                if gas < 0:
                    gas = 0.0
                elif gas > 1000:
                    gas = 1000.0
            else:
                gas = 0.0
            
            return (temp, hum, pres, gas)
        except Exception as e:
            print(f"⚠️ BME680 Read Error: {e}")
            return None

driver = BME680Driver(0x77)

# ==============================================================================
# IAQ State
# ==============================================================================
gas_baseline = 0.0
burn_in_count = 0
gas_history = []
bad_iaq_alarm_active = False

class Bme680Logic(Bme680Logic):
    def poll(self) -> list:
        global gas_baseline, burn_in_count, gas_history, bad_iaq_alarm_active
        readings = []
        
        try:
            result = driver.get_readings()
            
            if result:
                temp, humidity, pressure, gas = result
                timestamp = gpio_provider.get_timestamp_ms()
                
                burn_in_count += 1
                
                # Track gas history for smoothing
                gas_history.append(gas)
                if len(gas_history) > 5:
                    gas_history.pop(0)
                avg_gas = sum(gas_history) / len(gas_history)
                
                is_passive = driver.passive
                
                # CALIBRATION PHASE (first 3 minutes = 36 polls at 5s interval) - SKIP IN PASSIVE MODE
                if not is_passive and burn_in_count < 36:
                    if gas > gas_baseline:
                        gas_baseline = gas
                    
                    current_iaq = 0
                    iaq_accuracy = 0
                    print(f"🟣 [CALIBRATING] Warming up... ({burn_in_count}/36) | Gas: {gas:.1f} KΩ | Baseline: {gas_baseline:.1f} KΩ")
                    led_controller.set_led(2, 255, 0, 255)  # Purple
                elif is_passive:
                    # PASSIVE MODE - just monitor, don't control sensor timing
                    if gas > gas_baseline:
                        gas_baseline = gas
                    
                    if gas_baseline > 0:
                        gas_ratio = gas / gas_baseline
                        gas_score = (1.0 - gas_ratio) * 75.0
                        gas_score = max(0, min(75, gas_score))
                    else:
                        gas_score = 0
                    
                    hum_offset = abs(humidity - 40.0)
                    hum_score = (hum_offset / 40.0) * 25.0
                    hum_score = min(25, hum_score)
                    
                    current_iaq = (gas_score + hum_score) * 5.0
                    current_iaq = min(500, max(0, current_iaq))
                    iaq_accuracy = 1
                    
                    print(f"🔘 [PASSIVE] {temp:.1f}°C | {humidity:.0f}% | Gas: {gas:.0f} KΩ | IAQ: {int(current_iaq)} (Monitoring only)")
                else:
                    # ACTIVE PHASE - Calculate IAQ with absolute thresholds
                    if gas > gas_baseline:
                        gas_baseline = gas
                    else:
                        gas_baseline = gas_baseline * 0.9995 + gas * 0.0005
                    
                    # Absolute Gas Thresholds
                    if gas >= 100:
                        gas_score = 0
                    elif gas >= 50:
                        gas_score = (100 - gas) * 0.5
                    elif gas >= 20:
                        gas_score = 25 + (50 - gas) * (25 / 30)
                    elif gas >= 10:
                        gas_score = 50 + (20 - gas) * 1.5
                    else:
                        gas_score = 65 + min(10, (10 - gas) * 3.5)
                    
                    gas_score = max(0, min(75, gas_score))
                    
                    # Humidity Score
                    if 30 <= humidity <= 50:
                        hum_score = 0
                    elif humidity < 30:
                        hum_score = (30 - humidity) * 0.5
                    else:
                        hum_score = (humidity - 50) * 0.5
                    hum_score = min(25, hum_score)
                    
                    # Final IAQ
                    current_iaq = (gas_score + hum_score) * 5.0
                    current_iaq = min(500, max(0, current_iaq))
                    iaq_accuracy = 1
                    
                    # Status and LED Color
                    if current_iaq <= 50:
                        led_controller.set_led(2, 0, 255, 0)  # Green
                        status = "Excellent"
                    elif current_iaq <= 100:
                        led_controller.set_led(2, 100, 255, 0)  # Light Green
                        status = "Good"
                    elif current_iaq <= 150:
                        led_controller.set_led(2, 255, 200, 0)  # Yellow
                        status = "Moderate"
                    elif current_iaq <= 200:
                        led_controller.set_led(2, 255, 100, 0)  # Orange
                        status = "Poor"
                    elif current_iaq <= 300:
                        led_controller.set_led(2, 255, 50, 0)   # Deep Orange
                        status = "Unhealthy"
                    else:
                        led_controller.set_led(2, 255, 0, 0)  # Red
                        status = "Hazardous"
                        if not bad_iaq_alarm_active:
                            buzzer_controller.beep(2, 150, 150)
                            bad_iaq_alarm_active = True
                    
                    if current_iaq <= 250 and bad_iaq_alarm_active:
                        bad_iaq_alarm_active = False
                    
                    print(f"🌡️ [BME680] {temp:.1f}°C | {humidity:.0f}% | Gas: {gas:.0f} KΩ | IAQ: {int(current_iaq)} ({status})")
                
                from wit_world.exports.bme680_logic import Bme680Reading
                readings.append(Bme680Reading(
                    temperature=temp,
                    humidity=humidity,
                    pressure=pressure,
                    gas_resistance=gas,
                    iaq_score=int(current_iaq),
                    iaq_accuracy=iaq_accuracy,
                    timestamp_ms=timestamp
                ))
        except Exception as e:
            print(f"❌ BME680 poll error: {e}")
        
        return readings
