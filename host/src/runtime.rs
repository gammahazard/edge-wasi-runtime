//! ==============================================================================
//! runtime.rs - WASM Component Model Runtime with GPIO/HAL Capabilities
//! ==============================================================================
//!
//! purpose:
//!     loads and executes WASM plugins using wasmtime. implements the WASI
//!     capability model where:
//!     - HOST provides hardware access (gpio, led, buzzer, i2c, system-info)
//!     - GUEST runs sandboxed sensor/UI logic (Python compiled to WASM)
//!     - KEY security boundary: plugins can only access granted capabilities
//!
//! plugins:
//!     - dht22: Room temperature/humidity sensor, controls LED 1
//!     - bme680: Environmental sensor (temp, humidity, pressure, gas/IAQ), LED 2
//!     - pi-monitor: System health (CPU temp, RAM, uptime), controls LED 0
//!     - dashboard: HTML rendering (no hardware access)
//!
//! phase 3 (generic hal):
//!     - Implements i2c::Host trait for generic I2C access (uses hex strings)
//!     - Enables "Compile Once" - new sensors via Python plugins only
//!
//! relationships:
//!     - used by: main.rs (creates runtime, polling loop)
//!     - reads: ../wit/plugin.wit (interface definitions)
//!     - implements: gpio-provider, led-controller, buzzer-controller, i2c, system-info
//!     - uses: hal.rs (actual hardware access via rppal)
//!     - loads: ../plugins/{dht22,bme680,pi-monitor,dashboard}/*.wasm
//!
//! ==============================================================================

// use crate::hal;
use crate::domain::SensorReading;

use anyhow::{Result, Context};
use crate::config::HostConfig;
use wasmtime::{
    component::{Component, Linker, ResourceTable},
    Config, Engine, Store,
};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};
use std::path::PathBuf;
use std::time::SystemTime;
use std::sync::Arc;
use tokio::sync::Mutex;

// ==============================================================================
// bindgen - generate rust bindings from wit
// ==============================================================================

mod dht22_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "dht22-plugin",
        async: true,
    });
}
use dht22_bindings::Dht22Plugin;

mod dashboard_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "dashboard-plugin",
        async: true,
    });
}
use dashboard_bindings::DashboardPlugin;

mod bme680_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "bme680-plugin",
        async: true,
    });
}
use bme680_bindings::Bme680Plugin;

mod pi4_monitor_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "pi4-monitor-plugin",
        async: true,
    });
}
use pi4_monitor_bindings::Pi4MonitorPlugin;

mod revpi_monitor_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "revpi-monitor-plugin",
        async: true,
    });
}
use revpi_monitor_bindings::RevpiMonitorPlugin;

mod oled_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "oled-plugin",
        async: true,
    });
}
use oled_bindings::OledPlugin;

// ==============================================================================
// host state - provides capabilities to wasm guests
// ==============================================================================

pub struct HostState {
    ctx: WasiCtx,
    table: ResourceTable,
    pub config: HostConfig,
}

impl WasiView for HostState {
    fn table(&mut self) -> &mut ResourceTable { &mut self.table }
    fn ctx(&mut self) -> &mut WasiCtx { &mut self.ctx }
}

// ==============================================================================
// gpio-provider implementation
// ==============================================================================
//
// NOTE: We use `crate::hal::Hal` which handles cross-platform logic (mock vs real).
// All hardware access is performed safely via a non-blocking HAL.
// As of the Standalone Harvester update, consensus logic is replaced by local 
// aggregation on the Hub.

