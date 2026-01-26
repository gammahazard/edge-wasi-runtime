"""
==============================================================================
oled_plugin.py - OLED Display Plugin
==============================================================================

Drives a small OLED display (SSD1306) to show:
- Current sensor readings
- System status

Uses generic I2C HAL only.

Build:
    componentize-py -d ../../wit -w oled-plugin componentize app -o oled.wasm
"""

from wit_world.exports import OledLogic
from wit_world.imports import i2c
import json

OLED_ADDR = 0x3C

class OledLogic(OledLogic):
    def update(self, sensor_data: str):
        """Update OLED display with sensor data."""
        try:
            data = json.loads(sensor_data) if sensor_data else {}
            
            # Extract key values
            dht = data.get("dht22", {})
            temp = dht.get("temperature", 0.0)
            hum = dht.get("humidity", 0.0)
            
            bme = data.get("bme680", {})
            iaq = bme.get("iaq_score", 0)
            
            pi = data.get("pi", {})
            cpu_temp = pi.get("cpu_temp", 0.0)
            
            # Format display text
            line1 = f"TEMP: {temp:.1f}C"
            line2 = f"HUM:  {hum:.0f}%"
            line3 = f"IAQ:  {iaq}"
            line4 = f"CPU:  {cpu_temp:.1f}C"
            
            print(f"📺 [OLED] {line1} | {line2} | {line3} | {line4}")
            
            # In full implementation, we'd send SSD1306 commands via:
            # i2c.transfer(OLED_ADDR, command_hex, 0)
            
        except Exception as e:
            print(f"❌ OLED update error: {e}")
