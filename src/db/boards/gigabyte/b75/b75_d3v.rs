use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["b75-d3v"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte B75-D3V (Intel B75, IT8728)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8728/in0", "Vtt"),
        ("hwmon/it8728/in1", "+3.3V"),
        ("hwmon/it8728/in2", "+12V"),
        ("hwmon/it8728/in3", "+5V"),
        ("hwmon/it8728/in4", "Vaxg"),
        ("hwmon/it8728/in5", "Vcore"),
        ("hwmon/it8728/in6", "DRAM"),
        ("hwmon/it8728/in7", "+3.3V Standby"),
        ("hwmon/it8728/in8", "Vbat"),
        ("hwmon/it8728/fan1", "CPU Fan"),
        ("hwmon/it8728/fan2", "SYS Fan 1"),
        ("hwmon/it8728/fan3", "SYS Fan 2"),
        ("hwmon/it8728/fan4", "SYS Fan 3"),
        ("hwmon/it8728/temp1", "System"),
        ("hwmon/it8728/temp3", "Chipset"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8728/in1", 1.635), // +3.3V (board-specific, not 1.649)
            ("hwmon/it8728/in2", 6.0),   // +12V
            ("hwmon/it8728/in3", 2.5),   // +5V
        ],
    },
};
