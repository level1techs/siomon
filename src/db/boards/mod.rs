//! Per-board hardware templates, organized by vendor and chipset.
//!
//! Each board file defines a static `BoardTemplate` that combines sensor
//! labels, voltage scaling references, DIMM topology, and DDR5 I2C bus
//! config into a single declarative definition. Adding a new board requires:
//!
//! 1. Create `src/db/boards/<vendor>/<chipset>/<board>.rs` with `pub static BOARD: BoardTemplate`
//! 2. Add `pub mod <board>;` to `<chipset>/mod.rs` (create the chipset dir if new)
//! 3. Add `pub mod <chipset>;` to `<vendor>/mod.rs` (if new chipset)
//! 4. Add `&<vendor>::<chipset>::<board>::BOARD` to the `BOARDS` array below
//!
//! More-specific boards must come before more-generic ones in `BOARDS`
//! (first match wins).

mod asrock;
mod asus;
mod gigabyte;
mod nvidia;

use std::collections::HashMap;

use crate::db::voltage_scaling::VoltageChannel;

/// Platform hint for enabling platform-specific sensor sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Platform {
    /// Standard x86/ARM system, no special platform handling.
    #[default]
    Generic,
    /// NVIDIA Tegra (Jetson) — enables devfreq GPU, engine clocks.
    Tegra,
}

pub mod diagnostics;

/// A prerequisite for a board feature to work correctly.
#[derive(Debug)]
pub enum Requirement {
    /// BIOS version from `/sys/class/dmi/id/bios_version` must parse as
    /// integer >= this value. If parsing fails, treated as unverifiable.
    MinBiosVersion { version: u32, hint: &'static str },
    /// Manual BIOS setting that can't be verified programmatically.
    /// Always advisory — surfaced when probing returns zero results.
    BiosSetting { description: &'static str },
}

/// Per-feature requirements declared by a board template.
///
/// A map of feature name → requirement slice. Boards only declare entries
/// for features they have. Adding a new feature (e.g., DDR6) requires only
/// a new `FEAT_*` constant — no struct changes and no existing board files
/// touched.
#[derive(Debug)]
pub struct FeatureRequirements {
    pub entries: &'static [(&'static str, &'static [Requirement])],
}

impl FeatureRequirements {
    /// No requirements for any feature.
    pub const NONE: Self = Self { entries: &[] };

    /// Look up requirements for a feature by name. Returns empty slice if
    /// the feature has no declared requirements.
    pub fn get(&self, feature: &str) -> &'static [Requirement] {
        self.entries
            .iter()
            .find(|(name, _)| *name == feature)
            .map(|(_, reqs)| *reqs)
            .unwrap_or(&[])
    }
}

/// Feature name constants for use with [`FeatureRequirements`].
pub const FEAT_DDR5: &str = "ddr5";

/// DDR5 I2C bus topology for direct SPD/temperature probing.
///
/// Boards opt in to DDR5 probing by setting `ddr5_bus_config: Some(...)` in
/// their `BoardTemplate`. The config is resolved once at startup in `main.rs`
/// and threaded to the SPD EEPROM reader (`collectors/spd.rs`) and DDR5
/// temperature sensor (`sensors/i2c/ddr5_temp.rs`) via the board template.
/// Both paths also require `--direct-io` since they use raw I2C ioctls.
#[derive(Debug)]
pub struct Ddr5BusConfig {
    /// I2C bus numbers that connect to DIMM slots.
    pub i2c_buses: &'static [u32],
    /// Number of physical DIMM slots per bus.
    pub slots_per_bus: u16,
}

/// Unified per-board hardware template.
#[derive(Debug)]
pub struct BoardTemplate {
    /// Substrings that ALL must be present in the DMI board_name.
    /// Store as lowercase — matching is case-insensitive.
    pub match_substrings: &'static [&'static str],
    /// Substrings that must NOT be present. Store as lowercase.
    pub exclude_substrings: &'static [&'static str],
    /// At least one of these must match (OR logic for chipset variants).
    /// Empty means no additional constraint. Store as lowercase.
    pub match_any: &'static [&'static str],
    /// Human-readable board description for logging.
    pub description: &'static str,
    /// Platform hint for enabling platform-specific sensor sources.
    pub platform: Platform,
    /// Board-specific sensor labels (merged on top of `base_labels`).
    pub sensor_labels: &'static [(&'static str, &'static str)],
    /// Optional shared base labels applied first; board labels override.
    pub base_labels: Option<&'static [(&'static str, &'static str)]>,
    /// NCT6798/6799 voltage scaling table (18 channels).
    pub nct_voltage_scaling: Option<&'static [VoltageChannel; 18]>,
    /// DIMM slot topology mapping EDAC ranks to physical slot names.
    pub dimm_labels: &'static [DimmSlotLabel],
    /// DDR5 I2C bus topology for direct SPD/temperature probing.
    /// Set this to `Some(...)` to opt in to DDR5 EEPROM reads and per-DIMM
    /// temperature sensors. Only set on boards where raw I2C probing has
    /// been validated — see `Ddr5BusConfig` for the data flow.
    pub ddr5_bus_config: Option<&'static Ddr5BusConfig>,
    /// Per-feature prerequisites (BIOS version, settings, etc.).
    pub requirements: FeatureRequirements,
}

