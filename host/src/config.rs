//! ==============================================================================
//! config.rs - Runtime configuration loader
//! ==============================================================================
//!
//! purpose:
//!     loads host.toml and provides runtime configuration to the rest of the system.
//!     this allows behavior changes without recompiling the rust host.
//!
//! philosophy:
//!     "compile once, swap wasm" - the host should be as stable as an OS kernel.
//!     all frequently-changing values (poll interval, GPIO pins, thresholds)
//!     should live in config files or wasm plugins.
//!
//! ==============================================================================

use serde::Deserialize;
use std::path::Path;

/// Root configuration structure matching host.toml
#[derive(Debug, Deserialize, Clone)]
pub struct HostConfig {
    pub polling: PollingConfig,
    pub sensors: SensorsConfig,
    pub leds: LedConfig,
    pub buzzer: BuzzerConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub cluster: ClusterConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PollingConfig {
    pub interval_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SensorsConfig {
    pub dht22: Dht22Config,
    pub bme680: Bme680Config,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Dht22Config {
    pub gpio_pin: u8,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Bme680Config {
    pub i2c_address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LedConfig {
    pub count: u8,
    pub gpio_pin: u8,
    pub brightness: u8,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BuzzerConfig {
    pub gpio_pin: u8,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    pub level: String,
    pub show_sensor_data: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClusterConfig {
    pub role: String,      // "hub" or "spoke"
    pub hub_url: String,   // e.g. "http://192.168.40.9:3000/api/readings"
    pub node_id: String,   // e.g. "pi4-sensor-node"
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            role: "standalone".to_string(),
            hub_url: "".to_string(),
            node_id: "unknown".to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct PluginEntry {
    pub enabled: bool,
    #[serde(default)]
    #[allow(dead_code)]
    pub led: Option<u8>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct PluginsConfig {
    #[serde(default)]
    pub dht22: PluginEntry,
    #[serde(default)]
    pub pi_monitor: PluginEntry,
    #[serde(default)]
    pub bme680: PluginEntry,
    #[serde(default)]
    pub dashboard: PluginEntry,
    #[serde(default)]
    pub oled: PluginEntry,
}

impl HostConfig {
    /// Load configuration from file
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to read config file: {}", e))?;
        
        let config: HostConfig = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;
        
        Ok(config)
    }
    
    /// Load with default fallback
    pub fn load_or_default() -> Self {
        let paths = [
            std::path::PathBuf::from("config").join("host.toml"),      // Docker / Production
            std::path::PathBuf::from("..").join("config").join("host.toml"), // Local Development
        ];

        for path in &paths {
            if path.exists() {
                match Self::load(path) {
                    Ok(config) => {
                        println!("[CONFIG] Loaded from {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        println!("[CONFIG] Warning: Failed to load {}: {}", path.display(), e);
                    }
                }
            }
        }
        
        println!("[CONFIG] Warning: No config file found - using defaults");
        Self::default()
    }
    
    /// Print configuration summary for logging
    pub fn print_summary(&self) {
        println!("┌─────────────────────────────────────────┐");
        println!("│           HOST CONFIGURATION            │");
        println!("├─────────────────────────────────────────┤");
        println!("│ Poll Interval: {}s                      │", self.polling.interval_seconds);
        println!("│ DHT22 GPIO: {}                          │", self.sensors.dht22.gpio_pin);
        println!("│ BME680 I2C: {}                       │", self.sensors.bme680.i2c_address);
        println!("│ LED Count: {} (GPIO {}, bright {})      │", self.leds.count, self.leds.gpio_pin, self.leds.brightness);
        println!("│ Buzzer GPIO: {}                         │", self.buzzer.gpio_pin);
        println!("│ Log Level: {}                        │", self.logging.level);
        println!("│ Cluster Role: {} ({})                  │", self.cluster.role, self.cluster.node_id);
        println!("├─────────────────────────────────────────┤");
        println!("│ Plugins:                                │");
        println!("│   dht22: {}   pi-monitor: {}            │", 
            if self.plugins.dht22.enabled { "✓" } else { "✗" },
            if self.plugins.pi_monitor.enabled { "✓" } else { "✗" });
        println!("│   bme680: {}  dashboard: {}             │",
            if self.plugins.bme680.enabled { "✓" } else { "✗" },
            if self.plugins.dashboard.enabled { "✓" } else { "✗" });
        println!("└─────────────────────────────────────────┘");
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            polling: PollingConfig { interval_seconds: 5 },
            sensors: SensorsConfig {
                dht22: Dht22Config { gpio_pin: 4 },
                bme680: Bme680Config { i2c_address: "0x77".to_string() },
            },
            leds: LedConfig { count: 11, gpio_pin: 18, brightness: 50 },
            buzzer: BuzzerConfig { gpio_pin: 17 },
            logging: LoggingConfig { level: "info".to_string(), show_sensor_data: true },
            cluster: ClusterConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}
