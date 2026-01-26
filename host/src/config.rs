//! ==============================================================================
//! config.rs - Runtime Configuration Loader
//! ==============================================================================
//!
//! purpose:
//!     defines the schema for `host.toml`.
//!     loads configuration from file or falls back to defaults.
//!
//! structure:
//!     - RaftConfig: Identity (node_id) and Peers (who else is in the cluster).
//!     - PollingConfig: How often the Leader polls sensors.
//!     - SensorsConfig: GPIO pins and I2C addresses.
//!     - PluginsConfig: Toggles for individual WASM plugins.
//!
//! ==============================================================================

use serde::Deserialize;
use std::path::Path;

/// Root configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct HostConfig {
    pub polling: PollingConfig,
    pub sensors: SensorsConfig,
    #[allow(dead_code)]
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
#[allow(dead_code)]
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
    #[allow(dead_code)]
    pub show_sensor_data: bool,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ClusterConfig {
    pub role: String,      // "hub" or "spoke"
    pub node_id: String,
    pub hub_url: String,   // URL to push data to (if spoke)
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct PluginEntry {
    pub enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    pub led: Option<u8>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct PluginsConfig {
    #[serde(default)]
    pub dht22: PluginEntry,
    #[serde(default)]
    pub pi4_monitor: PluginEntry,
    #[serde(default)]
    pub revpi_monitor: PluginEntry,
    #[serde(default)]
    pub bme680: PluginEntry,
    #[allow(dead_code)]
    #[serde(default)]
    pub dashboard: PluginEntry,
    #[allow(dead_code)]
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
            std::path::PathBuf::from("config").join("host.toml"),
            std::path::PathBuf::from("..").join("config").join("host.toml"),
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
    
    /// Print configuration summary
    pub fn print_summary(&self) {
        println!("┌─────────────────────────────────────────┐");
        println!("│           HOST CONFIGURATION            │");
        println!("├─────────────────────────────────────────┤");
        println!("│ Role: {}                             │", self.cluster.role);
        println!("│ Node ID: {}                          │", self.cluster.node_id);
        println!("│ Poll Interval: {}s                      │", self.polling.interval_seconds);
        println!("│ Log Level: {}                        │", self.logging.level);
        println!("├─────────────────────────────────────────┤");
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
