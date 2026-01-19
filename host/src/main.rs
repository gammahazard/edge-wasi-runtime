//! ==============================================================================
//! main.rs - WASI Host Runtime Entry Point
//! ==============================================================================
//!
//! purpose:
//!     the "landlord" application that hosts Python WASM plugins.
//!     demonstrates the WASI Component Model pattern used in production by
//!     Fermyon Spin, WasmCloud, and other serverless/edge platforms.
//!
//! responsibilities:
//!     - initialize wasmtime engine (WASM execution environment)
//!     - load Python WASM plugins: dht22, bme680, pi-monitor, dashboard
//!     - provide WASI capabilities (stdio, clocks) to sandboxed plugins
//!     - run polling loop to collect sensor data (configurable interval)
//!     - serve web dashboard with WASM-rendered HTML
//!     - hot-reload plugins when .wasm files change
//!
//! relationships:
//!     - uses: runtime.rs (WASM loading, plugin execution, hot reload)
//!     - uses: config.rs (loads host.toml for runtime configuration)
//!     - uses: gpio.rs (hardware access via rppal)
//!     - reads: ../wit/plugin.wit (interface definitions)
//!     - loads: ../plugins/{dht22,bme680,pi-monitor,dashboard}/*.wasm
//!
//! architecture:
//!
//!     ┌────────────────────────────────────────────────────────────────┐
//!     │                    RUST HOST (this file)                        │
//!     │  ┌──────────────┐  ┌──────────────┐  ┌───────────────────────┐ │
//!     │  │ Polling Loop │  │ Web Server   │  │ Hot Reload Watcher    │ │
//!     │  │ (5s default) │  │ (port 3000)  │  │ (file timestamps)     │ │
//!     │  └──────┬───────┘  └──────┬───────┘  └───────────┬───────────┘ │
//!     │         └─────────────────┼──────────────────────┘             │
//!     │                           │                                    │
//!     │                    ┌──────┴──────┐                             │
//!     │                    │  runtime.rs │ <- WIT bindings & HAL       │
//!     │                    └──────┬──────┘                             │
//!     └───────────────────────────┼────────────────────────────────────┘
//!                                 │ WIT interfaces
//!          ┌──────────────┬───────┴───────┬──────────────┐
//!          ▼              ▼               ▼              ▼
//!     ┌─────────┐   ┌─────────┐   ┌──────────────┐  ┌───────────┐
//!     │ dht22   │   │ bme680  │   │ pi-monitor   │  │ dashboard │
//!     │  .wasm  │   │  .wasm  │   │  .wasm       │  │  .wasm    │
//!     │ (LED 1) │   │ (LED 2) │   │ (LED 0)      │  │ (UI only) │
//!     └─────────┘   └─────────┘   └──────────────┘  └───────────┘
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
//!     - use WASI capabilities explicitly granted (stdio, clocks)
//!     - call interface functions defined in plugin.wit
//!
//! industry usage:
//!     this architecture is used in production by:
//!     - Fermyon Spin: serverless functions with <1ms cold starts
//!     - Shopify Functions: sandboxed merchant logic
//!     - WasmCloud: distributed IoT/edge applications
//!     - Cloudflare Workers: (moving to component model)
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
                        // Logging handled by plugin
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
                        // Logging handled by plugin
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
    
    // Extract sensor readings
    let dht = state.readings.iter().find(|x| x.pressure.is_none());
    let bme = state.readings.iter().find(|x| x.pressure.is_some());
    
    // Get system stats
    let cpu_temp = gpio::get_cpu_temp();
    let (mem_used, mem_total) = gpio::get_memory_usage();
    let uptime = gpio::get_uptime();
    
    // Build JSON object with all sensor data
    // This allows adding new sensors without modifying this code!
    let sensor_data = serde_json::json!({
        "dht22": {
            "temp": dht.map(|x| x.temperature).unwrap_or(0.0),
            "humidity": dht.map(|x| x.humidity).unwrap_or(0.0)
        },
        "bme680": {
            "temp": bme.map(|x| x.temperature).unwrap_or(0.0),
            "humidity": bme.map(|x| x.humidity).unwrap_or(0.0),
            "pressure": bme.and_then(|x| x.pressure).unwrap_or(-1.0),
            "gas": bme.and_then(|x| x.gas_resistance).unwrap_or(-1.0),
            "iaq": bme.and_then(|x| x.iaq_score).unwrap_or(0)
        },
        "pi": {
            "cpu_temp": cpu_temp,
            "memory_used_mb": mem_used,
            "memory_total_mb": mem_total,
            "uptime_seconds": uptime
        }
    });
    
    let json_str = sensor_data.to_string();

    // Update OLED (fire and forget, just log errors)
    // This runs the python logic in plugins/oled/app.py
    if let Err(e) = runtime.update_oled(&json_str).await {
        // Only log if it's not the "plugin not loaded" error (which is normal if disabled)
        if !e.to_string().contains("not loaded") {
            println!("[ERROR] OLED update failed: {}", e);
        }
    }
    
    // Call python wasm to render html!
    match runtime.render_dashboard(&json_str).await {
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
