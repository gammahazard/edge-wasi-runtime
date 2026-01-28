#!/usr/bin/env python3
"""
==============================================================================
Pi Zero Native Service - Lightweight Sensor & Network Monitor
==============================================================================

This runs NATIVELY on the Pi Zero (no WASM) to:
1. Read BME680 sensor via I2C (smbus2)
2. Read CPU temperature
3. Monitor network health (ping Hub and Pi4)
4. Push all data to Hub API
5. Serve logs via simple HTTP API

Memory usage: ~30-50MB vs 300MB+ for WASM runtime
"""

import os
import sys
import time
import json
import subprocess
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler
from collections import deque
import requests
import smbus2

# ==============================================================================
# CONFIGURATION
# ==============================================================================
HUB_URL = os.getenv("HUB_URL", "http://192.168.7.10:3000/push")
NODE_ID = os.getenv("NODE_ID", "pizero-native")
POLL_INTERVAL = int(os.getenv("POLL_INTERVAL", "5"))  # seconds
PING_TARGETS = ["192.168.7.10", "192.168.7.11"]  # Hub, Pi4
API_PORT = 3000  # Same port as wasi-host for compatibility

# BME680 I2C
BME680_ADDR = 0x77
I2C_BUS = 1

# Log buffer (last 100 lines)
log_buffer = deque(maxlen=100)
original_print = print

def buffered_print(*args, **kwargs):
    """Print and also save to log buffer with EST timestamp."""
    from datetime import datetime, timezone, timedelta
    
    # EST is UTC-5
    est = timezone(timedelta(hours=-5))
    now = datetime.now(est)
    timestamp = now.strftime("[%Y/%m/%d @ %I:%M%p]").lower()
    
    msg = " ".join(str(a) for a in args)
    timestamped_msg = f"{timestamp} {msg}"
    log_buffer.append(timestamped_msg)
    original_print(timestamped_msg, **kwargs)

# Override print to capture logs
print = buffered_print

# ==============================================================================
# SIMPLE HTTP API (for log viewing from dashboard)
# ==============================================================================
class LogHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # Suppress default HTTP logging
    
    def do_GET(self):
        if self.path == "/api/logs":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            response = {"logs": list(log_buffer)}
            self.wfile.write(json.dumps(response).encode())
        elif self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
        else:
            self.send_response(404)
            self.end_headers()
    
    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.end_headers()

def start_api_server():
    """Start HTTP server in background thread."""
    server = HTTPServer(("0.0.0.0", API_PORT), LogHandler)
    print(f"🌐 API Server listening on port {API_PORT}")
    server.serve_forever()

