# WASI Python Host - Architecture Guide
**"Compile Once, Run Anything"**

This document explains the complete architecture of the WASI Python Host system, including the Component Model, Hub/Spoke clustering, and future roadmap.

---

## 1. Overview

The Rust Host acts as an **Operating System**, providing raw hardware access (I2C, SPI, GPIO), while Python plugins act as **Drivers** and **Apps**. This separation enables:

- **Security**: Sandboxed WASM execution
- **Hot Reload**: Update logic without restarting the host
- **Extensibility**: Add sensors without recompiling Rust

---

## 2. The Component Model (WIT)

The core is the **WebAssembly Component Model**. Components communicate via high-level, typed interfaces defined in WIT (Wasm Interface Type) files.

### The Contract (`plugin.wit`)
The `.wit` file is the source of truth. It defines:
1.  **Imports**: Capabilities the Host *gives* to the Guest (e.g., `gpio-provider`, `led-controller`)
2.  **Exports**: Functions the Guest *provides* to the Host (e.g., `dht22-logic.poll`)

### Current Plugins
| Plugin | World | Role | LED |
|--------|-------|------|-----|
| `dht22` | `dht22-plugin` | Reads DHT22, controls LED 1 (room temp) | 1 |
| `pi-monitor` | `pi-monitor-plugin` | Reads CPU temp, controls LED 0 (system health) | 0 |
| `bme680` | `bme680-plugin` | Reads BME680, calculates IAQ, controls LED 2 | 2 |
| `dashboard` | `dashboard-plugin` | Renders HTML dashboard (no hardware access) | - |
| `oled` | `oled-plugin` | Drives SSD1306 via generic I2C | - |

---

## 3. Generic HAL Interfaces

### The Problem
Adding a new sensor previously required **recompiling the Host**. This violates the "Compile Once" vision.

### The Solution
We transform the Host into a "Generalist". It provides access to hardware buses, and plugins act as the "Drivers".

| Layer | Before (Specialist) | After (Generic HAL) |
|-------|---------------------|---------------------|
| **Plugin (Python)** | `host.read_bme680()` | `i2c.transfer(0x77, "D0", 1)` → `"61"` |
| **Interface (WIT)** | `read-bme680: func()` | `transfer: func(addr, hex-data, len)` |
| **Host (Rust)** | *Hardcoded BME680 logic* | *Blindly passes bytes to /dev/i2c-1* |

### Implemented Interfaces (Phase 3) ✅

```wit
interface i2c {
    // Uses hex strings due to componentize-py marshalling limitations
    transfer: func(addr: u8, write-data: string, read-len: u32) -> result<string, string>;
}

interface spi {
    transfer: func(data: list<u8>) -> result<list<u8>, string>;
}

interface uart {
    read: func(max-len: u32) -> result<list<u8>, string>;
    write: func(data: list<u8>) -> result<u32, string>;
    set-baud: func(rate: u32) -> result<tuple<>, string>;
}
```

> **Note**: The `i2c` interface uses **hex-encoded strings** (e.g., `"D0"` instead of `[0xD0]`) due to a componentize-py marshalling issue with `list<u8>` return types.

---

## 4. The "Hybrid" Compromise

### Critical Discovery
Some sensors have timing requirements beyond what WASM can provide:

| Sensor | Issue | Solution |
|--------|-------|----------|
| **DHT22** | Microsecond bit-banging | Keep host driver (`read_dht22`) |
| **WS2812B** | 400ns pulse timing | Keep host driver (Python subprocess) |
| **BME680 Gas** | 100ms delay between trigger/read | Keep host driver (`read_bme680`) |
| **BME680 Temp/Humidity** | No timing issues | ✅ Works with Generic I2C |

### Key Insight
**WASM cannot call `time.sleep()`**. Any sensor requiring microsecond timing or millisecond delays must remain as host-side drivers.

### Generic-Friendly Sensors (Verified)
| Sensor | Protocol | Status |
|--------|----------|--------|
| **SSD1306 OLED** | I2C | ✅ Implemented (plugins/oled) |
| **AHT20** | I2C | ✅ Should work |
| **BMP280** | I2C | ✅ Should work (no gas) |
| **APA102 / DotStar** | SPI (has clock) | ✅ Should work |

---

## 5. Hub/Spoke Architecture (Current)

The system supports distributed monitoring via HTTP-based data aggregation.

### Topology
```
┌──────────────────┐     POST /push      ┌──────────────────┐
│   RevPi Hub      │◄────────────────────│   Pi 4 Spoke     │
│   192.168.40.9   │                     │   192.168.40.4   │
│                  │                     │                  │
│ - Dashboard      │                     │ - DHT22 Sensor   │
│ - Data Aggregator│                     │ - BME680 Sensor  │
│ - Pi-Monitor     │                     │ - Pi-Monitor     │
│ - Port 3000      │                     │ - WS2812B LEDs   │
└──────────────────┘                     └──────────────────┘
```

