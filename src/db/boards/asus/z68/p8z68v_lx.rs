use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["p8z68-v lx"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS P8Z68-V LX (Intel Z68, NCT6776D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6776/in1", "+12V"),
        ("hwmon/nct6776/in2", "AVCC"),
        ("hwmon/nct6776/in3", "+3.3V"),
        ("hwmon/nct6776/in4", "+5V"),
        ("hwmon/nct6776/fan1", "Chassis Fan 1"),
        ("hwmon/nct6776/fan2", "CPU Fan"),
        ("hwmon/nct6776/fan3", "Power Fan"),
        ("hwmon/nct6776/fan4", "Chassis Fan 2"),
        ("hwmon/nct6776/temp1", "System"),
        ("hwmon/nct6776/temp7", "CPU"),
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
