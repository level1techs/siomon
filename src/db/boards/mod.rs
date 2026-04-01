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
mod msi;
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

/// Hwmon voltage scaling for Gigabyte boards with IT8686 (AM4 300/400-series).
pub const GIGABYTE_IT8686_SCALING: &[(&str, f64)] = &[
    ("hwmon/it8686/in1", 1.65), // +3.3V: 33/20 divider
    ("hwmon/it8686/in2", 6.0),  // +12V: 120/20 divider
    ("hwmon/it8686/in3", 2.5),  // +5V: 50/20 divider
];

/// Common sensor labels for the primary IT8686 chip on Gigabyte AM4 300/400-series boards.
pub const GIGABYTE_IT8686_LABELS: &[(&str, &str)] = &[
    ("hwmon/it8686/in0", "Vcore"),
    ("hwmon/it8686/in1", "+3.3V"),
    ("hwmon/it8686/in2", "+12V"),
    ("hwmon/it8686/in3", "+5V"),
    ("hwmon/it8686/in4", "Vcore SoC"),
    ("hwmon/it8686/in5", "CPU VDDP"),
    ("hwmon/it8686/in6", "DRAM"),
    ("hwmon/it8686/in7", "+3.3V Standby"),
    ("hwmon/it8686/in8", "Vbat"),
    ("hwmon/it8686/fan1", "CPU Fan"),
    ("hwmon/it8686/fan2", "SYS Fan 1"),
    ("hwmon/it8686/fan3", "SYS Fan 2"),
    ("hwmon/it8686/temp1", "System"),
    ("hwmon/it8686/temp2", "Chipset"),
    ("hwmon/it8686/temp3", "CPU"),
    ("hwmon/it8686/temp4", "PCIe x16"),
    ("hwmon/it8686/temp5", "VRM MOS"),
    ("hwmon/it8686/temp6", "Vcore SoC MOS"),
];

/// Default hwmon voltage scaling for Gigabyte IT8728 boards (Z77-D3H baseline).
/// Boards with different divider networks (e.g. B75-D3V) override inline.
pub const GIGABYTE_IT8728_SCALING: &[(&str, f64)] = &[
    ("hwmon/it8728/in1", 1.649), // +3.3V
    ("hwmon/it8728/in2", 6.0),   // +12V: 72/12
    ("hwmon/it8728/in3", 2.5),   // +5V
];

/// Default sensor labels for Gigabyte IT8728 boards. Boards may override
/// individual entries (e.g. B75-D3V relabels temp3 as "Chipset").
pub const GIGABYTE_IT8728_LABELS: &[(&str, &str)] = &[
    ("hwmon/it8728/in0", "Vtt"),
    ("hwmon/it8728/in1", "+3.3V"),
    ("hwmon/it8728/in2", "+12V"),
    ("hwmon/it8728/in3", "+5V"),
    ("hwmon/it8728/in4", "Vaxg"),
    ("hwmon/it8728/in5", "Vcore"),
    ("hwmon/it8728/in6", "DRAM"),
    ("hwmon/it8728/in7", "+3.3V Standby"),
    ("hwmon/it8728/in8", "Vbat"),
    ("hwmon/it8728/fan1", "CPU Fan"),
    ("hwmon/it8728/fan2", "SYS Fan 1"),
    ("hwmon/it8728/fan3", "SYS Fan 2"),
    ("hwmon/it8728/temp1", "System"),
    ("hwmon/it8728/temp3", "CPU"),
];

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

/// Default hwmon voltage scaling for MSI AM4 NCT6795 boards (B350/X470 baseline).
/// Boards with different VIN mappings (e.g. X370 SLI Plus) define scaling inline.
pub const MSI_AM4_NCT6795_HWMON_SCALING: &[(&str, f64)] = &[
    ("hwmon/nct6795/in1", 5.0),   // +5V: (12/3)+1
    ("hwmon/nct6795/in4", 12.0),  // +12V: (220/20)+1
    ("hwmon/nct6795/in12", 2.0),  // NB/SOC: x2
    ("hwmon/nct6795/in13", 2.0),  // DRAM: x2
    ("hwmon/nct6795/in14", 3.33), // 5VSB: (768/330)+1
];

