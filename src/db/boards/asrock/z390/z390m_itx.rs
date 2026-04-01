use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["z390m-itx"],
    exclude_substrings: &[],
    match_vendor: &["asrock"],
    description: "ASRock Z390M-ITX/ac (Intel Z390, NCT6793D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6793/in0", "Vcore"),
        ("hwmon/nct6793/in2", "AVCC"),
        ("hwmon/nct6793/in3", "+3.3V"),
        ("hwmon/nct6793/in7", "+3.3V Standby"),
        ("hwmon/nct6793/in8", "Vbat"),
        ("hwmon/nct6793/in9", "VTT"),
        ("hwmon/nct6793/in12", "+12V"),
        ("hwmon/nct6793/in13", "+5V"),
        ("hwmon/nct6793/fan1", "Chassis Fan 1"),
        ("hwmon/nct6793/fan2", "CPU Fan"),
        ("hwmon/nct6793/fan5", "Chassis Fan 2"),
        ("hwmon/nct6793/temp2", "CPU"),
        ("hwmon/nct6793/temp3", "Motherboard"),
        ("hwmon/nct6793/temp7", "CPU PECI"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6793/in0", 2.0),  // Vcore: x2
            ("hwmon/nct6793/in12", 6.6), // +12V: (56+10)/10
            ("hwmon/nct6793/in13", 3.0), // +5V: (24/8)
        ],
    },
};
