"""
OLED Plugin - SSD1306 128x64 Display Driver
Uses generic I2C for compile-once portability
"""
import json
from wit_world.exports import OledLogic
from wit_world.imports import i2c

OLED_ADDR = 0x3C
WIDTH = 128
HEIGHT = 64


class SSD1306:
    """Pure Python SSD1306 driver using generic I2C"""
    
    def __init__(self, addr: int = 0x3C):
        self.addr = addr
        self.buffer = bytearray(WIDTH * HEIGHT // 8)
        self._init_display()
    
    def _cmd(self, cmd: int):
        """Send command byte"""
        data = bytes([0x00, cmd]).hex()
        i2c.transfer(self.addr, data, 0)
    
    def _init_display(self):
        """Initialize display"""
        try:
            cmds = [
                0xAE,  # Display off
                0xD5, 0x80,  # Clock div
                0xA8, 0x3F,  # Multiplex
                0xD3, 0x00,  # Display offset
                0x40,  # Start line
                0x8D, 0x14,  # Charge pump
                0x20, 0x00,  # Memory mode
                0xA1,  # Segment remap
                0xC8,  # COM scan
                0xDA, 0x12,  # COM pins
                0x81, 0xCF,  # Contrast
                0xD9, 0xF1,  # Precharge
                0xDB, 0x40,  # VCOMH
                0xA4,  # Display all on resume
                0xA6,  # Normal display
                0xAF,  # Display on
            ]
            for cmd in cmds:
                self._cmd(cmd)
            print("✓ OLED initialized")
        except Exception as e:
            print(f"⚠️ OLED init error: {e}")
    
    def clear(self):
        """Clear buffer"""
        self.buffer = bytearray(WIDTH * HEIGHT // 8)
    
    def pixel(self, x: int, y: int, on: bool = True):
        """Set pixel in buffer"""
        if 0 <= x < WIDTH and 0 <= y < HEIGHT:
            idx = x + (y // 8) * WIDTH
            if on:
                self.buffer[idx] |= (1 << (y % 8))
            else:
                self.buffer[idx] &= ~(1 << (y % 8))
    
    def text(self, s: str, x: int, y: int):
        """Draw text (simple 5x7 font)"""
        # Simplified - just set pixels for basic visualization
        for i, c in enumerate(s[:20]):  # Max 20 chars
            cx = x + i * 6
            if cx < WIDTH:
                # Simple block representation
                for dy in range(7):
                    for dx in range(5):
                        self.pixel(cx + dx, y + dy, True)
    
    def show(self):
        """Write buffer to display"""
        try:
            # Set column and page address
            self._cmd(0x21)
            self._cmd(0)
            self._cmd(127)
            self._cmd(0x22)
            self._cmd(0)
            self._cmd(7)
            
            # Write data in chunks
            for i in range(0, len(self.buffer), 16):
                chunk = bytes([0x40]) + self.buffer[i:i+16]
                i2c.transfer(self.addr, chunk.hex(), 0)
        except Exception as e:
            print(f"⚠️ OLED show error: {e}")


# Lazy init - can't call I2C at compile time
display = None


class OledLogic(OledLogic):
    def update(self, sensor_data: str):
        global display
        
        # Lazy init on first call
        if display is None:
            display = SSD1306(OLED_ADDR)
        
        try:
            data = json.loads(sensor_data)
            
            dht = data.get("dht22", {})
            bme = data.get("bme680", {})
            hub = data.get("hub", {})
            
            display.clear()
            
            # Line 1: Room temp
            temp = dht.get("temperature", 0)
            display.text(f"ROOM: {temp:.1f}C", 0, 0)
            
            # Line 2: IAQ
            iaq = bme.get("iaq_score", 0)
            display.text(f"IAQ: {iaq}", 0, 16)
            
            # Line 3: HUB CPU
            cpu = hub.get("cpu_temp", 0)
            display.text(f"HUB: {cpu:.1f}C", 0, 32)
            
            # Line 4: Status
            display.text("HARVESTER OS", 0, 48)
            
            display.show()
            
        except Exception as e:
            print(f"❌ OLED update error: {e}")
