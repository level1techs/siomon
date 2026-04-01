use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_IT8686_LABELS, GIGABYTE_IT8686_SCALING,
    HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["ax370m-ds3h"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte GA-AX370M-DS3H (AMD AM4, IT8686)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_IT8686_LABELS),
    sensor_labels: &[],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: GIGABYTE_IT8686_SCALING,
    },
};
