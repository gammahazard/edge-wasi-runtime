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

security model:
    this python code runs in a SANDBOX (wasm). it cannot:
    - directly access gpio pins
    - make syscalls
    - touch the filesystem
    
    instead, we call imported capabilities which are handled by
    the rust host. the host controls exactly what hardware access is allowed.

relationships:
    - implements: ../../wit/plugin.wit (sensor-logic)
    - imports: gpio-provider, led-controller, buzzer-controller (from rust host)
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
# these values control when alerts trigger. change them, rebuild wasm,
# and the host will auto-reload without restarting.

TEMP_HIGH = 30.0      # celsius - above this = red leds + buzzer
TEMP_LOW = 15.0       # celsius - below this = blue leds
HUMIDITY_HIGH = 80.0  # percent - above this = warning beep


class SensorLogic(SensorLogic):
    """
    implementation of the sensor-logic interface from plugin.wit.
    
    this plugin reads REAL data from the dht22 sensor and triggers
    visual/audio alerts based on thresholds defined above.
    
    all alert logic is in this hot-swappable wasm module!
    """
    
    def poll(self) -> list[SensorReading]:
        """
        poll the dht22 sensor, update leds, and trigger alerts.
        
        this method:
        1. reads sensor via gpio_provider
        2. updates led strip based on temperature
        3. triggers buzzer on high temp/humidity
        4. returns reading to host
        
        returns:
            list of sensor readings (typically one for single dht22)
        """
        
        # get current timestamp via host capability
        timestamp_ms = gpio_provider.get_timestamp_ms()
        
        # read dht22 sensor via host capability
        try:
            result = gpio_provider.read_dht22(4)  # BCM GPIO4 = physical pin 7
            
            if isinstance(result, tuple):
                temperature, humidity = result
            else:
                print(f"⚠️ dht22 read error: {result}")
                return []
                
        except Exception as e:
            print(f"⚠️ dht22 exception: {e}")
            return []
        
        # ======================================================================
        # turn off unused leds (2-10) to save power
        # ======================================================================
        for i in range(2, 11):
            led_controller.set_led(i, 0, 0, 0)
        
        # ======================================================================
        # cpu temperature indicator - LED 0
        # ======================================================================
        # first led shows pi cpu health
        cpu_temp = gpio_provider.get_cpu_temp()
        
        if cpu_temp > 70.0:
            led_controller.set_led(0, 255, 0, 0)  # red = hot cpu
        elif cpu_temp > 50.0:
            led_controller.set_led(0, 255, 255, 0)  # yellow = warm cpu
        else:
            led_controller.set_led(0, 0, 255, 0)  # green = cool cpu
        
        # ======================================================================
        # room temperature indicator - LED 1
        # ======================================================================
        # second led shows dht22 sensor reading
        
        if temperature > TEMP_HIGH:
            led_controller.set_led(1, 255, 0, 0)  # red = hot room
            buzzer_controller.beep(3, 100, 100)
            print(f"🔴 [ALERT] HIGH TEMP: {temperature:.1f}C (CPU: {cpu_temp:.1f}C)")
            
        elif temperature < TEMP_LOW:
            led_controller.set_led(1, 0, 0, 255)  # blue = cold room
            print(f"🔵 [COLD] {temperature:.1f}C (CPU: {cpu_temp:.1f}C)")
            
        elif humidity > HUMIDITY_HIGH:
            led_controller.set_led(1, 0, 255, 255)  # cyan = humid
            buzzer_controller.beep(2, 150, 200)
            print(f"💧 [HUMID] {humidity:.1f}% (CPU: {cpu_temp:.1f}C)")
            
        else:
            led_controller.set_led(1, 0, 255, 0)  # green = normal
            print(f"🟢 [OK] {temperature:.1f}C, {humidity:.1f}% (CPU: {cpu_temp:.1f}C)")
        
        # create sensor reading
        reading = SensorReading(
            sensor_id="dht22-gpio4",
            temperature=temperature,
            humidity=humidity,
            timestamp_ms=timestamp_ms,
        )
        
        return [reading]


# ==============================================================================
# note on testing
# ==============================================================================
# this module cannot be run directly as python because it depends on
# gpio_provider, led_controller, buzzer_controller which are only
# available when running inside wasmtime.
#
# to test:
# 1. build: componentize-py -d ../../wit -w sensor-plugin componentize app -o sensor.wasm
# 2. run host: cd ../../host && cargo run --release
# 3. check http://localhost:3000 for dashboard
# 4. to hot-reload: edit thresholds above, rebuild wasm, refresh browser
