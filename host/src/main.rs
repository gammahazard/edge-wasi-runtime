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
mod domain; // NEW

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
use crate::domain::{AppState, SensorReading};

#[derive(Clone)]
struct ApiState {
    state: Arc<RwLock<AppState>>,
    runtime: runtime::WasmRuntime,
    config: config::HostConfig,
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
    
    // step 4: start the web server in background (if hub or standalone)
    let role = config.cluster.role.clone();
    if role == "hub" || role == "standalone" {
        let web_state = state.clone();
        let web_runtime = runtime.clone();
        let web_config = config.clone();
        tokio::spawn(async move {
            println!("[STARTUP] ✓ Dashboard live at http://0.0.0.0:3000");
            let state_ctx = ApiState {
                state: web_state,
                runtime: web_runtime,
                config: web_config,
            };
            if let Err(e) = run_server(state_ctx).await {
                eprintln!("[ERROR] Web server error: {}", e);
            }
        });
    } else {
        println!("[STARTUP] Running in SPOKE mode (no web server)");
    }
    
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
                Ok(readings) => {
                    // Now returns a Vec<SensorReading> with full system stats
                    all_readings.extend(readings);
                }
                Err(e) => {
                    println!("[PI] ⚠ Monitor error: {}", e);
                }
            }
        }

        // 4. Sync LEDs atomically ONCE after all plugins finish
        tokio::task::spawn_blocking(|| gpio::sync_leds()).await.ok();

        if !all_readings.is_empty() {
             // If we are a HUB or STANDALONE, update local state
                  println!("[LOOP] Updating local state with {} readings (Role: {})", all_readings.len(), config.cluster.role);
                  let mut state_guard = state.write().await;
                  
                  // Update readings
                  for new_r in all_readings.clone() {
                      if let Some(pos) = state_guard.readings.iter().position(|r| r.sensor_id == new_r.sensor_id) {
                          state_guard.readings[pos] = new_r;
                      } else {
                          state_guard.readings.push(new_r);
                      }
                  }
                  
                  state_guard.last_update = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                  println!("[LOOP] State updated. Count: {}", state_guard.readings.len());
             
             // If we are a SPOKE, push data to HUB
             if config.cluster.role == "spoke" {
                 let hub_url = config.cluster.hub_url.clone();
                 if !hub_url.is_empty() {
                     let client = reqwest::Client::new();
                     match client.post(&hub_url)
                         .json(&all_readings)
                         .send()
                         .await {
                             Ok(resp) => {
                                 if resp.status().is_success() {
                                    // println!("[PUSH] ✓ Data sent to hub");
                                 } else {
                                     println!("[PUSH] ⚠ Hub rejected data: {}", resp.status());
                                 }
                             },
                             Err(e) => println!("[PUSH] ⚠ Failed to send to hub: {}", e),
                         }
                 }
             }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval)).await;
    }
}


// ==============================================================================
// web server
// ==============================================================================

