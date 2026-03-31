use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["b550m", "ds3h"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte B550M DS3H (AMD AM4, IT8689)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8689/in0", "Vcore"),
        ("hwmon/it8689/in1", "+3.3V"),
        ("hwmon/it8689/in2", "+12V"),
        ("hwmon/it8689/in3", "+5V"),
        ("hwmon/it8689/in4", "Vcore SoC"),
        ("hwmon/it8689/in5", "VDDP"),
        ("hwmon/it8689/in6", "DRAM"),
        ("hwmon/it8689/in7", "+3.3V Standby"),
        ("hwmon/it8689/in8", "Vbat"),
        ("hwmon/it8689/fan1", "CPU Fan"),
        ("hwmon/it8689/fan2", "SYS Fan 1"),
        ("hwmon/it8689/fan3", "SYS Fan 2"),
        ("hwmon/it8689/fan4", "SYS Fan 3 Pump"),
        ("hwmon/it8689/fan5", "CPU OPT"),
        ("hwmon/it8689/temp1", "System"),
        ("hwmon/it8689/temp2", "Chipset"),
        ("hwmon/it8689/temp3", "CPU"),
        ("hwmon/it8689/temp4", "PCIe x16"),
        ("hwmon/it8689/temp5", "VRM"),
        ("hwmon/it8689/temp6", "Vcore SoC"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8689/in1", 1.65),
            ("hwmon/it8689/in2", 6.0),
            ("hwmon/it8689/in3", 2.5),
        ],
    },
};
