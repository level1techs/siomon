//! Configuration file loading and management (`~/.config/siomon/config.toml`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::model::sensor::SensorCategory;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiomonConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    /// Sensor label overrides: "hwmon/nct6798/in0" -> "Vcore"
    #[serde(default)]
    pub sensor_labels: HashMap<String, String>,
    /// Hwmon voltage scaling multipliers: "hwmon/it8688/in2" -> 6.0
    #[serde(default)]
    pub voltage_scaling: HashMap<String, f64>,
    #[serde(default)]
    pub dashboard: DashboardConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DashboardConfig {
    /// User-defined panels. If non-empty, replaces all built-in panels.
    #[serde(default)]
    pub panels: Vec<PanelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    pub title: String,
    /// Glob pattern matched against "source/chip/sensor".
    #[serde(default)]
    pub filter: Option<String>,
    /// Category filter ANDed with `filter` (e.g. "temperature", "power").
    #[serde(default)]
    pub category: Option<String>,
    /// Per-panel max entries override (still clamped by adaptive limit).
    #[serde(default)]
    pub max_entries: Option<usize>,
    /// Whether to show sparklines (default true).
    #[serde(default = "default_true")]
    pub sparklines: bool,
    /// Sort order: "desc" (default), "asc", "name".
    #[serde(default)]
    pub sort: Option<String>,
}

/// Parse a category name (case-insensitive) into a `SensorCategory`.
pub fn parse_category(s: &str) -> Option<SensorCategory> {
    match s.to_ascii_lowercase().as_str() {
        "temperature" | "temp" => Some(SensorCategory::Temperature),
        "voltage" | "volt" => Some(SensorCategory::Voltage),
        "current" => Some(SensorCategory::Current),
        "power" => Some(SensorCategory::Power),
        "fan" => Some(SensorCategory::Fan),
        "frequency" | "freq" => Some(SensorCategory::Frequency),
        "utilization" | "util" => Some(SensorCategory::Utilization),
        "throughput" => Some(SensorCategory::Throughput),
        "memory" => Some(SensorCategory::Memory),
        "other" => Some(SensorCategory::Other),
        _ => None,
    }
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
    #[serde(default = "default_theme")]
    pub theme: String,
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
fn default_theme() -> String {
    "default".into()
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
            theme: default_theme(),
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
        assert_eq!(cfg.general.theme, "default");
        assert!(cfg.general.storage_exclude.contains(&"zd".to_string()));
        assert!(cfg.general.storage_exclude.contains(&"loop".to_string()));
        assert!(cfg.sensor_labels.is_empty());
        assert!(cfg.voltage_scaling.is_empty());
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = "";
        let cfg: SiomonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.general.format, "text");
        assert!(cfg.sensor_labels.is_empty());
        assert!(cfg.voltage_scaling.is_empty());
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
theme = "high-contrast"
storage_exclude = ["loop", "zd", "custom"]

[sensor_labels]
"hwmon/nct6798/in0" = "Vcore"
"hwmon/nct6798/fan1" = "CPU Fan"

[voltage_scaling]
"hwmon/it8688/in2" = 6.0
"hwmon/it8688/in3" = 2.5
"#;
        let cfg: SiomonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.general.format, "json");
        assert_eq!(cfg.general.poll_interval_ms, 500);
        assert!(!cfg.general.physical_net_only);
        assert!(cfg.general.no_nvidia);
        assert_eq!(cfg.general.color, "never");
        assert_eq!(cfg.general.theme, "high-contrast");
        assert_eq!(cfg.general.storage_exclude, vec!["loop", "zd", "custom"]);
        assert_eq!(cfg.sensor_labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(
            cfg.sensor_labels.get("hwmon/nct6798/fan1").unwrap(),
            "CPU Fan"
        );
        assert_eq!(*cfg.voltage_scaling.get("hwmon/it8688/in2").unwrap(), 6.0);
        assert_eq!(*cfg.voltage_scaling.get("hwmon/it8688/in3").unwrap(), 2.5);
    }

    #[test]
    fn test_config_path_uses_xdg() {
        // Just verify the function doesn't panic
        let path = config_path();
        assert!(path.to_str().unwrap().contains("siomon"));
        assert!(path.to_str().unwrap().ends_with("config.toml"));
    }

    #[test]
    fn test_empty_dashboard_defaults() {
        let cfg: SiomonConfig = toml::from_str("").unwrap();
        assert!(cfg.dashboard.panels.is_empty());
    }

    #[test]
    fn test_parse_dashboard_panels() {
        let toml_str = r#"
[[dashboard.panels]]
title = "GPU Temps"
filter = "gpu/*"
category = "temperature"
max_entries = 12

[[dashboard.panels]]
title = "All Power"
category = "power"
sparklines = false
sort = "name"
"#;
        let cfg: SiomonConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.dashboard.panels.len(), 2);

        let p0 = &cfg.dashboard.panels[0];
        assert_eq!(p0.title, "GPU Temps");
        assert_eq!(p0.filter.as_deref(), Some("gpu/*"));
        assert_eq!(p0.category.as_deref(), Some("temperature"));
        assert_eq!(p0.max_entries, Some(12));
        assert!(p0.sparklines);
        assert!(p0.sort.is_none());

        let p1 = &cfg.dashboard.panels[1];
        assert_eq!(p1.title, "All Power");
        assert!(p1.filter.is_none());
        assert_eq!(p1.category.as_deref(), Some("power"));
        assert!(p1.max_entries.is_none());
        assert!(!p1.sparklines);
        assert_eq!(p1.sort.as_deref(), Some("name"));
    }

    #[test]
    fn test_parse_category() {
        assert_eq!(
            parse_category("temperature"),
            Some(SensorCategory::Temperature)
        );
        assert_eq!(parse_category("Temp"), Some(SensorCategory::Temperature));
        assert_eq!(parse_category("POWER"), Some(SensorCategory::Power));
        assert_eq!(parse_category("fan"), Some(SensorCategory::Fan));
        assert_eq!(parse_category("freq"), Some(SensorCategory::Frequency));
        assert_eq!(parse_category("util"), Some(SensorCategory::Utilization));
        assert!(parse_category("nonexistent").is_none());
    }
}
