//! ==============================================================================
//! main.rs - wasi host runtime entry point
//! ==============================================================================
//!
//! purpose:
//!     this is the "landlord" application that hosts python wasm plugins.
//!     it demonstrates the wasi component model pattern used in production by
//!     fermyon spin, wasmcloud, and other serverless/edge platforms.
//!
//! responsibilities:
//!     - initialize wasmtime engine (wasm execution environment)
//!     - load python wasm plugins (dht22.wasm, pi-monitor.wasm, bme680.wasm, dashboard.wasm)
//!     - provide wasi capabilities (stdio, clocks) to sandboxed plugins
//!     - run polling loop to collect sensor data
//!     - serve web dashboard with data from wasm-rendered html
//!     - detect and apply hot reloads when plugins change
//!
//! relationships:
//!     - uses: runtime.rs (wasm loading, plugin execution, hot reload)
//!     - reads: ../wit/plugin.wit (interface definitions, via runtime.rs)
//!     - loads: ../plugins/dht22/dht22.wasm (python dht22 logic)
//!     - loads: ../plugins/dashboard/dashboard.wasm (python html rendering)
//!
//! architecture:
//!
//!     ┌─────────────────────────────────────────────────────────────┐
//!     │                    rust host (this file)                     │
//!     │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
//!     │  │ poll loop   │  │ web server  │  │ hot reload watcher  │  │
//!     │  │ (2s cycle)  │  │ (port 3000) │  │ (file timestamps)   │  │
//!     │  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
//!     │         │                │                    │             │
//!     │         └────────────────┼────────────────────┘             │
//!     │                          │                                  │
//!     │                    ┌─────┴─────┐                            │
//!     │                    │  runtime  │ <- runtime.rs              │
//!     │                    └─────┬─────┘                            │
//!     │     (Clone-able handle to shared engine & plugin state)      │
//!     └──────────────────────────┼──────────────────────────────────┘
//!                                │ wit interface
//!                    ┌───────────┴───────────┐
//!                    ▼                       ▼
//!             ┌─────────────┐         ┌─────────────┐
//!             │ dht22.wasm  │         │ dashboard   │
//!             │  (python)   │         │   .wasm     │
//!             └─────────────┘         └─────────────┘
//!
//! security model:
//!     plugins run in a sandbox. they CANNOT:
//!     - access the filesystem (unless host grants it)
//!     - make network requests (unless host grants it)
//!     - call arbitrary host functions
//!     - interfere with other plugins
//!
//!     they CAN only:
//!     - execute pure computation
//!     - use wasi capabilities explicitly granted (here: stdio, clocks)
//!     - return data through the wit interface
//!
//! industry usage:
//!     this architecture is used in production by:
//!     - fermyon spin: serverless functions with <1ms cold starts
//!     - shopify functions: sandboxed merchant logic
//!     - wasmcloud: distributed iot/edge applications
//!     - cloudflare workers: (moving to component model)
//!
//! ==============================================================================

mod config;
mod gpio;
mod runtime;

use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
    response::{Html, Json},
    extract::{State, Query},
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

// ==============================================================================
// shared state
// ==============================================================================
// this struct holds sensor readings that are shared between:
// - the polling loop (writes new readings)
// - the web server (reads for api and dashboard)
//
// we use arc<rwlock<>> for thread-safe sharing:
// - arc: reference-counted pointer for sharing across tasks
// - rwlock: multiple readers OR one writer (sensors write, http reads)

#[derive(Clone, Default, serde::Serialize)]
pub struct AppState {
    /// current sensor readings
    pub readings: Vec<SensorReading>,
    /// unix timestamp (ms) of last successful update
    pub last_update: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct SensorReading {
    /// unique sensor identifier (e.g., "dht22-gpio4")
    pub sensor_id: String,
    /// temperature in celsius
    pub temperature: f32,
    /// relative humidity (0-100%)
    pub humidity: f32,
    /// reading timestamp in milliseconds
    pub timestamp_ms: u64,
    /// pressure in hPa (optional, bme680 only)
    pub pressure: Option<f32>,
    /// gas resistance in KOhms (optional, bme680 only)
    pub gas_resistance: Option<f32>,
    /// IAQ Score 0-500 (optional, bme680 only)
    pub iaq_score: Option<u16>,
}

// ==============================================================================
// main entry point
// ==============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // startup banner
    println!("===========================================================");
    println!("  WASI Python Host - Reference Demo");
    println!("  \"Compile Once, Swap WASM\"");
    println!("===========================================================");
    
