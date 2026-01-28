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
        # componentize-py unwraps Result<T,E> - returns T on Ok, raises on Err
        hex_str = i2c.transfer(self.addr, bytes([reg]).hex(), length)
        return bytes.fromhex(hex_str) if hex_str else b''
    
    def _i2c_write(self, reg: int, value: int):
        """Write single byte to register"""
        data = bytes([reg, value]).hex()
        i2c.transfer(self.addr, data, 0)
    
    def _load_calibration(self):
        """Load temperature and humidity calibration from chip per Bosch BME680 datasheet"""
        try:
            # BME680 calibration registers:
            # par_t1 (uint16): 0xE9-0xEA 
            # par_t2 (int16):  0x8A-0x8B
            # par_t3 (int8):   0x8C
            
            # Read t1 from 0xE9
            t1_data = self._i2c_read(0xE9, 2)
            # Read t2 and t3 from 0x8A
            t23_data = self._i2c_read(0x8A, 3)
            
            if len(t1_data) >= 2 and len(t23_data) >= 3:
                # t1 is unsigned 16-bit
                self.cal['t1'] = t1_data[0] | (t1_data[1] << 8)
                # t2 is signed 16-bit
                self.cal['t2'] = self._signed16(t23_data[0] | (t23_data[1] << 8))
                # t3 is signed 8-bit
                t3_raw = t23_data[2]
                self.cal['t3'] = t3_raw if t3_raw < 128 else t3_raw - 256
                
                print(f"📊 [BME680] Temp cal: t1={self.cal['t1']} t2={self.cal['t2']} t3={self.cal['t3']}")
            else:
                print("⚠️ [BME680] Temp cal read incomplete, using defaults")
                self.cal = {'t1': 26000, 't2': 26500, 't3': 3}
            
            # Humidity calibration coefficients per Bosch datasheet
            # h1: 0xE2-0xE3 (12 bits, lower nibble of E2 + full E3)
            # h2: 0xE1-0xE2 (12 bits, full E1 + upper nibble of E2)
            # h3-h7: 0xE4-0xE8
            h_data1 = self._i2c_read(0xE1, 3)  # E1, E2, E3
            h_data2 = self._i2c_read(0xE4, 5)  # E4-E8
            
            if len(h_data1) >= 3 and len(h_data2) >= 5:
                # h2 uses full E1 + upper nibble of E2
                self.cal['h2'] = (h_data1[0] << 4) | (h_data1[1] >> 4)
                # h1 uses lower nibble of E2 + full E3
                self.cal['h1'] = (h_data1[2] << 4) | (h_data1[1] & 0x0F)
                # h3 is signed 8-bit
                self.cal['h3'] = h_data2[0] if h_data2[0] < 128 else h_data2[0] - 256
                # h4 is signed 8-bit
                self.cal['h4'] = h_data2[1] if h_data2[1] < 128 else h_data2[1] - 256
                # h5 is signed 8-bit
                self.cal['h5'] = h_data2[2] if h_data2[2] < 128 else h_data2[2] - 256
                # h6 is unsigned 8-bit
                self.cal['h6'] = h_data2[3]
                # h7 is signed 8-bit
                self.cal['h7'] = h_data2[4] if h_data2[4] < 128 else h_data2[4] - 256
                
                # Post-processing per Adafruit library - CRITICAL for humidity accuracy
                # The raw h1/h2 values need adjustment before use in humidity formula
                h1_raw = self.cal['h1']
                h2_raw = self.cal['h2']
                # h2 = h2_raw * 16 + (h1_raw % 16)
                # h1 = h1_raw / 16
                self.cal['h2'] = (h2_raw * 16) + (h1_raw % 16)
                self.cal['h1'] = h1_raw / 16.0
                
                print(f"📊 [BME680] Hum cal: h1={self.cal['h1']:.1f} h2={self.cal['h2']} h3={self.cal['h3']} h4={self.cal['h4']} h5={self.cal['h5']} h6={self.cal['h6']} h7={self.cal['h7']}")
            else:
                # Reasonable defaults
                self.cal.update({'h1': 800, 'h2': 800, 'h3': 0, 'h4': 45, 'h5': 20, 'h6': 120, 'h7': -100})
                print("⚠️ [BME680] Hum cal read incomplete, using defaults")
                
            # Store t_fine for humidity calc (will be updated during temp reading)
            self.t_fine = 0.0
            
        except Exception as e:
            print(f"⚠️ [BME680] Calibration error: {e}")
            self.cal = {'t1': 26000, 't2': 26500, 't3': 3, 'h1': 800, 'h2': 800, 'h3': 0, 'h4': 45, 'h5': 20, 'h6': 120, 'h7': -100}
            self.t_fine = 0.0
    
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
            
            # BME680 temperature calculation per Bosch datasheet
            var1 = ((raw_temp / 16384.0) - (self.cal['t1'] / 1024.0)) * self.cal['t2']
            var2 = ((raw_temp / 131072.0) - (self.cal['t1'] / 8192.0))
            var2 = var2 * var2 * self.cal['t3'] * 16.0
            t_fine = var1 + var2
            temp = t_fine / 5120.0
            
            # Store t_fine for humidity calc
            self.t_fine = t_fine
            
            # Parse humidity (0x25-0x26 -> indices 8-9)
            raw_hum = (data[8] << 8) | data[9]
            
            # BME680 humidity compensation - exact Adafruit formula
            # temp_scaled = ((t_fine * 5) + 128) / 256
            temp_scaled = ((t_fine * 5) + 128) / 256
            
            var1 = (raw_hum - (self.cal['h1'] * 16.0)) - (
                (temp_scaled * self.cal['h3']) / 200.0
            )
            var2 = (
                self.cal['h2']
                * (
                    ((temp_scaled * self.cal['h4']) / 100.0)
                    + (
                        ((temp_scaled * ((temp_scaled * self.cal['h5']) / 100.0)) / 64.0)
                        / 100.0
                    )
                    + 16384.0
                )
            ) / 1024.0
            var3 = var1 * var2
            var4 = self.cal['h6'] * 128.0
            var4 = (var4 + ((temp_scaled * self.cal['h7']) / 100.0)) / 16.0
            var5 = ((var3 / 16384.0) * (var3 / 16384.0)) / 1024.0
            var6 = (var4 * var5) / 2.0
            humidity = (((var3 + var6) / 1024.0) * 1000.0) / 4096.0
            humidity /= 1000.0  # get back to RH %
            
            # Clamp to valid range
            if humidity > 100.0:
                humidity = 100.0
            elif humidity < 0.0:
                humidity = 0.0
            
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