impl dht22_bindings::demo::plugin::gpio_provider::Host for HostState {
    async fn read_dht22(&mut self, _pin: u8) -> Result<(f32, f32), String> {
        let pin = self.config.sensors.dht22.gpio_pin;
        let hal = crate::hal::Hal::new();
        tokio::task::spawn_blocking(move || {
            use crate::hal::HardwareProvider;
            hal.read_dht22(pin)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?
        .map_err(|e: anyhow::Error| e.to_string())
    }
    
    async fn get_timestamp_ms(&mut self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
    }
    
    async fn get_cpu_temp(&mut self) -> f32 {
         let hal = crate::hal::Hal::new();
         use crate::hal::HardwareProvider;
         hal.get_cpu_temp()
    }
    
    async fn read_bme680(&mut self, _i2c_addr: u8) -> Result<(f32, f32, f32, f32), String> {
        let i2c_addr_str = &self.config.sensors.bme680.i2c_address;
        let i2c_addr = if i2c_addr_str.starts_with("0x") {
            u8::from_str_radix(&i2c_addr_str[2..], 16).unwrap_or(0x77)
        } else {
            i2c_addr_str.parse().unwrap_or(0x77)
        };
        
        let hal = crate::hal::Hal::new();
        tokio::task::spawn_blocking(move || {
            use crate::hal::HardwareProvider;
             // Dummy implementation for now via HAL
             let _ = hal.i2c_transfer(i2c_addr, &[], 0); 
             Ok((20.0, 50.0, 1013.0, 100.0))
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?
        .map_err(|e: anyhow::Error| e.to_string())
    }
}

// ==============================================================================
// led-controller implementation
// ==============================================================================

impl dht22_bindings::demo::plugin::led_controller::Host for HostState {
    async fn set_led(&mut self, index: u8, r: u8, g: u8, b: u8) {
         use crate::hal::HardwareProvider;
         let hal = crate::hal::Hal::new();
         let _ = hal.set_led(index, r, g, b);
    }
    
    async fn set_all(&mut self, r: u8, g: u8, b: u8) {
        use crate::hal::HardwareProvider;
        let hal = crate::hal::Hal::new();
        for i in 0..11 {
            let _ = hal.set_led(i, r, g, b);
        }
    }
    
    async fn set_two(&mut self, r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
        use crate::hal::HardwareProvider;
        let hal = crate::hal::Hal::new();
        let _ = hal.set_led(0, r0, g0, b0);
        let _ = hal.set_led(1, r1, g1, b1);
    }
    
    async fn clear(&mut self) {
        use crate::hal::HardwareProvider;
        let hal = crate::hal::Hal::new();
        for i in 0..11 {
            let _ = hal.set_led(i, 0, 0, 0);
        }
    }

    async fn sync_leds(&mut self) {
        use crate::hal::HardwareProvider;
        let hal = crate::hal::Hal::new();
        let _ = hal.sync_leds();
    }
}

// ==============================================================================
// buzzer-controller implementation
// ==============================================================================

impl dht22_bindings::demo::plugin::buzzer_controller::Host for HostState {
    async fn buzz(&mut self, duration_ms: u32) {
        let pin = self.config.buzzer.gpio_pin;
        let hal = crate::hal::Hal::new();
        tokio::task::spawn_blocking(move || {
            use crate::hal::HardwareProvider;
            let _ = hal.set_gpio_mode(pin, "OUT");
            let _ = hal.write_gpio(pin, false); // Relay on (Low)
            std::thread::sleep(std::time::Duration::from_millis(duration_ms as u64));
            let _ = hal.write_gpio(pin, true);  // Relay off (High)
        }).await.ok();
    }
    
    async fn beep(&mut self, count: u8, duration_ms: u32, interval_ms: u32) {
        let pin = self.config.buzzer.gpio_pin;
        let hal = crate::hal::Hal::new();
        tokio::task::spawn_blocking(move || {
            use crate::hal::HardwareProvider;
            let _ = hal.set_gpio_mode(pin, "OUT");
            for _ in 0..count {
                let _ = hal.write_gpio(pin, false);
                std::thread::sleep(std::time::Duration::from_millis(duration_ms as u64));
                let _ = hal.write_gpio(pin, true);
                std::thread::sleep(std::time::Duration::from_millis(interval_ms as u64));
            }
        }).await.ok();
    }
}

// ==============================================================================
// pi4-monitor bindings 
// ==============================================================================

impl pi4_monitor_bindings::demo::plugin::gpio_provider::Host for HostState {
    async fn read_dht22(&mut self, pin: u8) -> Result<(f32, f32), String> {
       <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::read_dht22(self, pin).await
    }
    async fn get_timestamp_ms(&mut self) -> u64 {
        <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::get_timestamp_ms(self).await
    }
    async fn get_cpu_temp(&mut self) -> f32 {
        <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::get_cpu_temp(self).await
    }
    async fn read_bme680(&mut self, addr: u8) -> Result<(f32, f32, f32, f32), String> {
         <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::read_bme680(self, addr).await
    }
}

impl pi4_monitor_bindings::demo::plugin::led_controller::Host for HostState {
    async fn set_led(&mut self, index: u8, r: u8, g: u8, b: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_led(self, index, r, g, b).await
    }
    async fn set_all(&mut self, r: u8, g: u8, b: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_all(self, r, g, b).await
    }
    async fn set_two(&mut self, r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_two(self, r0, g0, b0, r1, g1, b1).await
    }
    async fn clear(&mut self) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::clear(self).await
    }
    async fn sync_leds(&mut self) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::sync_leds(self).await
    }
}

impl pi4_monitor_bindings::demo::plugin::buzzer_controller::Host for HostState {
    async fn buzz(&mut self, d: u32) {
         <Self as dht22_bindings::demo::plugin::buzzer_controller::Host>::buzz(self, d).await
    }
    async fn beep(&mut self, c: u8, d: u32, i: u32) {
         <Self as dht22_bindings::demo::plugin::buzzer_controller::Host>::beep(self, c, d, i).await
    }
}

// ==============================================================================
// Real system info helpers (read from /proc on Linux, fallback for other OS)
// ==============================================================================

fn get_real_memory_usage() -> (u32, u32) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let mut total: u32 = 0;
            let mut available: u32 = 0;
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    total = line.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) / 1024;
                } else if line.starts_with("MemAvailable:") {
                    available = line.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) / 1024;
                }
            }
            let used = total.saturating_sub(available);
            return (used, total);
        }
    }
    (0, 0)
}

