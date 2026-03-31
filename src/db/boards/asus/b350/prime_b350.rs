use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["prime", "b350"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS PRIME B350-PLUS (AMD AM4, IT8655)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8655/in0", "Vcore"),
        ("hwmon/it8655/in1", "Vccp2"),
        ("hwmon/it8655/in2", "+12V"),
        ("hwmon/it8655/in3", "+5V"),
        ("hwmon/it8655/in7", "+3.3V Standby"),
        ("hwmon/it8655/in8", "Vbat"),
        ("hwmon/it8655/in9", "+3.3V"),
        ("hwmon/it8655/fan1", "CPU Fan"),
        ("hwmon/it8655/fan2", "Chassis Fan 1"),
        ("hwmon/it8655/fan3", "Chassis Fan 2"),
        ("hwmon/it8655/temp1", "CPU"),
        ("hwmon/it8655/temp2", "Motherboard"),
        ("hwmon/it8655/temp3", "System"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8655/in2", 6.0), // +12V
            ("hwmon/it8655/in3", 2.5), // +5V
        ],
    },
};
