//! ==============================================================================
//! main.rs - WASI Host Runtime (Standalone Edition)
//! ==============================================================================
//!
//! purpose:
//!     the entry point for the standalone host.
//!     initializes the Web API and the WASM Runtime.
//!
//! modules:
//!     - config: loads host.toml
//!     - runtime: wasmtime integration
//!     - domain: shared state types
//!     - hal: hardware abstraction
//!
//! ==============================================================================

mod config;
mod runtime;
mod domain;
mod hal;

use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
    response::{Html, Json, IntoResponse},
    extract::{State, Query},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::sync::{Mutex, OnceLock};
use std::collections::VecDeque;
use tower_http::cors::CorsLayer;
use crate::domain::{AppState, SensorReading};

// Global log buffer for /api/logs endpoint
static LOG_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn get_log_buffer() -> &'static Mutex<VecDeque<String>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(100)))
}

/// Add a message to the log buffer with EST timestamp
fn log_msg(msg: &str) {
    use chrono::{Utc, FixedOffset};
    
    // EST is UTC-5
    let est = FixedOffset::west_opt(5 * 3600).unwrap();
    let now = Utc::now().with_timezone(&est);
    let timestamp = now.format("[%Y/%m/%d @ %I:%M%P]").to_string();
    let timestamped_msg = format!("{} {}", timestamp, msg);
    
    if let Ok(mut buf) = get_log_buffer().lock() {
        if buf.len() >= 100 {
            buf.pop_front();
        }
        buf.push_back(timestamped_msg.clone());
    }
    println!("{}", timestamped_msg);
}

#[derive(Clone)]
struct ApiState {
    state: Arc<RwLock<AppState>>,
    #[allow(dead_code)]
    runtime: runtime::WasmRuntime,
    #[allow(dead_code)]
    config: config::HostConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    log_msg("===========================================================");
    log_msg("  WASI Host - Standalone Edition");
    log_msg("===========================================================");
    
    // 1. Load Config
    let config = config::HostConfig::load_or_default();
    config.print_summary();
    
    // 2. Initialize Shared State
    let state = Arc::new(RwLock::new(AppState::default()));
    
    // 3. Initialize WASM Runtime (with HAL)
    log_msg("[STARTUP] Initializing WASM Runtime...");
    let runtime = runtime::WasmRuntime::new(std::path::PathBuf::from(".."), &config).await?;
    
    // 4. Start Web/API Server
    let api_state = ApiState {
        state: state.clone(),
        runtime: runtime.clone(),
        config: config.clone(),
    };

    // Use a hardcoded bind address for the API for now, or add to config if needed
    let bind_addr = "0.0.0.0:3000";
    log_msg(&format!("[STARTUP] API listening on {}", bind_addr));
    
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/readings", get(api_handler))
        .route("/api/logs", get(logs_handler))            // Dashboard log viewing
        .route("/api/buzzer", post(buzzer_handler))       // Dashboard buzzer buttons
        .route("/api/buzzer/test", post(buzzer_test_handler)) // Manual trigger
        .route("/push", post(push_handler)) // Hub endpoint to receive data
        .fallback(fallback_handler)
        .layer(CorsLayer::permissive())
        .with_state(api_state.clone());
        
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    
    // Spawn server
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // 5. Start Polling Loop
    let poll_interval = config.polling.interval_seconds;
    let hub_url = config.cluster.hub_url.clone();
    let is_spoke = config.cluster.role == "spoke";
    let node_id = config.cluster.node_id.clone();

    log_msg(&format!("[RUNTIME] Starting sensor polling loop ({}s interval) as {}", poll_interval, config.cluster.role));
    
    let client = reqwest::Client::new();
    let mut heartbeat = false;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval)).await;

        // 0. Host Heartbeat (LED 0)
        heartbeat = !heartbeat;
        {
            let hal = crate::hal::Hal::new();
            use crate::hal::HardwareProvider;
            if heartbeat {
                let _ = hal.set_led(0, 0, 0, 255); // Solid Blue
            } else {
                let _ = hal.set_led(0, 0, 100, 255); // Cyan-ish blink
            }
            let _ = hal.sync_leds();
        }

        // 1. Hot Reload Plugins
        runtime.check_hot_reload().await;

        // 2. Poll sensors and update local state
        match runtime.poll_sensors().await {
            Ok(mut readings) => {
                // Add node_id to sensor_id for clarity
                for r in &mut readings {
                    r.sensor_id = format!("{}:{}", node_id, r.sensor_id);
                }

                if !readings.is_empty() {
                    let mut s = state.write().await;
                    // Merge local readings into state instead of overwriting
                    for nr in &readings {
                        if let Some(pos) = s.readings.iter().position(|r| r.sensor_id == nr.sensor_id) {
                            s.readings[pos] = nr.clone();
                        } else {
                            s.readings.push(nr.clone());
                        }
                    }
                    
                    s.last_update = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    
                    // 3. If Spoke, forward to Hub
                    if is_spoke && !hub_url.is_empty() {
                        match client.post(&hub_url).json(&readings).send().await {
                            Ok(_) => log_msg(&format!("✅ Pushed {} readings to hub", readings.len())),
                            Err(e) => log_msg(&format!("❌ Failed to push to hub: {}", e)),
                        }
                    } else {
                        log_msg(&format!("📊 State updated with {} readings", readings.len()));
                    }
                }
            }
            Err(e) => {
                log_msg(&format!("❌ Sensor polling failed: {}", e));
            }
        }
    }
}

