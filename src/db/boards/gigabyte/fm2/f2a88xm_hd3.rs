use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["f2a88xm-hd3"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte GA-F2A88XM-HD3 (AMD FM2+, IT8620)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8620/in0", "Vcore"),
        ("hwmon/it8620/in1", "DRAM"),
        ("hwmon/it8620/in2", "+12V"),
        ("hwmon/it8620/in3", "+5V"),
        ("hwmon/it8620/in4", "+3.3V"),
        ("hwmon/it8620/in7", "+3.3V Standby"),
        ("hwmon/it8620/in8", "Vbat"),
        ("hwmon/it8620/fan1", "CPU Fan"),
        ("hwmon/it8620/fan2", "System Fan"),
        ("hwmon/it8620/temp1", "System"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8620/in2", 6.0),   // +12V: (75/15)+1
            ("hwmon/it8620/in3", 2.5),   // +5V: (15/10)+1
            ("hwmon/it8620/in4", 1.649), // +3.3V: (649/1000)+1
        ],
    },
};
