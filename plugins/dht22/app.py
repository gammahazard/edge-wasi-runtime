"""
==============================================================================
dht22_plugin.py - DHT22 Temperature/Humidity Sensor Plugin
==============================================================================

DHT22 is a GPIO sensor measuring:
- Temperature (°C) 
- Humidity (%)

This plugin handles:
1. Reading sensor data via gpio_provider.read_dht22(pin)
2. LED feedback based on temperature thresholds (LED 1)
3. Buzzer alarm for critical temperatures
4. Hysteresis to prevent flicker

Build:
    componentize-py -d ../../wit -w dht22-plugin componentize app -o dht22.wasm
"""

from wit_world.exports import Dht22Logic
from wit_world.exports.dht22_logic import Dht22Reading
from wit_world.imports import gpio_provider, led_controller, buzzer_controller

# Thresholds
COLD_ALARM = 15.0
LOW_TEMP = 20.0
HIGH_ALARM = 30.0
CRITICAL_ALARM = 35.0
DEADBAND = 2.0  # Hysteresis

# State
high_alarm_active = False
low_alarm_active = False

class Dht22Logic(Dht22Logic):
    def poll(self) -> list[Dht22Reading]:
        global high_alarm_active, low_alarm_active
        readings = []
        
        try:
            # Read from HAL using gpio_provider
            result = gpio_provider.read_dht22(4)  # BCM GPIO4
            
            if isinstance(result, tuple):
                temp, humidity = result
                timestamp = gpio_provider.get_timestamp_ms()
                
                # High alarm with hysteresis
                if not high_alarm_active:
                    if temp >= HIGH_ALARM:
                        high_alarm_active = True
                else:
                    if temp <= (HIGH_ALARM - DEADBAND):
                        high_alarm_active = False
                
                # Low alarm with hysteresis
                if not low_alarm_active:
                    if temp <= LOW_TEMP:
                        low_alarm_active = True
                else:
                    if temp >= (LOW_TEMP + DEADBAND):
                        low_alarm_active = False
                
                # LED Color Logic (LED 1 = Room Temp)
                if temp >= CRITICAL_ALARM:
                    led_controller.set_led(1, 255, 0, 0)      # Bright Red
                    buzzer_controller.beep(3, 200, 100)
                    status = "CRITICAL"
                elif high_alarm_active:
                    led_controller.set_led(1, 255, 80, 0)     # Orange
                    buzzer_controller.buzz(50)
                    status = "HOT"
                elif temp >= 25.0:
                    led_controller.set_led(1, 255, 120, 0)    # Warm Orange
                    status = "WARM"
                elif low_alarm_active:
                    led_controller.set_led(1, 0, 100, 255)    # Blue
                    status = "COLD"
                elif temp >= LOW_TEMP:
                    led_controller.set_led(1, 0, 255, 0)      # Green
                    status = "NORMAL"
                else:
                    led_controller.set_led(1, 0, 50, 255)     # Deep Blue
                    status = "FREEZING"
                
                print(f"🌡️ [DHT22] {temp:.1f}°C | {humidity:.0f}% | {status}")
                
                readings.append(Dht22Reading(
                    sensor_id="dht22-gpio4",
                    temperature=temp,
                    humidity=humidity,
                    timestamp_ms=timestamp
                ))
            else:
                print(f"⚠️ DHT22 read error: {result}")
        except Exception as e:
            print(f"❌ DHT22 poll error: {e}")
        
        return readings
