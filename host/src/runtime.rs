//! ==============================================================================
//! runtime.rs - wasm component model runtime with gpio capability
//! ==============================================================================
//!
//! purpose:
//!     this module handles loading and executing wasm plugins using wasmtime.
//!     it implements the WASI CAPABILITY MODEL where:
//!     - the HOST provides gpio access (gpio-provider interface)
//!     - the GUEST implements sensor logic (calls gpio-provider)
//!     - the GUEST runs in a SANDBOX and cannot access hardware directly
//!
//! this is the KEY security boundary of WASI - the sandboxed python code
//! can only access what we explicitly grant through the linker.
//!
//! relationships:
//!     - used by: main.rs (creates runtime, calls poll_sensors/render_dashboard)
//!     - reads: ../wit/plugin.wit (interface definitions)
//!     - implements: gpio-provider interface (via GpioProviderImports trait)
//!     - uses: gpio.rs (actual hardware access)
//!     - loads: ../plugins/sensor/sensor.wasm (python sensor logic)
//!     - loads: ../plugins/dashboard/dashboard.wasm (python ui rendering)
//!
//! ==============================================================================

use crate::gpio;
use crate::SensorReading;

use anyhow::{Result, Context, anyhow};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};
use std::path::PathBuf;
use std::time::SystemTime;
use std::sync::Arc;
use tokio::sync::Mutex;

// ==============================================================================
// bindgen - generate rust bindings from wit
// ==============================================================================
// this macro reads ../wit/plugin.wit and generates:
// - rust structs for wit records
// - rust traits for wit interfaces
// - Host trait for gpio-provider that we must implement

mod sensor_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "sensor-plugin",
        async: true,
    });
}
use sensor_bindings::SensorPlugin;

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

// ==============================================================================
// host state - provides capabilities to wasm guests
// ==============================================================================
// this struct holds the state that wasm plugins can access.
// crucially, it implements the GpioProvider trait to provide
// REAL hardware access to the sandboxed python code.

pub struct HostState {
    ctx: WasiCtx,
    table: ResourceTable,
}

// wasiview trait is required for wasmtime_wasi integration
impl WasiView for HostState {
    fn table(&mut self) -> &mut ResourceTable { &mut self.table }
    fn ctx(&mut self) -> &mut WasiCtx { &mut self.ctx }
}

// ==============================================================================
// gpio-provider implementation - THE CAPABILITY
// ==============================================================================
// this is where we implement the gpio-provider interface from plugin.wit.
// when the python wasm plugin calls gpio_provider.read_dht22(4), it comes here.
// WE control what hardware access is allowed.

