"""
BME680 Plugin - Air Quality Sensor with IAQ Score
Uses PASSIVE mode via generic I2C - compile once, run anywhere
Controls LED 2 for air quality status
"""
from wit_world.exports import Bme680Logic
from wit_world.exports.bme680_logic import Bme680Reading
from wit_world.imports import gpio_provider, led_controller, buzzer_controller, i2c


class BME680Driver:
    """Pure Python BME680 driver using generic I2C"""
    
    def __init__(self, addr: int = 0x77):
        self.addr = addr
        self.cal = {}
        self._load_calibration()
        self._configure_passive()
    
    def _i2c_read(self, reg: int, length: int) -> bytes:
        """Read bytes from register"""
        result = i2c.transfer(self.addr, bytes([reg]).hex(), length)
        if isinstance(result, str):
            return bytes.fromhex(result)
        return bytes.fromhex(result[0]) if result[0] else b''
    
    def _i2c_write(self, reg: int, value: int):
        """Write single byte to register"""
        data = bytes([reg, value]).hex()
        i2c.transfer(self.addr, data, 0)
    
    def _load_calibration(self):
        """Load temperature calibration from chip"""
        try:
            # Read calibration bank (0xE1-0xF0)
            cal1 = self._i2c_read(0xE1, 16)
            cal2 = self._i2c_read(0x8A, 16)
            
            if len(cal1) >= 3 and len(cal2) >= 3:
                self.cal['t1'] = cal2[0] | (cal2[1] << 8)
                self.cal['t2'] = self._signed16(cal2[2] | (cal2[3] << 8))
                self.cal['t3'] = cal1[0]
            else:
                # Fallback defaults
                self.cal = {'t1': 26000, 't2': 26500, 't3': -100}
        except:
            self.cal = {'t1': 26000, 't2': 26500, 't3': -100}
    
    def _signed16(self, val):
        return val - 65536 if val > 32767 else val
    
    def _configure_passive(self):
        """Configure for PASSIVE mode - trigger measurement on demand"""
        try:
            # Soft reset
            self._i2c_write(0xE0, 0xB6)
            
            # Configure oversampling (humidity, temp, pressure)
            self._i2c_write(0x72, 0x01)  # Humidity 1x
            self._i2c_write(0x74, 0x54)  # Temp 2x, Pressure 4x
            
            # Gas heater config
            self._i2c_write(0x5A, 0x59)  # Heater temp 320C
            self._i2c_write(0x64, 0x59)  # Heater duration 100ms
            self._i2c_write(0x71, 0x10)  # Enable gas, heater step 0
            
            print("✓ BME680 configured in PASSIVE mode")
        except Exception as e:
            print(f"⚠️ BME680 config error: {e}")
    
    def trigger_measurement(self):
        """Trigger single FORCED measurement"""
        self._i2c_write(0x74, 0x55)  # Force mode
    
    def get_readings(self) -> tuple | None:
        """Read all sensor data after measurement completes"""
        try:
            # Trigger measurement
            self.trigger_measurement()
            
            # Wait and read status
            for _ in range(50):
                status = self._i2c_read(0x1D, 1)
                if status and (status[0] & 0x80):  # New data ready
                    break
            
            # Read all data registers (0x1D to 0x2F)
            data = self._i2c_read(0x1D, 17)
            if not data or len(data) < 15:
                return None
            
            # Parse temperature (registers 0x22-0x24 -> indices 5-7)
            raw_temp = ((data[5] << 12) | (data[6] << 4) | (data[7] >> 4))
            var1 = (raw_temp / 16384.0) - (self.cal['t1'] / 1024.0)
            var1 = var1 * self.cal['t2']
            var2 = (raw_temp / 131072.0) - (self.cal['t1'] / 8192.0)
            var2 = (var2 * var2) * (self.cal['t3'] * 16.0)
            t_fine = var1 + var2
            temp = t_fine / 5120.0
            
            # Parse humidity (0x25-0x26 -> indices 8-9)
            raw_hum = (data[8] << 8) | data[9]
            humidity = raw_hum / 1000.0
            
            # Parse pressure (0x1F-0x21 -> indices 2-4)
            raw_pres = (data[2] << 12) | (data[3] << 4) | (data[4] >> 4)
            pressure = raw_pres / 100.0
            
            # Parse gas (0x2A-0x2B -> indices 13-14)
            gas_status = data[14] & 0x30
            if gas_status:
                raw_gas = (data[13] << 2) | (data[14] >> 6)
                gas = float(raw_gas) * 2.5
            else:
                gas = 0.0
            
            return (temp, humidity, pressure, gas)
            
        except Exception as e:
            print(f"⚠️ BME680 read error: {e}")
            return None


