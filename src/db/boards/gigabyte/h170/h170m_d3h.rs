use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["h170m-d3h"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte GA-H170M-D3H (Intel H170, IT8628)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8628/in6", "DRAM"),
        ("hwmon/it8628/fan1", "CPU Fan"),
        ("hwmon/it8628/fan2", "Chassis Fan 1"),
        ("hwmon/it8628/fan3", "Chassis Fan 2"),
        ("hwmon/it8628/fan5", "Chassis Fan 3"),
        ("hwmon/it8628/temp3", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[],
    },
};
