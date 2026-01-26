//! ==============================================================================
//! hal.rs - Hardware Abstraction Layer
//! ==============================================================================
//!
//! purpose:
//!     provides a unified interface for hardware access (GPIO, I2C, SPI).
//!     abstracts away the difference between running on a real Raspberry Pi
//!     (using `rppal`) and a development machine (using mocks).
//!
//! design philosophy:
//!     - "Compile Anywhere": The host should compile on Windows/Mac/Linux.
//!     - "Zero Cost": On the Pi, this compiles down to direct `rppal` calls.
//!     - "Safety": Enforces proper locking/sharing of I2C bus if needed.
//!
//! relationships:
//!     - used by: runtime.rs (to fulfill wit contracts for plugins)
//!     - uses: rppal (on feature="hardware")
//!     - uses: std::process::Command (for legacy Python DHT driver until ported)
//!
//! ==============================================================================

use anyhow::Result;

pub trait HardwareProvider: Send + Sync {
    fn i2c_transfer(&self, addr: u8, write_data: &[u8], read_len: u32) -> Result<Vec<u8>>;
    #[allow(dead_code)]
    fn spi_transfer(&self, data: &[u8]) -> Result<Vec<u8>>;
    fn set_gpio_mode(&self, pin: u8, mode: &str) -> Result<()>;
    fn write_gpio(&self, pin: u8, level: bool) -> Result<()>;
    fn set_led(&self, index: u8, r: u8, g: u8, b: u8) -> Result<()>;
    fn sync_leds(&self) -> Result<()>;
    fn read_dht22(&self, pin: u8) -> Result<(f32, f32)>;
    fn get_cpu_temp(&self) -> f32;
}

// ==============================================================================================
// MOCK IMPLEMENTATION (For WSL / Non-Hardware Build)
// ==============================================================================================
#[cfg(not(feature = "hardware"))]
pub struct Hal {}
#[cfg(not(feature = "hardware"))]
static MOCK_LED_BUFFER: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<[(u8, u8, u8); 11]>>> = std::sync::OnceLock::new();

#[cfg(not(feature = "hardware"))]
impl Hal {
    pub fn new() -> Self {
        tracing::info!("Using MOCK HAL (No hardware access)");
        MOCK_LED_BUFFER.get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new([(0, 0, 0); 11])));
        Self {}
    }

    fn get_buffer(&self) -> std::sync::Arc<std::sync::Mutex<[(u8, u8, u8); 11]>> {
        MOCK_LED_BUFFER.get().unwrap().clone()
    }
}

#[cfg(not(feature = "hardware"))]
impl HardwareProvider for Hal {
    fn set_led(&self, index: u8, r: u8, g: u8, b: u8) -> Result<()> {
        if index < 11 {
            let arc = self.get_buffer();
            let mut buffer = arc.lock().unwrap();
            buffer[index as usize] = (r, g, b);
            tracing::debug!("[MOCK LED] Set LED {} to RBG({}, {}, {})", index, r, g, b);
        }
        Ok(())
    }

    fn sync_leds(&self) -> Result<()> {
        let arc = self.get_buffer();
        let buffer = arc.lock().unwrap();
        tracing::debug!("[MOCK LED] Syncing buffer: {:?}", *buffer);
        Ok(())
    }
    fn i2c_transfer(&self, addr: u8, write_data: &[u8], read_len: u32) -> Result<Vec<u8>> {
        tracing::debug!("[MOCK I2C] Addr: 0x{:02X}, Write: {:?}, ReadLen: {}", addr, write_data, read_len);
        Ok(vec![0u8; read_len as usize])
    }

    fn spi_transfer(&self, data: &[u8]) -> Result<Vec<u8>> {
        tracing::debug!("[MOCK SPI] Write: {:?} ({} bytes)", data, data.len());
        Ok(data.to_vec()) // Loopback
    }

    fn set_gpio_mode(&self, pin: u8, mode: &str) -> Result<()> {
        tracing::debug!("[MOCK GPIO] Pin {} set to {}", pin, mode);
        Ok(())
    }

    fn write_gpio(&self, pin: u8, level: bool) -> Result<()> {
        tracing::debug!("[MOCK GPIO] Pin {} write {}", pin, level);
        Ok(())
    }

