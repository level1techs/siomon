use crate::db::boards::{
    ASUS_AM5_NCT6798_LABELS, ASUS_NCT6798_HWMON_SCALING, BoardTemplate, FeatureRequirements,
    HwmonConfig, Platform,
};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["crosshair", "x670"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS ROG CROSSHAIR X670E HERO (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: &[("hwmon/nct6798/fan2", "CPU OPT")],

    nct_voltage_scaling: Some(&voltage_scaling::ASUS_AM5_NCT6798),
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};
