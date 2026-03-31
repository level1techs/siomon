use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["eq"],
    exclude_substrings: &[],
    match_vendor: &["azw"],
    description: "Beelink EQ series mini-PC (Intel N100, IT8613)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8613/in0", "Vcore"),
        ("hwmon/it8613/in1", "V SM"),
        ("hwmon/it8613/fan2", "CPU Fan"),
        ("hwmon/it8613/fan3", "SYS Fan"),
        ("hwmon/it8613/temp1", "CPU"),
        ("hwmon/it8613/temp2", "System"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[],
    },
};
