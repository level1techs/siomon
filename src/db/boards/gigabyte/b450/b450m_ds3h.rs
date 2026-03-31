use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["b450m", "ds3h"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte B450M DS3H (AMD AM4, IT8686)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8686/in0", "Vcore"),
        ("hwmon/it8686/in1", "+3.3V"),
        ("hwmon/it8686/in2", "+12V"),
        ("hwmon/it8686/in3", "+5V"),
        ("hwmon/it8686/in4", "Vcore SoC"),
        ("hwmon/it8686/in5", "VDDP"),
        ("hwmon/it8686/in6", "DRAM"),
        ("hwmon/it8686/in7", "+3.3V Standby"),
        ("hwmon/it8686/in8", "Vbat"),
        ("hwmon/it8686/fan1", "CPU Fan"),
        ("hwmon/it8686/fan2", "SYS Fan 1"),
        ("hwmon/it8686/fan3", "SYS Fan 2"),
        ("hwmon/it8686/fan4", "SYS Fan 3 Pump"),
        ("hwmon/it8686/fan5", "CPU OPT"),
        ("hwmon/it8686/temp1", "System"),
        ("hwmon/it8686/temp2", "Chipset"),
        ("hwmon/it8686/temp3", "CPU"),
        ("hwmon/it8686/temp4", "PCIe x16"),
        ("hwmon/it8686/temp5", "VRM"),
        ("hwmon/it8686/temp6", "Vcore SoC"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8686/in1", 1.65),
            ("hwmon/it8686/in2", 6.0),
            ("hwmon/it8686/in3", 2.5),
        ],
    },
};