fn get_real_cpu_usage() -> f32 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/loadavg") {
            // Returns 1-minute load average as percentage (rough approximation)
            if let Some(load) = content.split_whitespace().next() {
                if let Ok(val) = load.parse::<f32>() {
                    // Convert load average to rough percentage (assuming 4 cores)
                    return (val / 4.0 * 100.0).min(100.0);
                }
            }
        }
    }
    0.0
}

fn get_real_uptime() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
            if let Some(uptime_str) = content.split_whitespace().next() {
                if let Ok(uptime_secs) = uptime_str.parse::<f64>() {
                    return uptime_secs as u64;
                }
            }
        }
    }
    0
}

impl pi4_monitor_bindings::demo::plugin::system_info::Host for HostState {
    async fn get_memory_usage(&mut self) -> (u32, u32) {
        get_real_memory_usage()
    }
    async fn get_cpu_usage(&mut self) -> f32 {
        get_real_cpu_usage()
    }
    async fn get_uptime(&mut self) -> u64 {
        get_real_uptime()
    }
}

// ==============================================================================
// revpi-monitor bindings 
// ==============================================================================

impl revpi_monitor_bindings::demo::plugin::gpio_provider::Host for HostState {
    async fn read_dht22(&mut self, pin: u8) -> Result<(f32, f32), String> {
       <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::read_dht22(self, pin).await
    }
    async fn get_timestamp_ms(&mut self) -> u64 {
        <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::get_timestamp_ms(self).await
    }
    async fn get_cpu_temp(&mut self) -> f32 {
        <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::get_cpu_temp(self).await
    }
    async fn read_bme680(&mut self, addr: u8) -> Result<(f32, f32, f32, f32), String> {
         <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::read_bme680(self, addr).await
    }
}

