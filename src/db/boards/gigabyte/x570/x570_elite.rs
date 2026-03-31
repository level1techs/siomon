use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_IT8688_LABELS, GIGABYTE_IT8688_SCALING,
    HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["x570", "elite"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte X570 AORUS ELITE (AMD AM4, IT8688)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_IT8688_LABELS),
    sensor_labels: &[],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: GIGABYTE_IT8688_SCALING,
    },
};
