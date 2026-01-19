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
//!     - uses: gpio.rs (actual hardware access via rppal)
//!     - loads: ../plugins/{dht22,bme680,pi-monitor,dashboard}/*.wasm
//!
//! ==============================================================================

use crate::gpio;
use crate::SensorReading;

use anyhow::{Result, Context, anyhow};
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
// this macro reads ../wit/plugin.wit and generates:
// - rust structs for wit records
// - rust traits for wit interfaces
// - Host trait for gpio-provider that we must implement

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

mod pi_monitor_bindings {
    wasmtime::component::bindgen!({
        path: "../wit",
        world: "pi-monitor-plugin",
        async: true,
    });
}
use pi_monitor_bindings::PiMonitorPlugin;

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

impl dht22_bindings::demo::plugin::gpio_provider::Host for HostState {
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

impl dht22_bindings::demo::plugin::led_controller::Host for HostState {
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

    /// flush memory buffer to hardware
    async fn sync_leds(&mut self) {
        tokio::task::spawn_blocking(move || {
            gpio::sync_leds();
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

impl dht22_bindings::demo::plugin::buzzer_controller::Host for HostState {
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
// pi-monitor bindings - gpio-provider and led-controller for pi-monitor plugin
// ==============================================================================

impl pi_monitor_bindings::demo::plugin::gpio_provider::Host for HostState {
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

impl pi_monitor_bindings::demo::plugin::led_controller::Host for HostState {
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

    async fn sync_leds(&mut self) {
        tokio::task::spawn_blocking(move || gpio::sync_leds()).await.ok();
    }
}

impl pi_monitor_bindings::demo::plugin::system_info::Host for HostState {
    async fn get_memory_usage(&mut self) -> (u32, u32) {
        // (used_mb, total_mb)
        tokio::task::spawn_blocking(move || gpio::get_memory_usage())
            .await
            .unwrap_or((0, 0))
    }

    async fn get_cpu_usage(&mut self) -> f32 {
        tokio::task::spawn_blocking(move || gpio::get_sys_cpu_usage())
            .await
            .unwrap_or(0.0)
    }

    async fn get_uptime(&mut self) -> u64 {
        tokio::task::spawn_blocking(move || gpio::get_uptime())
            .await
            .unwrap_or(0)
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
    dht22_plugin: Arc<Mutex<Option<PluginState<Dht22Plugin>>>>,
    pi_monitor_plugin: Arc<Mutex<Option<PluginState<PiMonitorPlugin>>>>,
    dashboard_plugin: Arc<Mutex<Option<PluginState<DashboardPlugin>>>>,
    bme680_plugin: Arc<Mutex<Option<PluginState<Bme680Plugin>>>>,
    oled_plugin: Arc<Mutex<Option<PluginState<OledPlugin>>>>,
}

impl WasmRuntime {
    /// create a new wasm runtime and load available plugins
    pub async fn new(path: PathBuf, config: &HostConfig) -> Result<Self> {
        let mut wasm_config = Config::new();
        wasm_config.wasm_component_model(true);
        wasm_config.async_support(true);
        let engine = Engine::new(&wasm_config)?;

        // --- Helper to init state ---
        let create_host_state = || {
             let wasi = WasiCtxBuilder::new().inherit_stdio().build();
             HostState { ctx: wasi, table: ResourceTable::new() }
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
            
            let mut store = Store::new(&engine, create_host_state());
            let dht22_instance = Dht22Plugin::instantiate_async(&mut store, &dht22_component, &linker).await
                .context("failed to instantiate dht22 plugin")?;
            println!("[DEBUG] Loaded dht22 plugin.");
            
            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: dht22_path,
                store: store,
                instance: dht22_instance,
            })))
        } else {
            println!("[SKIP] dht22 plugin disabled");
            Arc::new(Mutex::new(None))
        };

        // 2. DASHBOARD Plugin
        let dashboard_plugin = if config.plugins.dashboard.enabled {
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

            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: dashboard_path,
                store: store_dash,
                instance: dashboard_instance,
            })))
        } else {
            println!("[SKIP] dashboard plugin disabled");
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
            
            let mut store_bme = Store::new(&engine, create_host_state());
            let bme680_instance = Bme680Plugin::instantiate_async(&mut store_bme, &bme680_component, &linker).await
                .context("failed to instantiate bme680 plugin")?;
            println!("[DEBUG] Loaded bme680 plugin.");
            
            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: bme680_path,
                store: store_bme,
                instance: bme680_instance,
            })))
        } else {
             println!("[SKIP] bme680 plugin disabled");
             Arc::new(Mutex::new(None))
        };

        // 4. Pi Monitor Plugin
        let pi_monitor_plugin = if config.plugins.pi_monitor.enabled {
            println!("[DEBUG] Loading pi-monitor plugin...");
            let pi_monitor_path = path.join("plugins/pi-monitor/pi-monitor.wasm");
            let pi_monitor_component = Component::from_file(&engine, &pi_monitor_path)
                .context("failed to load pi-monitor.wasm")?;

            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            pi_monitor_bindings::PiMonitorPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
            
            let mut store_pi = Store::new(&engine, create_host_state());
            let pi_monitor_instance = PiMonitorPlugin::instantiate_async(&mut store_pi, &pi_monitor_component, &linker).await
                .context("failed to instantiate pi-monitor plugin")?;
            println!("[DEBUG] Loaded pi-monitor plugin.");
            
            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: pi_monitor_path,
                store: store_pi,
                instance: pi_monitor_instance,
            })))
        } else {
             println!("[SKIP] pi-monitor plugin disabled");
             Arc::new(Mutex::new(None))
        };

        // 5. OLED Plugin
        let oled_plugin = if config.plugins.oled.enabled {
            println!("[DEBUG] Loading oled plugin...");
            let oled_path = path.join("plugins/oled/oled.wasm");
            let oled_component = Component::from_file(&engine, &oled_path)
                .context("failed to load oled.wasm")?;

            let mut linker = Linker::new(&engine);
            wasmtime_wasi::add_to_linker_async(&mut linker)?;
            oled_bindings::OledPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
            
            let mut store_oled = Store::new(&engine, create_host_state());
            let oled_instance = OledPlugin::instantiate_async(&mut store_oled, &oled_component, &linker).await
                .context("failed to instantiate oled plugin")?;
            println!("[DEBUG] Loaded oled plugin.");
            
            Arc::new(Mutex::new(Some(PluginState {
                last_modified: SystemTime::now(),
                path: oled_path,
                store: store_oled,
                instance: oled_instance,
            })))
        } else {
             println!("[SKIP] oled plugin disabled");
             Arc::new(Mutex::new(None))
        };

        Ok(Self {
            engine,
            dht22_plugin,
            pi_monitor_plugin,
            dashboard_plugin,
            bme680_plugin,
            oled_plugin,
        })
    }
    
    /// check for and apply hot reloads
    pub async fn check_hot_reload(&self) {
        let create_host_state = || {
             let wasi = WasiCtxBuilder::new().inherit_stdio().build();
             HostState { ctx: wasi, table: ResourceTable::new() }
        };

        // 1. DHT22
        let needs_reload = {
            let guard = self.dht22_plugin.lock().await;
            guard.as_ref().map(|p| p.needs_reload()).unwrap_or(false)
        };
        if needs_reload {
             println!("[HOT RELOAD] Reloading dht22 plugin...");
             { let mut guard = self.dht22_plugin.lock().await;
                 if let Some(old) = guard.as_ref() {
                     let path = old.path.clone();
                     
                     // Try loading new one
                     // Note: We use an async block to capture the logic, but we must await it safely.
                     // Since we hold the lock, we must be fast or careful. 
                     // But we are in async fn, so we can await.
                     // type annotation needed for async block
                     let res: Result<PluginState<Dht22Plugin>> = async {
                         let component = Component::from_file(&self.engine, &path)?;
                         let mut linker = Linker::new(&self.engine);
                         wasmtime_wasi::add_to_linker_async(&mut linker)?;
                         dht22_bindings::Dht22Plugin::add_to_linker(&mut linker, |s: &mut HostState| s)?;
                         let mut store = Store::new(&self.engine, create_host_state());
                         let instance = Dht22Plugin::instantiate_async(&mut store, &component, &linker).await?;
                         Ok(PluginState { /*component,*/ path, last_modified: SystemTime::now(), store, instance })
                     }.await;
                     
                     match res {
                         Ok(new_state) => { *guard = Some(new_state); println!("[OK] DHT22 reloaded"); }
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
    
    /// poll dht22 sensor by calling the python wasm plugin
    pub async fn poll_sensors(&self) -> Result<Vec<SensorReading>> {
        self.check_hot_reload().await;
        
        // get component clone safely
        let mut guard = self.dht22_plugin.lock().await;
        let plugin = guard.as_mut()
            .ok_or_else(|| anyhow!("dht22 plugin not loaded"))?;
        
        // Call the poll function using persistent store
        let readings = plugin.instance.demo_plugin_dht22_logic()
            .call_poll(&mut plugin.store)
            .await
            .context("dht22 poll failed")?;
        
        // convert to our types
        Ok(readings.into_iter().map(|r| SensorReading {
            sensor_id: r.sensor_id,
            temperature: r.temperature,
            humidity: r.humidity,
            pressure: None,        // DHT22 has no pressure
            gas_resistance: None,  // DHT22 has no gas sensor
            iaq_score: None,       // DHT22 has no IAQ sensor
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
            iaq_score: Some(r.iaq_score),
            timestamp_ms: r.timestamp_ms,
        }).collect())
    }
    
    /// poll pi monitor for system stats (CPU temp, etc.)
    /// Returns CPU temperature for use in dashboard
    pub async fn poll_pi_monitor(&self) -> Result<f32> {
        let mut guard = self.pi_monitor_plugin.lock().await;
        let plugin = guard.as_mut()
            .ok_or_else(|| anyhow!("pi-monitor plugin not loaded"))?;
        
        let stats = plugin.instance.demo_plugin_pi_monitor_logic()
            .call_poll(&mut plugin.store)
            .await
            .context("pi-monitor poll failed")?;
        
        println!("[PI] CPU: {:.1}°C", stats.cpu_temp);
        Ok(stats.cpu_temp)
    }
    /// render dashboard html using the python wasm plugin
    /// 
    /// Takes a JSON string containing all sensor data.
    /// This allows adding new sensors without modifying this function.
    pub async fn render_dashboard(&self, sensor_data: &str) -> Result<String> {
        self.check_hot_reload().await;
        
        let mut guard = self.dashboard_plugin.lock().await;
        let plugin = guard.as_mut()
            .ok_or_else(|| anyhow!("dashboard plugin not loaded"))?;
        
        let html = plugin.instance.demo_plugin_dashboard_logic()
            .call_render(&mut plugin.store, sensor_data)
            .await
            .context("dashboard render() failed")?;
        
        Ok(html)
    }

    /// update oled display with latest sensor data
    pub async fn update_oled(&self, sensor_data: &str) -> Result<()> {
        self.check_hot_reload().await;
        
        let mut guard = self.oled_plugin.lock().await;
        if let Some(plugin) = guard.as_mut() {
            plugin.instance.demo_plugin_oled_logic()
                .call_update(&mut plugin.store, sensor_data)
                .await
                .context("oled update() failed")?;
        }
        
        Ok(())
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
        tokio::task::spawn_blocking(move || {
            gpio::clear_leds();
        }).await.ok();
    }

    async fn sync_leds(&mut self) {
        tokio::task::spawn_blocking(move || {
            gpio::sync_leds();
        }).await.ok();
    }
}

// ==============================================================================
// i2c implementation for bme680-plugin (Phase 3 Generic HAL)
// ==============================================================================

impl bme680_bindings::demo::plugin::i2c::Host for HostState {
    async fn transfer(&mut self, addr: u8, write_data_hex: String, read_len: u32) -> Result<String, String> {
        tokio::task::spawn_blocking(move || {
            gpio::i2c_transfer(addr, &write_data_hex, read_len)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?
        .map_err(|e| e.to_string())
    }
}

// ==============================================================================
// OLED BINDINGS IMPLEMENTATION
// ==============================================================================
impl oled_bindings::demo::plugin::i2c::Host for HostState {
    async fn transfer(&mut self, addr: u8, write_data_hex: String, read_len: u32) -> Result<String, String> {
        tokio::task::spawn_blocking(move || {
            gpio::i2c_transfer(addr, &write_data_hex, read_len)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?
        .map_err(|e| e.to_string())
    }
}

