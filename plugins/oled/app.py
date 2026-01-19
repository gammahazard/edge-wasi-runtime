"""
==============================================================================
oled_plugin.py - SSD1306 OLED Display via Generic I2C
==============================================================================

purpose:
    drives an SSD1306 OLED display using the GENERIC I2C interface.
    this is the "Compile Once" demo - no Rust changes needed!
    
    this plugin was added without modifying the Rust host.

hardware:
    - SSD1306 128x64 or 128x32 OLED (I2C mode)
    - Default address: 0x3C (some variants use 0x3D)
    
connections:
    - VCC → 3.3V
    - GND → GND
    - SDA → GPIO 2 (I2C Data)
    - SCL → GPIO 3 (I2C Clock)

build command:
    componentize-py -d ../../wit -w generic-i2c-plugin componentize app -o oled.wasm
"""

# ==============================================================================
# WIT-Generated Imports
# ==============================================================================
import wit_world
from wit_world.imports import i2c

# ==============================================================================
# SSD1306 Constants
# ==============================================================================
OLED_ADDR = 0x3C  # Default I2C address (0x3D on some boards)
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

# Frame buffer (1 bit per pixel, 128x64 = 1024 bytes)
_buffer = bytearray(OLED_WIDTH * OLED_HEIGHT // 8)
_initialized = False


def _write_cmd(cmd: int):
    """Write a command byte to the OLED."""
    # I2C format: [control byte (0x00 = command), command byte]
    data_hex = format(0x00, '02x') + format(cmd, '02x')
    i2c.transfer(OLED_ADDR, data_hex, 0)


def _write_data(data: bytes):
    """Write display data to the OLED."""
    # I2C format: [control byte (0x40 = data), data bytes...]
    hex_str = format(0x40, '02x') + data.hex()
    i2c.transfer(OLED_ADDR, hex_str, 0)


def init_display():
    """Initialize the SSD1306 display."""
    global _initialized
    
    # Standard init sequence for 128x64 display
    init_cmds = [
        SSD1306_DISPLAYOFF,
        SSD1306_MEMORYMODE, 0x00,  # Horizontal addressing
        SSD1306_SETSTARTLINE | 0x00,
        0xA1,  # Segment remap
        0xC8,  # COM scan direction
        0xA8, 0x3F,  # Multiplex ratio (64 lines)
        0xD3, 0x00,  # Display offset
        0xD5, 0x80,  # Clock divide
        0xD9, 0xF1,  # Pre-charge
        0xDA, 0x12,  # COM pins
        0xDB, 0x40,  # VCOM deselect
        0x8D, 0x14,  # Charge pump
        SSD1306_NORMALDISPLAY,
        SSD1306_DISPLAYON,
    ]
    
    for cmd in init_cmds:
        _write_cmd(cmd)
    
    _initialized = True
    print("📺 [OLED] Initialized SSD1306 128x64")


def clear():
    """Clear the display buffer."""
    global _buffer
    _buffer = bytearray(OLED_WIDTH * OLED_HEIGHT // 8)


def set_pixel(x: int, y: int, color: bool = True):
    """Set a single pixel in the buffer."""
    if x < 0 or x >= OLED_WIDTH or y < 0 or y >= OLED_HEIGHT:
        return
    
    page = y // 8
    bit = 1 << (y % 8)
    index = x + page * OLED_WIDTH
    
    if color:
        _buffer[index] |= bit
    else:
        _buffer[index] &= ~bit


def draw_text(x: int, y: int, text: str):
    """Draw simple text (placeholder - needs font data)."""
    # TODO: Implement font rendering
    # For now, just mark the start position
    set_pixel(x, y, True)
    print(f"📺 [OLED] Text at ({x},{y}): {text}")


def update():
    """Push the buffer to the display."""
    if not _initialized:
        init_display()
    
    # Set column and page address range
    _write_cmd(SSD1306_COLUMNADDR)
    _write_cmd(0)           # Start column
    _write_cmd(OLED_WIDTH - 1)  # End column
    
    _write_cmd(SSD1306_PAGEADDR)
    _write_cmd(0)           # Start page
    _write_cmd(7)           # End page (64/8 - 1)
    
    # Send buffer in chunks (I2C has limits)
    CHUNK_SIZE = 128  # Bytes per I2C transaction
    for i in range(0, len(_buffer), CHUNK_SIZE):
        chunk = bytes(_buffer[i:i + CHUNK_SIZE])
        _write_data(chunk)


# ==============================================================================
# Demo function (call from host or test)
# ==============================================================================
def demo():
    """Draw a simple pattern on the display."""
    init_display()
    clear()
    
    # Draw a border
    for x in range(OLED_WIDTH):
        set_pixel(x, 0, True)
        set_pixel(x, OLED_HEIGHT - 1, True)
    for y in range(OLED_HEIGHT):
        set_pixel(0, y, True)
        set_pixel(OLED_WIDTH - 1, y, True)
    
    # Draw diagonal lines
    for i in range(min(OLED_WIDTH, OLED_HEIGHT)):
        set_pixel(i, i, True)
        set_pixel(OLED_WIDTH - 1 - i, i, True)
    
    update()
    print("📺 [OLED] Demo pattern displayed!")
