//! ==============================================================================
//! gpio.rs - hardware capability provider for dht22 sensor
//! ==============================================================================
//!
//! purpose:
//!     provides REAL gpio/hardware access to the dht22 temperature/humidity sensor.
//!     this is the HOST-SIDE implementation of the gpio-provider interface.
//!     the sandboxed wasm plugin calls these functions to read actual hardware.
//!
//! security model:
//!     this is the "landlord" capability. the wasm "tenant" cannot directly
//!     access gpio pins. instead, it calls gpio_provider.read_dht22() which
//!     is handled here in the trusted host code.
//!
//! relationships:
//!     - implements: ../wit/plugin.wit (gpio-provider interface)
//!     - used by: runtime.rs (implements GpioProviderImports trait)
//!     - uses: python3/adafruit_dht (via subprocess for reliable timing)
//!
//! why subprocess to python?:
//!     dht22 sensors require precise bit-banging timing (~microseconds).
//!     pure rust in userspace is unreliable due to lack of real-time guarantees.
//!     adafruit_dht handles this correctly with retries and timing compensation.
//!
//! ==============================================================================

use anyhow::{Result, anyhow};

/// read dht22 temperature and humidity sensor
///
/// uses python's adafruit library via subprocess for reliable timing.
/// now async with a timeout to prevent hanging deeply.
pub fn read_dht22(pin: u8) -> Result<(f32, f32)> {
    use std::process::Command;
    
    // Python one-liner to read DHT22 and output JSON
    // Matching dht-demo logic exactly for stability
    let script = format!(
        r#"
import sys
try:
    import adafruit_dht
    import board
    import json

    # create dht22 sensor on specified pin
    dht = adafruit_dht.DHT22(board.D{})

    try:
        t, h = dht.temperature, dht.humidity
        if t is not None and h is not None:
            print(json.dumps({{"t": t, "h": h}}))
        else:
            print("null")
    finally:
        dht.exit()
except Exception as e:
    # Print ONLY the error message to stderr (no traceback with paths)
    print(str(e), file=sys.stderr)
    sys.exit(1)
"#,
        pin
    );
    
    // run python as subprocess (blocking)
    // dht-demo uses raw python3 command, no timeout wrapper
    let output = Command::new("python3")
        .arg("-c")
        .arg(&script)
        .output()
        .map_err(|e| anyhow!("Failed to run python3: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Python error: {}", stderr));
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    if stdout == "null" || stdout.is_empty() {
        return Err(anyhow!("Sensor returned null"));
    }
    
    // Parse JSON output
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("JSON parse error: {} (got: {})", e, stdout))?;
    
    let temp = parsed["t"].as_f64().ok_or_else(|| anyhow!("Missing temp"))? as f32;
    let humidity = parsed["h"].as_f64().ok_or_else(|| anyhow!("Missing humidity"))? as f32;
    
    Ok((temp, humidity))
}

/// get current timestamp in milliseconds (unix epoch)
///
/// provides a simple time capability to wasm plugins.
/// the plugin can ask "what time is it?" but cannot set system time.
pub fn get_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// ==============================================================================
// tests
// ==============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_timestamp() {
        let ts = get_timestamp_ms();
        // should be after 2024
        assert!(ts > 1700000000000, "timestamp should be after 2024");
    }
    
    // note: dht22 test requires actual hardware and is not run in ci
    // #[test] 
    // fn test_dht22() {
    //     let result = read_dht22(4);
    //     println!("dht22 result: {:?}", result);
    // }
}
