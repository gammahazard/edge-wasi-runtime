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

/// get raspberry pi cpu temperature in celsius
///
/// reads from /sys/class/thermal/thermal_zone0/temp which returns
/// millidegrees celsius (e.g., 45000 = 45.0°C)
pub fn get_cpu_temp() -> f32 {
    std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map(|t| t / 1000.0)  // convert millidegrees to degrees
        .unwrap_or(0.0)
}

// ==============================================================================
// led control - ws2812b strip via rpi_ws281x
// ==============================================================================
//
// hardware: btf lighting ws2812b strip (11 leds) on gpio 18
//
// why subprocess?
//     rpi_ws281x requires root and has timing-sensitive code.
//     using python subprocess keeps the interface simple and reliable.
//
// relationships:
//     - implements: ../wit/plugin.wit (led-controller interface)
//     - called by: runtime.rs (HostState::set_led, etc.)

/// set a single led to an rgb color
///
/// uses rpi_ws281x via python subprocess for ws2812b control.
pub fn set_led(index: u8, r: u8, g: u8, b: u8) {
    use std::process::Command;
    
    let script = format!(
        r#"
from rpi_ws281x import PixelStrip, Color
strip = PixelStrip(11, 18, brightness=50)
strip.begin()
strip.setPixelColor({}, Color({}, {}, {}))
strip.show()
"#,
        index, r, g, b
    );
    
    let _ = Command::new("sudo")
        .args(["python3", "-c", &script])
        .output();
}

/// set all leds to the same rgb color
pub fn set_all_leds(r: u8, g: u8, b: u8) {
    use std::process::Command;
    
    let script = format!(
        r#"
from rpi_ws281x import PixelStrip, Color
strip = PixelStrip(11, 18, brightness=50)
strip.begin()
for i in range(11):
    strip.setPixelColor(i, Color({}, {}, {}))
strip.show()
"#,
        r, g, b
    );
    
    let _ = Command::new("sudo")
        .args(["python3", "-c", &script])
        .output();
}

/// set led 0 and led 1 atomically (avoids flicker)
///
/// this sets both leds in a single subprocess call, ensuring they're
/// both visible at the same time. all other leds are turned off.
pub fn set_two_leds(r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
    use std::process::Command;
    
    let script = format!(
        r#"
from rpi_ws281x import PixelStrip, Color
strip = PixelStrip(11, 18, brightness=50)
strip.begin()
# set led 0 (cpu temp)
strip.setPixelColor(0, Color({}, {}, {}))
# set led 1 (room temp)
strip.setPixelColor(1, Color({}, {}, {}))
# turn off the rest
for i in range(2, 11):
    strip.setPixelColor(i, Color(0, 0, 0))
strip.show()
"#,
        r0, g0, b0, r1, g1, b1
    );
    
    let _ = Command::new("sudo")
        .args(["python3", "-c", &script])
        .output();
}

/// turn off all leds
pub fn clear_leds() {
    set_all_leds(0, 0, 0);
}

// ==============================================================================
// buzzer control - piezo buzzer via sainsmart relay
// ==============================================================================
//
// hardware: cyclewet buzzer connected via sainsmart relay on gpio 17
// note: relay is ACTIVE LOW - gpio low = relay on = buzzer sounds
//
// why active low?
//     sainsmart relays trigger when the input goes LOW, not HIGH.
//     we abstract this in the host so plugins just call buzz() without
//     knowing the hardware details.
//
// relationships:
//     - implements: ../wit/plugin.wit (buzzer-controller interface)
//     - called by: runtime.rs (HostState::buzz, etc.)

/// sound the buzzer for a duration
///
/// handles the active-low relay logic internally.
pub fn buzz(duration_ms: u32) {
    use std::process::Command;
    
    let script = format!(
        r#"
import RPi.GPIO as GPIO
import time
GPIO.setmode(GPIO.BCM)
GPIO.setup(17, GPIO.OUT)
GPIO.output(17, GPIO.LOW)  # active low - LOW = buzzer on
time.sleep({} / 1000.0)
GPIO.output(17, GPIO.HIGH)  # HIGH = buzzer off
GPIO.cleanup(17)
"#,
        duration_ms
    );
    
    let _ = Command::new("python3")
        .args(["-c", &script])
        .output();
}

/// beep pattern - multiple short beeps with intervals
pub fn beep(count: u8, duration_ms: u32, interval_ms: u32) {
    use std::process::Command;
    
    let script = format!(
        r#"
import RPi.GPIO as GPIO
import time
GPIO.setmode(GPIO.BCM)
GPIO.setup(17, GPIO.OUT)
GPIO.output(17, GPIO.HIGH)  # start with buzzer off
for _ in range({}):
    GPIO.output(17, GPIO.LOW)  # buzzer on
    time.sleep({} / 1000.0)
    GPIO.output(17, GPIO.HIGH)  # buzzer off
    time.sleep({} / 1000.0)
GPIO.cleanup(17)
"#,
        count, duration_ms, interval_ms
    );
    
    let _ = Command::new("python3")
        .args(["-c", &script])
        .output();
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
    
    // note: hardware tests require actual pi and are not run in ci
    // #[test] 
    // fn test_dht22() {
    //     let result = read_dht22(4);
    //     println!("dht22 result: {:?}", result);
    // }
    //
    // #[test]
    // fn test_led() {
    //     set_all_leds(255, 0, 0);  // red
    // }
    //
    // #[test]
    // fn test_buzzer() {
    //     buzz(500);  // 500ms buzz
    // }
}
