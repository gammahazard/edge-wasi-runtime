"""
==============================================================================
bme680_plugin.py - BME680 environmental sensor logic
==============================================================================

purpose:
    this module implements the bme680-logic interface defined in plugin.wit.
    it polls the BME680 sensor via the host capability.
    calculates Indoor Air Quality (IAQ) score using gas resistance.
    manages LED 2 for air quality status.

design:
    - Stateful: Keeps track of "Gas Baseline" (cleanest air seen) to auto-calibrate.
    - Adaptive: Drifts baseline slowly to account for sensor aging/ambient shifts.
    - Hysteresis: Prevents buzzer from flapping on/off at thresholds.

relationships:
    - implements: ../../../wit/plugin.wit (bme680-logic interface)
    - imports: gpio-provider, led-controller, buzzer-controller
    - loaded by: host/src/runtime.rs
    - called by: host/src/main.rs (polling loop)
    
build command:
    componentize-py -d ../../wit -w bme680-plugin componentize app -o bme680.wasm
"""

# ==============================================================================
# wit-generated imports
# ==============================================================================
# ==============================================================================
# wit-generated imports
# ==============================================================================
import os
import wit_world
from wit_world.imports import gpio_provider, led_controller, buzzer_controller, i2c
from wit_world.exports import Bme680Logic
from wit_world.exports.bme680_logic import Bme680Reading

# ==============================================================================
# BME680 Driver Logic (Moved from Host to Guest)
# ==============================================================================
class BME680Driver:
    """
    Direct I2C driver for the BME680 sensor.
    Handles registers, calibration co-efficients, and compensation math.
    """
    def __init__(self, addr=0x77):
        self.addr = addr
        self.cal = {}
        self._initialized = False
        self.passive = os.getenv("HARVESTER_PASSIVE") == "1"

    def _read_reg(self, reg, length):
        # Convert register to hex string
        reg_hex = bytes([reg]).hex()
        # Call host generic I2C
        res_hex = i2c.transfer(self.addr, reg_hex, length)
        if isinstance(res_hex, str):
            return bytes.fromhex(res_hex)
        return None

    def _write_reg(self, reg, value):
        data_hex = bytes([reg, value]).hex()
        i2c.transfer(self.addr, data_hex, 0)

    def init_sensor(self):
        if self._initialized: return True
        try:
            # Check ID
            chip_id = self._read_reg(0xD0, 1)
            if not chip_id or chip_id[0] != 0x61:
                print(f"⚠️ BME680: Unexpected Chip ID {chip_id}")
                return False
            
            # Read Temperature Calibration (T1, T2, T3)
            # T1: 0xE9 (LSB), 0xEA (MSB) -> uint16
            t1_data = self._read_reg(0xE9, 2)
            self.cal['t1'] = (t1_data[1] << 8) | t1_data[0]
            
            # T2: 0x8A (LSB), 0x8B (MSB) -> int16
            t2_data = self._read_reg(0x8A, 2)
            t2 = (t2_data[1] << 8) | t2_data[0]
            self.cal['t2'] = t2 if t2 < 32768 else t2 - 65536
            
            # T3: 0x8C -> int8
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
        """
        Performs a forced mode measurement and returns raw data.
        BME680 requires the gas heater to be active for valid gas readings.
        """
        if not self._initialized: 
            if not self.init_sensor(): return None

        try:
            if not self.passive:
                # 1. Ensure Gas Heater is Configured (300°C target)
                self._write_reg(0x5A, 0x79) 
                self._write_reg(0x64, 0x14) # 40ms wait
                
                # 2. Trigger measurement (Forced Mode)
                self._write_reg(0x74, 0b01001001) 
            
            # 3. Read Status and Data (0x1D to 0x2C)
            # We read 16 bytes to cover status, temp, press, hum, and gas
            data = self._read_reg(0x1D, 16)
            if not data: return None
            
            # 4. Filter Gas validity
            # Note: status is index 0. Gas msb is index 13 (0x2A), lsb is index 14 (0x2B)
            gas_status = data[14]
            gas_valid = (gas_status & 0x20) != 0
            
            # --- PRECISION TEMPERATURE MATH ---
            # Temperature (Registers 0x22-0x24) -> Index 5,6,7
            raw_temp = ((data[5] << 12) | (data[6] << 4) | (data[7] >> 4))
            
            # Bosch Compensation Formula (Float version)
            var1 = (raw_temp / 16384.0) - (self.cal['t1'] / 1024.0)
            var1 = var1 * self.cal['t2']
            var2 = (raw_temp / 131072.0) - (self.cal['t1'] / 8192.0)
            var2 = (var2 * var2) * (self.cal['t3'] * 16.0)
            t_fine = var1 + var2
            temp = t_fine / 5120.0
            
            # Humidity (Registers 0x25-0x26) -> Index 8,9
            raw_hum = (data[8] << 8) | data[9]
            hum = raw_hum / 1000.0
            
            # Pressure (Registers 0x1F-0x21) -> Index 2,3,4
            raw_pres = (data[2] << 12) | (data[3] << 4) | (data[4] >> 4)
            pres = raw_pres / 100.0
            
            if gas_valid:
                # Gas Resistance (Registers 0x2A-0x2B) -> Index 13, 14
                raw_gas = (data[13] << 2) | (data[14] >> 6)
                gas = float(raw_gas) * 2.5 
            else:
                # Still warming up or bus clash
                gas = 0.0
            
            return (temp, hum, pres, gas)
        except Exception as e:
            print(f"⚠️ BME680 Read Error: {e}")
            return None

driver = BME680Driver(0x77)

