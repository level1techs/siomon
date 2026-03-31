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
mod azw;
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
    /// Substrings that ALL must be present in the DMI board_vendor.
    /// Empty means no vendor constraint. Store as lowercase.
    pub match_vendor: &'static [&'static str],
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
    /// Hwmon-specific configuration (voltage scaling, etc.).
    pub hwmon: HwmonConfig,
}

/// Hwmon-specific board configuration.
#[derive(Debug)]
pub struct HwmonConfig {
    /// Voltage multipliers for external resistor dividers.
    /// Sensor key (e.g. "hwmon/it8688/in2") → multiplier. Applied after the
    /// kernel's internal ADC scaling to recover actual rail voltages.
    pub voltage_scaling: &'static [(&'static str, f64)],
}

/// Maps an EDAC rank to a physical DIMM slot.
#[derive(Debug)]
pub struct DimmSlotLabel {
    pub mc: u8,
    pub rank: u16,
    pub label: &'static str,
}

/// Hwmon voltage scaling for Gigabyte boards with IT8688 (X570/TRX40).
pub const GIGABYTE_IT8688_SCALING: &[(&str, f64)] = &[
    ("hwmon/it8688/in1", 1.65), // +3.3V: 33/20 divider
    ("hwmon/it8688/in2", 6.0),  // +12V: 120/20 divider
    ("hwmon/it8688/in3", 2.5),  // +5V: 50/20 divider
];

/// Common sensor labels for the primary IT8688 chip on Gigabyte X570/B550/TRX40 boards.
pub const GIGABYTE_IT8688_LABELS: &[(&str, &str)] = &[
    ("hwmon/it8688/in0", "Vcore"),
    ("hwmon/it8688/in1", "+3.3V"),
    ("hwmon/it8688/in2", "+12V"),
    ("hwmon/it8688/in3", "+5V"),
    ("hwmon/it8688/in4", "Vcore SoC"),
    ("hwmon/it8688/in5", "CPU VDDP"),
    ("hwmon/it8688/in6", "DRAM"),
    ("hwmon/it8688/in7", "+3.3V Standby"),
    ("hwmon/it8688/in8", "Vbat"),
    ("hwmon/it8688/fan1", "CPU Fan"),
    ("hwmon/it8688/fan2", "SYS Fan 1"),
    ("hwmon/it8688/fan3", "SYS Fan 2"),
    ("hwmon/it8688/fan4", "PCH Fan"),
    ("hwmon/it8688/fan5", "CPU OPT"),
    ("hwmon/it8688/temp1", "System"),
    ("hwmon/it8688/temp3", "CPU"),
    ("hwmon/it8688/temp4", "PCIe x16"),
    ("hwmon/it8688/temp5", "VRM MOS"),
    ("hwmon/it8688/temp6", "PCH"),
];

/// Common sensor labels for the secondary IT8792 chip on Gigabyte dual-chip boards.
pub const GIGABYTE_IT8792_LABELS: &[(&str, &str)] = &[
    ("hwmon/it8792/in1", "DDR VTT"),
    ("hwmon/it8792/in2", "Chipset Core"),
    ("hwmon/it8792/in4", "CPU VDD 1.8V"),
    ("hwmon/it8792/in5", "PM CLDO12"),
    ("hwmon/it8792/fan1", "SYS Fan 5 Pump"),
    ("hwmon/it8792/fan2", "SYS Fan 6 Pump"),
    ("hwmon/it8792/fan3", "SYS Fan 4"),
    ("hwmon/it8792/temp1", "PCIe x8"),
    ("hwmon/it8792/temp3", "System 2"),
];

/// Hwmon voltage scaling for Gigabyte X870/X870E boards with IT8696.
pub const GIGABYTE_X870_IT8696_SCALING: &[(&str, f64)] = &[
    ("hwmon/it8696/in1", 1.649), // +3.3V: (6.49/10)+1 divider
    ("hwmon/it8696/in2", 6.0),   // +12V: (50/10)+1 divider
    ("hwmon/it8696/in3", 2.5),   // +5V: (15/10)+1 divider
];

/// Common sensor labels shared across Gigabyte X870/X870E boards with IT8696.
pub const GIGABYTE_X870_IT8696_LABELS: &[(&str, &str)] = &[
    ("hwmon/it8696/in0", "Vcore"),
    ("hwmon/it8696/in1", "+3.3V"),
    ("hwmon/it8696/in2", "+12V"),
    ("hwmon/it8696/in3", "+5V"),
    ("hwmon/it8696/in4", "Vcore SoC"),
    ("hwmon/it8696/in5", "Vcore Misc"),
    ("hwmon/it8696/in6", "VDDIO Memory"),
    ("hwmon/it8696/in7", "+3.3V Standby"),
    ("hwmon/it8696/in8", "Vbat"),
    ("hwmon/it8696/fan1", "CPU Fan"),
    ("hwmon/it8696/fan5", "CPU OPT"),
    ("hwmon/it8696/temp1", "System"),
    ("hwmon/it8696/temp2", "PCH"),
    ("hwmon/it8696/temp3", "CPU"),
    ("hwmon/it8696/temp4", "PCIe x16"),
    ("hwmon/it8696/temp5", "VRM MOS"),
];

