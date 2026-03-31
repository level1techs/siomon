use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["prime", "b450"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS PRIME B450-PLUS (AMD AM4, IT8665)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8665/in0", "Vcore"),
        ("hwmon/it8665/in1", "+3.3V"),
        ("hwmon/it8665/in2", "+5V"),
        ("hwmon/it8665/in3", "+12V"),
        ("hwmon/it8665/in7", "+3.3V Standby"),
        ("hwmon/it8665/in8", "Vbat"),
        ("hwmon/it8665/fan1", "CPU Fan"),
        ("hwmon/it8665/fan2", "Chassis Fan 1"),
        ("hwmon/it8665/fan3", "Chassis Fan 2"),
        ("hwmon/it8665/fan4", "Chassis Fan 3"),
        ("hwmon/it8665/fan6", "AIO Pump"),
        ("hwmon/it8665/temp1", "CPU"),
        ("hwmon/it8665/temp2", "Motherboard"),
        ("hwmon/it8665/temp3", "System"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8665/in1", 1.33), // +3.3V
            ("hwmon/it8665/in2", 2.5),  // +5V (swapped vs Gigabyte)
            ("hwmon/it8665/in3", 6.0),  // +12V (swapped vs Gigabyte)
        ],
    },
};
