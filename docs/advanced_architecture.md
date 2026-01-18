# Architecture Deep Dive: WASI Python Host

This document explains the internal mechanics of the **Stateful WASM Host**. It details how we achieve **secure plugin execution**, **hot reloading**, and **state persistence** for Python modules on embedded hardware.

## The Component Model (WIT)

The core is the **WebAssembly Component Model**. Unlike traditional linking, components communicate via high-level, typed interfaces defined in WIT (Wasm Interface Type) files.

### The Contract (`plugin.wit`)
The `.wit` file is the source of truth. It defines:
1.  **Imports**: Capabilities the Host *gives* to the Guest (e.g., `gpio-provider`).
2.  **Exports**: Functions the Guest *provides* to the Host (e.g., `sensor-logic.poll`).

## Runtime Lifecycle

### 1. Initialization (`WasmRuntime::new`)
*   The Rust host initializes a `wasmtime::Engine`.
*   It loads `.wasm` files from disk.
*   It creates a **Linker** and links the host's `HostState` (which implements capabilities).
*   **Key Innovation**: It instantiates the Python interpreter inside WASM *once* and stores the `Store` and `Instance` in a `PluginState` struct.

### 2. State Persistence
In earlier versions, we re-instantiated the plugin every poll cycle. This was safe but stateless—Python global variables were reset every 5 seconds.

**Current Architecture (Stateful):**
*   The `Store` (memory) and `Instance` (execution context) are kept alive in `Arc<Mutex<PluginState>>`.
*   When `poll()` is called, we reuse the *existing* instance.
*   **Benefit**: Python global variables (e.g., `high_alarm_active`) persist forever (until reload). This enables sticky logic like **Alarm Hysteresis**.

### 3. Hot Reloading (`check_hot_reload`)
Before every poll, the host checks the file modification time of the `.wasm` file.
*   **If changed**:
    1.  The host locks the plugin mutex.
    2.  It loads the new `.wasm` bytes.
    3.  It creates a *new* `Store` and `Instance`.
    4.  It replaces the old `PluginState` with the new one.
*   **Result**: The next poll uses the new logic. The old state is dropped (garbage collected).

## Concurrency Model

The host runs on **Tokio**, an async runtime for Rust.

### The Mutex Challenge
Standard `std::sync::Mutex` locks cannot be held across `.await` points because they are not `Send` (they are tied to a specific OS thread).
Since our WASM execution matches the async nature of the host (potentially waiting on I/O), we use `tokio::sync::Mutex`.

*   **Flow**:
    ```rust
    // runtime.rs
    let mut guard = self.sensor_plugin.lock().await; // Async wait for lock
    let result = guard.instance.call_poll(...).await; // Run WASM
    ```
*   This ensures the web server and polling loop can access plugins concurrently without blocking the entire system.

## Data Flow

1.  **Poll Loop** (Rust) triggers every 5s.
2.  Calls `runtime.poll_sensors()`.
3.  Runtime enters WASM Sandbox (Python).
4.  Python calls `gpio_provider.read_dht22()`.
5.  Runtime exits WASM, executes Rust `gpio::read_dht22` (via `spawn_blocking` to avoid blocking async runtime).
6.  Rust returns tuple `(temp, hum)` to Python.
7.  Python processes logic (Hysteresis, coloring).
8.  Python calls `led_controller.set_two()`.
9.  Host updates physical LEDs.
10. Python returns `SensorReading` list to Host.
11. Host updates shared `AppState`.

## Security

*   **Sandboxing**: The Python code cannot open files or sockets. It can *only* call functions imported from `plugin.wit`.
*   **Resource Limits**: Wasmtime limits CPU and Memory usage per instance (configurable).
*   **Isolation**: If the Python plugin panics, it returns a Rust `Err`, which we catch and log. The Host continues running.