# Lazy init - can't call I2C at compile time
driver = None

# IAQ state
gas_baseline = 0.0
burn_in_count = 0
gas_history = []
bad_iaq_alarm = False


class Bme680Logic(Bme680Logic):
    def poll(self) -> list[Bme680Reading]:
        global driver, gas_baseline, burn_in_count, gas_history, bad_iaq_alarm
        
        # Lazy init driver on first poll
        if driver is None:
            driver = BME680Driver(0x77)
        
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
                    print(f"🟣 [BME680] Calibrating... ({burn_in_count}/12) gas={gas:.0f}")
                else:
                    # Update gas baseline (slow adaptation)
                    if gas > gas_baseline:
                        gas_baseline = gas  # New clean air reference
                    else:
                        # Slowly drift baseline toward current reading
                        gas_baseline = gas_baseline * 0.995 + gas * 0.005
                    
                    # Gas score: Higher resistance = cleaner air = lower score
                    # Scale: 0 (excellent) to 75 (terrible)
                    if gas_baseline > 0 and gas > 0:
                        # Ratio of current to baseline (1.0 = same as clean air)
                        gas_ratio = gas / gas_baseline
                        # Invert: clean air (ratio=1) -> score=0, polluted (ratio=0.5) -> score=37
                        if gas_ratio >= 1.0:
                            gas_score = 0  # Better than baseline = excellent
                        else:
                            gas_score = (1.0 - gas_ratio) * 75.0
                        gas_score = max(0, min(75, gas_score))
                    else:
                        gas_score = 25  # Unknown, assume moderate
                    
                    # Humidity score: 40% is ideal, deviation adds to score
                    # Scale: 0 (ideal) to 25 (very humid/dry)
                    hum_offset = abs(humidity - 40.0)
                    hum_score = min(25, (hum_offset / 60.0) * 25.0)
                    
                    # Final IAQ (0-500 scale, lower is better)
                    # gas_score max=75, hum_score max=25, total max=100
                    # Multiply by 3 for 0-300 range (more reasonable than *5)
                    iaq = int((gas_score + hum_score) * 3.0)
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
                    
                    print(f"🔵 [BME680] {temp:.1f}°C | {humidity:.0f}% | Gas: {gas:.0f}KΩ (base:{gas_baseline:.0f}) | IAQ: {iaq} ({status})")
                
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