/// Default sensor labels for MSI AM4 NCT6795 boards. Boards with different
/// VIN mappings (e.g. X370 SLI Plus) define labels inline instead.
pub const MSI_AM4_NCT6795_LABELS: &[(&str, &str)] = &[
    ("hwmon/nct6795/in0", "Vcore"),
    ("hwmon/nct6795/in1", "+5V"),
    ("hwmon/nct6795/in2", "AVCC"),
    ("hwmon/nct6795/in3", "+3.3V"),
    ("hwmon/nct6795/in4", "+12V"),
    ("hwmon/nct6795/in7", "+3.3V Standby"),
    ("hwmon/nct6795/in8", "Vbat"),
    ("hwmon/nct6795/temp1", "Super I/O"),
    ("hwmon/nct6795/temp2", "SoC VRM"),
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
    // Gigabyte AM4 (AB350N-Gaming WIFI must come before AB350-Gaming 3)
    &gigabyte::x570::x570_pro::BOARD,
    &gigabyte::x570::x570_elite::BOARD,
    &gigabyte::b550::b550_vision_d::BOARD,
    &gigabyte::b550::b550m_ds3h::BOARD,
    &gigabyte::am4_300::x470_ultra_gaming::BOARD,
    &gigabyte::am4_300::ax370_gaming5::BOARD,
    &gigabyte::am4_300::ab350n_gaming_wifi::BOARD,
    &gigabyte::am4_300::ab350_gaming3::BOARD,
    &gigabyte::am4_300::ax370m_ds3h::BOARD,
    &gigabyte::b450::b450_elite::BOARD,
    &gigabyte::b450::b450m_ds3h::BOARD,
    // Gigabyte Intel
    &gigabyte::z690::z690_pro::BOARD,
    &gigabyte::z77::z77_d3h::BOARD,
    &gigabyte::h170::h170m_d3h::BOARD,
    &gigabyte::fm2::f2a88xm_hd3::BOARD,
    &gigabyte::b75::b75_d3v::BOARD,
    &gigabyte::h67::h67ma_ud2h::BOARD,
    &gigabyte::am3::ga_870a_ud3::BOARD,
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
    &asus::x570::tuf_x570_plus::BOARD,
    &asus::b350::prime_b350::BOARD,
    &asus::b450::prime_b450::BOARD,
    // ASUS Intel
    &asus::z370::prime_z370a::BOARD,
    &asus::h87::h87_pro::BOARD,
    &asus::c236::p10s_m_ws::BOARD,
    &asus::p67::p8p67_pro::BOARD,
    &asus::z68::p8z68v_lx::BOARD,
    &asus::b75::p8b75v::BOARD,
    &asus::q1900::q1900_itx::BOARD,
    // MSI AM4
    &msi::am4::b350_tomahawk::BOARD,
    &msi::am4::x370_sli_plus::BOARD,
    &msi::am4::x470_gaming_pro::BOARD,
    &msi::am4::b450m_mortar::BOARD,
    // ASRock AM4
    &asrock::am4::ab350_pro4::BOARD,
    &asrock::am4::x370_taichi::BOARD,
    &asrock::am4::x370_gaming_k4::BOARD,
    &asrock::am4::b450_gaming_itx::BOARD,
    &asrock::am4::a300m_deskmini::BOARD,
    // ASRock Intel
    &asrock::z390::z390_extreme4::BOARD,
    &asrock::z390::z390m_itx::BOARD,
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

    // --- MSI boards ---

    #[test]
    fn test_lookup_msi_b350_tomahawk() {
        let b = lookup_board("MS-7A34").unwrap();
        assert!(b.description.contains("MSI"));
        assert!(b.description.contains("B350"));
    }

    #[test]
    fn test_lookup_msi_x470_gaming_pro() {
        let b = lookup_board("MS-7B79").unwrap();
        assert!(b.description.contains("X470"));
    }

    #[test]
    fn test_lookup_msi_b450m_mortar() {
        let b = lookup_board("MS-7B89").unwrap();
        assert!(b.description.contains("B450M MORTAR"));
    }

    #[test]
    fn test_lookup_msi_x370_sli_plus() {
        let b = lookup_board_with_vendor("X370 SLI PLUS", "Micro-Star International Co., Ltd.")
            .unwrap();
        assert!(b.description.contains("X370 SLI"));
    }

    // --- ASRock boards ---

    #[test]
    fn test_lookup_asrock_ab350_pro4() {
        let b = lookup_board_with_vendor("AB350 Pro4", "ASRock").unwrap();
        assert!(b.description.contains("AB350"));
    }

    #[test]
    fn test_lookup_asrock_x370_taichi() {
        let b = lookup_board_with_vendor("X370 Taichi", "ASRock").unwrap();
        assert!(b.description.contains("X370 Taichi"));
    }

    #[test]
    fn test_lookup_asrock_b450_gaming_itx() {
        let b = lookup_board_with_vendor("B450 Gaming-ITX/ac", "ASRock").unwrap();
        assert!(b.description.contains("B450 Gaming-ITX"));
    }

    #[test]
    fn test_lookup_asrock_a300m_deskmini() {
        let b = lookup_board_with_vendor("A300M-STX", "ASRock").unwrap();
        assert!(b.description.contains("DeskMini"));
    }

    #[test]
    fn test_lookup_asrock_z390_extreme4() {
        let b = lookup_board_with_vendor("Z390 Extreme4", "ASRock").unwrap();
        assert!(b.description.contains("Z390 Extreme4"));
    }

    #[test]
    fn test_lookup_asrock_z390m_itx() {
        let b = lookup_board_with_vendor("Z390M-ITX/ac", "ASRock").unwrap();
        assert!(b.description.contains("Z390M-ITX"));
    }

    // --- ASUS Intel/X570 boards ---

    #[test]
    fn test_lookup_asus_tuf_x570() {
        let b = lookup_board("TUF GAMING X570-PLUS").unwrap();
        assert!(b.description.contains("X570"));
    }

    #[test]
    fn test_lookup_asus_tuf_x570_wifi() {
        let b = lookup_board("TUF GAMING X570-PLUS (WI-FI)").unwrap();
        assert!(b.description.contains("X570"));
    }

    #[test]
    fn test_lookup_asus_prime_z370a() {
        let b = lookup_board("PRIME Z370-A").unwrap();
        assert!(b.description.contains("Z370"));
    }

    #[test]
    fn test_lookup_asus_h87_pro() {
        let b = lookup_board("H87-PRO").unwrap();
        assert!(b.description.contains("H87"));
    }

    #[test]
    fn test_lookup_asus_p10s_m_ws() {
        let b = lookup_board("P10S-M WS").unwrap();
        assert!(b.description.contains("P10S"));
    }

    // --- Gigabyte 300-series + older ITE boards ---

    #[test]
    fn test_lookup_gigabyte_ax370_gaming5() {
        let b = lookup_board("AX370-Gaming 5").unwrap();
        assert!(b.description.contains("AX370"));
    }

    #[test]
    fn test_lookup_gigabyte_ab350_gaming3() {
        let b = lookup_board("AB350-Gaming 3").unwrap();
        assert!(b.description.contains("AB350"));
    }

    #[test]
    fn test_lookup_gigabyte_ab350n_gaming_wifi() {
        let b = lookup_board("AB350N-Gaming WIFI-CF").unwrap();
        assert!(b.description.contains("AB350N"));
    }

    #[test]
    fn test_lookup_gigabyte_ax370m_ds3h() {
        let b = lookup_board("AX370M-DS3H").unwrap();
        assert!(b.description.contains("AX370M"));
    }

    #[test]
    fn test_lookup_gigabyte_x470_ultra_gaming() {
        let b = lookup_board("X470 AORUS ULTRA GAMING").unwrap();
        assert!(b.description.contains("X470"));
    }

    #[test]
    fn test_lookup_gigabyte_z77_d3h() {
        let b = lookup_board("Z77-D3H").unwrap();
        assert!(b.description.contains("Z77"));
    }

    #[test]
    fn test_lookup_gigabyte_h170m_d3h() {
        let b = lookup_board("H170M-D3H-CF").unwrap();
        assert!(b.description.contains("H170"));
    }

    #[test]
    fn test_lookup_gigabyte_f2a88xm_hd3() {
        let b = lookup_board("F2A88XM-HD3").unwrap();
        assert!(b.description.contains("F2A88"));
    }

    // --- ASUS Intel (older) + Gigabyte legacy boards ---

    #[test]
    fn test_lookup_asus_p8p67_pro() {
        let b = lookup_board("P8P67 PRO").unwrap();
        assert!(b.description.contains("P8P67"));
    }

    #[test]
    fn test_lookup_asus_p8z68v_lx() {
        let b = lookup_board("P8Z68-V LX").unwrap();
        assert!(b.description.contains("P8Z68"));
    }

    #[test]
    fn test_lookup_asus_p8b75v() {
        let b = lookup_board("P8B75-V").unwrap();
        assert!(b.description.contains("P8B75"));
    }

    #[test]
    fn test_lookup_asus_q1900_itx() {
        let b = lookup_board("Q1900-ITX").unwrap();
        assert!(b.description.contains("Q1900"));
    }

    #[test]
    fn test_lookup_asrock_x370_gaming_k4() {
        let b = lookup_board_with_vendor("X370 Gaming K4", "ASRock").unwrap();
        assert!(b.description.contains("Gaming K4"));
    }

    #[test]
    fn test_lookup_gigabyte_b75_d3v() {
        let b = lookup_board("B75-D3V").unwrap();
        assert!(b.description.contains("B75"));
    }

    #[test]
    fn test_lookup_gigabyte_h67ma_ud2h() {
        let b = lookup_board("H67MA-UD2H").unwrap();
        assert!(b.description.contains("H67"));
    }

    #[test]
    fn test_lookup_gigabyte_870a_ud3() {
        let b = lookup_board("GA-870A-UD3").unwrap();
        assert!(b.description.contains("870A"));
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
            "MS-7A34",
            "MS-7B79",
            "MS-7B89",
            "TUF GAMING X570-PLUS",
            "PRIME Z370-A",
            "H87-PRO",
            "P10S-M WS",
            "AX370-Gaming 5",
            "AB350-Gaming 3",
            "AB350N-Gaming WIFI-CF",
            "AX370M-DS3H",
            "X470 AORUS ULTRA GAMING",
            "Z77-D3H",
            "H170M-D3H-CF",
            "F2A88XM-HD3",
            "P8P67 PRO",
            "P8Z68-V LX",
            "P8B75-V",
            "Q1900-ITX",
            "B75-D3V",
            "H67MA-UD2H",
            "GA-870A-UD3",
        ];
        // Use an empty vendor string — boards without match_vendor constraints
        // match any vendor, and boards with constraints (e.g. Beelink "azw")
        // won't match the empty string, which is correct for this test.
        let vendor = "";
        for name in &known_boards {
            let lower = name.to_lowercase();
            let vendor_lower = vendor.to_lowercase();
            let match_count = BOARDS
                .iter()
                .filter(|b| {
                    b.match_substrings.iter().all(|s| lower.contains(s))
                        && b.exclude_substrings.iter().all(|s| !lower.contains(s))
                        && b.match_vendor.iter().all(|s| vendor_lower.contains(s))
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
