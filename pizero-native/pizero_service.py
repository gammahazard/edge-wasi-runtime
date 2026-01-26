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
    """Print and also save to log buffer."""
    msg = " ".join(str(a) for a in args)
    log_buffer.append(msg)
    original_print(*args, **kwargs)

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
# BME680 DRIVER (Simplified - matches our WASM plugin logic)
# ==============================================================================
class BME680:
    def __init__(self, bus_num=1, addr=0x77):
        self.bus = smbus2.SMBus(bus_num)
        self.addr = addr
        self.cal = {}
        self._initialized = False
    
    def init_sensor(self):
        if self._initialized:
            return True
        try:
            # Check chip ID
            chip_id = self.bus.read_byte_data(self.addr, 0xD0)
            if chip_id != 0x61:
                print(f"⚠️ BME680: Unexpected Chip ID {hex(chip_id)}")
                return False
            
            # Read temperature calibration
            t1_data = self.bus.read_i2c_block_data(self.addr, 0xE9, 2)
            self.cal['t1'] = (t1_data[1] << 8) | t1_data[0]
            
            t2_data = self.bus.read_i2c_block_data(self.addr, 0x8A, 2)
            t2 = (t2_data[1] << 8) | t2_data[0]
            self.cal['t2'] = t2 if t2 < 32768 else t2 - 65536
            
            t3_data = self.bus.read_byte_data(self.addr, 0x8C)
            self.cal['t3'] = t3_data if t3_data < 128 else t3_data - 256
            
            print(f"🟢 BME680: Initialized | Cal: T1={self.cal['t1']} T2={self.cal['t2']} T3={self.cal['t3']}")
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
            self.bus.write_byte_data(self.addr, 0x5A, 0x73)  # Heater target
            self.bus.write_byte_data(self.addr, 0x64, 0x59)  # Heater duration
            self.bus.write_byte_data(self.addr, 0x71, 0x10)  # Enable gas
            self.bus.write_byte_data(self.addr, 0x74, 0b01001001)  # Forced mode
            
            time.sleep(0.25)  # Wait for measurement
            
            # Read data registers
            data = self.bus.read_i2c_block_data(self.addr, 0x1D, 16)
            
            # Temperature
            raw_temp = ((data[5] << 12) | (data[6] << 4) | (data[7] >> 4))
            var1 = (raw_temp / 16384.0) - (self.cal['t1'] / 1024.0)
            var1 = var1 * self.cal['t2']
            var2 = (raw_temp / 131072.0) - (self.cal['t1'] / 8192.0)
            var2 = (var2 * var2) * (self.cal['t3'] * 16.0)
            t_fine = var1 + var2
            temp = t_fine / 5120.0
            
            # Humidity (simplified)
            raw_hum = (data[8] << 8) | data[9]
            hum = raw_hum / 1000.0
            
            # Pressure (simplified)
            raw_pres = (data[2] << 12) | (data[3] << 4) | (data[4] >> 4)
            pres = raw_pres / 100.0
            
            # Gas resistance
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
            
            return {"temp": temp, "humidity": hum, "pressure": pres, "gas": gas}
        except Exception as e:
            print(f"⚠️ BME680 Read Error: {e}")
            return None

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
    
    bme = BME680(I2C_BUS, BME680_ADDR)
    
    while True:
        readings = []
        timestamp = int(time.time() * 1000)
        
        # 1. Read BME680
        bme_data = bme.read()
        if bme_data:
            gas = bme_data["gas"]
            hum = bme_data["humidity"]
            
            # Simple IAQ estimate (no calibration needed - just passive monitoring)
            # Lower gas resistance = more VOCs = worse air quality
            if gas >= 100:
                iaq = 50  # Excellent
            elif gas >= 50:
                iaq = 100  # Good
            elif gas >= 20:
                iaq = 150  # Moderate
            elif gas >= 10:
                iaq = 200  # Poor
            else:
                iaq = 300  # Bad
            
            status = "Excellent" if iaq <= 50 else "Good" if iaq <= 100 else "Moderate" if iaq <= 150 else "Poor" if iaq <= 200 else "Bad"
            print(f"🔘 [PASSIVE] {bme_data['temp']:.1f}°C | {hum:.0f}% | Gas: {gas:.0f} KΩ | IAQ: {iaq} ({status})")
            
            readings.append({
                "sensor_id": f"{NODE_ID}:bme680",
                "sensor_type": "bme680",
                "data": {
                    "temperature": bme_data["temp"],
                    "humidity": bme_data["humidity"],
                    "pressure": bme_data["pressure"],
                    "gas_resistance": gas,
                    "iaq_score": iaq
                },
                "timestamp_ms": timestamp
            })
        
        # 2. System stats
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
