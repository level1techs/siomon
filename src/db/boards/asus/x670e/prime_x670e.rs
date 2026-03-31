use crate::db::boards::{
    ASUS_AM5_NCT6798_LABELS, ASUS_NCT6798_HWMON_SCALING, BoardTemplate, FeatureRequirements,
    HwmonConfig, Platform,
};

/// Shared sensor labels for both X670E and B650 PRIME variants.
const PRIME_LABELS: &[(&str, &str)] = &[("hwmon/nct6798/fan2", "Chassis Fan 1")];

pub static BOARD_X670: BoardTemplate = BoardTemplate {
    match_substrings: &["prime", "x670"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS PRIME X670E (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: PRIME_LABELS,

    // No board-specific voltage scaling known
    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};

pub static BOARD_B650: BoardTemplate = BoardTemplate {
    match_substrings: &["prime", "b650"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS PRIME B650 (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: PRIME_LABELS,

    // No board-specific voltage scaling known
    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};
