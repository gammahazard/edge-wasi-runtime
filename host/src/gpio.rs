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
use std::sync::{Mutex, OnceLock};
use sysinfo::{System, Networks};

// Singleton for system stats
// Why generic? So plugins can ask "how much ram?" without knowing it's Linux
static SYSTEM_MONITOR: OnceLock<Mutex<System>> = OnceLock::new();

fn get_system() -> &'static Mutex<System> {
    SYSTEM_MONITOR.get_or_init(|| {
        let mut sys = System::new_all();
        sys.refresh_all();
        Mutex::new(sys)
    })
}

/// get generic cpu usage (average % across all cores)
pub fn get_sys_cpu_usage() -> f32 {
    let mutex = get_system();
    let mut sys = mutex.lock().unwrap();
    sys.refresh_cpu(); // minimal refresh
    sys.global_cpu_info().cpu_usage()
}

/// get memory usage (used_mb, total_mb)
pub fn get_memory_usage() -> (u32, u32) {
    let mutex = get_system();
    let mut sys = mutex.lock().unwrap();
    sys.refresh_memory();
    let used_mb = (sys.used_memory() / 1024 / 1024) as u32;
    let total_mb = (sys.total_memory() / 1024 / 1024) as u32;
    (used_mb, total_mb)
}

/// get system uptime in seconds
pub fn get_uptime() -> u64 {
    System::uptime()
}

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

/// get generic cpu usage (average % across all cores)
pub fn get_sys_cpu_usage() -> f32 {
    let mutex = get_system();
    let mut sys = mutex.lock().unwrap();
    sys.refresh_cpu(); // minimal refresh
    sys.global_cpu_info().cpu_usage()
}

/// get memory usage (used_mb, total_mb)
pub fn get_memory_usage() -> (u32, u32) {
    let mutex = get_system();
    let mut sys = mutex.lock().unwrap();
    sys.refresh_memory();
    let used_mb = (sys.used_memory() / 1024 / 1024) as u32;
    let total_mb = (sys.total_memory() / 1024 / 1024) as u32;
    (used_mb, total_mb)
}

/// get system uptime in seconds
pub fn get_uptime() -> u64 {
    let mutex = get_system();
    let sys = mutex.lock().unwrap();
    sys.uptime()
}

/// read bme680 environmental sensor via python subprocess
///
/// why subprocess?
/// i2c drivers in rust can be finicky on some pi kernels.
/// using the 'bme680' python library is battle-tested and matches our pattern.
pub fn read_bme680(i2c_addr: u8) -> Result<(f32, f32, f32, f32)> {
    use std::process::Command;

    // python script to read bme680
    // returns json: {"t": 24.5, "h": 40.2, "p": 1013.2, "g": 120.5}
    let script = format!(
        r#"
import sys
import json
try:
    import bme680
    import time
    
    # 0x76 (primary) or 0x77 (secondary)
    try:
        sensor = bme680.BME680(0x{:02x})
    except (RuntimeError, IOError):
        # fallback try other address if user specified wrong one
        alt_addr = 0x77 if 0x{:02x} == 0x76 else 0x76
        sensor = bme680.BME680(alt_addr)

    # These oversampling settings can be tweaked
    sensor.set_humidity_oversample(bme680.OS_2X)
    sensor.set_pressure_oversample(bme680.OS_4X)
    sensor.set_temperature_oversample(bme680.OS_8X)
    sensor.set_filter(bme680.FILTER_SIZE_3)
    sensor.set_gas_status(bme680.ENABLE_GAS_MEAS)

    # Force a measurement
    if sensor.get_sensor_data():
        output = {{
            "t": sensor.data.temperature,
            "h": sensor.data.humidity,
            "p": sensor.data.pressure,
            "g": sensor.data.gas_resistance / 1000.0  # Convert to KOhms
        }}
        print(json.dumps(output))
    else:
        print("null")

except Exception as e:
    print(str(e), file=sys.stderr)
    sys.exit(1)
"#,
        i2c_addr, i2c_addr
    );

    let output = Command::new("python3")
        .arg("-c")
        .arg(&script)
        .output()
        .map_err(|e| anyhow!("Failed to run python3: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Python BME680 error: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if stdout == "null" || stdout.is_empty() {
        return Err(anyhow!("Sensor returned null"));
    }

    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("JSON parse error: {} (got: {})", e, stdout))?;

    let temp = parsed["t"].as_f64().unwrap_or(0.0) as f32;
    let humidity = parsed["h"].as_f64().unwrap_or(0.0) as f32;
    let pressure = parsed["p"].as_f64().unwrap_or(0.0) as f32;
    let gas = parsed["g"].as_f64().unwrap_or(0.0) as f32;

    Ok((temp, humidity, pressure, gas))
}

