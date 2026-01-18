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
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::sync::{Arc, Mutex};

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

pub struct PluginState {
    component: Component,
    path: PathBuf,
    last_modified: SystemTime,
}

impl PluginState {
    fn load(engine: &Engine, path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("failed to read {:?}", path))?;
        let last_modified = metadata.modified()
            .with_context(|| format!("failed to get mtime for {:?}", path))?;
        let component = Component::from_file(engine, path)
            .with_context(|| format!("failed to compile {:?}", path))?;
        
        Ok(Self { component, path: path.to_path_buf(), last_modified })
    }
    
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
    sensor_plugin: Arc<Mutex<Option<PluginState>>>,
    dashboard_plugin: Arc<Mutex<Option<PluginState>>>,
    sensor_path: PathBuf,
    dashboard_path: PathBuf,
}

impl WasmRuntime {
    /// create a new wasm runtime and load available plugins
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        
        let engine = Engine::new(&config)
            .context("failed to create wasmtime engine")?;
        
        let sensor_path = PathBuf::from("../plugins/sensor/sensor.wasm");
        let dashboard_path = PathBuf::from("../plugins/dashboard/dashboard.wasm");
        
        // Initial load
        let sensor_plugin = match PluginState::load(&engine, &sensor_path) {
            Ok(p) => {
                println!("[OK] Loaded sensor plugin: {:?}", sensor_path);
                Some(p)
            }
            Err(e) => {
                println!("[WARN] Sensor plugin not available: {:#}", e);
                None
            }
        };
        
        let dashboard_plugin = match PluginState::load(&engine, &dashboard_path) {
            Ok(p) => {
                println!("[OK] Loaded dashboard plugin: {:?}", dashboard_path);
                Some(p)
            }
            Err(e) => {
                println!("[WARN] Dashboard plugin not available: {:#}", e);
                None
            }
        };
        
        Ok(Self { 
            engine, 
            sensor_plugin: Arc::new(Mutex::new(sensor_plugin)), 
            dashboard_plugin: Arc::new(Mutex::new(dashboard_plugin)),
            sensor_path,
            dashboard_path,
        })
    }
    
    /// check for and apply hot reloads
    pub fn check_hot_reload(&self) {
        // sensor plugin
        {
            let mut plugin_guard = self.sensor_plugin.lock().unwrap();
            if let Some(ref plugin) = *plugin_guard {
                if plugin.needs_reload() {
                    println!("[HOT RELOAD] Sensor plugin changed");
                    match PluginState::load(&self.engine, &self.sensor_path) {
                        Ok(p) => {
                            *plugin_guard = Some(p);
                            println!("[OK] Sensor plugin reloaded");
                        }
                        Err(e) => println!("[ERROR] Reload failed: {:#}", e),
                    }
                }
            } else if self.sensor_path.exists() {
                if let Ok(p) = PluginState::load(&self.engine, &self.sensor_path) {
                    *plugin_guard = Some(p);
                    println!("[OK] Sensor plugin loaded");
                }
            }
        }
        
        // dashboard plugin
        {
            let mut plugin_guard = self.dashboard_plugin.lock().unwrap();
            if let Some(ref plugin) = *plugin_guard {
                if plugin.needs_reload() {
                    println!("[HOT RELOAD] Dashboard plugin changed");
                    match PluginState::load(&self.engine, &self.dashboard_path) {
                        Ok(p) => {
                            *plugin_guard = Some(p);
                            println!("[OK] Dashboard plugin reloaded");
                        }
                        Err(e) => println!("[ERROR] Reload failed: {:#}", e),
                    }
                }
            } else if self.dashboard_path.exists() {
                if let Ok(p) = PluginState::load(&self.engine, &self.dashboard_path) {
                    *plugin_guard = Some(p);
                    println!("[OK] Dashboard plugin loaded");
                }
            }
        }
    }
    
    /// poll sensors by calling the python wasm plugin
    pub async fn poll_sensors(&self) -> Result<Vec<SensorReading>> {
        self.check_hot_reload();
        
        // get component clone safely
        let component = {
            let guard = self.sensor_plugin.lock().unwrap();
            guard.as_ref()
                .map(|p| p.component.clone())
                .ok_or_else(|| anyhow!("sensor plugin not loaded"))?
        };
        
        // create linker and add capabilities
        let mut linker = Linker::new(&self.engine);
        
        // add WASI capabilities (stdio, clocks)
        wasmtime_wasi::add_to_linker_async(&mut linker)
            .context("failed to add wasi")?;
        
        // add OUR gpio-provider capability
        sensor_bindings::demo::plugin::gpio_provider::add_to_linker(&mut linker, |state: &mut HostState| state)
            .context("failed to add gpio-provider")?;
        
        // add led-controller capability
        sensor_bindings::demo::plugin::led_controller::add_to_linker(&mut linker, |state: &mut HostState| state)
            .context("failed to add led-controller")?;
        
        // add buzzer-controller capability
        sensor_bindings::demo::plugin::buzzer_controller::add_to_linker(&mut linker, |state: &mut HostState| state)
            .context("failed to add buzzer-controller")?;
        
        // create wasi context
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build();
        
        let state = HostState {
            ctx: wasi,
            table: ResourceTable::new(),
        };
        
        let mut store = Store::new(&self.engine, state);
        
        // instantiate and call the plugin
        let instance = SensorPlugin::instantiate_async(&mut store, &component, &linker)
            .await
            .context("failed to instantiate sensor plugin")?;
        
        // call poll() - this triggers the python code which calls OUR gpio-provider
        let readings = instance
            .demo_plugin_sensor_logic()
            .call_poll(&mut store)
            .await
            .context("sensor poll() failed")?;
        
        // convert to our types
        Ok(readings.into_iter().map(|r| SensorReading {
            sensor_id: r.sensor_id,
            temperature: r.temperature,
            humidity: r.humidity,
            timestamp_ms: r.timestamp_ms,
        }).collect())
    }
    
    /// render dashboard html using the python wasm plugin
    pub async fn render_dashboard(&self, temp: f32, humidity: f32) -> Result<String> {
        // checks hot reload for dashboard too
        self.check_hot_reload();

        let component = {
            let guard = self.dashboard_plugin.lock().unwrap();
            guard.as_ref()
                .map(|p| p.component.clone())
                .ok_or_else(|| anyhow!("dashboard plugin not loaded"))?
        };
        
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build();
        
        let state = HostState {
            ctx: wasi,
            table: ResourceTable::new(),
        };
        
        let mut store = Store::new(&self.engine, state);
        
        let instance = DashboardPlugin::instantiate_async(&mut store, &component, &linker)
            .await
            .context("failed to instantiate dashboard plugin")?;
        
        let html = instance
            .demo_plugin_dashboard_logic()
            .call_render(&mut store, temp, humidity)
            .await
            .context("dashboard render() failed")?;
        
        Ok(html)
    }
}