    // step 1: load configuration
    let config = config::HostConfig::load_or_default();
    config.print_summary();
    
    // step 2: initialize shared state
    let state = Arc::new(RwLock::new(AppState::default()));
    
    // step 3: initialize the wasm runtime
    println!("\n[STARTUP] Initializing WASM Runtime...");
    let runtime = match runtime::WasmRuntime::new(std::path::PathBuf::from(".."), &config).await {
        Ok(r) => {
            println!("[STARTUP] ✓ WASM runtime ready");
            println!("[STARTUP] ✓ Loaded plugins: dht22, pi-monitor, bme680, dashboard");
            r
        }
        Err(e) => {
            eprintln!("[ERROR] Fatal: failed to create wasm runtime: {}", e);
            return Err(e);
        }
    };
    
    // step 4: start the web server in background
    let web_state = state.clone();
    let web_runtime = runtime.clone();
    tokio::spawn(async move {
        println!("[STARTUP] ✓ Dashboard live at http://0.0.0.0:3000");
        if let Err(e) = run_server(web_state, web_runtime).await {
            eprintln!("[ERROR] Web server error: {}", e);
        }
    });
    
    // step 5: main polling loop
    let poll_interval = config.polling.interval_seconds;
    let show_data = config.logging.show_sensor_data;
    println!("\n[RUNTIME] Starting sensor polling ({}s interval)", poll_interval);
    println!("────────────────────────────────────────────────────────────");
    
    loop {
        let mut all_readings = Vec::new();

        // 1. Poll DHT22 (if enabled)
        if config.plugins.dht22.enabled {
            match runtime.poll_sensors().await {
                Ok(readings) => {
                    if show_data {
                        for r in &readings {
                            println!("[DHT22] Temp: {:.1}°C | Humidity: {:.1}%", r.temperature, r.humidity);
                        }
                    }
                    all_readings.extend(readings);
                }
                Err(e) => {
                    println!("[DHT22] ⚠ Read error: {}", e);
                }
            }
        }

        // 2. Poll BME680 (if enabled)
        if config.plugins.bme680.enabled {
            match runtime.poll_bme680().await {
                Ok(readings) => {
                    if show_data {
                        for r in &readings {
                            let iaq = r.iaq_score.unwrap_or(0);
                            let status = match iaq {
                                0 => "CALIBRATING",
                                1..=50 => "Excellent",
                                51..=100 => "Good",
                                101..=150 => "Moderate",
                                _ => "Poor",
                            };
                            println!("[BME680] Temp: {:.1}°C | Humidity: {:.1}% | IAQ: {} ({})", 
                                r.temperature, r.humidity, iaq, status);
                        }
                    }
                    all_readings.extend(readings);
                }
                Err(e) => {
                    println!("[BME680] ⚠ Read error: {}", e);
                }
            }
        }

        // 3. Poll Pi Monitor (if enabled)
        if config.plugins.pi_monitor.enabled {
            match runtime.poll_pi_monitor().await {
                Ok(_cpu_temp) => {
                    // CPU temp is logged inside the plugin
                }
                Err(e) => {
                    println!("[PI] ⚠ Monitor error: {}", e);
                }
            }
        }

        // 4. Sync LEDs atomically ONCE after all plugins finish
        tokio::task::spawn_blocking(|| gpio::sync_leds()).await.ok();

        if !all_readings.is_empty() {
             let mut state_guard = state.write().await;
             state_guard.readings = all_readings;
             state_guard.last_update = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval)).await;
    }
}


