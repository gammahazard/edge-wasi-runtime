"""
==============================================================================
app.py - python sensor plugin for wasi host (REAL DHT22 DATA)
==============================================================================

purpose:
    this module implements the sensor-logic interface defined in plugin.wit.
    it demonstrates the WASI CAPABILITY MODEL where:
    - we IMPORT gpio_provider from the rust host
    - we call gpio_provider.read_dht22() to get REAL sensor data
    - we return structured readings to the host

this is NOT simulated data - it reads from the actual DHT22 sensor
connected to GPIO pin 4 on the raspberry pi.

security model:
    this python code runs in a SANDBOX (wasm). it cannot:
    - directly access gpio pins
    - make syscalls
    - touch the filesystem
    
    instead, we call gpio_provider.read_dht22() which is handled by
    the rust host. the host controls exactly what hardware access is allowed.

relationships:
    - implements: ../../wit/plugin.wit (sensor-logic)
    - imports: gpio-provider (from rust host)
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
# - imports: gpio_provider module to call host hardware functions

from wit_world.exports import SensorLogic
from wit_world.exports.sensor_logic import SensorReading
from wit_world.imports import gpio_provider


class SensorLogic(SensorLogic):
    """
    implementation of the sensor-logic interface from plugin.wit.
    
    this plugin reads REAL data from the dht22 sensor by calling
    the gpio-provider interface imported from the rust host.
    """
    
    def poll(self) -> list[SensorReading]:
        """
        poll the dht22 sensor and return real readings.
        
        this method:
        1. calls gpio_provider.read_dht22(4) to read gpio pin 4
        2. handles errors gracefully (dht22 can be flaky)
        3. packages data into sensor-reading record
        4. returns to rust host for dashboard/logging
        
        returns:
            list of sensor readings (typically one for single dht22)
        """
        
        # get current timestamp via host capability
        timestamp_ms = gpio_provider.get_timestamp_ms()
        
        # read dht22 sensor via host capability
        # the RUST HOST handles the actual hardware access
        # we just call the interface and get data back
        try:
            result = gpio_provider.read_dht22(4)  # BCM GPIO4 = physical pin 7
            
            # result is a result<tuple<f32, f32>, string>
            # in python this comes as either (temp, humidity) or raises
            if isinstance(result, tuple):
                temperature, humidity = result
            else:
                # error case - log and use fallback
                print(f"⚠️ dht22 read error: {result}")
                # return empty list so dashboard doesn't show stale data
                return []
                
        except Exception as e:
            # handle any exceptions from the host
            print(f"⚠️ dht22 exception: {e}")
            return []
        
        # create sensor reading with REAL data
        reading = SensorReading(
            sensor_id="dht22-gpio4",  # identifies this sensor
            temperature=temperature,
            humidity=humidity,
            timestamp_ms=timestamp_ms,
        )
        
        # log to stdout (visible in host console via inherit_stdio)
        print(f"[WASM] {temperature:.1f}C, {humidity:.1f}%")
        
        # return as list (interface supports multiple sensors)
        return [reading]


# ==============================================================================
# note on testing
# ==============================================================================
# this module cannot be run directly as python because it depends on
# gpio_provider which is only available when running inside wasmtime.
#
# to test:
# 1. build: componentize-py -d ../../wit -w sensor-plugin componentize app -o sensor.wasm
# 2. run host: cd ../../host && cargo run --release
# 3. check http://localhost:3000 for dashboard with real data
