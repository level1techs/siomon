use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["870a-ud3"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte GA-870A-UD3 (AMD AM3, IT8720)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8720/in0", "Vcore"),
        ("hwmon/it8720/in1", "DDR3"),
        ("hwmon/it8720/in2", "+3.3V"),
        ("hwmon/it8720/in3", "+5V"),
        ("hwmon/it8720/in4", "+12V"),
        ("hwmon/it8720/in7", "+5V Standby"),
        ("hwmon/it8720/in8", "Vbat"),
        ("hwmon/it8720/fan1", "CPU Fan"),
        ("hwmon/it8720/fan2", "SYS Fan 1"),
        ("hwmon/it8720/fan3", "SYS Fan 2"),
        ("hwmon/it8720/fan5", "Power Fan"),
        ("hwmon/it8720/temp1", "System"),
        ("hwmon/it8720/temp2", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8720/in3", 1.68),  // +5V: (6.8/10)+1
            ("hwmon/it8720/in4", 3.963), // +12V
            ("hwmon/it8720/in7", 1.68),  // +5V Standby: (6.8/10)+1
        ],
    },
};
