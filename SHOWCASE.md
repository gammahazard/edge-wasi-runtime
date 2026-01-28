# 🎤 Showcase Script: HARVESTER OS

> **The Question:** Why use WebAssembly for IoT?
> **The Answer:** It solves the "Secure Plugin" problem at scale.

This demo proves you can run untrusted Python code on devices with bare-metal access (GPIO, I2C), yet keep them **completely sandboxed**, **hot-swappable**, and **distributed across multiple nodes**.

---

## 🚀 What is this?
A **secure, multi-node edge computing platform**. Python plugins control hardware sensors across 3 Raspberry Pi devices, managed by a Rust host that enforces security, stability, and live updates.

**Key Features:**
1.  **Multi-Node Architecture**: Hub + 2 Spokes over local network
2.  **Polyglot**: Rust (System) + Python (Scripting) + WASM (Sandboxing)
3.  **Sandboxed**: Python cannot hack the device; it can only request what the Rust host explicitly allows
4.  **Hot Reload**: Update Python logic *instantly* without restarting the Rust server
5.  **Live Dashboard**: Real-time sensor data updates every 3 seconds
6.  **Hardware Control**: Sensors (DHT22, BME680), LEDs (WS2812B), Buzzer

---

## 🏗️ Architecture

```
                    ┌─────────────────────────────────────────┐
                    │           HARVESTER OS                   │
                    │         Web Dashboard (:3000)            │
                    └────────────────┬────────────────────────┘
                                     │
         ┌───────────────────────────┼───────────────────────────┐
         │                           │                           │
   ┌─────┴─────┐              ┌──────┴──────┐            ┌──────┴──────┐
   │  REVPI    │              │   PI4       │            │  PIZERO     │
   │  HUB      │◄────────────►│  SPOKE 1    │◄──────────►│  SPOKE 2    │
   │ 192.168.7.10            │ 192.168.7.11 │           │ 192.168.7.12 │
   └───────────┘              └─────────────┘            └─────────────┘
```

| Node | Hardware | Role |
|------|----------|------|
| RevPi Connect 4 | Industrial Pi | Hub - aggregates data, hosts dashboard |
| Raspberry Pi 4 | DHT22, BME680, LEDs, Buzzer | Spoke - sensor readings, alerts |
| Pi Zero 2W | BME680 (shared I2C) | Spoke - system/network monitoring |

---

## 🛠️ The Demo Flow

### 1. The Security Contract (`wit/plugin.wit`)
Open `wit/plugin.wit`. This is the "Constitution" - plugins can ONLY use interfaces declared here.
*   **Talking Point**: The Python code imports `gpio-provider`, `led-controller`, `buzzer-controller`, and `i2c`. Without explicit imports, the sandbox physically prevents hardware access. Deny-by-default.

### 2. The Live Dashboard
Open **http://192.168.7.10:3000**
*   **Talking Point**: Real sensor data from DHT22 (temp/humidity) and BME680 (air quality/IAQ). Updates every 3 seconds without page refresh. Includes buzzer controls and log viewer with tabs for each node.

### 3. Multi-Node Log Viewer
Click through Hub/Pi4/PiZero tabs in the log viewer.
*   **Talking Point**: Each node streams its own logs with EST timestamps. Pi4 shows sensor readings, PiZero shows network health checks, Hub shows aggregation status.

### 4. Buzzer Control
Click "Short Beep", "3x Beep", or "Long Tone" buttons.
*   **Talking Point**: Dashboard on RevPi proxies the command to Pi4 over HTTP. The buzzer is physically on Pi4, but controllable from any device viewing the dashboard.

### 5. Hot Reload (The Magic Trick)
1.  Open `plugins/dht22/app.py`
2.  Change `HIGH_TEMP = 30.0` to `HIGH_TEMP = 20.0`
3.  Run: `./scripts/build-plugins-wsl.sh && ./scripts/update-plugins.sh`
4.  **Do NOT restart the Rust host**
5.  Watch LED 1 change color based on new threshold!
*   **Talking Point**: Updated alert logic without stopping the server. No dropped connections. Thresholds live in hot-swappable Python.

---

## 🔌 Hardware Setup

### Pi 4 (Spoke 1) - 192.168.7.11
- **DHT22 Temperature/Humidity Sensor**:
    - VCC → 3.3V (Pin 1)
    - Data → GPIO4 (Pin 7)
    - GND → Ground (Pin 6)
    - Pull-up: 4.7kΩ between VCC and Data
- **BME680 Environmental Sensor** (I2C):
    - VCC → 3.3V
    - GND → Ground
    - SDA → GPIO2 (Pin 3)
    - SCL → GPIO3 (Pin 5)
    - Address: 0x77
- **WS2812B LED Strip (11 LEDs)**:
    - VCC → 5V (Pin 2)
    - GND → Ground (Pin 14)
    - DIN → GPIO18 (Pin 12)
- **Buzzer via Relay**:
    - Relay VCC → 5V
    - Relay GND → Ground
    - Relay IN → GPIO17 (Pin 11) - Active LOW
    - Buzzer on relay NO terminals

### Pi Zero 2W (Spoke 2) - 192.168.7.12
- **BME680** (shared I2C bus via Wago lever nuts with Pi4)
- Native Python service (no WASM - saves ~250MB RAM)
- Pings Hub and Pi4 for network health checks

---

## 📊 LED Status Guide

| LED | Color | Meaning |
|-----|-------|---------|
| 0 | Green | CPU < 50°C |
| 0 | Yellow | CPU 50-70°C |
| 0 | Red | CPU > 70°C |
| 1 | Green | Room temp normal (15-25°C) |
| 1 | Orange | Room warm (25-30°C) |
| 1 | Red | Room > 30°C (alarm) |
| 1 | Blue | Room < 15°C (cold) |
| 2 | Green | IAQ Excellent (0-50) |
| 2 | Yellow | IAQ Moderate (100-150) |
| 2 | Red | IAQ Bad (200+) |
| 2 | Purple | BME680 Calibrating |

---

## 🎯 Why This Matters

If you just wanted to read a sensor, a 5-line Python script is fine.
**But this architecture allows you to:**
*   **Isolate crashy code**: If Python plugins crash, the Rust host stays up
*   **Update over-the-air**: Hot reload plugins without system reboots
*   **Securely run 3rd-party code**: Sandboxed plugins with capability-based security
*   **Scale to multiple nodes**: Same plugin code runs on any node
*   **Monitor everything**: Live dashboard with real-time data and alerts