// ==============================================================================
// web server
// ==============================================================================

async fn run_server(
    state: Arc<RwLock<AppState>>,
    runtime: runtime::WasmRuntime,
) -> Result<()> {
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api", get(api_handler))
        .route("/api/buzzer", post(buzzer_handler))
        .layer(CorsLayer::permissive())
        .with_state((state, runtime));
    
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dashboard_handler(
    State((state, runtime)): State<(Arc<RwLock<AppState>>, runtime::WasmRuntime)>,
) -> Html<String> {
    let state = state.read().await;
    
    let (dht_temp, dht_hum, bme_temp, bme_hum, pressure, gas, iaq) = {
        let dht = state.readings.iter().find(|x| x.pressure.is_none());
        let dt = dht.map(|x| x.temperature).unwrap_or(0.0);
        let dh = dht.map(|x| x.humidity).unwrap_or(0.0);

        let bme = state.readings.iter().find(|x| x.pressure.is_some());
        let bt = bme.map(|x| x.temperature).unwrap_or(0.0);
        let bh = bme.map(|x| x.humidity).unwrap_or(0.0);
        let p = bme.and_then(|x| x.pressure).unwrap_or(-1.0);
        let g = bme.and_then(|x| x.gas_resistance).unwrap_or(-1.0);
        let i = bme.and_then(|x| x.iaq_score).unwrap_or(0);
        
        (dt, dh, bt, bh, p, g, i)
    };
    
    // get cpu temperature and system stats
    let cpu_temp = gpio::get_cpu_temp();
    let (mem_used, mem_total) = gpio::get_memory_usage();
    let uptime = gpio::get_uptime();
    
    // call python wasm to render html!
    match runtime.render_dashboard(dht_temp, dht_hum, bme_temp, bme_hum, cpu_temp, mem_used, mem_total, uptime, pressure, gas, iaq).await {
        Ok(html) => Html(html),
        Err(e) => {
            // render error page if plugin fails
            Html(format!(
                r#"<!doctype html>
<html>
<head><title>error</title></head>
<body style="font-family: system-ui; padding: 2rem; background: #1a1a2e; color: #eee;">
    <h1 style="color: #ff6b6b;">⚠️ dashboard error</h1>
    <p>failed to render dashboard from python wasm plugin:</p>
    <pre style="background: #16213e; padding: 1rem; border-radius: 8px; overflow-x: auto;">{}</pre>
    <p style="color: #888;">check that dashboard.wasm is built and located at plugins/dashboard/dashboard.wasm</p>
</body>
</html>"#,
                html_escape(&format!("{:#}", e))
            ))
        }
    }
}

/// json api endpoint for programmatic access
/// returns current sensor readings as json
async fn api_handler(
    State((state, _)): State<(Arc<RwLock<AppState>>, runtime::WasmRuntime)>,
) -> Json<AppState> {
    let state = state.read().await;
    Json(state.clone())
}

/// buzzer control params
#[derive(Deserialize)]
struct BuzzerParams {
    action: String,
}

/// buzzer control endpoint
/// POST /api/buzzer?action=beep|beep3|long
async fn buzzer_handler(
    Query(params): Query<BuzzerParams>,
) -> Json<serde_json::Value> {
    match params.action.as_str() {
        "beep" => {
            tokio::task::spawn_blocking(|| gpio::buzz(200));
            Json(serde_json::json!({"status": "ok", "action": "beep"}))
        }
        "beep3" => {
            tokio::task::spawn_blocking(|| gpio::beep(3, 100, 100));
            Json(serde_json::json!({"status": "ok", "action": "beep3"}))
        }
        "long" => {
            tokio::task::spawn_blocking(|| gpio::buzz(5000));  // 5 second beep
            Json(serde_json::json!({"status": "ok", "action": "long"}))
        }
        _ => {
            Json(serde_json::json!({"status": "error", "message": "unknown action"}))
        }
    }
}

/// escape html special characters to prevent xss
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}