// ==============================================================================
// HANDLERS
// ==============================================================================

async fn dashboard_handler(State(api_state): State<ApiState>) -> impl IntoResponse {
    let s = api_state.state.read().await;
    
    // Transform readings list into the format the dashboard plugin expects:
    // {dht22: {...}, bme680: {...}, hub: {...}, pi4: {...}, pizero: {...}}
    let mut dashboard_data = serde_json::json!({});
    
    for reading in &s.readings {
        let sensor_id = &reading.sensor_id;
        
        // Parse sensor_id like "pi4:dht22" or "revpi-hub:revpi-monitor"
        if sensor_id.contains("dht22") {
            dashboard_data["dht22"] = reading.data.clone();
        } else if sensor_id.contains("bme680") {
            let mut bme = reading.data.clone();
            // Add iaq_score at top level if it's nested
            if let Some(iaq) = bme.get("iaq_score") {
                dashboard_data["bme680"] = bme.clone();
            } else {
                dashboard_data["bme680"] = bme;
            }
        } else if sensor_id.contains("revpi-monitor") {
            dashboard_data["hub"] = reading.data.clone();
        } else if sensor_id.contains("pi4-monitor") {
            dashboard_data["pi4"] = reading.data.clone();
        } else if sensor_id.contains("pizero") && sensor_id.contains("monitor") {
            // Only use the monitor reading for pizero card (has cpu_temp, memory)
            let mut pz = reading.data.clone();
            pz["online"] = serde_json::json!(true); // If we got data, it's online
            dashboard_data["pizero"] = pz;
        } else if sensor_id.contains("network") {
            // Network health pings from PiZero
            dashboard_data["network"] = reading.data.clone();
        }
    }
    
    // Add uptime to hub (should come from revpi-monitor plugin)
    if let Some(hub) = dashboard_data.get_mut("hub") {
        if hub.get("uptime_seconds").is_none() {
            hub["uptime_seconds"] = serde_json::json!(0);
        }
    }
    
    let json_data = serde_json::to_string(&dashboard_data).unwrap_or_else(|_| "{}".to_string());
    
    // Call the WASM Dashboard plugin to render the HTML
    match api_state.runtime.render_dashboard(json_data).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Dashboard plugin failed: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Dashboard Logic Error").into_response()
        }
    }
}

async fn api_handler(State(state): State<ApiState>) -> Json<AppState> {
    let s = state.state.read().await;
    Json(s.clone())
}

/// Returns logs for the dashboard (merges host logs + WASM plugin logs)
async fn logs_handler() -> impl IntoResponse {
    let mut all_logs: Vec<String> = Vec::new();
    
    // 1. Add host logs from in-memory buffer
    if let Ok(buf) = get_log_buffer().lock() {
        all_logs.extend(buf.iter().cloned());
    }
    
    // 2. Add WASM plugin logs from file (last 50 lines)
    if let Ok(content) = std::fs::read_to_string("wasi-logs.log") {
        let lines: Vec<&str> = content.lines().collect();
        let start = if lines.len() > 50 { lines.len() - 50 } else { 0 };
        for line in &lines[start..] {
            if !line.trim().is_empty() {
                all_logs.push(line.to_string());
            }
        }
    }
    
    // 3. Sort by timestamp if present, otherwise maintain order
    // Most logs have EST timestamps at start like "[01:05:23 EST]"
    all_logs.sort_by(|a, b| {
        // Extract timestamp if present for sorting
        fn get_time(s: &str) -> Option<String> {
            if s.starts_with('[') {
                s.find(']').map(|i| s[1..i].to_string())
            } else {
                None
            }
        }
        match (get_time(a), get_time(b)) {
            (Some(ta), Some(tb)) => ta.cmp(&tb),
            _ => std::cmp::Ordering::Equal
        }
    });
    
    // Keep last 100 logs
    if all_logs.len() > 100 {
        all_logs = all_logs.split_off(all_logs.len() - 100);
    }
    
    Json(serde_json::json!({"logs": all_logs}))
}

