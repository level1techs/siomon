use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_IT8728_LABELS, GIGABYTE_IT8728_SCALING,
    HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["z77-d3h"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte Z77-D3H (Intel Z77, IT8728)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_IT8728_LABELS),
    sensor_labels: &[
        ("hwmon/it8728/fan4", "SYS Fan 3"),
        ("hwmon/it8728/fan5", "SYS Fan 4"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: GIGABYTE_IT8728_SCALING,
    },
};
