use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiomonConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    /// Sensor label overrides: "hwmon/nct6798/in0" -> "Vcore"
    #[serde(default)]
    pub sensor_labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_interval")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_true")]
    pub physical_net_only: bool,
    #[serde(default)]
    pub no_nvidia: bool,
    #[serde(default = "default_color")]
    pub color: String,
    /// Block device name prefixes to exclude from storage listings and disk sensors.
    #[serde(default = "default_storage_exclude")]
    pub storage_exclude: Vec<String>,
}

fn default_format() -> String {
    "text".into()
}
fn default_interval() -> u64 {
    1000
}
fn default_true() -> bool {
    true
}
fn default_color() -> String {
    "auto".into()
}
fn default_storage_exclude() -> Vec<String> {
    ["loop", "dm-", "ram", "zram", "sr", "nbd", "zd", "md"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            format: default_format(),
            poll_interval_ms: default_interval(),
            physical_net_only: default_true(),
            no_nvidia: false,
            color: default_color(),
            storage_exclude: default_storage_exclude(),
        }
    }
}

impl SiomonConfig {
    /// Load the configuration from disk. Returns defaults if the file is missing
    /// or cannot be parsed.
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        log::warn!("Failed to parse config {}: {e}", path.display());
                        Self::default()
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read config {}: {e}", path.display());
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }
}

/// Return the path to the configuration file.
///
/// Uses `$XDG_CONFIG_HOME/siomon/config.toml` if `XDG_CONFIG_HOME` is set,
/// otherwise falls back to `$HOME/.config/siomon/config.toml`.
pub fn config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("siomon").join("config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join("siomon")
            .join("config.toml")
    } else {
        // Last resort: relative path (unlikely to be useful, but avoids a panic)
        PathBuf::from(".config").join("siomon").join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = SiomonConfig::default();
        assert_eq!(cfg.general.format, "text");
        assert_eq!(cfg.general.poll_interval_ms, 1000);
        assert!(cfg.general.physical_net_only);
        assert!(!cfg.general.no_nvidia);
        assert_eq!(cfg.general.color, "auto");
        assert!(cfg.general.storage_exclude.contains(&"zd".to_string()));
        assert!(cfg.general.storage_exclude.contains(&"loop".to_string()));
        assert!(cfg.sensor_labels.is_empty());
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = "";
        let cfg: SiomonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.general.format, "text");
        assert!(cfg.sensor_labels.is_empty());
    }

    #[test]
    fn test_parse_full_toml() {
        let toml_str = r#"
[general]
format = "json"
poll_interval_ms = 500
physical_net_only = false
no_nvidia = true
color = "never"
storage_exclude = ["loop", "zd", "custom"]

[sensor_labels]
"hwmon/nct6798/in0" = "Vcore"
"hwmon/nct6798/fan1" = "CPU Fan"
"#;
        let cfg: SiomonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.general.format, "json");
        assert_eq!(cfg.general.poll_interval_ms, 500);
        assert!(!cfg.general.physical_net_only);
        assert!(cfg.general.no_nvidia);
        assert_eq!(cfg.general.color, "never");
        assert_eq!(cfg.general.storage_exclude, vec!["loop", "zd", "custom"]);
        assert_eq!(cfg.sensor_labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(
            cfg.sensor_labels.get("hwmon/nct6798/fan1").unwrap(),
            "CPU Fan"
        );
    }

    #[test]
    fn test_config_path_uses_xdg() {
        // Just verify the function doesn't panic
        let path = config_path();
        assert!(path.to_str().unwrap().contains("siomon"));
        assert!(path.to_str().unwrap().ends_with("config.toml"));
    }
}