### Configuration
**Hub (`config/hub.toml`)**:
```toml
[cluster]
role = "hub"
hub_url = ""
node_id = "revpi-hub"

[plugins.dht22]
enabled = false  # No sensors on hub
```

**Spoke (`config/spoke.toml`)**:
```toml
[cluster]
role = "spoke"
hub_url = "http://192.168.40.9:3000/push"
node_id = "pi4-node-1"

[plugins.dashboard]
enabled = false  # Spokes don't serve UI
```

---

## 6. Runtime Lifecycle

### Initialization (`WasmRuntime::new`)
1. Load `config/host.toml`
2. Initialize `wasmtime::Engine`
3. Load `.wasm` files from disk
4. Create **Linker** and link host capabilities
5. Instantiate each plugin and store in `PluginState`

### State Persistence
- The `Store` (memory) and `Instance` (execution context) are kept alive
- Python global variables persist between poll cycles
- Enables features like IAQ calibration and alarm hysteresis

### Data Flow (Single Poll Cycle)
1. Poll loop triggers based on `config.polling.interval_seconds`
2. Each plugin is polled: DHT22 → BME680 → Pi-Monitor
3. Plugins update LED buffer via `led_controller.set_led()`
4. Host calls `gpio::sync_leds()` once (prevents flicker)
5. Host updates shared `AppState` with all readings
6. If Spoke: POST readings to Hub
7. If Hub: Merge incoming readings with local state

---

## 7. LED Buffer Architecture

To prevent flicker from multiple plugins updating LEDs:
1. Plugins call `set_led()` which updates an in-memory buffer only
2. After all plugins finish, Host calls `sync_leds()` to write the entire buffer to hardware once
3. This ensures atomic LED updates regardless of how many plugins are active

---

## 8. Security Model

- **Sandboxing**: Python code cannot open files or sockets. It can *only* call functions imported via `plugin.wit`
- **Capability-Based**: Each plugin world declares exactly which host functions it can access
- **Isolation**: If a Python plugin returns an error, the Host logs it and continues

---

## 9. Future Work

### Phase 5: Permission System ⏳
Giving generic "Raw I/O" (I2C/GPIO) access is powerful but adds risk.

**Planned Architecture:**
```toml
# permission.toml
[plugins.oled]
allow_i2c = [0x3C]  # ✅ Allowed
allow_gpio = []     # ❌ Blocked
```

### Phase 6: Raft Consensus ⏳
The current Hub/Spoke uses simple HTTP push. Future plans include **Raft Consensus** for leader election and log replication among the sensor nodes.

**Planned Topology (4 Nodes):**

```
┌─────────────────────────────────────────────────────────────┐
│                    RAFT CLUSTER (3 Voters)                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │   Pi 4       │◄─►│ Pi Zero 2W A │◄─►│ Pi Zero 2W B │       │
│  │  (Leader)    │  │   (Voter)    │  │   (Voter)    │       │
│  │  Sensors     │  │   Sensors    │  │   Sensors    │       │
│  └──────┬───────┘  └──────────────┘  └──────────────┘       │
│         │                                                    │
└─────────┼────────────────────────────────────────────────────┘
          │ HTTP GET (replicated log)
          ▼
┌──────────────────┐
│   RevPi Hub      │  ← External Aggregator (NOT in Raft)
│   Dashboard      │
│   Port 3000      │
└──────────────────┘
```

**Why this design:**
- **3 voters = odd number** - No split-brain risk
- **RevPi stays simple** - Industrial device doesn't run consensus code
- **Pi 4 is natural leader** - Most powerful node in the cluster
- **Clean separation** - Raft handles distributed sensor collection, RevPi just reads and displays

### Phase 7: Dynamic Plugin Discovery ⏳
- Watch `plugins/` folder for new `.wasm` files
- Auto-load with permissions from toml
- No Host restart required

---

## 10. Lessons Learned

1. **componentize-py has marshalling issues** with `list<u8>` return types
   - Workaround: Use hex-encoded strings

2. **WASM cannot sleep()** - timing-critical operations must stay in host
   - This is a fundamental WASM limitation, not a bug

3. **Generic HAL still provides value** for:
   - Read-only sensors (temp, pressure, light)
   - Write-only devices (OLED displays, SPI LEDs)
   - Any device without strict timing requirements

4. **Hub/Spoke works for simple aggregation**
   - HTTP POST is reliable for 5-second poll intervals
   - Raft will be needed for real-time coordination
