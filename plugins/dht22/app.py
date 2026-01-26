"""
DHT22 Plugin - Room Temperature/Humidity Sensor
Controls LED 1 for room temperature status
"""
from wit_world.exports import Dht22Logic
from wit_world.exports.dht22_logic import Dht22Reading
from wit_world.imports import gpio_provider, led_controller, buzzer_controller

# Thresholds
HIGH_TEMP = 30.0
LOW_TEMP = 15.0
DEADBAND = 2.0
HIGH_HUM = 70.0
LOW_HUM = 25.0

# State
high_temp_alarm = False
low_temp_alarm = False


class Dht22Logic(Dht22Logic):
    def poll(self) -> list[Dht22Reading]:
        global high_temp_alarm, low_temp_alarm
        
        readings = []
        timestamp = gpio_provider.get_timestamp_ms()
        
        try:
            # componentize-py unwraps Result<T,E> - returns T on Ok, raises on Err
            temp, hum = gpio_provider.read_dht22(4)  # GPIO4
            
            # Hysteresis logic
            if not high_temp_alarm and temp >= HIGH_TEMP:
                high_temp_alarm = True
            elif high_temp_alarm and temp <= (HIGH_TEMP - DEADBAND):
                high_temp_alarm = False
                
            if not low_temp_alarm and temp <= LOW_TEMP:
                low_temp_alarm = True
            elif low_temp_alarm and temp >= (LOW_TEMP + DEADBAND):
                low_temp_alarm = False
            
            # LED 1 control
            if high_temp_alarm:
                led_controller.set_led(1, 255, 0, 0)  # Red
                buzzer_controller.beep(3, 100, 100)
                print(f"🔴 [DHT22] HOT: {temp:.1f}°C")
            elif low_temp_alarm:
                led_controller.set_led(1, 0, 0, 255)  # Blue
                print(f"🔵 [DHT22] COLD: {temp:.1f}°C")
            elif temp > 25.0:
                led_controller.set_led(1, 255, 120, 0)  # Orange
                print(f"🟠 [DHT22] Warm: {temp:.1f}°C")
            else:
                led_controller.set_led(1, 0, 255, 0)  # Green
                print(f"🟢 [DHT22] OK: {temp:.1f}°C")
            
            led_controller.sync_leds()
            
            readings.append(Dht22Reading(
                sensor_id="dht22-gpio4",
                temperature=temp,
                humidity=hum,
                timestamp_ms=timestamp
            ))
                
        except Exception as e:
            print(f"❌ DHT22 exception: {e}")
            
        return readings
