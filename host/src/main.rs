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
    extract::State,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use crate::domain::{AppState, SensorReading};

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

    println!("===========================================================");
    println!("  WASI Host - Standalone Edition");
    println!("===========================================================");
    
    // 1. Load Config
    let config = config::HostConfig::load_or_default();
    config.print_summary();
    
    // 2. Initialize Shared State
    let state = Arc::new(RwLock::new(AppState::default()));
    
    // 3. Initialize WASM Runtime (with HAL)
    println!("\n[STARTUP] Initializing WASM Runtime...");
    let runtime = runtime::WasmRuntime::new(std::path::PathBuf::from(".."), &config).await?;
    
    // 4. Start Web/API Server
    let api_state = ApiState {
        state: state.clone(),
        runtime: runtime.clone(),
        config: config.clone(),
    };

    // Use a hardcoded bind address for the API for now, or add to config if needed
    let bind_addr = "0.0.0.0:3000";
    println!("[STARTUP] API listening on {}", bind_addr);
    
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/readings", get(api_handler))
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

    println!("[RUNTIME] Starting sensor polling loop ({}s interval) as {}", poll_interval, config.cluster.role);
    
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
                            Ok(_) => tracing::info!("Pushed {} readings to hub at {}", readings.len(), hub_url),
                            Err(e) => tracing::error!("Failed to push to hub: {}", e),
                        }
                    } else {
                        tracing::info!("State updated with {} readings", readings.len());
                    }
                }
            }
            Err(e) => {
                tracing::error!("Sensor polling failed: {}", e);
            }
        }
    }
}

// ==============================================================================
// HANDLERS
// ==============================================================================

async fn dashboard_handler(State(api_state): State<ApiState>) -> impl IntoResponse {
    let s = api_state.state.read().await;
    
    // Convert current aggregated state to JSON for the dashboard plugin
    let json_data = serde_json::to_string(&*s).unwrap_or_else(|_| "{}".to_string());
    
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

async fn fallback_handler() -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::NOT_FOUND, "Not Found".to_string())
}
