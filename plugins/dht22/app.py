"""
==============================================================================
app.py - DHT22 temperature/humidity sensor plugin
==============================================================================

purpose:
    this module implements the dht22-logic interface defined in plugin.wit.
    it reads from the DHT22 sensor and controls LED 1 for room temperature.

the ALERT LOGIC in this file is HOT-SWAPPABLE:
    - change temperature thresholds
    - change LED colors
    - change buzzer patterns
    - rebuild wasm, host auto-reloads without restart

relations:
    - implements: ../../wit/plugin.wit (dht22-logic)
    - imports: gpio-provider, led-controller, buzzer-controller (from rust host)
    - loaded by: ../../host/src/runtime.rs
    - called by: ../../host/src/main.rs (polling loop)

build command:
    componentize-py -d ../../wit -w dht22-plugin componentize app -o dht22.wasm

==============================================================================
"""

from wit_world.exports import Dht22Logic
from wit_world.exports.dht22_logic import Dht22Reading
from wit_world.imports import gpio_provider, led_controller, buzzer_controller


# ==============================================================================
# alert thresholds - EDIT THESE AND HOT-RELOAD!
# ==============================================================================
HIGH_ALARM = 30.0
LOW_ALARM = 15.0
DEADBAND = 2.0  # hysteresis band (prevents flickering)

# Humidity thresholds
HIGH_HUMIDITY_ALARM = 70.0  # Too humid
LOW_HUMIDITY_ALARM = 25.0   # Too dry
HUMIDITY_DEADBAND = 5.0

# State variables (persist between polls)
high_alarm_active = False
low_alarm_active = False
high_humidity_active = False
low_humidity_active = False


class Dht22Logic(Dht22Logic):
    """
    Implementation of the dht22-logic interface from plugin.wit.
    Controls LED 1 for room temperature status.
    """
    
    def poll(self) -> list[Dht22Reading]:
        """
        Poll the DHT22 sensor, update LED 1, and trigger alerts.
        """
        global high_alarm_active, low_alarm_active, high_humidity_active, low_humidity_active
        
        timestamp_ms = gpio_provider.get_timestamp_ms()
        readings = []
        
        try:
            result = gpio_provider.read_dht22(4)  # BCM GPIO4 = physical pin 7
            
            if isinstance(result, tuple):
                temperature, humidity = result
                
                # -------- High Temperature Alarm Logic (Hysteresis) --------
                if not high_alarm_active:
                    if temperature >= HIGH_ALARM:
                        high_alarm_active = True
                else:
                    if temperature <= (HIGH_ALARM - DEADBAND):
                        high_alarm_active = False

                # -------- Low Temperature Alarm Logic --------
                if not low_alarm_active:
                    if temperature <= LOW_ALARM:
                        low_alarm_active = True
                else:
                    if temperature >= (LOW_ALARM + DEADBAND):
                        low_alarm_active = False
                
                # -------- Control LED 1 (Room Temp) --------
                if high_alarm_active:
                    led_controller.set_led(1, 255, 0, 0)  # Red
                    buzzer_controller.beep(3, 100, 100)  # 3 beeps - much more noticeable!
                    print(f"🔴 [DHT22] DANGER: {temperature:.1f}°C > {HIGH_ALARM}°C")
                elif low_alarm_active:
                    led_controller.set_led(1, 0, 0, 255)  # Blue
                    print(f"🔵 [DHT22] COLD: {temperature:.1f}°C < {LOW_ALARM}°C")
                elif temperature > 25.0:
                    led_controller.set_led(1, 255, 120, 0)  # Orange (warm)
                    print(f"🟠 [DHT22] Warm: {temperature:.1f}°C")
                else:
                    led_controller.set_led(1, 0, 255, 0)  # Green (normal)
                    print(f"🟢 [DHT22] OK: {temperature:.1f}°C")
                
                # Push changes to hardware
                led_controller.sync_leds()
                
                # -------- Humidity Alarm Logic --------
                if not high_humidity_active:
                    if humidity >= HIGH_HUMIDITY_ALARM:
                        high_humidity_active = True
                        buzzer_controller.beep(2, 200, 100)  # 2 long beeps for humidity
                        print(f"💧 [DHT22] HIGH HUMIDITY: {humidity:.1f}% > {HIGH_HUMIDITY_ALARM}%")
                else:
                    if humidity <= (HIGH_HUMIDITY_ALARM - HUMIDITY_DEADBAND):
                        high_humidity_active = False
                
                if not low_humidity_active:
                    if humidity <= LOW_HUMIDITY_ALARM:
                        low_humidity_active = True
                        buzzer_controller.beep(1, 300, 0)  # 1 long beep for dry
                        print(f"🏜️ [DHT22] LOW HUMIDITY: {humidity:.1f}% < {LOW_HUMIDITY_ALARM}%")
                else:
                    if humidity >= (LOW_HUMIDITY_ALARM + HUMIDITY_DEADBAND):
                        low_humidity_active = False

                # Create reading record
                reading = Dht22Reading(
                    sensor_id="dht22-gpio-4",
                    temperature=temperature,
                    humidity=humidity,
                    timestamp_ms=timestamp_ms
                )
                readings.append(reading)
            else:
                print(f"⚠️ DHT22 read error: {result}")
                
        except Exception as e:
            print(f"Error reading DHT22: {e}")
            
        return readings
