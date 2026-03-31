use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_IT8688_LABELS, GIGABYTE_IT8792_LABELS,
    HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["b550", "vision"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte B550 VISION D (AMD AM4, IT8688 + IT8792)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_IT8688_LABELS),
    sensor_labels: GIGABYTE_IT8792_LABELS,

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8688/in1", 1.68), // +3.3V: (6.8/10)+1 divider
            ("hwmon/it8688/in2", 6.0),  // +12V
            ("hwmon/it8688/in3", 2.5),  // +5V
        ],
    },
};