impl revpi_monitor_bindings::demo::plugin::led_controller::Host for HostState {
    async fn set_led(&mut self, index: u8, r: u8, g: u8, b: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_led(self, index, r, g, b).await
    }
    async fn set_all(&mut self, r: u8, g: u8, b: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_all(self, r, g, b).await
    }
    async fn set_two(&mut self, r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_two(self, r0, g0, b0, r1, g1, b1).await
    }
    async fn clear(&mut self) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::clear(self).await
    }
    async fn sync_leds(&mut self) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::sync_leds(self).await
    }
}

impl revpi_monitor_bindings::demo::plugin::buzzer_controller::Host for HostState {
    async fn buzz(&mut self, d: u32) {
         <Self as dht22_bindings::demo::plugin::buzzer_controller::Host>::buzz(self, d).await
    }
    async fn beep(&mut self, c: u8, d: u32, i: u32) {
         <Self as dht22_bindings::demo::plugin::buzzer_controller::Host>::beep(self, c, d, i).await
    }
}

impl revpi_monitor_bindings::demo::plugin::system_info::Host for HostState {
    async fn get_memory_usage(&mut self) -> (u32, u32) {
        get_real_memory_usage()
    }
    async fn get_cpu_usage(&mut self) -> f32 {
        get_real_cpu_usage()
    }
    async fn get_uptime(&mut self) -> u64 {
        get_real_uptime()
    }
}


// ==============================================================================
// plugin metadata 
// ==============================================================================

pub struct PluginState<T> {
    #[allow(dead_code)]
    path: PathBuf,
    #[allow(dead_code)]
    last_modified: SystemTime,
    store: Store<HostState>,
    instance: T,
}

impl<T> PluginState<T> {
    #[allow(dead_code)]
    fn needs_reload(&self) -> bool {
        std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .map(|t| t > self.last_modified)
            .unwrap_or(false)
    }
}

// ==============================================================================
// Standalone Wasm Runtime
// ==============================================================================
//
// Handles loading, execution, and hot-reloading of WASM plugins.
// In this revision, the runtime is responsible for fulfilling all hardware
// capabilities for the sandboxed Guest plugins.

#[derive(Clone)]
pub struct WasmRuntime {
    #[allow(dead_code)]
    engine: Engine,
    #[allow(dead_code)]
    config: HostConfig,
    dht22_plugin: Arc<Mutex<Option<PluginState<Dht22Plugin>>>>,
    pi4_monitor_plugin: Arc<Mutex<Option<PluginState<Pi4MonitorPlugin>>>>,
    revpi_monitor_plugin: Arc<Mutex<Option<PluginState<RevpiMonitorPlugin>>>>,
    #[allow(dead_code)]
    dashboard_plugin: Arc<Mutex<Option<PluginState<DashboardPlugin>>>>,
    bme680_plugin: Arc<Mutex<Option<PluginState<Bme680Plugin>>>>,
    #[allow(dead_code)]
    oled_plugin: Arc<Mutex<Option<PluginState<OledPlugin>>>>,
}

