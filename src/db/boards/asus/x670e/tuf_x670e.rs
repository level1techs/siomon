use crate::db::boards::{
    ASUS_AM5_NCT6798_LABELS, ASUS_NCT6798_HWMON_SCALING, BoardTemplate, FeatureRequirements,
    HwmonConfig, Platform,
};
use crate::db::voltage_scaling;

/// Shared sensor labels for both X670E and B650 TUF variants.
const TUF_LABELS: &[(&str, &str)] = &[
    ("hwmon/nct6798/fan2", "Chassis Fan 1"),
    ("hwmon/nct6798/fan3", "Chassis Fan 2"),
];

pub static BOARD_X670: BoardTemplate = BoardTemplate {
    match_substrings: &["tuf", "x670"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS TUF GAMING X670E (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: TUF_LABELS,

    nct_voltage_scaling: Some(&voltage_scaling::ASUS_AM5_NCT6798),
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};

pub static BOARD_B650: BoardTemplate = BoardTemplate {
    match_substrings: &["tuf", "b650"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS TUF GAMING B650 (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: TUF_LABELS,

    nct_voltage_scaling: Some(&voltage_scaling::ASUS_AM5_NCT6798),
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};