# ==============================================================================
# BME680 DRIVER (Full calibration - matches our WASM plugin logic exactly)
# ==============================================================================
class BME680:
    def __init__(self, bus_num=1, addr=0x77):
        self.bus = smbus2.SMBus(bus_num)
        self.addr = addr
        self.cal = {}
        self.t_fine = 0.0
        self._initialized = False
        # IAQ adaptive baseline
        self.gas_baseline = 0.0
        self.burn_in_count = 0
        self.gas_history = []
    
    def _signed8(self, val):
        return val if val < 128 else val - 256
    
    def _signed16(self, val):
        return val if val < 32768 else val - 65536
    
    def init_sensor(self):
        if self._initialized:
            return True
        try:
            # Check chip ID
            chip_id = self.bus.read_byte_data(self.addr, 0xD0)
            if chip_id != 0x61:
                print(f"⚠️ BME680: Unexpected Chip ID {hex(chip_id)}")
                return False
            
            # ===== TEMPERATURE CALIBRATION =====
            t1_data = self.bus.read_i2c_block_data(self.addr, 0xE9, 2)
            self.cal['t1'] = t1_data[0] | (t1_data[1] << 8)  # unsigned 16-bit
            
            t23_data = self.bus.read_i2c_block_data(self.addr, 0x8A, 3)
            self.cal['t2'] = self._signed16(t23_data[0] | (t23_data[1] << 8))
            self.cal['t3'] = self._signed8(t23_data[2])
            
            print(f"📊 [BME680] Temp cal: t1={self.cal['t1']} t2={self.cal['t2']} t3={self.cal['t3']}")
            
            # ===== HUMIDITY CALIBRATION (h1-h7 per Bosch datasheet) =====
            h_data1 = self.bus.read_i2c_block_data(self.addr, 0xE1, 3)  # E1, E2, E3
            h_data2 = self.bus.read_i2c_block_data(self.addr, 0xE4, 5)  # E4-E8
            
            # h2 uses full E1 + upper nibble of E2
            h2_raw = (h_data1[0] << 4) | (h_data1[1] >> 4)
            # h1 uses lower nibble of E2 + full E3
            h1_raw = (h_data1[2] << 4) | (h_data1[1] & 0x0F)
            
            # Post-processing per Adafruit library
            self.cal['h2'] = (h2_raw * 16) + (h1_raw % 16)
            self.cal['h1'] = h1_raw / 16.0
            self.cal['h3'] = self._signed8(h_data2[0])
            self.cal['h4'] = self._signed8(h_data2[1])
            self.cal['h5'] = self._signed8(h_data2[2])
            self.cal['h6'] = h_data2[3]  # unsigned
            self.cal['h7'] = self._signed8(h_data2[4])
            
            print(f"📊 [BME680] Hum cal: h1={self.cal['h1']:.1f} h2={self.cal['h2']} h3={self.cal['h3']} h4={self.cal['h4']} h5={self.cal['h5']} h6={self.cal['h6']} h7={self.cal['h7']}")
            
            # ===== PRESSURE CALIBRATION (p1-p10 per Bosch datasheet) =====
            p_data1 = self.bus.read_i2c_block_data(self.addr, 0x8E, 16)  # 0x8E-0x9D
            p_data2 = self.bus.read_i2c_block_data(self.addr, 0x9E, 2)   # 0x9E-0x9F
            
            self.cal['p1'] = p_data1[0] | (p_data1[1] << 8)  # unsigned
            self.cal['p2'] = self._signed16(p_data1[2] | (p_data1[3] << 8))
            self.cal['p3'] = self._signed8(p_data1[4])
            self.cal['p4'] = self._signed16(p_data1[6] | (p_data1[7] << 8))
            self.cal['p5'] = self._signed16(p_data1[8] | (p_data1[9] << 8))
            self.cal['p6'] = self._signed8(p_data1[11])
            self.cal['p7'] = self._signed8(p_data1[10])
            self.cal['p8'] = self._signed16(p_data1[14] | (p_data1[15] << 8))
            self.cal['p9'] = self._signed16(p_data2[0] | (p_data2[1] << 8))
            self.cal['p10'] = p_data1[13]  # unsigned
            
            print(f"🟢 BME680: Initialized with full calibration")
            self._initialized = True
            return True
        except Exception as e:
            print(f"❌ BME680 Init Error: {e}")
            return False
    
    def read(self):
        if not self._initialized:
            if not self.init_sensor():
                return None
        
        try:
            # Configure and trigger measurement
            self.bus.write_byte_data(self.addr, 0x72, 0x01)  # Humidity 1x
            self.bus.write_byte_data(self.addr, 0x74, 0x54)  # Temp 2x, Pressure 4x, sleep mode
            self.bus.write_byte_data(self.addr, 0x5A, 0x59)  # Heater target 320C
            self.bus.write_byte_data(self.addr, 0x64, 0x59)  # Heater duration 100ms
            self.bus.write_byte_data(self.addr, 0x71, 0x10)  # Enable gas, heater step 0
            self.bus.write_byte_data(self.addr, 0x74, 0x55)  # Force mode
            
            time.sleep(0.25)  # Wait for measurement
            
            # Read data registers (0x1D to 0x2F)
            data = self.bus.read_i2c_block_data(self.addr, 0x1D, 17)
            
            # ===== TEMPERATURE (Bosch formula) =====
            raw_temp = ((data[5] << 12) | (data[6] << 4) | (data[7] >> 4))
            var1 = ((raw_temp / 16384.0) - (self.cal['t1'] / 1024.0)) * self.cal['t2']
            var2 = ((raw_temp / 131072.0) - (self.cal['t1'] / 8192.0))
            var2 = var2 * var2 * self.cal['t3'] * 16.0
            self.t_fine = var1 + var2
            temp = self.t_fine / 5120.0
            
            # ===== HUMIDITY (Adafruit formula with full calibration) =====
            raw_hum = (data[8] << 8) | data[9]
            temp_scaled = ((self.t_fine * 5) + 128) / 256
            
            var1 = (raw_hum - (self.cal['h1'] * 16.0)) - (
                (temp_scaled * self.cal['h3']) / 200.0
            )
            var2 = (
                self.cal['h2']
                * (
                    ((temp_scaled * self.cal['h4']) / 100.0)
                    + (
                        ((temp_scaled * ((temp_scaled * self.cal['h5']) / 100.0)) / 64.0)
                        / 100.0
                    )
                    + 16384.0
                )
            ) / 1024.0
            var3 = var1 * var2
            var4 = self.cal['h6'] * 128.0
            var4 = (var4 + ((temp_scaled * self.cal['h7']) / 100.0)) / 16.0
            var5 = ((var3 / 16384.0) * (var3 / 16384.0)) / 1024.0
            var6 = (var4 * var5) / 2.0
            humidity = (((var3 + var6) / 1024.0) * 1000.0) / 4096.0
            humidity /= 1000.0  # get back to RH %
            
            # Clamp to valid range
            humidity = max(0.0, min(100.0, humidity))
            
            # ===== PRESSURE (Bosch formula with calibration) =====
            raw_pres = (data[2] << 12) | (data[3] << 4) | (data[4] >> 4)
            var1 = (self.t_fine / 2.0) - 64000.0
            var2 = var1 * var1 * self.cal['p6'] / 131072.0
            var2 = var2 + (var1 * self.cal['p5'] * 2.0)
            var2 = (var2 / 4.0) + (self.cal['p4'] * 65536.0)
            var1 = (self.cal['p3'] * var1 * var1 / 16384.0 + self.cal['p2'] * var1) / 524288.0
            var1 = (1.0 + var1 / 32768.0) * self.cal['p1']
            pressure = 1048576.0 - raw_pres
            if var1 != 0:
                pressure = (pressure - (var2 / 4096.0)) * 6250.0 / var1
                var1 = self.cal['p9'] * pressure * pressure / 2147483648.0
                var2 = pressure * self.cal['p8'] / 32768.0
                var3 = (pressure / 256.0) ** 3 * self.cal['p10'] / 131072.0
                pressure = pressure + (var1 + var2 + var3 + self.cal['p7'] * 128.0) / 16.0
            pressure = pressure / 100.0  # Convert to hPa
            
            # ===== GAS RESISTANCE (with range table) =====
            gas_valid = (data[14] & 0x20) != 0
            heater_stab = (data[14] & 0x10) != 0
            
            if gas_valid and heater_stab:
                raw_gas = (data[13] << 2) | ((data[14] & 0xC0) >> 6)
                gas_range = data[14] & 0x0F
                gas_range_table = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768]
                if raw_gas > 0:
                    gas = (1340.0 * 1000000.0) / (raw_gas * gas_range_table[gas_range]) / 1000.0
                    gas = min(gas, 1000.0)  # Cap at 1000 KΩ
                else:
                    gas = 0.0
            else:
                gas = 0.0
            
            return {"temp": temp, "humidity": humidity, "pressure": pressure, "gas": gas}
        except Exception as e:
            print(f"⚠️ BME680 Read Error: {e}")
            return None
    
    def calculate_iaq(self, gas, humidity):
        """Calculate IAQ with adaptive baseline (matches Pi4 WASM plugin)"""
        self.burn_in_count += 1
        
        # Smooth gas readings
        self.gas_history.append(gas)
        if len(self.gas_history) > 5:
            self.gas_history.pop(0)
        
        # Calibration phase (60 seconds at 5s interval = 12 readings)
        if self.burn_in_count < 12:
            if gas > self.gas_baseline:
                self.gas_baseline = gas
            return 0, 0, "Calibrating"
        
        # Update gas baseline (slow adaptation)
        if gas > self.gas_baseline:
            self.gas_baseline = gas  # New clean air reference
        else:
            # Slowly drift baseline toward current reading
            self.gas_baseline = self.gas_baseline * 0.995 + gas * 0.005
        
        # Gas score: Higher resistance = cleaner air = lower score
        if self.gas_baseline > 0 and gas > 0:
            gas_ratio = gas / self.gas_baseline
            if gas_ratio >= 1.0:
                gas_score = 0  # Better than baseline = excellent
            else:
                gas_score = (1.0 - gas_ratio) * 75.0
            gas_score = max(0, min(75, gas_score))
        else:
            gas_score = 25  # Unknown, assume moderate
        
        # Humidity score: 40% is ideal, deviation adds to score
        hum_offset = abs(humidity - 40.0)
        hum_score = min(25, (hum_offset / 60.0) * 25.0)
        
        # Final IAQ (0-300 scale, lower is better)
        iaq = int((gas_score + hum_score) * 3.0)
        iaq = min(500, max(0, iaq))
        
        # Status text
        if iaq <= 50:
            status = "Excellent"
        elif iaq <= 100:
            status = "Good"
        elif iaq <= 150:
            status = "Moderate"
        elif iaq <= 200:
            status = "Poor"
        else:
            status = "Bad"
        
        return iaq, 1, status

