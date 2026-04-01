use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["tuf", "x570", "plus"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS TUF GAMING X570-PLUS (AMD AM4, NCT6798D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6798/in0", "Vcore"),
        ("hwmon/nct6798/in2", "AVCC"),
        ("hwmon/nct6798/in3", "+3.3V"),
        ("hwmon/nct6798/in4", "+12V"),
        ("hwmon/nct6798/in6", "+5V"),
        ("hwmon/nct6798/in7", "+3.3V Standby"),
        ("hwmon/nct6798/in8", "Vbat"),
        ("hwmon/nct6798/fan2", "CPU Fan"),
        ("hwmon/nct6798/fan5", "Chassis Fan 1"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6798/in4", 12.0), // +12V
            ("hwmon/nct6798/in6", 5.0),  // +5V
        ],
    },
};
