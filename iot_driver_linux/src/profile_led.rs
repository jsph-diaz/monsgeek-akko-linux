use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Persistent LED configuration for a single profile
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ProfileLedConfig {
    pub mode: u8,
    pub brightness: u8,
    pub speed: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub dazzle: bool,
}

/// Persistent LED configuration for all profiles of a device
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DeviceLedConfig {
    pub profiles: HashMap<u8, ProfileLedConfig>,
}

/// Global persistent configuration for all devices
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AllDevicesConfig {
    pub devices: HashMap<u32, DeviceLedConfig>,
}

impl AllDevicesConfig {
    /// Get path to the configuration file (~/.config/monsgeek-akko-linux/led_profiles.json)
    pub fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let mut path = PathBuf::from(home);
        path.push(".config");
        path.push("monsgeek-akko-linux");
        path.push("led_profiles.json");
        path
    }

    /// Load configuration from disk
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save configuration to disk
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, content)
    }

    /// Get LED settings for a specific device and profile
    pub fn get_profile_led(&self, device_id: u32, profile: u8) -> Option<&ProfileLedConfig> {
        self.devices.get(&device_id)?.profiles.get(&profile)
    }

    /// Update LED settings for a specific device and profile
    pub fn set_profile_led(&mut self, device_id: u32, profile: u8, config: ProfileLedConfig) {
        self.devices
            .entry(device_id)
            .or_default()
            .profiles
            .insert(profile, config);
    }
}