    fn read_dht22(&self, pin: u8) -> Result<(f32, f32)> {
        tracing::debug!("[MOCK DHT22] Reading pin {}", pin);
        Ok((25.0, 50.0)) // Mock data
    }

    fn get_cpu_temp(&self) -> f32 {
        45.0 // Mock data
    }
}

// ==============================================================================================
// REAL IMPLEMENTATION (For Raspberry Pi)
// ==============================================================================================
#[cfg(feature = "hardware")]
pub struct Hal {}
#[cfg(feature = "hardware")]
static REAL_LED_BUFFER: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<[(u8, u8, u8); 11]>>> = std::sync::OnceLock::new();

#[cfg(feature = "hardware")]
impl Hal {
    pub fn new() -> Self {
        tracing::info!("Using REAL HARDWARE HAL (rppal)");
        REAL_LED_BUFFER.get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new([(0, 0, 0); 11])));
        Self {}
    }

    fn get_buffer(&self) -> std::sync::Arc<std::sync::Mutex<[(u8, u8, u8); 11]>> {
        REAL_LED_BUFFER.get().unwrap().clone()
    }
}

#[cfg(feature = "hardware")]
impl HardwareProvider for Hal {
    fn set_led(&self, index: u8, r: u8, g: u8, b: u8) -> Result<()> {
        if index < 11 {
            let arc = self.get_buffer();
            let mut buffer = arc.lock().unwrap();
            buffer[index as usize] = (r, g, b);
        }
        Ok(())
    }

    fn sync_leds(&self) -> Result<()> {
        use std::process::Command;
        
        let data = {
            let arc = self.get_buffer();
            let buffer = arc.lock().unwrap();
            buffer.clone()
        };
        
        // Generate python script to set the whole strip
        let mut pixel_logic = String::new();
        for (i, (r, g, b)) in data.iter().enumerate() {
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
        Ok(())
    }
    fn i2c_transfer(&self, addr: u8, write_data: &[u8], read_len: u32) -> Result<Vec<u8>> {
        use rppal::i2c::I2c;
        let mut i2c = I2c::new()?;
        i2c.set_slave_address(addr as u16)?;
        
        if !write_data.is_empty() {
             i2c.write(write_data)?;
        }
        
        if read_len > 0 {
            let mut read_buf = vec![0u8; read_len as usize];
            i2c.read(&mut read_buf)?;
            Ok(read_buf)
        } else {
            Ok(vec![])
        }
    }

    fn spi_transfer(&self, data: &[u8]) -> Result<Vec<u8>> {
        use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
        let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode0)?;
        let mut read_buf = vec![0u8; data.len()];
        spi.transfer(&mut read_buf, data)?;
        Ok(read_buf)
    }

    fn set_gpio_mode(&self, _pin: u8, _mode: &str) -> Result<()> {
        Ok(())
    }

    fn write_gpio(&self, pin: u8, level: bool) -> Result<()> {
        use rppal::gpio::Gpio;
        let gpio = Gpio::new()?;
        let mut p = gpio.get(pin)?.into_output();
        if level { p.set_high(); } else { p.set_low(); }
        Ok(())
    }

    fn read_dht22(&self, pin: u8) -> Result<(f32, f32)> {
        // NOTE: For now, we fallback to Python subprocess for DHT22 stability on generic Linux kernels
        // native bit-banging is notoriously flaky without a kernel driver.
        use std::process::Command;
        let script = format!(
            r#"
import adafruit_dht, board, json, sys
try:
    dht = adafruit_dht.DHT22(board.D{})
    print(json.dumps({{"t": dht.temperature, "h": dht.humidity}}))
except Exception:
    print("null")
"#,
            pin
        );
        let output = Command::new("python3").args(["-c", &script]).output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim() == "null" {
            anyhow::bail!("DHT22 read failed");
        }
        let v: serde_json::Value = serde_json::from_str(&stdout)?;
        Ok((
            v["t"].as_f64().unwrap_or(0.0) as f32,
            v["h"].as_f64().unwrap_or(0.0) as f32
        ))
    }

    fn get_cpu_temp(&self) -> f32 {
        std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .map(|t| t / 1000.0)
            .unwrap_or(0.0)
    }
}