impl sensor_bindings::demo::plugin::gpio_provider::Host for HostState {
    /// read dht22 sensor - called by python wasm plugin
    ///
    /// this is the CAPABILITY boundary. the sandboxed python code calls
    /// gpio_provider.read_dht22(pin) and we handle the actual hardware access.
    async fn read_dht22(&mut self, pin: u8) -> Result<(f32, f32), String> {
        // offload blocking io to dedicated thread
        tokio::task::spawn_blocking(move || {
            gpio::read_dht22(pin)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?
        .map_err(|e| e.to_string())
    }
    
    /// get timestamp - called by python wasm plugin
    async fn get_timestamp_ms(&mut self) -> u64 {
        gpio::get_timestamp_ms()
    }
    
    /// get cpu temperature - called by python wasm plugin
    async fn get_cpu_temp(&mut self) -> f32 {
        gpio::get_cpu_temp()
    }
    
    /// read bme680 sensor via i2c - called by python wasm plugin
    async fn read_bme680(&mut self, i2c_addr: u8) -> Result<(f32, f32, f32, f32), String> {
        // offload blocking io
        tokio::task::spawn_blocking(move || {
            gpio::read_bme680(i2c_addr)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?
        .map_err(|e| e.to_string())
    }
}

// ==============================================================================
// led-controller implementation - ws2812b strip capability
// ==============================================================================
//
// hardware: btf lighting ws2812b strip (11 leds) on gpio 18
//
// when the python wasm plugin calls led_controller.set_all(255, 0, 0),
// it comes here. we then call gpio::set_all_leds() which runs a python
// subprocess to control the actual hardware.
//
// relationships:
//     - implements: ../wit/plugin.wit (led-controller interface)
//     - calls: gpio.rs (set_led, set_all_leds, clear_leds)

impl sensor_bindings::demo::plugin::led_controller::Host for HostState {
    /// set a single led to an rgb color
    async fn set_led(&mut self, index: u8, r: u8, g: u8, b: u8) {
        tokio::task::spawn_blocking(move || {
            gpio::set_led(index, r, g, b);
        }).await.ok();
    }
    
    /// set all leds to the same rgb color
    async fn set_all(&mut self, r: u8, g: u8, b: u8) {
        tokio::task::spawn_blocking(move || {
            gpio::set_all_leds(r, g, b);
        }).await.ok();
    }
    
    /// set led 0 and led 1 atomically (avoids flicker)
    async fn set_two(&mut self, r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
        tokio::task::spawn_blocking(move || {
            gpio::set_two_leds(r0, g0, b0, r1, g1, b1);
        }).await.ok();
    }
    
    /// turn off all leds
    async fn clear(&mut self) {
        tokio::task::spawn_blocking(move || {
            gpio::clear_leds();
        }).await.ok();
    }
}

// ==============================================================================
// buzzer-controller implementation - piezo buzzer via relay
// ==============================================================================
//
// hardware: cyclewet buzzer connected via sainsmart relay on gpio 17
// note: relay is ACTIVE LOW (handled in gpio.rs)
//
// relationships:
//     - implements: ../wit/plugin.wit (buzzer-controller interface)
//     - calls: gpio.rs (buzz, beep)

impl sensor_bindings::demo::plugin::buzzer_controller::Host for HostState {
    /// sound the buzzer for a duration
    async fn buzz(&mut self, duration_ms: u32) {
        tokio::task::spawn_blocking(move || {
            gpio::buzz(duration_ms);
        }).await.ok();
    }
    
    /// beep pattern - multiple short beeps with intervals
    async fn beep(&mut self, count: u8, duration_ms: u32, interval_ms: u32) {
        tokio::task::spawn_blocking(move || {
            gpio::beep(count, duration_ms, interval_ms);
        }).await.ok();
    }
}

// ==============================================================================
// plugin metadata - for hot reload tracking
// ==============================================================================

pub struct PluginState<T> {
    // component: Component, // unused
    path: PathBuf,
    last_modified: SystemTime,
    store: Store<HostState>,
    instance: T,
}

impl<T> PluginState<T> {
    fn needs_reload(&self) -> bool {
        std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .map(|t| t > self.last_modified)
            .unwrap_or(false)
    }
}

// ==============================================================================
// wasm runtime - main public interface
// ==============================================================================

#[derive(Clone)]
pub struct WasmRuntime {
    engine: Engine,
    // Shared state via Arc<Mutex> to allow cloning WasmRuntime
    sensor_plugin: Arc<Mutex<Option<PluginState<SensorPlugin>>>>,
    dashboard_plugin: Arc<Mutex<Option<PluginState<DashboardPlugin>>>>,
    bme680_plugin: Arc<Mutex<Option<PluginState<Bme680Plugin>>>>,
}

impl WasmRuntime {
    /// create a new wasm runtime and load available plugins
    pub async fn new(path: PathBuf) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Engine::new(&config)?;

        // --- Helper to init state ---
        let create_host_state = || {
             let wasi = WasiCtxBuilder::new().inherit_stdio().build();
             HostState { ctx: wasi, table: ResourceTable::new() }
        };

        // 1. SENSORPlugin
        println!("[DEBUG] Loading sensor plugin...");
        let sensor_path = path.join("plugins/sensor/sensor.wasm");
        let sensor_component = Component::from_file(&engine, &sensor_path)
            .context("failed to load sensor.wasm")?;
        
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        sensor_bindings::SensorPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
        
        let mut store = Store::new(&engine, create_host_state());
        let sensor_instance = SensorPlugin::instantiate_async(&mut store, &sensor_component, &linker).await
            .context("failed to instantiate sensor plugin")?;
        println!("[DEBUG] Loaded sensor plugin.");

        // 2. DASHBOARDPlugin
        println!("[DEBUG] Loading dashboard plugin...");
        let dashboard_path = path.join("plugins/dashboard/dashboard.wasm");
        let dashboard_component = Component::from_file(&engine, &dashboard_path)
            .context("failed to load dashboard.wasm")?;
            
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        // dashboard_bindings::DashboardPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
        
        let mut store_dash = Store::new(&engine, create_host_state());
        let dashboard_instance = DashboardPlugin::instantiate_async(&mut store_dash, &dashboard_component, &linker).await
            .context("failed to instantiate dashboard plugin")?;
        println!("[DEBUG] Loaded dashboard plugin.");

        // 3. BME680Plugin
        println!("[DEBUG] Loading bme680 plugin...");
        let bme680_path = path.join("plugins/bme680/bme680.wasm");
        let bme680_component = Component::from_file(&engine, &bme680_path)
            .context("failed to load bme680.wasm")?;

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        bme680_bindings::Bme680Plugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
        
        let mut store_bme = Store::new(&engine, create_host_state());
        let bme680_instance = Bme680Plugin::instantiate_async(&mut store_bme, &bme680_component, &linker).await
            .context("failed to instantiate bme680 plugin")?;
        println!("[DEBUG] Loaded bme680 plugin.");

        Ok(Self {
            engine,
            sensor_plugin: Arc::new(Mutex::new(Some(PluginState {
                // component: sensor_component,
                last_modified: SystemTime::now(),
                path: sensor_path,
                store: store,
                instance: sensor_instance,
            }))),
            dashboard_plugin: Arc::new(Mutex::new(Some(PluginState {
                // component: dashboard_component,
                last_modified: SystemTime::now(),
                path: dashboard_path,
                store: store_dash,
                instance: dashboard_instance,
            }))),
            bme680_plugin: Arc::new(Mutex::new(Some(PluginState {
                // component: bme680_component,
                last_modified: SystemTime::now(),
                path: bme680_path,
                store: store_bme,
                instance: bme680_instance,
            }))),
        })
    }
    
    /// check for and apply hot reloads
    pub async fn check_hot_reload(&self) {
        let create_host_state = || {
             let wasi = WasiCtxBuilder::new().inherit_stdio().build();
             HostState { ctx: wasi, table: ResourceTable::new() }
        };

        // 1. SENSOR
        let needs_reload = {
            let guard = self.sensor_plugin.lock().await;
            guard.as_ref().map(|p| p.needs_reload()).unwrap_or(false)
        };
        if needs_reload {
             println!("[HOT RELOAD] Reloading sensor plugin...");
             { let mut guard = self.sensor_plugin.lock().await;
                 if let Some(old) = guard.as_ref() {
                     let path = old.path.clone();
                     
                     // Try loading new one
                     // Note: We use an async block to capture the logic, but we must await it safely.
                     // Since we hold the lock, we must be fast or careful. 
                     // But we are in async fn, so we can await.
                     // type annotation needed for async block
                     let res: Result<PluginState<SensorPlugin>> = async {
                         let component = Component::from_file(&self.engine, &path)?;
                         let mut linker = Linker::new(&self.engine);
                         wasmtime_wasi::add_to_linker_async(&mut linker)?;
                         sensor_bindings::SensorPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
                         let mut store = Store::new(&self.engine, create_host_state());
                         let instance = SensorPlugin::instantiate_async(&mut store, &component, &linker).await?;
                         Ok(PluginState { /*component,*/ path, last_modified: SystemTime::now(), store, instance })
                     }.await;
                     
                     match res {
                         Ok(new_state) => { *guard = Some(new_state); println!("[OK] Sensor reloaded"); }
                         Err(e) => println!("[ERR] Sensor reload failed: {:#}", e),
                     }
                 }
             }
        }

        // 2. DASHBOARD
        let needs_reload = {
            let guard = self.dashboard_plugin.lock().await;
            guard.as_ref().map(|p| p.needs_reload()).unwrap_or(false)
        };
        if needs_reload {
             println!("[HOT RELOAD] Reloading dashboard plugin...");
             { let mut guard = self.dashboard_plugin.lock().await;
                 if let Some(old) = guard.as_ref() {
                     let path = old.path.clone();
                     // type annotation needed for async block
                     let res: Result<PluginState<DashboardPlugin>> = async {
                         let component = Component::from_file(&self.engine, &path)?;
                         let mut linker = Linker::new(&self.engine);
                         wasmtime_wasi::add_to_linker_async(&mut linker)?;
                         // dashboard_bindings::DashboardPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
                         let mut store = Store::new(&self.engine, create_host_state());
                         let instance = DashboardPlugin::instantiate_async(&mut store, &component, &linker).await?;
                         Ok(PluginState { /*component,*/ path, last_modified: SystemTime::now(), store, instance })
                     }.await;
                     
                     match res {
                         Ok(new_state) => { *guard = Some(new_state); println!("[OK] Dashboard reloaded"); }
                         Err(e) => println!("[ERR] Dashboard reload failed: {:#}", e),
                     }
                 }
             }
        }

        // 3. BME680
        let needs_reload = {
            let guard = self.bme680_plugin.lock().await;
            guard.as_ref().map(|p| p.needs_reload()).unwrap_or(false)
        };
        if needs_reload {
             println!("[HOT RELOAD] Reloading bme680 plugin...");
             { let mut guard = self.bme680_plugin.lock().await;
                 if let Some(old) = guard.as_ref() {
                     let path = old.path.clone();
                     // type annotation needed for async block
                     let res: Result<PluginState<Bme680Plugin>> = async {
                         let component = Component::from_file(&self.engine, &path)?;
                         let mut linker = Linker::new(&self.engine);
                         wasmtime_wasi::add_to_linker_async(&mut linker)?;
                         bme680_bindings::Bme680Plugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
                         let mut store = Store::new(&self.engine, create_host_state());
                         let instance = Bme680Plugin::instantiate_async(&mut store, &component, &linker).await?;
                         Ok(PluginState { /*component,*/ path, last_modified: SystemTime::now(), store, instance })
                     }.await;
                     
                     match res {
                         Ok(new_state) => { *guard = Some(new_state); println!("[OK] BME680 reloaded"); }
                         Err(e) => println!("[ERR] BME680 reload failed: {:#}", e),
                     }
                 }
             }
        }
    }
    
    /// poll sensors by calling the python wasm plugin
    pub async fn poll_sensors(&self) -> Result<Vec<SensorReading>> {
        self.check_hot_reload().await;
        
        // get component clone safely
        let mut guard = self.sensor_plugin.lock().await;
        let plugin = guard.as_mut()
            .ok_or_else(|| anyhow!("sensor plugin not loaded"))?;
        
        // Call the poll function using persistent store
        let readings = plugin.instance.demo_plugin_sensor_logic()
            .call_poll(&mut plugin.store)
            .await
            .context("poll failed")?;
        
        // convert to our types
        Ok(readings.into_iter().map(|r| SensorReading {
            sensor_id: r.sensor_id,
            temperature: r.temperature,
            humidity: r.humidity,
            pressure: None,        // DHT22 has no pressure
            gas_resistance: None,  // DHT22 has no gas sensor
            timestamp_ms: r.timestamp_ms,
        }).collect())
    }

    /// poll bme680 sensor
    pub async fn poll_bme680(&self) -> Result<Vec<SensorReading>> {
        // self.check_hot_reload().await; // already checked
        
        let mut guard = self.bme680_plugin.lock().await;
        let plugin = guard.as_mut()
            .ok_or_else(|| anyhow!("bme680 plugin not loaded"))?;
        
        let readings = plugin.instance.demo_plugin_bme680_logic()
            .call_poll(&mut plugin.store)
            .await
            .context("bme680 poll failed")?;

        Ok(readings.into_iter().map(|r| SensorReading {
            sensor_id: r.sensor_id,
            temperature: r.temperature,
            humidity: r.humidity,
            pressure: Some(r.pressure),
            gas_resistance: Some(r.gas_resistance),
            timestamp_ms: r.timestamp_ms,
        }).collect())
    }
    
    /// render dashboard html using the python wasm plugin
    pub async fn render_dashboard(&self, dht_temp: f32, dht_hum: f32, bme_temp: f32, bme_hum: f32, cpu_temp: f32, pressure: f32, gas: f32) -> Result<String> {
        self.check_hot_reload().await;
        
        let mut guard = self.dashboard_plugin.lock().await;
        let plugin = guard.as_mut()
            .ok_or_else(|| anyhow!("dashboard plugin not loaded"))?;
        
        let html = plugin.instance.demo_plugin_dashboard_logic()
            .call_render(&mut plugin.store, dht_temp, dht_hum, bme_temp, bme_hum, cpu_temp, pressure, gas)
            .await
            .context("dashboard render() failed")?;
        
        Ok(html)
    }
}

// ==============================================================================
// BME680 BINDINGS IMPLEMENTATION
// ==============================================================================
// Duplicate implementations for the bme680-plugin world capabilities
// (Rust treats generated traits from different bindgen! calls as distinct types)

impl bme680_bindings::demo::plugin::gpio_provider::Host for HostState {
    async fn read_dht22(&mut self, pin: u8) -> Result<(f32, f32), String> {
        tokio::task::spawn_blocking(move || gpio::read_dht22(pin))
            .await
            .map_err(|e| format!("task join error: {}", e))?
            .map_err(|e| e.to_string())
    }
    
    async fn get_timestamp_ms(&mut self) -> u64 {
        gpio::get_timestamp_ms()
    }
    
    async fn get_cpu_temp(&mut self) -> f32 {
        gpio::get_cpu_temp()
    }
    
    async fn read_bme680(&mut self, i2c_addr: u8) -> Result<(f32, f32, f32, f32), String> {
        tokio::task::spawn_blocking(move || gpio::read_bme680(i2c_addr))
            .await
            .map_err(|e| format!("task join error: {}", e))?
            .map_err(|e| e.to_string())
    }
}

impl bme680_bindings::demo::plugin::led_controller::Host for HostState {
    async fn set_led(&mut self, index: u8, r: u8, g: u8, b: u8) {
        tokio::task::spawn_blocking(move || gpio::set_led(index, r, g, b)).await.ok();
    }
    
    async fn set_all(&mut self, r: u8, g: u8, b: u8) {
        tokio::task::spawn_blocking(move || gpio::set_all_leds(r, g, b)).await.ok();
    }
    
    async fn set_two(&mut self, r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
        tokio::task::spawn_blocking(move || gpio::set_two_leds(r0, g0, b0, r1, g1, b1)).await.ok();
    }
    
    async fn clear(&mut self) {
        tokio::task::spawn_blocking(move || gpio::clear_leds()).await.ok();
    }
}
