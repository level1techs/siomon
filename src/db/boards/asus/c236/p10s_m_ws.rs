use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["p10s-m ws"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS P10S-M WS (Intel C236, NCT6791D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6791/in0", "Vcore"),
        ("hwmon/nct6791/in1", "+5V"),
        ("hwmon/nct6791/in3", "+3.3V"),
        ("hwmon/nct6791/in4", "+12V"),
        ("hwmon/nct6791/in6", "VCCSA"),
        ("hwmon/nct6791/in7", "+3.3V Standby"),
        ("hwmon/nct6791/in8", "Vbat"),
        ("hwmon/nct6791/in9", "VCCIO"),
        ("hwmon/nct6791/in10", "+5V Standby"),
        ("hwmon/nct6791/in12", "VDDQ AB CPU1"),
        ("hwmon/nct6791/fan1", "Rear Fan"),
        ("hwmon/nct6791/fan2", "CPU Fan"),
        ("hwmon/nct6791/fan3", "Front Fan 1"),
        ("hwmon/nct6791/fan4", "Front Fan 2"),
        ("hwmon/nct6791/fan5", "Front Fan 3"),
        ("hwmon/nct6791/fan6", "Front Fan 4"),
        ("hwmon/nct6791/temp5", "Motherboard"),
        ("hwmon/nct6791/temp7", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6791/in1", 5.0),  // +5V
            ("hwmon/nct6791/in4", 12.0), // +12V
            ("hwmon/nct6791/in10", 5.0), // +5V Standby
        ],
    },
};
