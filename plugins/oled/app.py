"""
==============================================================================
oled_plugin.py - SSD1306 OLED Display via Generic I2C
==============================================================================
"""

import wit_world
from wit_world.imports import i2c
from wit_world.exports import OledLogic
import json

# ==============================================================================
# SSD1306 Constants & Driver
# ==============================================================================
OLED_ADDR = 0x3C
OLED_WIDTH = 128
OLED_HEIGHT = 64

# Command bytes
SSD1306_DISPLAYOFF = 0xAE
SSD1306_DISPLAYON = 0xAF
SSD1306_SETCONTRAST = 0x81
SSD1306_NORMALDISPLAY = 0xA6
SSD1306_INVERTDISPLAY = 0xA7
SSD1306_SETSTARTLINE = 0x40
SSD1306_MEMORYMODE = 0x20
SSD1306_COLUMNADDR = 0x21
SSD1306_PAGEADDR = 0x22

_buffer = bytearray(OLED_WIDTH * OLED_HEIGHT // 8)
_initialized = False

def _write_cmd(cmd: int):
    data_hex = format(0x00, '02x') + format(cmd, '02x')
    i2c.transfer(OLED_ADDR, data_hex, 0)

def _write_data(data: bytes):
    hex_str = format(0x40, '02x') + data.hex()
    i2c.transfer(OLED_ADDR, hex_str, 0)

def init_display():
    global _initialized
    init_cmds = [
        SSD1306_DISPLAYOFF,
        SSD1306_MEMORYMODE, 0x00,
        SSD1306_SETSTARTLINE | 0x00,
        0xA1, 0xC8, 0xA8, 0x3F, 0xD3, 0x00,
        0xD5, 0x80, 0xD9, 0xF1, 0xDA, 0x12,
        0xDB, 0x40, 0x8D, 0x14,
        SSD1306_NORMALDISPLAY,
        SSD1306_DISPLAYON,
    ]
    for cmd in init_cmds: _write_cmd(cmd)
    _initialized = True
    print("📺 [OLED] Initialized SSD1306")

def clear():
    global _buffer
    _buffer = bytearray(OLED_WIDTH * OLED_HEIGHT // 8)

def set_pixel(x: int, y: int, color: bool = True):
    if not (0 <= x < OLED_WIDTH and 0 <= y < OLED_HEIGHT): return
    index = x + (y // 8) * OLED_WIDTH
    if color: _buffer[index] |= (1 << (y % 8))
    else: _buffer[index] &= ~(1 << (y % 8))

def flush_display():
    if not _initialized: init_display()
    _write_cmd(SSD1306_COLUMNADDR)
    _write_cmd(0)
    _write_cmd(OLED_WIDTH - 1)
    _write_cmd(SSD1306_PAGEADDR)
    _write_cmd(0)
    _write_cmd(7)
    
    CHUNK_SIZE = 128
    for i in range(0, len(_buffer), CHUNK_SIZE):
        _write_data(bytes(_buffer[i:i + CHUNK_SIZE]))

def draw_text_params(x: int, y: int, text: str):
    # Placeholder for font rendering
    print(f"📺 [OLED] Text at ({x},{y}): {text}")
    # Draw a line for each char to show activity
    for i in range(len(text) * 4):
        set_pixel(x + i, y, True)

# ==============================================================================
# Export Implementation
# ==============================================================================
class OledLogic(OledLogic):
    """
    Implements oled-logic interface.
    Receives JSON sensor data from Host and updates display.
    """
    
    def update(self, sensor_data: str):
        # 1. Parse Data
        try:
            data = json.loads(sensor_data)
        except:
            print(f"📺 [OLED] Error parsing JSON: {sensor_data[:20]}...")
            return

        # 2. Extract Values
        dht = data.get("dht22", {})
        temp = dht.get("temp", 0.0)
        hum = dht.get("humidity", 0.0)
        
        pi = data.get("pi", {})
        cpu = pi.get("cpu_temp", 0.0)

        # 3. Draw UI
        clear()
        
        # Border
        for x in range(OLED_WIDTH): set_pixel(x, 0)
        for x in range(OLED_WIDTH): set_pixel(x, OLED_HEIGHT-1)
        
        # Data (simulated text)
        draw_text_params(4, 10, f"Temp: {temp:.1f}C")
        draw_text_params(4, 25, f"Hum:  {hum:.1f}%")
        draw_text_params(4, 40, f"CPU:  {cpu:.1f}C")
        
        # 4. Flush to Hardware
        flush_display()
