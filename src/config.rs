use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Desired charge limit (20–100)
    pub charge_limit: u8,
    /// Apply charge limit on app startup
    pub auto_apply_on_start: bool,
    /// Make limit persistent via systemd service
    pub persistent_limit: bool,
    /// Refresh interval in seconds
    pub refresh_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            charge_limit: 80,
            auto_apply_on_start: false,
            persistent_limit: false,
            refresh_secs: 5,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        match fs::read_to_string(Self::path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ghelper-linux")
            .join("config.json")
    }
}
