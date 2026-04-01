use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["h87-pro"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS H87-PRO (Intel LGA1150, NCT6791D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6791/in0", "Vcore"),
        ("hwmon/nct6791/in1", "+5V"),
        ("hwmon/nct6791/in2", "AVCC"),
        ("hwmon/nct6791/in3", "+3.3V"),
        ("hwmon/nct6791/in4", "+12V"),
        ("hwmon/nct6791/in7", "+3.3V Standby"),
        ("hwmon/nct6791/in8", "Vbat"),
        ("hwmon/nct6791/fan1", "Chassis Fan 1"),
        ("hwmon/nct6791/fan2", "CPU Fan"),
        ("hwmon/nct6791/fan3", "Chassis Fan 2"),
        ("hwmon/nct6791/fan4", "Chassis Fan 3"),
        ("hwmon/nct6791/temp7", "CPU (PECI)"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6791/in0", 2.0),  // Vcore: x2
            ("hwmon/nct6791/in1", 5.0),  // +5V: 40/8
            ("hwmon/nct6791/in4", 12.0), // +12V: 96/8
        ],
    },
};
