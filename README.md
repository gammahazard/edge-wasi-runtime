# WASI Python Host

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![Python](https://img.shields.io/badge/python-3670A0?style=for-the-badge&logo=python&logoColor=ffdd54)
![WebAssembly](https://img.shields.io/badge/webassembly-%23654FF0.svg?style=for-the-badge&logo=webassembly&logoColor=white)
![Raspberry Pi](https://img.shields.io/badge/-Raspberry_Pi-C51A4A?style=for-the-badge&logo=Raspberry-Pi)
![License](https://img.shields.io/badge/license-MIT-green?style=for-the-badge)

![Dashboard Preview](screenshots/dashboard-full.png)

A reference implementation demonstrating **Python WASM modules** reading **real DHT22 sensor data** on a Raspberry Pi, using the **WASI Component Model** with a Rust host.

## The Key Demonstration

This project shows the **WASI capability model** in action:

```
┌──────────────────────────────────────────────────────────────┐
│                    Rust Host (Wasmtime)                       │
│                                                              │
│   ┌──────────────────────────────────────────────────────┐   │
│   │  Host Capabilities (Rust Implements)                 │   │
│   │  • gpio-provider:                                    │   │
│   │      read_dht22(pin) → (temp, humidity)             │   │
│   │      get_cpu_temp() → celsius                        │   │
│   │      get_timestamp_ms() → unix timestamp             │   │
│   │  • led-controller:                                   │   │
│   │      set_led(index, r, g, b)                         │   │
│   │      set_all(r, g, b)                                │   │
│   │      set_two(led0_rgb, led1_rgb) ← atomic update    │   │
│   │  • buzzer-controller:                                │   │
│   │      buzz(duration_ms)                               │   │
│   │      beep(count, duration_ms, interval_ms)          │   │
│   └────────────────────────┬─────────────────────────────┘   │
│                            │                                  │
│   ┌────────────────────────┼─────────────────────────────┐   │
│   │  Wasmtime Sandbox      │                             │   │
│   │         ┌──────────────┴──────────────┐              │   │
│   │         │ Python WASM Plugin          │              │   │
│   │         │  • imports capabilities     │              │   │
│   │         │  • contains ALERT LOGIC     │              │   │
│   │         │  • hot-swappable!           │              │   │
│   │         └─────────────────────────────┘              │   │
│   └──────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────┘
                            ↓
    ┌───────────────┐  ┌───────────────┐  ┌───────────────┐
    │  DHT22 Sensor │  │  LED Strip    │  │  Buzzer       │
    │  (GPIO 4)     │  │  (GPIO 18)    │  │  (GPIO 17)    │
    └───────────────┘  └───────────────┘  └───────────────┘
```

**The sandboxed Python WASM plugin CANNOT directly access GPIO.**  
It must call host capabilities which control access.  
This is the "landlord/tenant" security model that makes WASI powerful.

## 🎬 Showcase & Demo

This project demonstrates **Secure Hot-Swapping** on embedded hardware.

### 🏗️ System Architecture (Mermaid)
The following diagram illustrates how the Rust Host securely manages the Python plugins and hardware access:

```mermaid
graph TD
    subgraph Pi [Raspberry Pi Device]
        DHT22[DHT22 Sensor]
        GPIO[GPIO Pins]
    end

    subgraph Host [Rust Host Process]
        Runtime[WasmRuntime]
        Linker[Wasmtime Linker]
        
        subgraph Caps [Host Capabilities]
            GPIO_Mod[GPIO Module]
            HTTP_Mod[HTTP Server]
        end
    end

    subgraph Guest [WASM Sandbox]
        subgraph Python1 [Sensor Plugin]
            SensorApp[app.py]
        end
        
        subgraph Python2 [Dashboard Plugin]
            DashboardApp[app.py]
        end
    end

    HTTP_Mod -->|Request| Runtime
    Runtime -->|render calls| DashboardApp
    DashboardApp -->|HTML| HTTP_Mod
    
    Runtime -->|poll calls| SensorApp
    SensorApp -->|read_dht22| Linker
    Linker -->|Secure Call| GPIO_Mod
    GPIO_Mod -->|Subprocess| GPIO
    GPIO -->|Signal| DHT22

    style DHT22 fill:#ff6666,stroke:#333
    style GPIO fill:#ff6666,stroke:#333
    style SensorApp fill:#66ff66,stroke:#333,color:black
    style DashboardApp fill:#66bbff,stroke:#333,color:black
    style Host fill:#eee,stroke:#333
```

### 🎥 Live Demos
We have captured the following capabilities in action:

#### 1. Dashboard Hot Swap (Terminal/CRT Theme)
[![Watch Dashboard Demo](https://img.shields.io/badge/Watch_Visual_Update-Streamable-33ff33?style=for-the-badge&logo=streamable&logoColor=white)](https://streamable.com/klbojw)

#### 2. Manual Controls (Buzzer & CPU Temp)
Dashboard includes **CPU Temp** and **Buzzer buttons** (Beep/Long Beep) to show bi-directional control.

#### 3. Sensor Hot Swap (Logic Update)
[![Watch Sensor Demo](https://img.shields.io/badge/Watch_Logic_Update-Streamable-00aaff?style=for-the-badge&logo=streamable&logoColor=white)](https://streamable.com/cd3050)

> **Key Takeaway**: The Rust host acts as a stable "Operating System", dealing with GPIO and networking, while the Python guests provide flexible, hot-reloadable "User Space" logic.

## 🎨 User Experience Features

The dashboard demonstrates modern web UX patterns:

- **Live Updates**: Browser polls `/api` endpoint every 2 seconds via JavaScript `fetch()`
- **Smooth Animations**: Temperature and humidity values fade out → update → fade in (300ms total)
- **No Page Refresh**: Only the numbers update, eliminating jarring full-page reloads
- **Terminal Aesthetic**: Retro CRT theme with green-on-black colors and scanline effects
- **Responsive Design**: Mobile-friendly layout with CSS Grid

**API Endpoint:**
```bash
curl http://raspberry-pi:3000/api
```

**Response:**
```json
{
  "readings": [
    {
      "temperature": 21.5,
      "humidity": 40.9,
      "timestamp_ms": 1737073200000
    }
  ],
  "last_update": 1737073200000
}
```

This `/api` endpoint enables:
- Programmatic access to sensor data
- Third-party integrations (Home Assistant, Grafana, etc.)
- Custom dashboards and mobile apps

## 🔌 Hardware Requirements

To run this demo, you need:
1.  **Raspberry Pi** (3, 4, or 5)
2.  **DHT22 Temperature/Humidity Sensor**
3.  **Wiring**:
    *   **VCC (+) [Pin 1]** → 3.3V (Physical Pin 1)
    *   **Data [Pin 2]** → **GPIO 4 (Physical Pin 7)**
    *   **GND (-) [Pin 4]** → Ground (Physical Pin 6)
    *   *Note: Most modules have a built-in pull-up resistor. If using a raw sensor, add a 4.7kΩ resistor between VCC and Data.*

> **Why GPIO 4?** The host is hardcoded to use BCM GPIO 4 (Physical Pin 7) for simplicity in this demo.

## 💡 Why This Architecture Matters

**Is this overengineering? No.**

If your *only* goal is to read a sensor, a 5-line Python script is better. But this project demonstrates an architecture for **Secure, Multi-Tenant Edge Computing**.

**Why use this architecture?**
1.  **Security Isolation**: The Python code runs in a sandbox. If you download a plugin from the internet, it physically cannot hack your network or access files unless you explicitly grant that capability in `plugin.wit`.
2.  **Resilience**: If the Python plugin crashes or hangs (e.g., bad sensor read), the Rust host survives. We implemented a "Dead Man's Switch" in `gpio.rs` so a stuck plugin never freezes the device.
3.  **Hot Swapping**: You can update the business logic (Python) over-the-air without rebooting the system or dropping active network connections.
4.  **Polyglot**: You can mix and match languages. One plugin can be Python (for data science), another Rust (for speed), another JavaScript.

## 🏗️ Migration Strategy (How to use this as a model)

**"I have a massive Python legacy codebase. How do I use this?"**

Don't rewrite everything at once. Use the **Strangler Fig Pattern**:

1.  **Identify the Core**: Find the one piece of logic you change most often (e.g., "Business Logic" or "Data Formatting").
2.  **Move just THAT to WASM**: Keep your hardware drivers in Python on the host (or migrate them to Rust later), but run the changing logic in a sandbox.
3.  **Define the Interface**: Write a `.wit` file that describes what that one piece of logic needs (inputs/outputs).
4.  **Swap it out**: Replace the Python function call with a call to the WASM module.

**Why is this better?**
*   Legacy code runs as usual.
*   New/Risky code runs in a sandbox.
*   If the new code crashes, it's caught by the host. It doesn't crash the whole robot/server.

| Feature | Raw Python Script | This WASI Host |
| :--- | :--- | :--- |
| **Simplicity** | High (Easy) | Medium (Requires Build) |
| **Security** | None (Full Access) | **Sandboxed (Capability Model)** |
| **Isolation** | Process Level | **WASM Level (Micro-VM)** |
| **Hot Reload** | Restart Process | **Instant (No Restart)** |
| **Stability** | Crash = Downtime | **Crash = Log Error & Retry** |

## Quick Start (On Raspberry Pi)

### Prerequisites

- Raspberry Pi with DHT22 sensor on GPIO pin 4
- Rust toolchain installed (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Python 3.11+ with adafruit_dht (`pip3 install adafruit-circuitpython-dht`)
- componentize-py (`pip3 install componentize-py`)

### Build & Run

```bash
# 1. Build Python plugins to WASM
cd plugins/sensor
componentize-py -d ../../wit -w sensor-plugin componentize app -o sensor.wasm

cd ../dashboard
componentize-py -d ../../wit -w dashboard-plugin componentize app -o dashboard.wasm

# 2. Build and run the Rust host
cd ../../host
cargo run --release

# 3. Open http://raspberry-pi-ip:3000 in browser
```

You'll see **real temperature and humidity** from your DHT22 sensor!

## 📜 The WIT "Constitution"

The `wit/plugin.wit` file is the most important file in this project. It is not just code; it is the **Security Contract**.

```wit
// The HOST provides this to the sandboxed WASM plugin
interface gpio-provider {
    read-dht22: func(pin: u8) -> result<tuple<f32, f32>, string>;
    get-timestamp-ms: func() -> u64;
}

// The PLUGIN implements this, host calls it
interface sensor-logic {
    poll: func() -> list<sensor-reading>;
}
```

**Why highlight this?**
*   **Deny by Default**: If a function isn't in this file, the Python guest *literally cannot call it*. There is no `import os`, no `subprocess`, no network.
*   **Type Safety**: The host guarantees it triggers `poll()`, and the guest guarantees it returns a `list<sensor-reading>`. No JSON parsing errors at runtime.

## How It Works

### 1. Python Calls the Host Capability

```python
# app.py (runs in WASM sandbox)
from wit_world.imports import gpio_provider

class SensorLogic(SensorLogic):
    def poll(self) -> list[SensorReading]:
        # This calls the RUST HOST which reads the actual hardware
        temperature, humidity = gpio_provider.read_dht22(4)
        
        return [SensorReading(
            sensor_id="dht22-gpio4",
            temperature=temperature,
            humidity=humidity,
            timestamp_ms=gpio_provider.get_timestamp_ms(),
        )]
```

### 2. Rust Implements the Capability

```rust
// The host provides this implementation
impl gpio_provider::Host for HostState {
    async fn read_dht22(&mut self, pin: u8) -> Result<(f32, f32), String> {
        // Offloaded to blocking thread for responsiveness AND reliability
        tokio::task::spawn_blocking(move || {
            gpio::read_dht22(pin)
        }).await ...
    }
}
```

## Industry Context

This architecture is used in production by:

| Company | Use Case |
|---------|----------|
| **Fermyon Spin** | Serverless Python with capability-based security |
| **wasmCloud** | Distributed IoT/edge actors with sandboxed plugins |
| **Shopify** | Sandboxed merchant scripts |
| **Siemens** | IoT edge computing with isolated sensor modules |

## Hot Reload Demo

Edit the Python plugin, rebuild WASM, and see changes without restarting the host:

```bash
# Terminal 1: Run host
cd host && cargo run --release

# Terminal 2: Edit and rebuild
vim ../plugins/sensor/app.py  # Make changes
componentize-py -d ../../wit -w sensor-plugin componentize app -o sensor.wasm

# The host detects the change and reloads automatically!
```

## License

MIT