# ==============================================================================
# IAQ State (Persisted via Stateful Runtime)
# ==============================================================================
gas_baseline = 0.0      # Tracks cleanest air seen (in KOhms)
burn_in_count = 0       # Calibration counter (12 polls = 60 seconds)
gas_history = []        # Rolling history for smoothing
bad_iaq_alarm_active = False  # Hysteresis for IAQ buzzer

class Bme680Logic(Bme680Logic):
    def poll(self) -> list[Bme680Reading]:
        """
        Read BME680 sensor and calculate Indoor Air Quality (IAQ) score.
        
        IAQ Scale (Bosch BSEC-inspired):
          0-50   = Excellent (Green)
          51-100 = Good (Green)
          101-150 = Moderate (Yellow/Orange)  
          151-200 = Poor (Orange)
          201-300 = Bad (Red)
          301-500 = Hazardous (Red)
        
        Gas Resistance Meaning:
          HIGH (100+ KOhm) = Clean air
          LOW (1-50 KOhm) = Polluted air (VOCs present)
        """
        global gas_baseline, burn_in_count, gas_history, bad_iaq_alarm_active
        readings = []
        
        try:
            # result = gpio_provider.read_bme680(0x77) # DEPRECATED specialized call
            result = driver.get_readings() # NEW: Generic I2C driver logic
            
            if result:
                temp, humidity, pressure, gas = result
                timestamp = gpio_provider.get_timestamp_ms()
                
                burn_in_count += 1
                
                # Track gas history for smoothing (last 5 readings)
                gas_history.append(gas)
                if len(gas_history) > 5:
                    gas_history.pop(0)
                avg_gas = sum(gas_history) / len(gas_history)
                
                # --- IMPROVED IAQ ALGORITHM ---
                
                # CALIBRATION PHASE (first 60 seconds)
                if burn_in_count < 12:
                    # During warm-up, track the baseline but don't calculate IAQ
                    if gas > gas_baseline:
                        gas_baseline = gas
                    
                    current_iaq = 0
                    iaq_accuracy = 0
                    print(f"🟣 [CALIBRATING] Warming up... ({burn_in_count}/12) | Gas: {gas:.1f} KΩ | Baseline: {gas_baseline:.1f} KΩ")
                    led_controller.set_led(2, 255, 0, 255)  # Purple
                else:
                    # ACTIVE PHASE - Calculate IAQ
                    
                    # 1. Slowly drift baseline toward current reading (adaptive)
                    #    This handles environmental changes over time.
                    if gas > gas_baseline:
                        gas_baseline = gas  # New clean air peak
                    else:
                        # Decay baseline slowly (0.1% per reading) to adapt
                        gas_baseline = gas_baseline * 0.999 + gas * 0.001
                    
                    # 2. Gas Score (0-75 points)
                    #    Lower resistance = more pollution = higher score
                    if gas_baseline > 0:
                        # Ratio of current to baseline (1.0 = baseline, <1.0 = degraded)
                        gas_ratio = gas / gas_baseline
                        # Invert: 1.0 baseline = 0 score, 0.5 baseline = 37.5 score
                        gas_score = (1.0 - gas_ratio) * 75.0
                        gas_score = max(0, min(75, gas_score))  # Clamp to 0-75
                    else:
                        gas_score = 0
                    
                    # 3. Humidity Score (0-25 points)
                    #    Ideal humidity is 40%. Deviation adds to score.
                    hum_offset = abs(humidity - 40.0)
                    hum_score = (hum_offset / 40.0) * 25.0
                    hum_score = min(25, hum_score)  # Clamp to 0-25
                    
                    # 4. Final IAQ (0-500)
                    current_iaq = (gas_score + hum_score) * 5.0
                    current_iaq = min(500, max(0, current_iaq))
                    iaq_accuracy = 1
                    
                    # LED Color based on IAQ
                    if current_iaq <= 50:
                        led_controller.set_led(2, 0, 255, 0)  # Green
                        status = "Excellent"
                    elif current_iaq <= 100:
                        led_controller.set_led(2, 0, 200, 50)  # Green-ish
                        status = "Good"
                    elif current_iaq <= 150:
                        led_controller.set_led(2, 255, 150, 0)  # Yellow
                        status = "Moderate"
                    elif current_iaq <= 200:
                        led_controller.set_led(2, 255, 100, 0)  # Orange
                        status = "Poor"
                    else:
                        led_controller.set_led(2, 255, 0, 0)  # Red
                        status = "Bad"
                        # Trigger buzzer for bad air quality (with hysteresis)
                        if not bad_iaq_alarm_active:
                            buzzer_controller.beep(2, 150, 150)  # 2 beeps
                            bad_iaq_alarm_active = True
                    
                    # Reset alarm when IAQ improves
                    if current_iaq <= 180 and bad_iaq_alarm_active:
                        bad_iaq_alarm_active = False
                    
                    # Debug output
                    print(f"🔵 [BME680] {temp:.1f}°C | {humidity:.0f}% | Gas: {gas:.0f} KΩ (Base: {gas_baseline:.0f}) | IAQ: {int(current_iaq)} ({status})")
                
                # Push changes to hardware
                led_controller.sync_leds()
                
                reading = Bme680Reading(
                    sensor_id="bme680-i2c-0x77",
                    temperature=temp,
                    humidity=humidity,
                    pressure=pressure,
                    gas_resistance=gas,
                    iaq_score=int(current_iaq),
                    iaq_accuracy=iaq_accuracy,
                    timestamp_ms=timestamp
                )
                readings.append(reading)
                    
            else:
                led_controller.set_led(2, 255, 0, 255)
                print(f"⚠️ BME680 read error: {result}")
                
        except Exception as e:
            led_controller.set_led(2, 255, 0, 255)
            print(f"❌ BME680 Exception: {e}")
            
        return readings

