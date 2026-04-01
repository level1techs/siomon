use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["p8b75-v"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS P8B75-V (Intel B75, NCT6779D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6779/in0", "Vcore"),
        ("hwmon/nct6779/in1", "+5V"),
        ("hwmon/nct6779/in2", "AVCC"),
        ("hwmon/nct6779/in3", "+3.3V"),
        ("hwmon/nct6779/in4", "+12V"),
        ("hwmon/nct6779/in7", "+3.3V Standby"),
        ("hwmon/nct6779/in8", "Vbat"),
        ("hwmon/nct6779/fan1", "Chassis Fan 1"),
        ("hwmon/nct6779/fan2", "CPU Fan"),
        ("hwmon/nct6779/fan4", "Chassis Fan 2"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6779/in1", 5.0),  // +5V
            ("hwmon/nct6779/in4", 12.0), // +12V
        ],
    },
};
