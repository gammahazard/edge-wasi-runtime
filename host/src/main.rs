//! ==============================================================================
//! main.rs - wasi host runtime (standalone edition)
//! ==============================================================================
//!
//! purpose:
//!     entry point for the standalone host. initializes the web api server
//!     and the wasm runtime. handles the main polling loop that orchestrates
//!     sensor readings, state updates, and data forwarding in hub/spoke mode.
//!
//! what this file does:
//!     1. loads configuration from toml (hub.toml, spoke.toml, etc.)
//!     2. initializes shared state for sensor readings
//!     3. creates the wasm runtime with all enabled plugins
//!     4. starts an axum http server with api endpoints
//!     5. runs the main polling loop that:
//!        - toggles led 0 as a heartbeat indicator
//!        - checks for plugin hot-reloads
//!        - polls all sensors via wasm plugins
//!        - pushes data to hub (if spoke) or updates local state (if hub)
//!
//! http endpoints:
//!     GET  /             - dashboard html (rendered by wasm plugin)
//!     GET  /api/readings - json sensor readings
//!     GET  /api/logs     - combined host + wasm plugin logs
//!     POST /api/buzzer   - control buzzer (forwards to spoke if hub)
//!     POST /api/buzzer/test - manual 3-beep test
//!     POST /push         - hub receives data from spokes
//!
//! relationships:
//!     - uses: config.rs (loads toml configuration)
//!     - uses: runtime.rs (wasm plugin loading and execution)
//!     - uses: domain.rs (appstate and sensorreading types)
//!     - uses: hal.rs (hardware abstraction for led heartbeat)
//!
//! log buffer:
//!     the log_msg() function adds messages to a global buffer that the
//!     /api/logs endpoint returns. note: wasm plugin stdout (python print)
//!     goes to terminal only, not this buffer. this is a known limitation.
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

// ==============================================================================
// helper - format sensor data for readable log output
// ==============================================================================

fn format_sensor_summary(sensor_id: &str, data: &serde_json::Value) -> String {
    // extract key values based on sensor type
    if sensor_id.contains("dht22") {
        let temp = data.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let hum = data.get("humidity").and_then(|v| v.as_f64()).unwrap_or(0.0);
        format!("{} → {:.1}°C, {:.0}% humidity", sensor_id, temp, hum)
    } else if sensor_id.contains("bme680") {
        let temp = data.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let hum = data.get("humidity").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let iaq = data.get("iaq_score").and_then(|v| v.as_u64()).unwrap_or(0);
        let gas = data.get("gas_resistance").and_then(|v| v.as_f64()).unwrap_or(0.0);
        format!("{} → {:.1}°C, {:.0}%, IAQ:{}, Gas:{:.0}KΩ", sensor_id, temp, hum, iaq, gas)
    } else if sensor_id.contains("monitor") {
        let cpu = data.get("cpu_temp").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let used = data.get("memory_used_mb").and_then(|v| v.as_u64()).unwrap_or(0);
        let total = data.get("memory_total_mb").and_then(|v| v.as_u64()).unwrap_or(0);
        format!("{} → CPU:{:.1}°C, RAM:{}/{}MB", sensor_id, cpu, used, total)
    } else if sensor_id.contains("network") {
        let hub_ping = data.get("192.168.7.10").and_then(|v| v.as_f64());
        let pi4_ping = data.get("192.168.7.11").and_then(|v| v.as_f64());
        let hub_str = hub_ping.map(|p| if p >= 0.0 { format!("{:.1}ms", p) } else { "OFFLINE".to_string() }).unwrap_or("N/A".to_string());
        let pi4_str = pi4_ping.map(|p| if p >= 0.0 { format!("{:.1}ms", p) } else { "OFFLINE".to_string() }).unwrap_or("N/A".to_string());
        format!("{} → Hub:{}, Pi4:{}", sensor_id, hub_str, pi4_str)
    } else {
        format!("{} → {:?}", sensor_id, data)
    }
}

// ==============================================================================
// log buffer - stores messages for /api/logs endpoint
// ==============================================================================
//
// this is a circular buffer that holds the last 100 log messages.
// messages are added via log_msg() which also prints to terminal.
// note: wasm plugin print() statements bypass this buffer and go
// directly to terminal via inherit_stdio().

static LOG_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn get_log_buffer() -> &'static Mutex<VecDeque<String>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(100)))
}

