use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_X870_IT8696_LABELS, GIGABYTE_X870_IT8696_SCALING,
    HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["x870e", "master"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte X870E AORUS MASTER (AMD AM5, IT8696)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_X870_IT8696_LABELS),
    sensor_labels: &[
        ("hwmon/it8696/fan2", "SYS Fan 1"),
        ("hwmon/it8696/fan3", "SYS Fan 2"),
        ("hwmon/it8696/fan4", "SYS Fan 3"),
        ("hwmon/it8696/fan6", "SYS Fan 4"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: GIGABYTE_X870_IT8696_SCALING,
    },
};
