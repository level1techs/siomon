use std::collections::HashMap;

use super::boards::Platform;

/// Load sensor label overrides and detect the board platform. Checks:
/// 1. Built-in board-specific labels (matched by board name from DMI)
/// 2. User overrides from config file (these take precedence)
///
/// Returns (labels, platform) where platform is from the matched board
/// template, or falls back to filesystem detection for Tegra.
pub fn load_labels(
    board_name: Option<&str>,
    user_labels: &HashMap<String, String>,
) -> (HashMap<String, String>, Platform) {
    let mut labels = HashMap::new();
    let mut platform = Platform::Generic;

    // Built-in board labels via board template
    if let Some(board) = board_name.and_then(super::boards::lookup_board) {
        labels = super::boards::resolve_labels(board);
        platform = board.platform;
    }

    // Filesystem fallback: detect Tegra even without a board template match
    if platform == Platform::Generic && crate::platform::tegra::is_tegra() {
        platform = Platform::Tegra;
    }

    // User labels override built-ins
    labels.extend(user_labels.iter().map(|(k, v)| (k.clone(), v.clone())));

    (labels, platform)
}

/// Load hwmon voltage scaling multipliers. Checks:
/// 1. Built-in board-specific scaling (matched by board name from DMI)
/// 2. User overrides from config file (these take precedence)
pub fn load_voltage_scaling(
    board_name: Option<&str>,
    user_scaling: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut scaling = HashMap::new();

    if let Some(board) = board_name.and_then(super::boards::lookup_board) {
        scaling = super::boards::resolve_voltage_scaling(board);
    }

    // User scaling overrides built-ins
    scaling.extend(user_scaling.iter().map(|(k, &v)| (k.clone(), v)));

    scaling
}

/// Read the board name from DMI sysfs.
pub fn read_board_name() -> Option<String> {
    crate::platform::sysfs::read_string_optional(std::path::Path::new(
        "/sys/class/dmi/id/board_name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_labels_wrx90e() {
        let (labels, _) = load_labels(Some("Pro WS WRX90E-SAGE SE"), &HashMap::new());
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(labels.get("hwmon/nct6798/fan7").unwrap(), "AIO Pump");
    }

    #[test]
    fn test_builtin_labels_asrock_wrx90() {
        let (labels, _) = load_labels(Some("WRX90 WS EVO"), &HashMap::new());
        assert_eq!(labels.get("hwmon/nct6799/in0").unwrap(), "Vcore");
        assert_eq!(labels.get("hwmon/nct6799/fan1").unwrap(), "CPU Fan 1");
        assert_eq!(labels.get("superio/nct6799/vin0").unwrap(), "Vcore");
        assert_eq!(labels.get("superio/nct6799/fan6").unwrap(), "MOS Fan 1");
        // Should NOT match ASUS WRX90E labels
        assert!(!labels.contains_key("hwmon/nct6798/in0"));
    }

    #[test]
    fn test_builtin_labels_crosshair_x670() {
        let (labels, _) = load_labels(Some("ROG CROSSHAIR X670E HERO"), &HashMap::new());
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(labels.get("hwmon/nct6798/in4").unwrap(), "+12V");
        assert_eq!(labels.get("hwmon/nct6798/fan1").unwrap(), "CPU Fan");
        assert_eq!(labels.get("hwmon/nct6798/fan2").unwrap(), "CPU OPT");
    }

    #[test]
    fn test_builtin_labels_strix_x670e() {
        let (labels, _) = load_labels(Some("ROG STRIX X670E-E GAMING WIFI"), &HashMap::new());
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(labels.get("hwmon/nct6798/in1").unwrap(), "+5V");
        assert_eq!(labels.get("hwmon/nct6798/fan1").unwrap(), "CPU Fan");
        assert_eq!(labels.get("hwmon/nct6798/fan2").unwrap(), "Chassis Fan 1");
    }

    #[test]
    fn test_builtin_labels_tuf_b650() {
        let (labels, _) = load_labels(Some("TUF GAMING B650-PLUS WIFI"), &HashMap::new());
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(labels.get("hwmon/nct6798/fan1").unwrap(), "CPU Fan");
        assert_eq!(labels.get("hwmon/nct6798/fan2").unwrap(), "Chassis Fan 1");
    }

    #[test]
    fn test_builtin_labels_prime_x670e() {
        let (labels, _) = load_labels(Some("PRIME X670E-PRO WIFI"), &HashMap::new());
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
        assert_eq!(labels.get("hwmon/nct6798/fan1").unwrap(), "CPU Fan");
    }

    #[test]
    fn test_builtin_labels_unknown_board() {
        let (labels, _) = load_labels(Some("Some Unknown Board"), &HashMap::new());
        assert!(labels.is_empty());
    }

    #[test]
    fn test_user_labels_override_builtin() {
        let mut user = HashMap::new();
        user.insert("hwmon/nct6798/in0".into(), "My Custom Vcore".into());

        let (labels, _) = load_labels(Some("WRX90E-SAGE SE"), &user);
        // User label takes precedence over the built-in "Vcore"
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "My Custom Vcore");
        // Built-in labels for other sensors still present
        assert_eq!(labels.get("hwmon/nct6798/fan1").unwrap(), "CPU Fan");
    }

    #[test]
    fn test_load_labels_no_board() {
        let mut user = HashMap::new();
        user.insert("hwmon/coretemp/temp1".into(), "CPU Package".into());

        let (labels, _) = load_labels(None, &user);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels.get("hwmon/coretemp/temp1").unwrap(), "CPU Package");
    }

    #[test]
    fn test_voltage_scaling_from_board() {
        let scaling = load_voltage_scaling(Some("Pro WS WRX90E-SAGE SE"), &HashMap::new());
        assert_eq!(*scaling.get("hwmon/nct6798/in1").unwrap(), 5.0);
        assert_eq!(*scaling.get("hwmon/nct6798/in4").unwrap(), 12.0);
    }

    #[test]
    fn test_voltage_scaling_user_override() {
        let mut user = HashMap::new();
        user.insert("hwmon/nct6798/in1".into(), 4.8);

        let scaling = load_voltage_scaling(Some("Pro WS WRX90E-SAGE SE"), &user);
        // User override wins
        assert_eq!(*scaling.get("hwmon/nct6798/in1").unwrap(), 4.8);
        // Built-in preserved for other channels
        assert_eq!(*scaling.get("hwmon/nct6798/in4").unwrap(), 12.0);
    }

    #[test]
    fn test_voltage_scaling_unknown_board() {
        let scaling = load_voltage_scaling(Some("Unknown Board XYZ"), &HashMap::new());
        assert!(scaling.is_empty());
    }
}
