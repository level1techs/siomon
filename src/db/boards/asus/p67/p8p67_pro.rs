use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["p8p67", "pro"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS P8P67 PRO (Intel P67, NCT6776D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6776/in0", "Vcore"),
        ("hwmon/nct6776/in1", "+12V"),
        ("hwmon/nct6776/in2", "AVCC"),
        ("hwmon/nct6776/in3", "+3.3V"),
        ("hwmon/nct6776/in4", "+5V"),
        ("hwmon/nct6776/in7", "+3.3V Standby"),
        ("hwmon/nct6776/in8", "Vbat"),
        ("hwmon/nct6776/fan2", "CPU Fan"),
        ("hwmon/nct6776/temp1", "Motherboard"),
        ("hwmon/nct6776/temp3", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6776/in1", 12.0), // +12V
            ("hwmon/nct6776/in4", 5.0),  // +5V
        ],
    },
};
