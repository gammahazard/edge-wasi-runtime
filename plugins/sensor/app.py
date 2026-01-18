"""
==============================================================================
app.py - python sensor plugin for wasi host (with led/buzzer alerts)
==============================================================================

purpose:
    this module implements the sensor-logic interface defined in plugin.wit.
    it demonstrates the WASI CAPABILITY MODEL where:
    - we IMPORT gpio_provider from the rust host for sensor readings
    - we IMPORT led_controller for visual status indicators
    - we IMPORT buzzer_controller for audio alerts
    - we return structured readings to the host

this is NOT simulated data - it reads from the actual DHT22 sensor
connected to GPIO pin 4 on the raspberry pi.

the ALERT LOGIC in this file is HOT-SWAPPABLE:
    - change temperature thresholds
    - change LED colors
    - change buzzer patterns
    - rebuild wasm, host auto-reloads without restart

relations:
    - implements: ../../wit/plugin.wit (sensor-logic)
    - imports: gpio-provider, led_controller, buzzer-controller (from rust host)
    - loaded by: ../../host/src/runtime.rs
    - called by: ../../host/src/main.rs (polling loop)

build command:
    componentize-py -d ../../wit -w sensor-plugin componentize app -o sensor.wasm

==============================================================================
"""

# ==============================================================================
# wit-generated imports
# ==============================================================================
# componentize-py generates wit_world from plugin.wit
# - exports: SensorLogic base class we implement
# - exports.sensor_logic: SensorReading record type
# - imports: gpio_provider, led_controller, buzzer_controller from host

from wit_world.exports import SensorLogic
from wit_world.exports.sensor_logic import SensorReading
from wit_world.imports import gpio_provider, led_controller, buzzer_controller


# ==============================================================================
# alert thresholds - EDIT THESE AND HOT-RELOAD!
# ==============================================================================
# Hysteresis Configuration
HIGH_ALARM = 30.0
LOW_ALARM = 15.0
DEADBAND = 2.0 # hysteresis band (prevents flickering)

# State variables (persist between polls now!)
# This works because we upgraded the Host to persist the Python interpreter instance.
high_alarm_active = False
low_alarm_active = False

class SensorLogic(SensorLogic):
    """
    implementation of the sensor-logic interface from plugin.wit.
    """
    
    def poll(self) -> list[SensorReading]:
        """
        poll the dht22 sensor, update leds, and trigger alerts.
        """
        global high_alarm_active, low_alarm_active
        
        # get current timestamp via host capability
        timestamp_ms = gpio_provider.get_timestamp_ms()
        
        readings = []
        
        # read dht22 sensor via host capability
        try:
            result = gpio_provider.read_dht22(4)  # BCM GPIO4 = physical pin 7
            
            if isinstance(result, tuple):
                temperature, humidity = result
                
                # -------- High Temperature Alarm Logic (Hysteresis) --------
                if not high_alarm_active:
                    if temperature >= HIGH_ALARM:
                        high_alarm_active = True
                else:
                    # Stays active until drops safely below allowed limit
                    if temperature <= (HIGH_ALARM - DEADBAND):
                        high_alarm_active = False

                # -------- Low Temperature Alarm Logic --------
                if not low_alarm_active:
                    if temperature <= LOW_ALARM:
                        low_alarm_active = True
                else:
                    if temperature >= (LOW_ALARM + DEADBAND):
                        low_alarm_active = False
                
                # -------- Control LEDs based on Sticky State --------
                cpu_temp = gpio_provider.get_cpu_temp()
                
                # CPU Temp Color (LED 0)
                if cpu_temp > 70.0:
                    cpu_color = (255, 0, 0)
                elif cpu_temp > 50.0:
                    cpu_color = (255, 255, 0)
                else:
                    cpu_color = (0, 255, 0)

                # Room Temp Color (LED 1)
                room_color = (0, 255, 0) # default green
                
                if high_alarm_active:
                    # DANGER: Red
                    room_color = (255, 0, 0)
                    buzzer_controller.buzz(50) # Short tick
                    print(f"🔴 [DANGER] Temp {temperature:.1f}C > Limit {HIGH_ALARM}C")
                elif low_alarm_active:
                    # COLD: Blue
                    room_color = (0, 0, 255)
                    print(f"🔵 [COLD] Temp {temperature:.1f}C < Limit {LOW_ALARM}C")
                elif temperature > 25.0:
                    # Warm: Orange
                    room_color = (255, 120, 0)
                    print(f"🟠 [WARM] Temp {temperature:.1f}C")
                else:
                    # Normal: Green
                    print(f"🟢 [OK] Temp {temperature:.1f}C")

                # Set LEDs atomically
                led_controller.set_two(
                    cpu_color[0], cpu_color[1], cpu_color[2],
                    room_color[0], room_color[1], room_color[2]
                )
                    
                # Create reading record
                reading = SensorReading(
                    sensor_id="dht22-gpio-4",
                    temperature=temperature,
                    humidity=humidity,
                    timestamp_ms=timestamp_ms
                )
                readings.append(reading)
            else:
                print(f"⚠️ dht22 read error: {result}")
                
        except Exception as e:
            # return error reading or empty
            print(f"Error reading sensor: {e}")
            
        return readings
