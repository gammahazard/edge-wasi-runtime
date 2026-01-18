"""
==============================================================================
bme680_plugin.py - BME680 environmental sensor logic
==============================================================================

purpose:
    polls the BME680 sensor via the host capability.
    manages LED 2 for air quality status.
    exports readings to the host.

relationships:
    - implements: ../../../wit/plugin.wit (bme680-logic interface)
    - imports: gpio-provider, led-controller
    - loaded by: host/src/runtime.rs
    
build command:
    componentize-py -d ../../wit -w bme680-plugin componentize app -o bme680.wasm
"""

import wit_world
# Standard correct import pattern
from wit_world.imports import gpio_provider, led_controller
from wit_world.exports import Bme680Logic
from wit_world.exports.bme680_logic import Bme680Reading

class Bme680Logic(Bme680Logic):
    def poll(self) -> list[Bme680Reading]:
        """
        read bme680 sensor and return readings list.
        also updates LED 2 based on air quality (Gas Resistance).
        """
        readings = []
        
        try:
            # 0x77 is the address detected by user's i2cdetect
            result = gpio_provider.read_bme680(0x77)
            
            # result is Result<(t, h, p, g), string>
            if isinstance(result, tuple):
                temp, humidity, pressure, gas = result
                timestamp = gpio_provider.get_timestamp_ms()
                
                # Create reading record
                reading = Bme680Reading(
                    sensor_id="bme680-i2c-0x77",
                    temperature=temp,
                    humidity=humidity,
                    pressure=pressure,
                    gas_resistance=gas,
                    timestamp_ms=timestamp
                )
                readings.append(reading)
                
                # Update LED 2 (Air Quality Status)
                # Gas resistance: Higher is usually cleaner air (VOCs reduce resistance)
                # > 50 KOhms: Good (Green)
                # < 10 KOhms: Bad (Red)
                # In between: Moderate (Yellow)
                
                if gas > 50.0:
                    # Good Air Quality -> Green
                    led_controller.set_led(2, 0, 255, 0)
                elif gas < 10.0:
                    # Bad Air Quality -> Red
                    led_controller.set_led(2, 255, 0, 0)
                else:
                    # Moderate -> Yellow
                    led_controller.set_led(2, 255, 120, 0)
                    
            else:
                # Error case (result is string error message)
                # Flash LED 2 Purple to indicate sensor error
                led_controller.set_led(2, 255, 0, 255)
                # We could log this if we had a logging capability
                
        except Exception as e:
            # Failsafe
            led_controller.set_led(2, 255, 0, 255)
            
        return readings
