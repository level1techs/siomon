use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["b450", "gaming-itx"],
    exclude_substrings: &[],
    match_vendor: &["asrock"],
    description: "ASRock B450 Gaming-ITX/ac (AMD AM4, NCT6792D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6792/in0", "Vcore"),
        ("hwmon/nct6792/in2", "AVCC"),
        ("hwmon/nct6792/in3", "+3.3V"),
        ("hwmon/nct6792/in7", "+3.3V Standby"),
        ("hwmon/nct6792/in8", "Vbat"),
        ("hwmon/nct6792/in9", "+12V"),
        ("hwmon/nct6792/in13", "+5V"),
        ("hwmon/nct6792/fan1", "Chassis Fan 1"),
        ("hwmon/nct6792/fan2", "CPU Fan"),
        ("hwmon/nct6792/fan3", "Chassis Fan 2"),
        ("hwmon/nct6792/temp2", "VRM"),
        ("hwmon/nct6792/temp3", "Motherboard"),
        ("hwmon/nct6792/temp7", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6792/in0", 2.0),   // Vcore: x2
            ("hwmon/nct6792/in9", 6.625), // +12V: (53/8)
            ("hwmon/nct6792/in13", 3.0),  // +5V: (24/8)
        ],
    },
};