# ==============================================================================
# SYSTEM METRICS
# ==============================================================================
def get_cpu_temp():
    try:
        with open("/sys/class/thermal/thermal_zone0/temp", "r") as f:
            return float(f.read().strip()) / 1000.0
    except:
        return 0.0

def get_memory():
    try:
        with open("/proc/meminfo", "r") as f:
            lines = f.readlines()
        mem = {}
        for line in lines:
            parts = line.split()
            if parts[0] in ["MemTotal:", "MemAvailable:"]:
                mem[parts[0]] = int(parts[1])  # KB
        total = mem.get("MemTotal:", 0) // 1024
        available = mem.get("MemAvailable:", 0) // 1024
        used = total - available
        return used, total
    except:
        return 0, 0

def ping_host(host, timeout=1):
    """Ping host and return latency in ms, or -1 if unreachable."""
    try:
        result = subprocess.run(
            ["ping", "-c", "1", "-W", str(timeout), host],
            capture_output=True,
            text=True,
            timeout=timeout + 1
        )
        if result.returncode == 0:
            # Parse latency from output like "time=1.23 ms"
            import re
            match = re.search(r'time[=<](\d+\.?\d*)', result.stdout)
            if match:
                return float(match.group(1))
            return 0.5  # Success but couldn't parse, assume fast
        return -1  # Failed
    except:
        return -1

