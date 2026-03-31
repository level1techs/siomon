use crate::db::boards::{
    ASUS_AM5_NCT6798_LABELS, ASUS_NCT6798_HWMON_SCALING, BoardTemplate, FeatureRequirements,
    HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["proart", "x670"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS ProArt X670E-CREATOR (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: &[
        ("hwmon/nct6798/fan2", "Chassis Fan 1"),
        ("hwmon/nct6798/fan3", "Chassis Fan 2"),
    ],

    // No board-specific voltage scaling known
    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};