async fn run_server(
    state: ApiState
) -> Result<()> {
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/readings", get(api_handler)) // Move legacy /api to here for clarity
        .route("/push", post(push_handler))   // SIMPLIFIED PUSH
        .route("/api/buzzer", post(buzzer_handler))
        .fallback(fallback_handler)
        .layer(CorsLayer::permissive())
        .with_state(state);
    
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn fallback_handler(req: axum::http::Request<axum::body::Body>) -> (axum::http::StatusCode, String) {
    println!("[WEB] FALLBACK: {} {}", req.method(), req.uri().path());
    (axum::http::StatusCode::NOT_FOUND, "Not Found".to_string())
}

async fn dashboard_handler(
    State(state): State<ApiState>,
) -> Html<String> {
    let app_state = state.state.read().await;
    println!("[WEB] Rendering dashboard with {} readings", app_state.readings.len());
    
    // Generic Sensor Data Construction
    let mut sensor_data = serde_json::Map::new();
    
    for reading in &app_state.readings {
        // Map "dht22-*" -> "dht22"
        if reading.sensor_id.contains("dht22") {
            sensor_data.insert("dht22".to_string(), reading.data.clone());
        }
        // Map "bme680-*" -> "bme680"
        else if reading.sensor_id.contains("bme680") {
            // Flatten generic data into the object? Or just replace?
            // Dashboard expects {temp, humidity, pressure...} which is exactly what data is.
            sensor_data.insert("bme680".to_string(), reading.data.clone());
        }
        // Map "system_stats" -> "pi" (Legacy/Hub) or "pi4" (Spoke)
        else if reading.sensor_id.contains("system_stats") {
            // If it's the Hub's own stats, map to "pi" for main dashboard card
            if reading.sensor_id.contains("revpi-hub") {
                sensor_data.insert("pi".to_string(), reading.data.clone());
            } 
            // If it's the Pi 4 Spoke, map to "pi4" for the secondary card
            else if reading.sensor_id.contains("pi4-node-1") {
                sensor_data.insert("pi4".to_string(), reading.data.clone());
            }
            // Fallback for others
            else {
                 sensor_data.insert(reading.sensor_id.clone(), reading.data.clone());
            }
        }
        // Future sensors could just be inserted by raw ID
        else {
             sensor_data.insert(reading.sensor_id.clone(), reading.data.clone());
        }
    }
    
    // Fallback: If no system stats in readings (e.g. pi-monitor disabled),
    // we can optionally grab local stats here?
    // No, let's rely on the plugin. If plugin disabled, "pi" key might be missing, 
    // leading to empty dashboard card. That is correct behavior (Visual feedback of disabled plugin).

    let final_json = serde_json::Value::Object(sensor_data);
    let json_str = final_json.to_string();

    // Update OLED 
    if let Err(e) = state.runtime.update_oled(&json_str).await {
        if !e.to_string().contains("not loaded") {
            println!("[ERROR] OLED update failed: {}", e);
        }
    }
    
    // Render HTML
    match state.runtime.render_dashboard(&json_str).await {
        Ok(html) => Html(html),
        Err(e) => {
            Html(format!(
                r#"<!doctype html><html><body><h1>⚠️ dashboard error</h1><pre>{}</pre></body></html>"#,
                html_escape(&format!("{:#}", e))
            ))
        }
    }
}

/// json api endpoint for programmatic access
/// returns current sensor readings as json
async fn api_handler(
    State(state): State<ApiState>,
) -> Json<AppState> {
    let app_state = state.state.read().await;
    Json(app_state.clone())
}

/// buzzer control params
#[derive(Deserialize)]
struct BuzzerParams {
    action: String,
}

/// buzzer control endpoint
/// POST /api/buzzer?action=beep|beep3|long
async fn buzzer_handler(
    State(state): State<ApiState>,
    Query(params): Query<BuzzerParams>,
) -> Json<serde_json::Value> {
    let pin = state.config.buzzer.gpio_pin;
    match params.action.as_str() {
        "beep" => {
            tokio::task::spawn_blocking(move || gpio::buzz(pin, 200));
            Json(serde_json::json!({"status": "ok", "action": "beep"}))
        }
        "beep3" => {
            tokio::task::spawn_blocking(move || gpio::beep(pin, 3, 100, 100));
            Json(serde_json::json!({"status": "ok", "action": "beep3"}))
        }
        "long" => {
            tokio::task::spawn_blocking(move || gpio::buzz(pin, 5000));  // 5 second beep
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

/// receive data from spokes
/// POST /api/readings
async fn push_handler(
    State(state_ctx): State<ApiState>,
    Json(readings): Json<Vec<SensorReading>>,
) -> Json<serde_json::Value> {
    println!("[PUSH] Received {} readings from spoke", readings.len());
    let mut state = state_ctx.state.write().await;
    
    for new_r in readings {
        if let Some(pos) = state.readings.iter().position(|r| r.sensor_id == new_r.sensor_id) {
            state.readings[pos] = new_r;
        } else {
            state.readings.push(new_r);
        }
    }
    
    state.last_update = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    println!("[PUSH] Shared state updated. Reading count: {}", state.readings.len());
    Json(serde_json::json!({"status": "ok"}))
}
