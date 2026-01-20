use serde::{Deserialize, Serialize};

/// current sensor readings shared state
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    /// list of all sensor readings from all nodes
    pub readings: Vec<SensorReading>,
    /// unix timestamp (ms) of last successful update
    pub last_update: u64,
}

/// a generic sensor reading
/// replaces the old rigid struct with a flexible json payload
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SensorReading {
    /// unique sensor identifier (e.g., "dht22-gpio4" or "pi4-system-stats")
    pub sensor_id: String,
    
    /// reading timestamp in milliseconds
    pub timestamp_ms: u64,
    
    /// generic data payload
    /// examples:
    /// - {"temperature": 22.5, "humidity": 45.0}
    /// - {"cpu_temp": 55.0, "ram_used": 1024, "uptime": 3600}
    pub data: serde_json::Value,
}