# Initialize driver
driver = BME680Driver(0x77)

# IAQ state
gas_baseline = 0.0
burn_in_count = 0
gas_history = []
bad_iaq_alarm = False


class Bme680Logic(Bme680Logic):
    def poll(self) -> list[Bme680Reading]:
        global gas_baseline, burn_in_count, gas_history, bad_iaq_alarm
        
        readings = []
        
        try:
            result = driver.get_readings()
            
            if result:
                temp, humidity, pressure, gas = result
                timestamp = gpio_provider.get_timestamp_ms()
                
                burn_in_count += 1
                
                # Smooth gas readings
                gas_history.append(gas)
                if len(gas_history) > 5:
                    gas_history.pop(0)
                
                # Calibration phase (60 seconds)
                if burn_in_count < 12:
                    if gas > gas_baseline:
                        gas_baseline = gas
                    iaq = 0
                    iaq_accuracy = 0
                    led_controller.set_led(2, 128, 0, 255)  # Purple = calibrating
                    print(f"🟣 [BME680] Calibrating... ({burn_in_count}/12)")
                else:
                    # Calculate IAQ
                    if gas > gas_baseline:
                        gas_baseline = gas
                    else:
                        gas_baseline = gas_baseline * 0.999 + gas * 0.001
                    
                    # Gas score (0-75)
                    if gas_baseline > 0:
                        gas_ratio = gas / gas_baseline
                        gas_score = (1.0 - gas_ratio) * 75.0
                        gas_score = max(0, min(75, gas_score))
                    else:
                        gas_score = 0
                    
                    # Humidity score (0-25)
                    hum_offset = abs(humidity - 40.0)
                    hum_score = (hum_offset / 40.0) * 25.0
                    hum_score = min(25, hum_score)
                    
                    # Final IAQ (0-500)
                    iaq = int((gas_score + hum_score) * 5.0)
                    iaq = min(500, max(0, iaq))
                    iaq_accuracy = 1
                    
                    # LED 2 based on IAQ
                    if iaq <= 50:
                        led_controller.set_led(2, 0, 255, 0)  # Green
                        status = "Excellent"
                    elif iaq <= 100:
                        led_controller.set_led(2, 0, 200, 50)  # Green-ish
                        status = "Good"
                    elif iaq <= 150:
                        led_controller.set_led(2, 255, 150, 0)  # Yellow
                        status = "Moderate"
                    elif iaq <= 200:
                        led_controller.set_led(2, 255, 100, 0)  # Orange
                        status = "Poor"
                    else:
                        led_controller.set_led(2, 255, 0, 0)  # Red
                        status = "Bad"
                        if not bad_iaq_alarm:
                            buzzer_controller.beep(2, 150, 150)
                            bad_iaq_alarm = True
                    
                    if iaq <= 180 and bad_iaq_alarm:
                        bad_iaq_alarm = False
                    
                    print(f"🔵 [BME680] {temp:.1f}°C | Gas: {gas:.0f}KΩ | IAQ: {iaq} ({status})")
                
                led_controller.sync_leds()
                
                readings.append(Bme680Reading(
                    sensor_id="bme680-i2c",
                    temperature=temp,
                    humidity=humidity,
                    pressure=pressure,
                    gas_resistance=gas,
                    iaq_score=iaq,
                    iaq_accuracy=iaq_accuracy,
                    timestamp_ms=timestamp
                ))
            else:
                print("⚠️ BME680 read failed")
                led_controller.set_led(2, 255, 0, 255)
                led_controller.sync_leds()
                
        except Exception as e:
            print(f"❌ BME680 exception: {e}")
            
        return readings
