# Architecture Deep Dive: WASI Python Host

This document explains the internal mechanics of the **Stateful WASM Host**. It details how we achieve **secure plugin execution**, **hot reloading**, and **state persistence** for Python modules on embedded hardware.

## The Component Model (WIT)

The core is the **WebAssembly Component Model**. Components communicate via high-level, typed interfaces defined in WIT (Wasm Interface Type) files.

### The Contract (`plugin.wit`)
The `.wit` file is the source of truth. It defines:
1.  **Imports**: Capabilities the Host *gives* to the Guest (e.g., `gpio-provider`, `led-controller`).
2.  **Exports**: Functions the Guest *provides* to the Host (e.g., `sensor-logic.poll`, `bme680-logic.poll`).

### Current Plugins
| Plugin | World | Role | LED |
|--------|-------|------|-----|
| `dht22` | `dht22-plugin` | Reads DHT22, controls LED 1 (room temp) | 1 |
| `pi-monitor` | `pi-monitor-plugin` | Reads CPU temp, controls LED 0 (system health) | 0 |
| `bme680` | `bme680-plugin` | Reads BME680, calculates IAQ, controls LED 2 | 2 |
| `dashboard` | `dashboard-plugin` | Renders HTML dashboard (no hardware access) | - |

## Configuration (`config/host.toml`)

Runtime settings are externalized to a TOML file. This allows behavior changes without recompiling the Rust host:

```toml
[polling]
interval_seconds = 5

[sensors.dht22]
gpio_pin = 4

[sensors.bme680]
i2c_address = "0x77"

[logging]
level = "info"
show_sensor_data = true

[plugins.dht22]
enabled = true
led = 1

[plugins.pi_monitor]
enabled = true
led = 0
```

## Runtime Lifecycle

### 1. Initialization (`WasmRuntime::new`)
*   The Rust host loads `config/host.toml`.
*   It initializes a `wasmtime::Engine`.
*   It loads `.wasm` files from disk.
*   It creates a **Linker** and links the host's `HostState` (which implements capabilities).
*   It instantiates each plugin *once* and stores the `Store` and `Instance` in a `PluginState` struct.

### 2. State Persistence
*   The `Store` (memory) and `Instance` (execution context) are kept alive in `Arc<Mutex<PluginState>>`.
*   When `poll()` is called, we reuse the *existing* instance.
*   **Benefit**: Python global variables (e.g., `gas_baseline`, `high_alarm_active`) persist between poll cycles. This enables features like IAQ calibration and alarm hysteresis.

### 3. Hot Reloading (`check_hot_reload`)
Before every poll, the host checks the file modification time of each `.wasm` file.
*   **If changed**: The host loads the new WASM, creates a new Store/Instance, and replaces the old state.
*   **Result**: The next poll uses the new logic. State is reset.

## Data Flow (Single Poll Cycle)

1.  **Poll Loop** (Rust) triggers based on `config.polling.interval_seconds`.
2.  Calls `runtime.poll_sensors()` → DHT22 plugin.
3.  Calls `runtime.poll_bme680()` → BME680 plugin.
4.  Each plugin updates the **LED buffer** via `led_controller.set_led()`.
5.  **Host calls `gpio::sync_leds()`** once after all plugins finish → Single hardware write.
6.  Host updates shared `AppState` with all readings.
7.  Host logs sensor data if `show_sensor_data = true`.

## Concurrency Model

The host runs on **Tokio**, an async runtime for Rust. We use `tokio::sync::Mutex` for plugin state because standard `std::sync::Mutex` cannot be held across `.await` points.

Hardware I/O (DHT22, BME680, LEDs) is offloaded to blocking threads via `tokio::task::spawn_blocking()` to avoid blocking the async runtime.

## LED Buffer Architecture

To prevent flicker from multiple plugins updating LEDs:
1.  Plugins call `set_led()` / `set_two()` which updates an in-memory buffer only.
2.  After all plugins finish, the Host calls `sync_leds()` which writes the entire buffer to hardware once.
3.  This ensures atomic LED updates regardless of how many plugins are active.

## Security

*   **Sandboxing**: Python code cannot open files or sockets. It can *only* call functions imported via `plugin.wit`.
*   **Capability-Based**: Each plugin world declares exactly which host functions it can access. The dashboard plugin has no hardware access.
*   **Isolation**: If a Python plugin returns an error, the Host logs it and continues. The system doesn't crash.