/// Maps an EDAC rank to a physical DIMM slot.
#[derive(Debug)]
pub struct DimmSlotLabel {
    pub mc: u8,
    pub rank: u16,
    pub label: &'static str,
}

/// Common sensor labels shared across ASUS AM5 boards with NCT6798D.
pub const ASUS_AM5_NCT6798_LABELS: &[(&str, &str)] = &[
    ("hwmon/nct6798/in0", "Vcore"),
    ("hwmon/nct6798/in1", "+5V"),
    ("hwmon/nct6798/in2", "AVCC"),
    ("hwmon/nct6798/in3", "+3.3V"),
    ("hwmon/nct6798/in4", "+12V"),
    ("hwmon/nct6798/in7", "+3.3V AUX"),
    ("hwmon/nct6798/in8", "Vbat"),
    ("hwmon/nct6798/temp1", "SYSTIN"),
    ("hwmon/nct6798/temp2", "CPUTIN"),
    ("hwmon/nct6798/temp3", "AUXTIN0"),
    ("hwmon/nct6798/fan1", "CPU Fan"),
];

/// All known board templates. First match wins.
static BOARDS: &[&BoardTemplate] = &[
    // ASUS WRX90E must come before ASRock WRX90 (excludes WRX90E)
    &asus::wrx90::wrx90e_sage::BOARD,
    &asrock::wrx90::wrx90_ws_evo::BOARD,
    // TRX50
    &asus::trx50::trx50_sage::BOARD,
    &gigabyte::trx50::trx50_ai_top::BOARD,
    // AM5
    &asus::x670e::crosshair_x670e::BOARD,
    &asus::x670e::strix_x670e::BOARD,
    &asus::x670e::tuf_x670e::BOARD,
    &asus::x670e::prime_x670e::BOARD,
    &asus::x670e::proart_x670e::BOARD,
    // NVIDIA
    &nvidia::gb10::dgx_spark::BOARD,
    &nvidia::thor::jetson_thor::BOARD,
];

/// Look up a board template by DMI board name.
pub fn lookup_board(board_name: &str) -> Option<&'static BoardTemplate> {
    let lower = board_name.to_lowercase();
    BOARDS.iter().copied().find(|b| {
        b.match_substrings.iter().all(|s| lower.contains(s))
            && b.exclude_substrings.iter().all(|s| !lower.contains(s))
            && (b.match_any.is_empty() || b.match_any.iter().any(|s| lower.contains(s)))
    })
}