/// add a message to the log buffer with est timestamp.
/// this is the primary logging function for host-side messages.
/// messages are also printed to stdout for terminal viewing.
fn log_msg(msg: &str) {
    use chrono::{Utc, FixedOffset};
    
    // est is utc-5
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

// ==============================================================================
// api state - shared across all http handlers
// ==============================================================================
//
// holds the shared sensor readings, wasm runtime, and config.
// wrapped in arc for thread-safe sharing across async handlers.

#[derive(Clone)]
struct ApiState {
    state: Arc<RwLock<AppState>>,
    #[allow(dead_code)]
    runtime: runtime::WasmRuntime,
    #[allow(dead_code)]
    config: config::HostConfig,
}

// ==============================================================================
// main - entry point
// ==============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // initialize tracing/logging subscriber
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    log_msg("===========================================================");
    log_msg("  WASI Host - Standalone Edition");
    log_msg("===========================================================");
    
    // 1. load config from toml file
    let config = config::HostConfig::load_or_default();
    config.print_summary();
    
    // 2. initialize shared state for sensor readings
    let state = Arc::new(RwLock::new(AppState::default()));
    
    // 3. initialize wasm runtime (loads all enabled plugins)
    log_msg("[STARTUP] Initializing WASM Runtime...");
    let runtime = runtime::WasmRuntime::new(std::path::PathBuf::from(".."), &config).await?;
    
    // 4. create api state for handlers
    let api_state = ApiState {
        state: state.clone(),
        runtime: runtime.clone(),
        config: config.clone(),
    };

    // start web/api server on port 3000
    let bind_addr = "0.0.0.0:3000";
    log_msg(&format!("[STARTUP] API listening on {}", bind_addr));
    
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/readings", get(api_handler))
        .route("/api/logs", get(logs_handler))            // dashboard log viewing
        .route("/api/buzzer", post(buzzer_handler))       // dashboard buzzer buttons
        .route("/api/buzzer/test", post(buzzer_test_handler)) // manual trigger
        .route("/api/fan/status", get(fan_status_handler))    // get fan state
        .route("/api/fan/test", post(fan_test_handler))       // manual fan test
        .route("/push", post(push_handler)) // hub endpoint to receive data from spokes
        .fallback(fallback_handler)
        .layer(CorsLayer::permissive())
        .with_state(api_state.clone());
        
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    
    // spawn server in background task
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // ==============================================================================
    // polling loop - main runtime loop
    // ==============================================================================
    //
    // runs every N seconds (configurable via polling.interval_seconds).
    // this is the heart of the system:
    // - toggles led 0 as heartbeat (blue <-> cyan)
    // - checks for hot-reloaded plugins
    // - polls all sensors via wasm plugins
    // - pushes to hub (spoke) or updates local state (hub)

    let poll_interval = config.polling.interval_seconds;
    let hub_url = config.cluster.hub_url.clone();
    let is_spoke = config.cluster.role == "spoke";
    let node_id = config.cluster.node_id.clone();

    log_msg(&format!("[RUNTIME] Starting sensor polling loop ({}s interval) as {}", poll_interval, config.cluster.role));
    
    let client = reqwest::Client::new();
    let mut heartbeat = false;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval)).await;

        // 0. host heartbeat (led 0) - visual indicator that host is running
        heartbeat = !heartbeat;
        {
            let hal = crate::hal::Hal::new();
            use crate::hal::HardwareProvider;
            if heartbeat {
                let _ = hal.set_led(0, 0, 0, 255); // solid blue
            } else {
                let _ = hal.set_led(0, 0, 100, 255); // cyan-ish blink
            }
            let _ = hal.sync_leds();
        }

        // 1. check for hot-reloaded plugins (modified wasm files)
        runtime.check_hot_reload().await;

        // 2. poll sensors and update local state
        match runtime.poll_sensors().await {
            Ok(mut readings) => {
                // add node_id prefix to sensor_id for clarity (e.g., "pi4:dht22")
                for r in &mut readings {
                    r.sensor_id = format!("{}:{}", node_id, r.sensor_id);
                }

                if !readings.is_empty() {
                    let mut s = state.write().await;
                    
                    // merge local readings into state (update existing or add new)
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
                    
                    // 3. log detailed readings for dashboard visibility
                    for r in &readings {
                        let summary = format_sensor_summary(&r.sensor_id, &r.data);
                        log_msg(&format!("📡 {}", summary));
                    }
                    
                    // 4. if spoke, forward readings to hub via http post
                    if is_spoke && !hub_url.is_empty() {
                        match client.post(&hub_url).json(&readings).send().await {
                            Ok(_) => log_msg(&format!("✅ Pushed {} readings to hub", readings.len())),
                            Err(e) => log_msg(&format!("❌ Failed to push to hub: {}", e)),
                        }
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
// http handlers
// ==============================================================================

/// dashboard handler - renders the main web ui.
/// transforms sensor readings into the format expected by the dashboard plugin,
/// then calls the wasm plugin to render html.
async fn dashboard_handler(State(api_state): State<ApiState>) -> impl IntoResponse {
    let s = api_state.state.read().await;
    
    // transform readings list into the format the dashboard plugin expects:
    // {dht22: {...}, bme680: {...}, hub: {...}, pi4: {...}, pizero: {...}}
    let mut dashboard_data = serde_json::json!({});
    
    for reading in &s.readings {
        let sensor_id = &reading.sensor_id;
        
        // parse sensor_id like "pi4:dht22" or "revpi-hub:revpi-monitor"
        if sensor_id.contains("dht22") {
            dashboard_data["dht22"] = reading.data.clone();
        } else if sensor_id.contains("bme680") {
            let mut bme = reading.data.clone();
            // add iaq_score at top level if it's nested
            if let Some(_iaq) = bme.get("iaq_score") {
                dashboard_data["bme680"] = bme.clone();
            } else {
                dashboard_data["bme680"] = bme;
            }
        } else if sensor_id.contains("revpi-monitor") {
            dashboard_data["hub"] = reading.data.clone();
        } else if sensor_id.contains("pi4-monitor") {
            dashboard_data["pi4"] = reading.data.clone();
        } else if sensor_id.contains("pizero") && sensor_id.contains("monitor") {
            // only use the monitor reading for pizero card (has cpu_temp, memory)
            let mut pz = reading.data.clone();
            pz["online"] = serde_json::json!(true); // if we got data, it's online
            dashboard_data["pizero"] = pz;
        } else if sensor_id.contains("network") {
            // network health pings from pizero
            dashboard_data["network"] = reading.data.clone();
        }
    }
    
    // add uptime to hub (should come from revpi-monitor plugin)
    if let Some(hub) = dashboard_data.get_mut("hub") {
        if hub.get("uptime_seconds").is_none() {
            hub["uptime_seconds"] = serde_json::json!(0);
        }
    }
    
    let json_data = serde_json::to_string(&dashboard_data).unwrap_or_else(|_| "{}".to_string());
    
    // call the wasm dashboard plugin to render the html
    match api_state.runtime.render_dashboard(json_data).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Dashboard plugin failed: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Dashboard Logic Error").into_response()
        }
    }
}

/// api handler - returns raw sensor readings as json.
/// used by dashboard for live updates via javascript fetch.
async fn api_handler(State(state): State<ApiState>) -> Json<AppState> {
    let s = state.state.read().await;
    Json(s.clone())
}

/// logs handler - returns logs for the dashboard.
/// merges host logs from log_buffer + any wasm logs from file.
/// note: wasm plugin stdout currently bypasses the log buffer.
async fn logs_handler() -> impl IntoResponse {
    let mut all_logs: Vec<String> = Vec::new();
    
    // 1. add host logs from in-memory buffer
    if let Ok(buf) = get_log_buffer().lock() {
        all_logs.extend(buf.iter().cloned());
    }
    
    // 2. add wasm plugin logs from file (last 50 lines)
    // note: this file may not exist if wasm stdout isn't redirected
    if let Ok(content) = std::fs::read_to_string("wasi-logs.log") {
        let lines: Vec<&str> = content.lines().collect();
        let start = if lines.len() > 50 { lines.len() - 50 } else { 0 };
        for line in &lines[start..] {
            if !line.trim().is_empty() {
                all_logs.push(line.to_string());
            }
        }
    }
    
    // 3. sort by timestamp if present
    all_logs.sort_by(|a, b| {
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
    
    // keep last 100 logs
    if all_logs.len() > 100 {
        all_logs = all_logs.split_off(all_logs.len() - 100);
    }
    
    Json(serde_json::json!({"logs": all_logs}))
}

/// push handler - receives sensor data from spoke nodes.
/// hub uses this endpoint to aggregate data from all spokes.
async fn push_handler(
    State(state): State<ApiState>,
    Json(new_readings): Json<Vec<SensorReading>>,
) -> impl axum::response::IntoResponse {
    let mut s = state.state.write().await;
    
    // log detailed incoming data for each sensor
    for nr in &new_readings {
        let summary = format_sensor_summary(&nr.sensor_id, &nr.data);
        log_msg(&format!("📥 [PUSH] {}", summary));
    }
    
    // merge readings from this spoke into global state
    // update/replace readings with the same sensor_id
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
    
    axum::http::StatusCode::OK
}

/// buzzer test handler - manual 3-beep test.
/// directly controls gpio without going through wasm plugin.
async fn buzzer_test_handler() -> impl IntoResponse {
    let hal = crate::hal::Hal::new();
    use crate::hal::HardwareProvider;
    
    // 3 short beeps (active low relay)
    for _ in 0..3 {
        let _ = hal.write_gpio(17, false); // active low on
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let _ = hal.write_gpio(17, true); // active low off
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    axum::http::StatusCode::OK
}

/// fan status handler - returns current fan state for dashboard button logic
async fn fan_status_handler() -> impl IntoResponse {
    use std::sync::atomic::Ordering;
    let fan_on = crate::hal::GLOBAL_FAN_STATE.load(Ordering::SeqCst);
    Json(serde_json::json!({ "fan_on": fan_on }))
}

/// fan test handler - runs fan for 10 seconds with 2 beeps
/// only runs if fan is currently off (dashboard should disable button if on)
async fn fan_test_handler(State(state): State<ApiState>) -> impl IntoResponse {
    use std::sync::atomic::Ordering;
    use crate::hal::HardwareProvider;
    
    // Check if fan is already on
    if crate::hal::GLOBAL_FAN_STATE.load(Ordering::SeqCst) {
        return (axum::http::StatusCode::CONFLICT, "Fan already running");
    }
    
    let hal = crate::hal::Hal::new();
    let fan_pin = state.config.fan.gpio_pin;
    let buzzer_pin = state.config.buzzer.gpio_pin;
    
    // 2 beeps to signal fan test starting
    for _ in 0..2 {
        let _ = hal.write_gpio(buzzer_pin, false);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let _ = hal.write_gpio(buzzer_pin, true);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    log_msg("🌀 [FAN TEST] Starting 10-second fan test");
    
    // Turn fan on (active low)
    let _ = hal.set_gpio_mode(fan_pin, "OUT");
    let _ = hal.write_gpio(fan_pin, false); // LOW = relay ON = fan running
    crate::hal::GLOBAL_FAN_STATE.store(true, Ordering::SeqCst);
    
    // Run for 10 seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    
    // Turn fan off
    let _ = hal.write_gpio(fan_pin, true); // HIGH = relay OFF = fan stopped
    crate::hal::GLOBAL_FAN_STATE.store(false, Ordering::SeqCst);
    
    log_msg("🌀 [FAN TEST] Fan test complete");
    
    (axum::http::StatusCode::OK, "Fan test complete")
}

/// buzzer query params from dashboard buttons
#[derive(serde::Deserialize, Default)]
struct BuzzerQuery {
    action: Option<String>,
}

/// buzzer body for forwarded requests from hub
#[derive(serde::Deserialize, Default)]
struct BuzzerBody {
    pattern: Option<String>,
}

/// buzzer handler - controls buzzer from dashboard.
/// if hub: forwards request to spoke (where buzzer is physically connected).
/// if spoke: controls local gpio directly.
async fn buzzer_handler(
    State(state): State<ApiState>,
    Query(params): Query<BuzzerQuery>,
    body: Option<axum::Json<BuzzerBody>>,
) -> impl IntoResponse {
    // get pattern from json body (forwarded from hub) or query params (direct dashboard)
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
    
    // if we have a spoke buzzer url configured (hub mode), forward the request
    if !spoke_url.is_empty() {
        log_msg(&format!("🔔 [BUZZER] Forwarding to spoke: {}", spoke_url));
        
        let client = reqwest::Client::new();
        
        // map dashboard actions to spoke buzzer patterns
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
    
    // fallback: try local gpio (for when running on spoke directly)
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

/// fallback handler - returns 404 for unknown routes
async fn fallback_handler() -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::NOT_FOUND, "Not Found".to_string())
}