/// Receives sensor data from spoke nodes
async fn push_handler(
    State(state): State<ApiState>,
    Json(new_readings): Json<Vec<SensorReading>>,
) -> impl axum::response::IntoResponse {
    let mut s = state.state.write().await;
    
    // Merge readings from this spoke into global state
    // We update/replace readings with the same sensor_id
    for nr in new_readings {
        if let Some(pos) = s.readings.iter().position(|r| r.sensor_id == nr.sensor_id) {
            s.readings[pos] = nr;
        } else {
            s.readings.push(nr);
        }
    }
    
    s.last_update = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    tracing::info!("Hub received data push (total sensors tracked: {})", s.readings.len());
    axum::http::StatusCode::OK
}

/// Manually trigger the buzzer for testing
async fn buzzer_test_handler() -> impl IntoResponse {
    let hal = crate::hal::Hal::new();
    use crate::hal::HardwareProvider;
    
    // 3 short beeps
    for _ in 0..3 {
        let _ = hal.write_gpio(17, false); // Active low on
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let _ = hal.write_gpio(17, true); // Active low off
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    axum::http::StatusCode::OK
}

/// Dashboard buzzer buttons
#[derive(serde::Deserialize, Default)]
struct BuzzerQuery {
    action: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct BuzzerBody {
    pattern: Option<String>,
}

async fn buzzer_handler(
    State(state): State<ApiState>,
    Query(params): Query<BuzzerQuery>,
    body: Option<axum::Json<BuzzerBody>>,
) -> impl IntoResponse {
    // Get pattern from JSON body (forwarded from Hub) or query params (direct dashboard)
    let pattern = body
        .and_then(|b| b.pattern.clone())
        .or_else(|| params.action.clone().map(|a| match a.as_str() {
            "beep" => "single".to_string(),
            "beep3" => "triple".to_string(),
            "long" => "long".to_string(),
            _ => "single".to_string(),
        }))
        .unwrap_or_else(|| "single".to_string());
    
    let action = params.action.unwrap_or_else(|| pattern.clone());
    let spoke_url = &state.config.cluster.spoke_buzzer_url;
    
    log_msg(&format!("🔔 [BUZZER] Received action='{}', spoke_url='{}'", action, spoke_url));
    
    // If we have a spoke buzzer URL configured, forward the request there
    if !spoke_url.is_empty() {
        log_msg(&format!("🔔 [BUZZER] Forwarding to spoke: {}", spoke_url));
        
        let client = reqwest::Client::new();
        
        // Map dashboard actions to spoke buzzer patterns
        let pattern = match action.as_str() {
            "beep" => "single",
            "beep3" => "triple",
            "long" => "long",
            _ => "single",
        };
        
        log_msg(&format!("🔔 [BUZZER] Sending pattern='{}' to {}", pattern, spoke_url));
        
        let body = serde_json::json!({
            "pattern": pattern
        });
        
        match client.post(spoke_url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await 
        {
            Ok(resp) => {
                let status = resp.status();
                log_msg(&format!("🔔 [BUZZER] Spoke responded with status: {}", status));
                if status.is_success() {
                    return axum::http::StatusCode::OK;
                } else {
                    log_msg(&format!("❌ [BUZZER] Spoke error: {:?}", resp.text().await));
                    return axum::http::StatusCode::BAD_GATEWAY;
                }
            }
            Err(e) => {
                log_msg(&format!("❌ [BUZZER] Failed to reach spoke: {}", e));
                return axum::http::StatusCode::BAD_GATEWAY;
            }
        }
    }
    
    // Fallback: try local GPIO (for when running on spoke directly)
    log_msg(&format!("🔔 [BUZZER] No spoke URL, trying local GPIO pin {}", state.config.buzzer.gpio_pin));
    
    let hal = crate::hal::Hal::new();
    use crate::hal::HardwareProvider;
    
    let pin = state.config.buzzer.gpio_pin;
    
    log_msg(&format!("🔔 [BUZZER] Local pattern='{}' on pin {}", pattern, pin));
    
    match hal.buzz(pin, &pattern) {
        Ok(_) => log_msg("🔔 [BUZZER] Done."),
        Err(e) => log_msg(&format!("❌ [BUZZER] Failed: {}", e)),
    }
    
    axum::http::StatusCode::OK
}

async fn fallback_handler() -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::NOT_FOUND, "Not Found".to_string())
}