impl WasmRuntime {
    pub async fn new(path: PathBuf, config: &HostConfig) -> Result<Self> {
        let mut wasm_config = Config::new();
        wasm_config.wasm_component_model(true);
        wasm_config.async_support(true);
        let engine = Engine::new(&wasm_config)?;

        let create_host_state = |conf: HostConfig, node_id: String| {
             let mut builder = WasiCtxBuilder::new();
             builder.inherit_stdio();
             
             // Set Environment Variables for Plugins
             builder.env("HARVESTER_NODE_ID", &node_id);
             if node_id.contains("pizero") {
                 builder.env("HARVESTER_PASSIVE", "1");
             }
             
             let wasi = builder.build();
             HostState { ctx: wasi, table: ResourceTable::new(), config: conf }
        };

        // 1. DHT22 Plugin
        let dht22_plugin = if config.plugins.dht22.enabled {
            println!("[DEBUG] Loading dht22 plugin...");
            let dht22_path = path.join("plugins/dht22/dht22.wasm");
            let dht22_component = Component::from_file(&engine, &dht22_path)
                .context("failed to load dht22.wasm")?;
            
            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            dht22_bindings::Dht22Plugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
            
            let mut store = Store::new(&engine, create_host_state(config.clone(), config.cluster.node_id.clone()));
            let dht22_instance = Dht22Plugin::instantiate_async(&mut store, &dht22_component, &linker).await
                .context("failed to instantiate dht22 plugin")?;
            
            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: dht22_path,
                store: store,
                instance: dht22_instance,
            })))
        } else {
            Arc::new(Mutex::new(None))
        };
        
        // 2a. Pi 4 Monitor Plugin
        let pi4_monitor_plugin = if config.plugins.pi4_monitor.enabled {
            println!("[DEBUG] Loading pi4-monitor plugin...");
            let path = path.join("plugins/pi4-monitor/pi4-monitor.wasm");
            let comp = Component::from_file(&engine, &path).context("failed to load pi4-monitor.wasm")?;
            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            pi4_monitor_bindings::Pi4MonitorPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
            let mut store = Store::new(&engine, create_host_state(config.clone(), config.cluster.node_id.clone()));
            let inst = Pi4MonitorPlugin::instantiate_async(&mut store, &comp, &linker).await?;
            Arc::new(Mutex::new(Some(PluginState { last_modified: SystemTime::now(), path, store, instance: inst })))
        } else {
            Arc::new(Mutex::new(None))
        };

        // 2b. RevPi Monitor Plugin
        let revpi_monitor_plugin = if config.plugins.revpi_monitor.enabled {
            println!("[DEBUG] Loading revpi-monitor plugin...");
            let path = path.join("plugins/revpi-monitor/revpi-monitor.wasm");
            let comp = Component::from_file(&engine, &path).context("failed to load revpi-monitor.wasm")?;
            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            revpi_monitor_bindings::RevpiMonitorPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
            let mut store = Store::new(&engine, create_host_state(config.clone(), config.cluster.node_id.clone()));
            let inst = RevpiMonitorPlugin::instantiate_async(&mut store, &comp, &linker).await?;
            Arc::new(Mutex::new(Some(PluginState { last_modified: SystemTime::now(), path, store, instance: inst })))
        } else {
            Arc::new(Mutex::new(None))
        };

        // 3. BME680 Plugin
        let bme680_plugin = if config.plugins.bme680.enabled {
            println!("[DEBUG] Loading bme680 plugin...");
            let bme680_path = path.join("plugins/bme680/bme680.wasm");
            let bme680_component = Component::from_file(&engine, &bme680_path)
                .context("failed to load bme680.wasm")?;
            
            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            bme680_bindings::Bme680Plugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
            
            let mut store = Store::new(&engine, create_host_state(config.clone(), config.cluster.node_id.clone()));
            let bme680_instance = Bme680Plugin::instantiate_async(&mut store, &bme680_component, &linker).await
                .context("failed to instantiate bme680 plugin")?;
            
            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: bme680_path,
                store: store,
                instance: bme680_instance,
            })))
        } else {
            Arc::new(Mutex::new(None))
        };

        // 4. Dashboard Plugin
        let dashboard_plugin = if config.plugins.dashboard.enabled {
            println!("[DEBUG] Loading dashboard plugin...");
            let path = path.join("plugins/dashboard/dashboard.wasm");
            let comp = Component::from_file(&engine, &path).context("failed to load dashboard.wasm")?;
            
            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            // Note: Dashboard only exports logic, no host imports needed in the linker
            
            let mut store = Store::new(&engine, create_host_state(config.clone(), config.cluster.node_id.clone()));
            let inst = DashboardPlugin::instantiate_async(&mut store, &comp, &linker).await?;
            Arc::new(Mutex::new(Some(PluginState { last_modified: SystemTime::now(), path, store, instance: inst })))
        } else {
            Arc::new(Mutex::new(None))
        };
        
        Ok(Self {
            engine,
            config: config.clone(),
            dht22_plugin,
            pi4_monitor_plugin,
            revpi_monitor_plugin,
            dashboard_plugin,
            bme680_plugin,
            oled_plugin: Arc::new(Mutex::new(None)),
        })
    }
    
    pub async fn check_hot_reload(&self) {
        // Since we have different types, we'll revert to individual checks to avoid type mismatch in a vector
        self.check_plugin_reload("dht22", self.dht22_plugin.clone()).await;
        self.check_plugin_reload_bme680("bme680", self.bme680_plugin.clone()).await;
        // ... etc
    }

    async fn check_plugin_reload<T>(&self, _name: &str, _plugin: Arc<Mutex<Option<PluginState<T>>>>) {
        // Placeholder or implement generic reload logic if possible
    }

    async fn check_plugin_reload_bme680(&self, _name: &str, _plugin: Arc<Mutex<Option<PluginState<Bme680Plugin>>>>) {
        // ...
    }
    
    pub async fn poll_sensors(&self) -> Result<Vec<SensorReading>> {
        let mut all_readings = Vec::new();

        // 1. Poll DHT22
        {
            let mut guard = self.dht22_plugin.lock().await;
            if let Some(plugin) = guard.as_mut() {
                if let Ok(readings) = plugin.instance.demo_plugin_dht22_logic().call_poll(&mut plugin.store).await {
                    all_readings.extend(readings.into_iter().map(|r| SensorReading {
                        sensor_id: r.sensor_id,
                        timestamp_ms: r.timestamp_ms,
                        data: serde_json::json!({ "temperature": r.temperature, "humidity": r.humidity }),
                    }));
                }
            }
        }

        // 2. Poll BME680
        {
            let mut guard = self.bme680_plugin.lock().await;
            if let Some(plugin) = guard.as_mut() {
                if let Ok(readings) = plugin.instance.demo_plugin_bme680_logic().call_poll(&mut plugin.store).await {
                    all_readings.extend(readings.into_iter().map(|r| SensorReading {
                        sensor_id: r.sensor_id,
                        timestamp_ms: r.timestamp_ms,
                        data: serde_json::json!({ 
                            "temperature": r.temperature, 
                            "humidity": r.humidity,
                            "pressure": r.pressure,
                            "gas_resistance": r.gas_resistance,
                            "iaq_score": r.iaq_score
                        }),
                    }));
                }
            }
        }

        // 3. Poll Pi Monitor (Pi4)
        {
            let mut guard = self.pi4_monitor_plugin.lock().await;
            if let Some(plugin) = guard.as_mut() {
                if let Ok(stats) = plugin.instance.demo_plugin_pi_monitor_logic().call_poll(&mut plugin.store).await {
                    all_readings.push(SensorReading {
                        sensor_id: "pi4-monitor".to_string(),
                        timestamp_ms: stats.timestamp_ms,
                        data: serde_json::json!({
                            "cpu_temp": stats.cpu_temp,
                            "cpu_usage": stats.cpu_usage,
                            "memory_used_mb": stats.memory_used_mb,
                            "memory_total_mb": stats.memory_total_mb,
                            "uptime_seconds": stats.uptime_seconds,
                        }),
                    });
                }
            }
        }

        // 4. Poll Pi Monitor (RevPi)
        {
            let mut guard = self.revpi_monitor_plugin.lock().await;
            if let Some(plugin) = guard.as_mut() {
                if let Ok(stats) = plugin.instance.demo_plugin_pi_monitor_logic().call_poll(&mut plugin.store).await {
                    all_readings.push(SensorReading {
                        sensor_id: "revpi-monitor".to_string(),
                        timestamp_ms: stats.timestamp_ms,
                        data: serde_json::json!({
                            "cpu_temp": stats.cpu_temp,
                            "cpu_usage": stats.cpu_usage,
                            "memory_used_mb": stats.memory_used_mb,
                            "memory_total_mb": stats.memory_total_mb,
                            "uptime_seconds": stats.uptime_seconds,
                        }),
                    });
                }
            }
        }

        Ok(all_readings)
    }
    
    pub async fn render_dashboard(&self, json_data: String) -> Result<String> {
        let mut guard = self.dashboard_plugin.lock().await;
        if let Some(plugin) = guard.as_mut() {
            plugin.instance.demo_plugin_dashboard_logic()
                .call_render(&mut plugin.store, &json_data).await
                .map_err(|e| anyhow::anyhow!("Dashboard render failed: {}", e))
        } else {
            Ok("<h1 style='color:red'>Dashboard Plugin Not Loaded</h1>".to_string())
        }
    }
}


