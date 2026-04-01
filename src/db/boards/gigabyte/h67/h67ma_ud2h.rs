use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["h67ma-ud2h"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte H67MA-UD2H (Intel H67, IT8728)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/it8728/in0", "Vtt"),
        ("hwmon/it8728/in1", "+3.3V"),
        ("hwmon/it8728/in2", "+12V"),
        ("hwmon/it8728/in5", "Vcore"),
        ("hwmon/it8728/in6", "DRAM"),
        ("hwmon/it8728/in7", "+3.3V Standby"),
        ("hwmon/it8728/in8", "Vbat"),
        ("hwmon/it8728/fan1", "CPU Fan"),
        ("hwmon/it8728/fan2", "System Fan"),
        ("hwmon/it8728/temp1", "PCH"),
        ("hwmon/it8728/temp2", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8728/in1", 1.649), // +3.3V
            ("hwmon/it8728/in2", 4.090), // +12V (non-standard)
        ],
    },
};
