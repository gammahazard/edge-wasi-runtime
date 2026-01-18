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
//!     - load python wasm plugins (sensor.wasm, dashboard.wasm)
//!     - provide wasi capabilities (stdio, clocks) to sandboxed plugins
//!     - run polling loop to collect sensor data
//!     - serve web dashboard with data from wasm-rendered html
//!     - detect and apply hot reloads when plugins change
//!
//! relationships:
//!     - uses: runtime.rs (wasm loading, plugin execution, hot reload)
//!     - reads: ../wit/plugin.wit (interface definitions, via runtime.rs)
//!     - loads: ../plugins/sensor/sensor.wasm (python sensor logic)
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
//!             │ sensor.wasm │         │ dashboard   │
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
}

// ==============================================================================
// main entry point
// ==============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // startup banner
    println!("===========================================================");
    println!("  WASI Python Host - Reference Demo");
    println!("===========================================================");
    println!("  Demonstrates:");
    println!("    - Python code running as WASM components");
    println!("    - Type-safe cross-language calls via WIT");
    println!("    - Hot reload without restart");
    println!("    - Sandboxed plugin execution");
    println!("===========================================================");
    println!();
    
    // step 1: initialize shared state
    // arc<rwlock<>> enables safe concurrent access from multiple tasks
    let state = Arc::new(RwLock::new(AppState::default()));
    
    // step 2: initialize the wasm runtime
    // this loads our python plugins compiled to wasm
    println!("[*] Initializing WASM runtime...");
    let runtime = match runtime::WasmRuntime::new(std::path::PathBuf::from("..")).await {
        Ok(r) => {
            println!("[OK] WASM runtime ready");
            r // NO WRAPPER - WasmRuntime is now thread-safe (Clone)
        }
        Err(e) => {
            // fatal error - can't proceed without runtime
            eprintln!("[ERROR] Fatal: failed to create wasm runtime: {}", e);
            eprintln!("   ensure wasmtime is installed and wit files are valid");
            return Err(e);
        }
    };
    
    // step 3: start the web server in background
    println!();
    println!("[*] Web server on http://0.0.0.0:3000");
    println!("    GET /           -> Dashboard (HTML from Python WASM)");
    println!("    GET /api/sensors -> JSON API");
    println!();
    
    let web_state = state.clone();
    let web_runtime = runtime.clone(); // independent clone for web server
    tokio::spawn(async move {
        if let Err(e) = run_server(web_state, web_runtime).await {
            eprintln!("[ERROR] Web server error: {}", e);
        }
    });
    
    // step 4: main polling loop
    println!("[*] Sensor polling loop (5s interval)");
    println!("    Tip: Edit Python plugins and rebuild WASM - hot reload will pick up changes!");
    println!();
    println!("-----------------------------------------------------------");
    
    // polling loop owns its own copy of runtime methods
    // no locking required because internal state is protected
    loop {
        let mut all_readings = Vec::new();

        // 1. Poll DHT22 (Sensor Plugin)
        match runtime.poll_sensors().await {
            Ok(readings) => all_readings.extend(readings),
            Err(e) => eprintln!("[WARN] DHT22 Sensor error: {:#}", e),
        }

        // 2. Poll BME680 (BME680 Plugin)
        match runtime.poll_bme680().await {
            Ok(readings) => all_readings.extend(readings),
            Err(e) => eprintln!("[WARN] BME680 Sensor error: {:#}", e),
        }

        // Only update state if we got ANY readings
        if !all_readings.is_empty() {
             // Print one summary line
             // Print one summary line
             if let Some(first) = all_readings.first() {
                 let p = all_readings.iter().find_map(|x| x.pressure).unwrap_or(-1.0);
                 let g = all_readings.iter().find_map(|x| x.gas_resistance).unwrap_or(-1.0);
                 
                 print!("🟢 [OK] {:.1}C, {:.1}%", first.temperature, first.humidity);
                 if p > 0.0 { print!(", {:.1}hPa", p); }
                 if g > 0.0 { print!(", {:.1}KΩ", g); }
                 println!(" (CPU: {:.1}C)", gpio::get_cpu_temp());
             }
             
             let mut state = state.write().await;
             state.readings = all_readings;
             state.last_update = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
        }
        
        // dht22 sensors are slow and can heat up if polled too fast
        // 5 seconds is a safe, stable interval
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

// ==============================================================================
// web server
// ==============================================================================
// serves the dashboard html (rendered by python wasm) and a json api.
// uses axum for ergonomic async http handling.

async fn run_server(
    state: Arc<RwLock<AppState>>,
    runtime: runtime::WasmRuntime, // NO Arc<RwLock<>> wrapper
) -> Result<()> {
    // create router with shared state
    let app = Router::new()
        // dashboard endpoint - html rendered by python wasm
        .route("/", get(dashboard_handler))
        // json api for programmatic access
        .route("/api", get(api_handler))
        // buzzer control api
        .route("/api/buzzer", post(buzzer_handler))
        // enable cors for development convenience
        .layer(CorsLayer::permissive())
        // share state and runtime with handlers
        .with_state((state, runtime));
    
    // bind and serve
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

/// dashboard endpoint - html is rendered by the python wasm plugin!
///
/// this demonstrates the power of the component model:
/// - rust handles http (fast, secure)
/// - python handles templating (flexible, familiar)
/// - communication is type-safe via wit
async fn dashboard_handler(
    State((state, runtime)): State<(Arc<RwLock<AppState>>, runtime::WasmRuntime)>,
) -> Html<String> {
    let state = state.read().await;
    // NO runtime lock needed
    
    // get latest reading (or defaults if none)
    // iterate to find bme680 data if available
    // get latest reading (or defaults if none)
    // iterate to find dht (pressure=None) and bme680 (pressure=Some)
    let (dht_temp, dht_hum, bme_temp, bme_hum, pressure, gas) = {
        let dht = state.readings.iter().find(|x| x.pressure.is_none());
        let dt = dht.map(|x| x.temperature).unwrap_or(0.0);
        let dh = dht.map(|x| x.humidity).unwrap_or(0.0);

        let bme = state.readings.iter().find(|x| x.pressure.is_some());
        let bt = bme.map(|x| x.temperature).unwrap_or(0.0);
        let bh = bme.map(|x| x.humidity).unwrap_or(0.0);
        let p = bme.and_then(|x| x.pressure).unwrap_or(-1.0);
        let g = bme.and_then(|x| x.gas_resistance).unwrap_or(-1.0);
        
        (dt, dh, bt, bh, p, g)
    };
    
    // get cpu temperature
    let cpu_temp = gpio::get_cpu_temp();
    
    // call python wasm to render html!
    match runtime.render_dashboard(dht_temp, dht_hum, bme_temp, bme_hum, cpu_temp, pressure, gas).await {
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