/// Resolve all sensor labels for a board template into a HashMap.
/// Base labels are applied first, then board-specific labels override.
pub fn resolve_labels(board: &BoardTemplate) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(base) = board.base_labels {
        for &(key, val) in base {
            m.insert(key.into(), val.into());
        }
    }
    for &(key, val) in board.sensor_labels {
        m.insert(key.into(), val.into());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_asus_wrx90e() {
        let b = lookup_board("Pro WS WRX90E-SAGE SE").unwrap();
        assert!(b.description.contains("WRX90E"));
    }

    #[test]
    fn test_lookup_asrock_wrx90() {
        let b = lookup_board("WRX90 WS EVO").unwrap();
        assert!(b.description.contains("ASRock"));
    }

    #[test]
    fn test_wrx90_no_cross_match() {
        // ASRock WRX90 must not match ASUS WRX90E
        let b = lookup_board("WRX90 WS EVO").unwrap();
        assert!(!b.description.contains("ASUS"));

        // ASUS WRX90E must not match ASRock WRX90
        let b = lookup_board("Pro WS WRX90E-SAGE SE").unwrap();
        assert!(!b.description.contains("ASRock"));
    }

    #[test]
    fn test_lookup_crosshair_x670e() {
        let b = lookup_board("ROG CROSSHAIR X670E HERO").unwrap();
        assert!(b.description.contains("CROSSHAIR"));
    }

    #[test]
    fn test_lookup_strix_x670e() {
        assert!(lookup_board("ROG STRIX X670E-E GAMING WIFI").is_some());
    }

    #[test]
    fn test_lookup_strix_b650e() {
        assert!(lookup_board("ROG STRIX B650E-F GAMING WIFI").is_some());
    }

    #[test]
    fn test_lookup_tuf_x670e() {
        assert!(lookup_board("TUF GAMING X670E-PLUS").is_some());
    }

    #[test]
    fn test_lookup_tuf_b650() {
        assert!(lookup_board("TUF GAMING B650-PLUS WIFI").is_some());
    }

    #[test]
    fn test_lookup_prime_x670e() {
        assert!(lookup_board("PRIME X670E-PRO WIFI").is_some());
    }

    #[test]
    fn test_lookup_prime_b650() {
        assert!(lookup_board("PRIME B650-PLUS").is_some());
    }

    #[test]
    fn test_lookup_proart_x670e() {
        assert!(lookup_board("ProArt X670E-CREATOR WIFI").is_some());
    }

    #[test]
    fn test_lookup_asus_trx50_sage() {
        let b = lookup_board("Pro WS TRX50-SAGE WIFI A").unwrap();
        assert!(b.description.contains("TRX50"));
        assert!(b.ddr5_bus_config.is_some());
        assert_eq!(b.ddr5_bus_config.unwrap().i2c_buses, &[0, 1]);
    }

    #[test]
    fn test_lookup_gigabyte_trx50_ai_top() {
        let b = lookup_board("TRX50 AI TOP").unwrap();
        assert!(b.description.contains("Gigabyte"));
        assert!(b.ddr5_bus_config.is_some());
        assert_eq!(b.ddr5_bus_config.unwrap().i2c_buses, &[1, 2]);
    }

    #[test]
    fn test_lookup_wrx90e_has_ddr5_config() {
        let b = lookup_board("Pro WS WRX90E-SAGE SE").unwrap();
        assert!(b.ddr5_bus_config.is_some());
        assert_eq!(b.ddr5_bus_config.unwrap().i2c_buses, &[1, 2]);
        assert_eq!(b.ddr5_bus_config.unwrap().slots_per_bus, 4);
    }

    #[test]
    fn test_lookup_unknown() {
        assert!(lookup_board("Some Unknown Board").is_none());
    }

    #[test]
    fn test_non_am5_strix_does_not_match() {
        // Intel STRIX boards must not match the AM5 STRIX template
        assert!(lookup_board("ROG STRIX Z790-E GAMING WIFI").is_none());
        assert!(lookup_board("ROG STRIX Z690-A GAMING WIFI D4").is_none());
    }

    #[test]
    fn test_non_am5_tuf_does_not_match() {
        assert!(lookup_board("TUF GAMING Z790-PLUS WIFI").is_none());
    }

    #[test]
    fn test_non_am5_prime_does_not_match() {
        assert!(lookup_board("PRIME Z790-P WIFI").is_none());
    }

    #[test]
    fn test_lookup_nvidia_dgx_spark() {
        let b = lookup_board("P4242").unwrap();
        assert!(b.description.contains("DGX Spark"));
        assert_eq!(b.platform, Platform::Generic);
    }

    #[test]
    fn test_lookup_nvidia_jetson_thor() {
        let b = lookup_board("Jetson AGX Thor").unwrap();
        assert!(b.description.contains("Jetson"));
        assert_eq!(b.platform, Platform::Tegra);
    }

    #[test]
    fn test_no_ambiguous_matches() {
        let known_boards = [
            "Pro WS WRX90E-SAGE SE",
            "WRX90 WS EVO",
            "ROG CROSSHAIR X670E HERO",
            "ROG STRIX X670E-E GAMING WIFI",
            "ROG STRIX B650E-F GAMING WIFI",
            "TUF GAMING X670E-PLUS",
            "TUF GAMING B650-PLUS WIFI",
            "PRIME X670E-PRO WIFI",
            "PRIME B650-PLUS",
            "ProArt X670E-CREATOR WIFI",
            "Pro WS TRX50-SAGE WIFI A",
            "TRX50 AI TOP",
            "P4242",
            "Jetson AGX Thor",
        ];
        for name in &known_boards {
            let lower = name.to_lowercase();
            let match_count = BOARDS
                .iter()
                .filter(|b| {
                    b.match_substrings.iter().all(|s| lower.contains(s))
                        && b.exclude_substrings.iter().all(|s| !lower.contains(s))
                        && (b.match_any.is_empty() || b.match_any.iter().any(|s| lower.contains(s)))
                })
                .count();
            assert!(
                match_count <= 1,
                "{name} matched {match_count} templates (expected 0 or 1)"
            );
        }
    }

    #[test]
    fn test_resolve_labels_base_plus_override() {
        let board = BoardTemplate {
            match_substrings: &["test"],
            exclude_substrings: &[],
            match_any: &[],
            description: "test board",
            platform: Platform::Generic,
            base_labels: Some(&[
                ("hwmon/nct6798/in0", "Vcore"),
                ("hwmon/nct6798/fan1", "CPU Fan"),
            ]),
            sensor_labels: &[("hwmon/nct6798/fan1", "My Fan")],
            nct_voltage_scaling: None,
            dimm_labels: &[],
            ddr5_bus_config: None,
            requirements: FeatureRequirements::NONE,
        };
        let labels = resolve_labels(&board);
        // Board override wins
        assert_eq!(labels.get("hwmon/nct6798/fan1").unwrap(), "My Fan");
        // Base label preserved
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
    }

    #[test]
    fn test_resolve_labels_no_base() {
        let board = BoardTemplate {
            match_substrings: &["test"],
            exclude_substrings: &[],
            match_any: &[],
            description: "test board",
            platform: Platform::Generic,
            base_labels: None,
            sensor_labels: &[("hwmon/nct6798/in0", "Vcore")],
            nct_voltage_scaling: None,
            dimm_labels: &[],
            ddr5_bus_config: None,
            requirements: FeatureRequirements::NONE,
        };
        let labels = resolve_labels(&board);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels.get("hwmon/nct6798/in0").unwrap(), "Vcore");
    }

    #[test]
    fn feature_requirements_none_returns_empty() {
        assert!(FeatureRequirements::NONE.get(FEAT_DDR5).is_empty());
        assert!(FeatureRequirements::NONE.get("nonexistent").is_empty());
    }

    #[test]
    fn feature_requirements_get_hit() {
        let reqs = FeatureRequirements {
            entries: &[(
                FEAT_DDR5,
                &[Requirement::MinBiosVersion {
                    version: 1317,
                    hint: "test",
                }],
            )],
        };
        assert_eq!(reqs.get(FEAT_DDR5).len(), 1);
    }

    #[test]
    fn feature_requirements_get_miss() {
        let reqs = FeatureRequirements {
            entries: &[(
                FEAT_DDR5,
                &[Requirement::MinBiosVersion {
                    version: 1317,
                    hint: "test",
                }],
            )],
        };
        assert!(reqs.get("ddr6").is_empty());
    }
}
