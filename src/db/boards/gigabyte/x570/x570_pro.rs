use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_IT8688_LABELS, GIGABYTE_IT8688_SCALING,
    GIGABYTE_IT8792_LABELS, HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["x570", "pro"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte X570 AORUS PRO (AMD AM4, IT8688 + IT8792)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_IT8688_LABELS),
    sensor_labels: GIGABYTE_IT8792_LABELS,

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: GIGABYTE_IT8688_SCALING,
    },
};
