use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["q1900-itx"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS Q1900-ITX (Intel Bay Trail, NCT6776D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6776/in1", "+12V"),
        ("hwmon/nct6776/in5", "+5V"),
        ("hwmon/nct6776/fan1", "System Fan"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6776/in1", 6.625), // +12V
            ("hwmon/nct6776/in5", 3.0),   // +5V
        ],
    },
};