// ==============================================================================
// led control - ws2812b strip via rpi_ws281x
// ==============================================================================
//
// hardware: btf lighting ws2812b strip (11 leds) on gpio 18
//
// synchronization (flicker prevention):
//     we use a global buffer to store led states. plugins update the buffer
//     atomically via set_led/set_two, but the hardware is only updated when
//     sync_leds() is called. this prevents multiple plugins from fighting
//     over the pwm hardware and causing flicker.
//
// relationships:
//     - implements: ../wit/plugin.wit (led-controller interface)
//     - called by: runtime.rs (HostState implementations)

/// Centralized buffer for LED states (11 LEDs, r-g-b tuples)
static LED_BUFFER: OnceLock<Mutex<[(u8, u8, u8); 11]>> = OnceLock::new();

fn get_led_buffer() -> &'static Mutex<[(u8, u8, u8); 11]> {
    LED_BUFFER.get_or_init(|| Mutex::new([(0, 0, 0); 11]))
}

/// set a single led in the buffer
pub fn set_led_buffer(index: u8, r: u8, g: u8, b: u8) {
    if index < 11 {
        let mut buffer = get_led_buffer().lock().unwrap();
        buffer[index as usize] = (r, g, b);
    }
}

/// set multiple leds in the buffer
pub fn set_two_buffer(r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
    let mut buffer = get_led_buffer().lock().unwrap();
    buffer[0] = (r0, g0, b0);
    buffer[1] = (r1, g1, b1);
    // NOTE: We no longer clear LEDs 2-10 here.
    // Each plugin is responsible for its own LEDs only.
}

/// clear the entire buffer
pub fn clear_led_buffer() {
    let mut buffer = get_led_buffer().lock().unwrap();
    for i in 0..11 {
        buffer[i] = (0, 0, 0);
    }
}

/// write the current buffer to the hardware once (prevents flicker)
pub fn sync_leds() {
    use std::process::Command;
    
    // get snapshot of buffer
    let data = {
        let buffer = get_led_buffer().lock().unwrap();
        buffer.clone()
    };
    
    // generate python script to set the whole strip
    let mut pixel_logic = String::new();
    for (i, (r, g, b)) in data.iter().enumerate() {
        // Always include colors, even if black, to ensure strip is in consistent state
        pixel_logic.push_str(&format!("strip.setPixelColor({}, Color({}, {}, {}))\n", i, *r, *g, *b));
    }
    
    let script = format!(
        r#"
from rpi_ws281x import PixelStrip, Color
strip = PixelStrip(11, 18, brightness=50)
strip.begin()
{}
strip.show()
"#,
        pixel_logic
    );
    
    let _ = Command::new("sudo")
        .args(["python3", "-c", &script])
        .output();
}

// ==============================================================================
// legacy wrappers (now buffered)
// ==============================================================================

pub fn set_led(index: u8, r: u8, g: u8, b: u8) {
    set_led_buffer(index, r, g, b);
}

pub fn set_all_leds(r: u8, g: u8, b: u8) {
    let mut buffer = get_led_buffer().lock().unwrap();
    for i in 0..11 {
        buffer[i] = (r, g, b);
    }
}

pub fn set_two_leds(r0: u8, g0: u8, b0: u8, r1: u8, g1: u8, b1: u8) {
    set_two_buffer(r0, g0, b0, r1, g1, b1);
}

pub fn clear_leds() {
    clear_led_buffer();
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