// ==============================================================================
// bme680-plugin bindings 
// ==============================================================================

impl bme680_bindings::demo::plugin::gpio_provider::Host for HostState {
    async fn read_dht22(&mut self, pin: u8) -> Result<(f32, f32), String> {
       <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::read_dht22(self, pin).await
    }
    async fn get_timestamp_ms(&mut self) -> u64 {
        <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::get_timestamp_ms(self).await
    }
    async fn get_cpu_temp(&mut self) -> f32 {
        <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::get_cpu_temp(self).await
    }
    async fn read_bme680(&mut self, addr: u8) -> Result<(f32, f32, f32, f32), String> {
         <Self as dht22_bindings::demo::plugin::gpio_provider::Host>::read_bme680(self, addr).await
    }
}

impl bme680_bindings::demo::plugin::led_controller::Host for HostState {
    async fn set_led(&mut self, index: u8, r: u8, g: u8, b: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_led(self, index, r, g, b).await
    }
    async fn set_all(&mut self, r: u8, g: u8, b: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_all(self, r, g, b).await
    }
    async fn set_two(&mut self, r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::set_two(self, r0, g0, b0, r1, g1, b1).await
    }
    async fn clear(&mut self) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::clear(self).await
    }
    async fn sync_leds(&mut self) {
         <Self as dht22_bindings::demo::plugin::led_controller::Host>::sync_leds(self).await
    }
}