# ==============================================================================
# MAIN LOOP
# ==============================================================================
def main():
    print("=" * 60)
    print("  Pi Zero Native Service v2.2.0")
    print("=" * 60)
    print(f"  Hub URL: {HUB_URL}")
    print(f"  Node ID: {NODE_ID}")
    print(f"  Poll Interval: {POLL_INTERVAL}s")
    print(f"  API Port: {API_PORT}")
    print("=" * 60)
    
    # Start API server in background
    api_thread = threading.Thread(target=start_api_server, daemon=True)
    api_thread.start()
    
    # BME680 removed - now Pi4-only sensor
    
    while True:
        readings = []
        timestamp = int(time.time() * 1000)
        
        # BME680 removed - now Pi4-only sensor
        # PiZero reports system stats and network health only
        
        # 1. System stats
        cpu_temp = get_cpu_temp()
        mem_used, mem_total = get_memory()
        
        readings.append({
            "sensor_id": f"{NODE_ID}:monitor",
            "sensor_type": "pi-monitor",
            "data": {
                "cpu_temp": cpu_temp,
                "cpu_usage": 0.0,  # Would need psutil for accurate reading
                "memory_used_mb": mem_used,
                "memory_total_mb": mem_total,
                "uptime_seconds": int(time.time())
            },
            "timestamp_ms": timestamp
        })
        
        # 3. Network health
        network_status = {}
        ping_log = []
        for target in PING_TARGETS:
            latency = ping_host(target)
            network_status[target] = latency
            label = "HUB" if target.endswith('.10') else "PI4" if target.endswith('.11') else target
            if latency < 0:
                ping_log.append(f"🔴 {label} OFFLINE")
            else:
                ping_log.append(f"🟢 {label} {latency:.1f}ms")
        print(f"🌐 [PING] {' | '.join(ping_log)}")
        
        readings.append({
            "sensor_id": f"{NODE_ID}:network",
            "sensor_type": "network-health",
            "data": network_status,
            "timestamp_ms": timestamp
        })
        
        # 4. Push to Hub
        try:
            response = requests.post(
                HUB_URL,
                json=readings,  # Hub expects array directly, not {"node_id":..., "readings":...}
                timeout=5
            )
            if response.status_code == 200:
                print(f"✅ Pushed {len(readings)} readings to Hub")
            else:
                print(f"⚠️ Hub returned {response.status_code}")
        except requests.exceptions.RequestException as e:
            print(f"❌ Failed to push to Hub: {e}")
        
        time.sleep(POLL_INTERVAL)

if __name__ == "__main__":
    main()