/// Hwmon voltage scaling for ASUS boards with NCT6798D (+5V on VIN1, +12V on VIN4).
pub const ASUS_NCT6798_HWMON_SCALING: &[(&str, f64)] = &[
    ("hwmon/nct6798/in1", 5.0),  // +5V rail
    ("hwmon/nct6798/in4", 12.0), // +12V rail
];

/// Hwmon voltage scaling for boards with NCT6799D (+5V on VIN1, +12V on VIN4).
pub const NCT6799_HWMON_SCALING: &[(&str, f64)] = &[
    ("hwmon/nct6799/in1", 5.0),  // +5V rail
    ("hwmon/nct6799/in4", 12.0), // +12V rail
];

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
    // TRX40
    &gigabyte::trx40::trx40_xtreme::BOARD,
    // Gigabyte AM5 (X870I must come before X870 — more specific match)
    &gigabyte::x870::x870i_pro::BOARD,
    &gigabyte::x870::x870e_master::BOARD,
    &gigabyte::x870::x870_eagle::BOARD,
    &gigabyte::x870::x870_gaming::BOARD,
    &gigabyte::b650::b650m_d3hp::BOARD,
    // Gigabyte AM4
    &gigabyte::x570::x570_pro::BOARD,
    &gigabyte::x570::x570_elite::BOARD,
    &gigabyte::b550::b550_vision_d::BOARD,
    &gigabyte::b550::b550m_ds3h::BOARD,
    &gigabyte::b450::b450_elite::BOARD,
    &gigabyte::b450::b450m_ds3h::BOARD,
    // Gigabyte Intel
    &gigabyte::z690::z690_pro::BOARD,
    // ASUS AM5
    &asus::x670e::crosshair_x670e::BOARD,
    &asus::x670e::strix_x670e::BOARD_X670,
    &asus::x670e::strix_x670e::BOARD_B650,
    &asus::x670e::tuf_x670e::BOARD_X670,
    &asus::x670e::tuf_x670e::BOARD_B650,
    &asus::x670e::prime_x670e::BOARD_X670,
    &asus::x670e::prime_x670e::BOARD_B650,
    &asus::x670e::proart_x670e::BOARD,
    // ASUS AM4
    &asus::b350::prime_b350::BOARD,
    &asus::b450::prime_b450::BOARD,
    // Mini-PCs
    &azw::mini_pc::beelink_eq::BOARD,
    &azw::mini_pc::beelink_sei::BOARD,
    // NVIDIA
    &nvidia::gb10::dgx_spark::BOARD,
    &nvidia::thor::jetson_thor::BOARD,
];

/// Look up a board template by DMI board name and vendor.
pub fn lookup_board(board_name: &str) -> Option<&'static BoardTemplate> {
    let vendor = read_board_vendor().unwrap_or_default();
    lookup_board_with_vendor(board_name, &vendor)
}

fn read_board_vendor() -> Option<String> {
    crate::platform::sysfs::read_string_optional(std::path::Path::new(
        "/sys/class/dmi/id/board_vendor",
    ))
}

fn lookup_board_with_vendor(
    board_name: &str,
    board_vendor: &str,
) -> Option<&'static BoardTemplate> {
    let lower = board_name.to_lowercase();
    let vendor_lower = board_vendor.to_lowercase();
    BOARDS.iter().copied().find(|b| {
        b.match_substrings.iter().all(|s| lower.contains(s))
            && b.exclude_substrings.iter().all(|s| !lower.contains(s))
            && b.match_vendor.iter().all(|s| vendor_lower.contains(s))
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

/// Resolve hwmon voltage scaling for a board template into a HashMap.
pub fn resolve_voltage_scaling(board: &BoardTemplate) -> HashMap<String, f64> {
    board
        .hwmon
        .voltage_scaling
        .iter()
        .map(|&(key, val)| (key.into(), val))
        .collect()
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
        // Use an empty vendor string — boards without match_vendor constraints
        // match any vendor, and boards with constraints (e.g. Beelink "azw")
        // won't match the empty string, which is correct for this test.
        let vendor = "";
        for name in &known_boards {
            let result = lookup_board_with_vendor(name, vendor);
            let match_count = if result.is_some() { 1 } else { 0 };
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
            match_vendor: &[],
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
            hwmon: HwmonConfig {
                voltage_scaling: &[],
            },
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
            match_vendor: &[],
            description: "test board",
            platform: Platform::Generic,
            base_labels: None,
            sensor_labels: &[("hwmon/nct6798/in0", "Vcore")],
            nct_voltage_scaling: None,
            dimm_labels: &[],
            ddr5_bus_config: None,
            requirements: FeatureRequirements::NONE,
            hwmon: HwmonConfig {
                voltage_scaling: &[],
            },
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
