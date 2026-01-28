# HARVESTER OS - System Architecture Deep Dive

> **Version**: 2.3.11  
> **Last Updated**: January 2026

This document provides a comprehensive technical analysis of the HARVESTER OS edge computing platform, including all components, data flows, and architectural decisions.

---

## Table of Contents

1. [Overview](#overview)
2. [The WIT File (API Contract)](#the-wit-file-api-contract)
3. [The Host (Rust)](#the-host-rust)
4. [Configuration System](#configuration-system)
5. [Plugins](#plugins)
6. [Native Pi Zero Service](#native-pi-zero-service)
7. [Data Flow](#data-flow)
8. [Key Architectural Decisions](#key-architectural-decisions)

---

## Overview

HARVESTER OS is a **multi-node edge computing platform** that uses the **WASI Component Model** to run sandboxed Python plugins on Raspberry Pi hardware. The architecture follows a **Hub-Spoke model**:

| Node | IP | Role |
|------|-----|------|
| **RevPi Connect 4** | 192.168.7.10 | Hub (aggregates data, serves dashboard) |
| **Raspberry Pi 4** | 192.168.7.11 | Spoke (runs sensors: DHT22, BME680) |
| **Pi Zero 2W** | 192.168.7.12 | Spoke (native Python service due to RAM constraints) |

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
   └─────┬─────┘              └──────┬──────┘            └──────┬──────┘
         │                           │                           │
   ┌─────┴─────┐              ┌──────┴──────┐            ┌──────┴──────┐
   │ Plugins:  │              │ Plugins:    │            │ Native Svc  │
   │ - dashboard             │ - dht22     │            │  (Python)   │
   │ - revpi-monitor         │ - bme680    │            │             │
   └───────────┘              │ - pi4-monitor           └─────────────┘
                              │ - oled      │
                              └─────────────┘
```

---

## The WIT File (API Contract)

**Location**: [`wit/plugin.wit`](file:///c:/Users/navra/Desktop/wasi-python-host/wit/plugin.wit)

The WIT (WebAssembly Interface Types) file is the **"Constitution"** of the application - the API contract between the Rust Host and Python WASM plugins.

### Security Model

1. **Sandboxing**: Python code runs in a WASM sandbox and **cannot** access files, network, or hardware unless explicitly granted via the interfaces defined in the WIT file.

2. **Capabilities**: The `import` statements are the ONLY capabilities granted. If an interface isn't imported, the Python code physically cannot use it.

3. **Type Safety**: Rust and Python data types are bridged automatically via the Component Model.

### Interfaces (Host Capabilities)

| Interface | Purpose | Key Functions |
|-----------|---------|---------------|
| `gpio-provider` | Sensor and system access | `read-dht22(pin)`, `get-cpu-temp()`, `get-timestamp-ms()` |
| `led-controller` | WS2812B LED strip control (11 LEDs) | `set-led(index, r, g, b)`, `set-all(r, g, b)`, `sync-leds()` |
| `buzzer-controller` | Piezo buzzer via relay (active low) | `buzz(duration-ms)`, `beep(count, duration-ms, interval-ms)` |
| `i2c` | Generic I2C bus access (hex encoded) | `transfer(addr, write-data, read-len)` |
| `spi` | SPI full-duplex transfers | `transfer(data)` |
| `uart` | Serial communication | `read(max-len)`, `write(data)`, `set-baud(rate)` |
| `system-info` | System metrics | `get-memory-usage()`, `get-cpu-usage()`, `get-uptime()` |

### Plugin Logic Interfaces (Guest Exports)

| Interface | Exports | Description |
|-----------|---------|-------------|
| `dht22-logic` | `poll() -> list<dht22-reading>` | Returns temperature/humidity readings |
| `bme680-logic` | `poll() -> list<bme680-reading>` | Returns environmental data + IAQ score |
| `pi-monitor-logic` | `poll() -> pi-stats` | Returns system health stats |
| `dashboard-logic` | `render(sensor-data: string) -> string` | Returns rendered HTML |
| `oled-logic` | `update(sensor-data: string)` | Updates OLED display |

### Plugin Worlds

Each plugin "world" defines what capabilities it imports and what logic it exports:

```wit
world dht22-plugin {
    import gpio-provider;
    import led-controller;
    import buzzer-controller;
    export dht22-logic;
}

world bme680-plugin {
    import gpio-provider;
    import led-controller;
    import buzzer-controller;
    import i2c;
    export bme680-logic;
}

world dashboard-plugin {
    export dashboard-logic;  // No imports - pure rendering
}
```

---

## The Host (Rust)

The host is written in Rust and serves as the secure "Operating System" that manages all hardware access and WASM plugin execution.

### File: [`host/src/main.rs`](file:///c:/Users/navra/Desktop/wasi-python-host/host/src/main.rs)

**Purpose**: Entry point, HTTP server, and main polling loop.

**Responsibilities**:
- Initializes the WASM runtime
- Loads configuration from TOML files
- Starts an **Axum** HTTP server on port 3000
- Runs the **polling loop** (configurable interval, default 2 seconds)

**Polling Loop Logic**:
```
1. Toggle LED 0 as heartbeat indicator (blue ↔ cyan)
2. Check for hot-reloaded plugins
3. Poll all sensors via WASM plugins
4. Add node_id prefix to sensor readings
5. If spoke: POST readings to Hub
6. Update shared state
```

**HTTP Endpoints**:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/` | GET | Dashboard HTML (rendered by WASM plugin) |
| `/api/readings` | GET | JSON sensor readings |
| `/api/logs` | GET | Combined host + WASM plugin logs |
| `/api/buzzer` | POST | Control buzzer (forwards to spoke if hub) |
| `/api/buzzer/test` | POST | Manual 3-beep test |
| `/push` | POST | Hub receives data from spokes |

---

### File: [`host/src/runtime.rs`](file:///c:/Users/navra/Desktop/wasi-python-host/host/src/runtime.rs)

**Purpose**: WASM plugin loading, instantiation, and execution using **Wasmtime**.

**Key Components**:

1. **Bindgen Macros**: Generate Rust bindings from WIT:
```rust
mod dht22_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "dht22-plugin",
        async: true,
    });
}
```

2. **HostState**: Holds WASI context + config, implements all WIT interfaces:
```rust
pub struct HostState {
    ctx: WasiCtx,
    table: ResourceTable,
    pub config: HostConfig,
}
```

3. **Trait Implementations**: Each WIT interface is implemented:
   - `gpio_provider::Host` → Calls HAL for DHT22, CPU temp
   - `led_controller::Host` → Calls HAL for LED control
   - `buzzer_controller::Host` → Controls relay via GPIO
   - `i2c::Host` → Raw I2C transfers via HAL

4. **WasmRuntime Struct**: Manages all plugin instances:
```rust
pub struct WasmRuntime {
    engine: Engine,
    config: HostConfig,
    dht22_plugin: Arc<Mutex<Option<PluginState<Dht22Plugin>>>>,
    bme680_plugin: Arc<Mutex<Option<PluginState<Bme680Plugin>>>>,
    pi4_monitor_plugin: Arc<Mutex<Option<PluginState<Pi4MonitorPlugin>>>>,
    revpi_monitor_plugin: Arc<Mutex<Option<PluginState<RevpiMonitorPlugin>>>>,
    dashboard_plugin: Arc<Mutex<Option<PluginState<DashboardPlugin>>>>,
    oled_plugin: Arc<Mutex<Option<PluginState<OledPlugin>>>>,
}
```

5. **Key Methods**:
   - `new()` → Creates engine, loads enabled plugins from config
   - `poll_sensors()` → Calls each plugin's `poll()` function
   - `render_dashboard()` → Calls dashboard plugin's `render()`
   - `check_hot_reload()` → Detects modified WASM files

---

### File: [`host/src/config.rs`](file:///c:/Users/navra/Desktop/wasi-python-host/host/src/config.rs)

**Purpose**: TOML configuration schema definition and loading.

**Structure**:
```rust
HostConfig
├── polling: PollingConfig           // interval_seconds
├── sensors: SensorsConfig           // DHT22 GPIO pin, BME680 I2C address
│   ├── dht22: Dht22Config
│   └── bme680: Bme680Config
├── leds: LedConfig                  // count (11), GPIO pin (18), brightness
├── buzzer: BuzzerConfig             // GPIO pin (17)
├── logging: LoggingConfig           // level, show_sensor_data
├── cluster: ClusterConfig           // role, node_id, hub_url, spoke_buzzer_url
└── plugins: PluginsConfig           // enabled flags for each plugin
    ├── dht22: PluginEntry
    ├── bme680: PluginEntry
    ├── pi4_monitor: PluginEntry
    ├── revpi_monitor: PluginEntry
    ├── dashboard: PluginEntry
    └── oled: PluginEntry
```

**Loading Priority**: 
1. `config/host.toml`
2. `../config/host.toml`
3. Programmatic defaults

---

### File: [`host/src/domain.rs`](file:///c:/Users/navra/Desktop/wasi-python-host/host/src/domain.rs)

**Purpose**: Shared state and sensor reading types.

```rust
/// Current sensor readings shared state
pub struct AppState {
    pub readings: Vec<SensorReading>,  // All sensor data from all nodes
    pub last_update: u64,              // Unix timestamp (ms)
}

/// A generic sensor reading with flexible JSON payload
pub struct SensorReading {
    pub sensor_id: String,             // e.g., "pi4:dht22-gpio4"
    pub timestamp_ms: u64,
    pub data: serde_json::Value,       // Flexible JSON payload
}
```

**Design Decision**: Using `serde_json::Value` for data allows any sensor to return arbitrary key-value pairs without schema changes.

---

### File: [`host/src/hal.rs`](file:///c:/Users/navra/Desktop/wasi-python-host/host/src/hal.rs)

**Purpose**: Hardware Abstraction Layer - unified interface for hardware access.

**Key Trait**:
```rust
pub trait HardwareProvider: Send + Sync {
    fn i2c_transfer(&self, addr: u8, write_data: &[u8], read_len: u32) -> Result<Vec<u8>>;
    fn spi_transfer(&self, data: &[u8]) -> Result<Vec<u8>>;
    fn set_gpio_mode(&self, pin: u8, mode: &str) -> Result<()>;
    fn write_gpio(&self, pin: u8, level: bool) -> Result<()>;
    fn set_led(&self, index: u8, r: u8, g: u8, b: u8) -> Result<()>;
    fn sync_leds(&self) -> Result<()>;
    fn read_dht22(&self, pin: u8) -> Result<(f32, f32)>;
    fn get_cpu_temp(&self) -> f32;
    fn buzz(&self, pin: u8, pattern: &str) -> Result<()>;
}
```

**Conditional Compilation**:
- `#[cfg(not(feature = "hardware"))]` → Mock implementation returns dummy data
- `#[cfg(feature = "hardware")]` → Real implementation using **rppal** library

**Python Subprocess Usage**: DHT22, LEDs, and buzzer use Python subprocess for timing-critical operations since pure Rust bit-banging is unreliable on Linux without a kernel driver.

---

### File: [`host/src/gpio.rs`](file:///c:/Users/navra/Desktop/wasi-python-host/host/src/gpio.rs)

**Purpose**: Extended hardware capability provider with LED buffering and additional sensor functions.

**Key Features**:

1. **LED Buffering (Flicker Prevention)**:
```rust
static LED_BUFFER: OnceLock<Mutex<[(u8, u8, u8); 11]>> = OnceLock::new();

pub fn set_led_buffer(index: u8, r: u8, g: u8, b: u8) {
    // Updates buffer only - no hardware write
}

pub fn sync_leds() {
    // Writes entire buffer to hardware in one Python call
}
```

2. **Buzzer Control**:
   - Active-low relay logic (GPIO LOW = relay on = buzzer sounds)
   - Patterns: single (100ms), triple (3×100ms), long (500ms)

3. **Generic HAL Functions**:
   - `i2c_transfer()` → Uses rppal for raw I2C with hex string encoding
   - `spi_transfer()` → Full-duplex SPI
   - `uart_read/write()` → Serial communication

---

## Configuration System

### [`config/hub.toml`](file:///c:/Users/navra/Desktop/wasi-python-host/config/hub.toml) (RevPi - 192.168.7.10)

```toml
[cluster]
role = "hub"
node_id = "revpi-hub"
hub_url = ""  # Hub doesn't push anywhere
spoke_buzzer_url = "http://192.168.7.11:3000/api/buzzer"  # Forward buzzer to Pi4

[polling]
interval_seconds = 2

[plugins]
dht22.enabled = false      # No sensors on Hub
revpi_monitor.enabled = true
bme680.enabled = false
dashboard.enabled = true   # Hub serves the UI
```

### [`config/spoke.toml`](file:///c:/Users/navra/Desktop/wasi-python-host/config/spoke.toml) (Pi4 - 192.168.7.11)

```toml
[cluster]
role = "spoke"
hub_url = "http://192.168.7.10:3000/push"  # Push data to Hub
node_id = "pi4-spoke"

[plugins]
dht22.enabled = true       # Has DHT22 sensor
bme680.enabled = true      # Has BME680 sensor
pi4_monitor.enabled = true
dashboard.enabled = false  # No UI on spoke
```

### [`config/pizero.toml`](file:///c:/Users/navra/Desktop/wasi-python-host/config/pizero.toml) (Pi Zero - 192.168.7.12)

```toml
[cluster]
role = "spoke"
hub_url = "http://192.168.7.10:3000/push"
node_id = "pizero-failsafe-spoke"

[plugins]
dht22.enabled = false      # Too resource-intensive
pi4_monitor.enabled = false
bme680.enabled = true      # Passive mode (via native service)
dashboard.enabled = false
```

> **Note**: Pi Zero cannot run WASM efficiently (416MB RAM), so it uses the native Python service instead.

---

## Plugins

All plugins are Python code compiled to WASM using `componentize-py`.

### [`plugins/dht22/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/dht22/app.py) - Room Temperature/Humidity

**Imports**: `gpio_provider`, `led_controller`, `buzzer_controller`  
**Exports**: `Dht22Logic.poll()`

**Behavior**:
- Reads from GPIO pin 4 via host's `read_dht22()` function
- Uses **hysteresis (deadband)** to prevent alarm flickering
- Controls **LED 1**:

| Condition | LED Color | Action |
|-----------|-----------|--------|
| Temp ≥ 30°C | 🔴 Red | Buzzer: 3 beeps |
| Temp ≤ 15°C | 🔵 Blue | - |
| Temp > 25°C | 🟠 Orange | - |
| Normal | 🟢 Green | - |

**Thresholds**:
```python
HIGH_TEMP = 30.0
LOW_TEMP = 15.0
DEADBAND = 2.0  # Hysteresis band
HIGH_HUM = 70.0
LOW_HUM = 25.0
```

---

### [`plugins/bme680/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/bme680/app.py) - Environmental/Air Quality Sensor

**Imports**: `gpio_provider`, `led_controller`, `buzzer_controller`, `i2c`  
**Exports**: `Bme680Logic.poll()`

**Key Features**:

1. **Pure Python I2C Driver**: Uses generic `i2c.transfer()` to communicate directly with the BME680 chip - no host-specific sensor code required.

2. **Calibration Loading**: Reads chip-specific calibration constants from registers (0xE9, 0x8A, 0xE1-0xE8) for accurate temperature and humidity compensation per Bosch datasheet.

3. **IAQ Algorithm**:
   - **Burn-in Period**: ~60 seconds (12 readings × ~5 seconds)
   - **Gas Baseline Tracking**: Higher resistance = cleaner air
   - **IAQ Score Calculation**:
     - Gas contribution: 75% (based on ratio to baseline)
     - Humidity deviation: 25% (40% is ideal)
     - Scale: 0-500 (lower is better)

4. **LED 2 Control**:

| IAQ Score | Status | LED Color |
|-----------|--------|-----------|
| 0 | Calibrating | 🟣 Purple (pulsing) |
| 1-50 | Excellent | 🟢 Green |
| 51-100 | Good | 🟢 Green-ish |
| 101-150 | Moderate | 🟡 Yellow |
| 151-200 | Poor | 🟠 Orange |
| >200 | Bad | 🔴 Red + Buzzer |

---

### [`plugins/pi4-monitor/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/pi4-monitor/app.py) - Pi4 System Health

**Imports**: `gpio_provider`, `led_controller`, `system_info`, `buzzer_controller`  
**Exports**: `PiMonitorLogic.poll()`

**Returns**: `PiStats` with cpu_temp, cpu_usage, memory_used_mb, memory_total_mb, uptime_seconds

**LED 3 Control**:

| CPU Temp | LED Color | Action |
|----------|-----------|--------|
| > 75°C | 🔴 Red | Buzzer: 2 beeps |
| > 60°C | 🟠 Orange | - |
| Normal | 🟢 Green | - |

---

### [`plugins/revpi-monitor/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/revpi-monitor/app.py) - RevPi Hub Health

Same as pi4-monitor but controls **LED 0** for Hub status.

---

### [`plugins/pizero-monitor/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/pizero-monitor/app.py) - Lightweight Pi Zero Monitor

**Minimal implementation** - No LED/buzzer control to conserve resources. Just reports stats back to Hub.

```python
# Minimal logging, no LED/buzzer control
print(f"📊 [PIZERO] {cpu_temp:.1f}°C | RAM: {used_mb}/{total_mb}MB")
```

---

### [`plugins/dashboard/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/dashboard/app.py) - Web Dashboard

**Exports**: `DashboardLogic.render(sensor_data: str) -> str`

**Features**:
- Parses JSON sensor data passed from host
- Renders complete HTML page with CSS (JetBrains Mono terminal aesthetic)
- **5 Sensor Cards**: DHT22, BME680, RevPi Hub, Pi4 Spoke, PiZero
- **Network Ping Display**: Shows latency from PiZero to Hub and Pi4
- **Buzzer Controls**: BEEP, BEEP x3, LONG buttons
- **Log Viewer**: Tabs for Hub/Pi4/PiZero logs
- **Live Updates**: JavaScript fetches `/api/readings` every 3 seconds

---

### [`plugins/oled/app.py`](file:///c:/Users/navra/Desktop/wasi-python-host/plugins/oled/app.py) - SSD1306 OLED Display

**Imports**: `i2c`  
**Exports**: `OledLogic.update(sensor_data: string)`

**Pure Python SSD1306 driver** using generic I2C. Displays:
- Line 1: Room temperature
- Line 2: IAQ score
- Line 3: Hub CPU temp
- Line 4: "HARVESTER OS"

---

## Native Pi Zero Service

**Location**: [`pizero-native/pizero_service.py`](file:///c:/Users/navra/Desktop/wasi-python-host/pizero-native/pizero_service.py)

### Why Native?

The Pi Zero has only **416MB RAM**. The WASM runtime (Wasmtime + Python WASM) consumes **300MB+**, leaving insufficient room for the OS and other processes.

**Solution**: Run a native Python service that participates in the cluster without WASM overhead.

### Features

1. **BME680 Reading**: Direct I2C via `smbus2` library
2. **CPU Temperature**: Reads `/sys/class/thermal/thermal_zone0/temp`
3. **Memory Stats**: Parses `/proc/meminfo`
4. **Network Health Monitoring**: Pings Hub and Pi4, reports latency
5. **Log Server**: HTTP server on port 3000 for dashboard integration
6. **Data Push**: POSTs readings to Hub every 5 seconds

### API Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/logs` | GET | Returns buffered log messages |
| `/health` | GET | Returns `{"status": "ok"}` |

### IAQ Calculation (Passive Mode)

The native service uses a simplified IAQ algorithm without active calibration:

```python
if gas >= 100:    iaq = 50   # Excellent
elif gas >= 50:   iaq = 100  # Good
elif gas >= 20:   iaq = 150  # Moderate
elif gas >= 10:   iaq = 200  # Poor
else:             iaq = 300  # Bad
```

---

## Data Flow

### Complete Polling Cycle (Every 2 seconds)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           POLLING CYCLE                                  │
└─────────────────────────────────────────────────────────────────────────┘

PI4 (SPOKE):
  1. runtime.poll_sensors()
     ├── dht22_plugin.poll()
     │   ├── gpio_provider.read_dht22(4)  → Host calls Python subprocess
     │   ├── led_controller.set_led(1, r, g, b)
     │   └── Returns Dht22Reading
     │
     ├── bme680_plugin.poll()
     │   ├── i2c.transfer(0x77, "...", n)  → Host calls rppal
     │   ├── Python driver calculates temp/hum/pressure/gas/IAQ
     │   ├── led_controller.set_led(2, r, g, b)
     │   └── Returns Bme680Reading
     │
     └── pi4_monitor_plugin.poll()
         ├── gpio_provider.get_cpu_temp()
         ├── system_info.get_memory_usage()
         ├── led_controller.set_led(3, r, g, b)
         └── Returns PiStats

  2. Add node_id prefix: "pi4:dht22-gpio4", "pi4:bme680-i2c"

  3. POST to http://192.168.7.10:3000/push

─────────────────────────────────────────────────────────────────────────

HUB (REVPI):
  1. runtime.poll_sensors()
     └── revpi_monitor_plugin.poll()
         ├── gpio_provider.get_cpu_temp()
         ├── system_info.get_memory_usage()
         ├── led_controller.set_led(0, r, g, b)
         └── Returns PiStats

  2. Receives pushed data from Pi4 via /push endpoint
     └── Merges into AppState.readings

  3. Receives pushed data from PiZero via /push endpoint
     └── Merges into AppState.readings

  4. On GET /
     ├── Serializes AppState to JSON
     ├── Calls dashboard_plugin.render(json_data)
     └── Returns HTML

─────────────────────────────────────────────────────────────────────────

PIZERO (NATIVE):
  1. bme.read()  → Direct I2C via smbus2

  2. get_cpu_temp()  → Read /sys/class/thermal

  3. ping_host()  → Ping Hub and Pi4

  4. POST readings to http://192.168.7.10:3000/push
```

---

## Key Architectural Decisions

### 1. Generic HAL (Phase 3 Architecture)

The BME680 plugin uses `i2c.transfer()` instead of a sensor-specific host function like `read_bme680()`.

**Benefits**:
- **Compile Once, Run Anywhere**: Same WASM binary works on any node with I2C
- **No Host Recompilation**: Add new I2C sensors by writing Python plugins only
- **Driver Logic in Plugin**: Sensor-specific math lives in sandboxed Python

### 2. LED Buffering (Flicker Prevention)

Multiple plugins control different LEDs. Without buffering, each plugin would reset the entire strip.

**Solution**: 
- Global buffer holds colors for all 11 LEDs
- `set_led()` updates buffer only
- `sync_leds()` writes entire buffer to hardware once per poll cycle

### 3. Hub-Spoke Model

Centralized data aggregation means:
- Dashboard queries single endpoint for all node data
- Spokes are stateless - they just push readings
- Hub handles all web UI and log aggregation

### 4. Hybrid WASM/Native Architecture

Pi Zero can't run WASM efficiently, so it runs native Python instead. This demonstrates that the architecture can gracefully degrade for resource-constrained nodes while still participating in the cluster.

### 5. Hot Reload

Plugins can be updated without restarting the host:
1. Rebuild plugin: `componentize-py ... -o plugin.wasm`
2. Copy to node
3. Runtime detects modified timestamp and reloads

### 6. Active-Low Relay Logic

The Sainsmart relay triggers when GPIO goes LOW, not HIGH. This is abstracted in the host so plugins simply call `buzz(duration)` without knowing hardware details.

### 7. Python Subprocess for Timing-Critical Operations

DHT22, WS2812B LEDs, and buzzer control use Python subprocess calls because:
- Pure Rust bit-banging is unreliable on generic Linux kernels
- Adafruit libraries are battle-tested
- The latency is acceptable for our polling interval

---

## Hardware Summary

| Component | GPIO/Interface | Library/Driver |
|-----------|----------------|----------------|
| DHT22 Sensor | GPIO 4 | adafruit_dht (Python subprocess) |
| BME680 Sensor | I2C 0x77 | Pure Python driver via generic HAL |
| WS2812B LEDs | GPIO 18 | rpi_ws281x (Python subprocess) |
| Buzzer Relay | GPIO 17 | RPi.GPIO (Python subprocess) |
| SSD1306 OLED | I2C 0x3C | Pure Python driver via generic HAL |

---

## LED Assignment

| LED | Controller | Purpose |
|-----|------------|---------|
| LED 0 | RevPi Monitor / Host Heartbeat | Hub CPU status / heartbeat blink |
| LED 1 | DHT22 Plugin | Room temperature status |
| LED 2 | BME680 Plugin | Air quality (IAQ) status |
| LED 3 | Pi4 Monitor Plugin | Pi4 CPU temperature status |
| LED 4-10 | Unused | Available for future plugins |

---

## Version History

| Version | Changes |
|---------|---------|
| 2.3.11 | Unified Log Access, real-time unbuffered log streaming |
| 2.3.x | BME680 datasheet math bug fixes, 3-minute calibration period |
| 2.2.x | Hybrid WASM/Native failsafe architecture |
| 2.0.x | Hub-Spoke model, multi-node dashboard |
| 1.x | Initial WASI Component Model implementation |