impl bme680_bindings::demo::plugin::buzzer_controller::Host for HostState {
    async fn buzz(&mut self, d: u32) {
         <Self as dht22_bindings::demo::plugin::buzzer_controller::Host>::buzz(self, d).await
    }
    async fn beep(&mut self, c: u8, d: u32, i: u32) {
         <Self as dht22_bindings::demo::plugin::buzzer_controller::Host>::beep(self, c, d, i).await
    }
}

impl bme680_bindings::demo::plugin::i2c::Host for HostState {
    async fn transfer(&mut self, addr: u8, write_data: String, read_len: u32) -> Result<String, String> {
        let hal = crate::hal::Hal::new();
        use crate::hal::HardwareProvider;
        let data = hex::decode(write_data).map_err(|e| e.to_string())?;
        
        let result = tokio::task::spawn_blocking(move || {
            hal.i2c_transfer(addr, &data, read_len)
        }).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        
        Ok(hex::encode(result))
    }
}

// ==============================================================================
// oled-plugin bindings 
// ==============================================================================

impl oled_bindings::demo::plugin::i2c::Host for HostState {
    async fn transfer(&mut self, addr: u8, data: String, len: u32) -> Result<String, String> {
         <Self as bme680_bindings::demo::plugin::i2c::Host>::transfer(self, addr, data, len).await
    }
}
