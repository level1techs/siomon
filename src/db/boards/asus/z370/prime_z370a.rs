use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["prime", "z370-a"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS PRIME Z370-A (Intel LGA1151, NCT6793D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6793/in0", "Vcore"),
        ("hwmon/nct6793/in1", "+5V"),
        ("hwmon/nct6793/in2", "AVCC"),
        ("hwmon/nct6793/in3", "+3.3V"),
        ("hwmon/nct6793/in4", "+12V"),
        ("hwmon/nct6793/in6", "CPU Graphics"),
        ("hwmon/nct6793/in7", "+3.3V Standby"),
        ("hwmon/nct6793/in8", "Vbat"),
        ("hwmon/nct6793/in9", "CPU Sustain"),
        ("hwmon/nct6793/in10", "DRAM"),
        ("hwmon/nct6793/in11", "CPU System Agent"),
        ("hwmon/nct6793/in12", "PCH Core"),
        ("hwmon/nct6793/in14", "CPU I/O"),
        ("hwmon/nct6793/fan1", "Chassis Fan 1"),
        ("hwmon/nct6793/fan2", "CPU Fan"),
        ("hwmon/nct6793/fan3", "M.2 Fan"),
        ("hwmon/nct6793/fan5", "AIO Pump"),
        ("hwmon/nct6793/fan6", "Chassis Fan 2"),
        ("hwmon/nct6793/temp1", "Motherboard"),
        ("hwmon/nct6793/temp7", "CPU"),
        ("hwmon/nct6793/temp8", "T Sensor"),
        ("hwmon/nct6793/temp9", "UEFI CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6793/in0", 2.0),  // Vcore: x2
            ("hwmon/nct6793/in1", 5.0),  // +5V
            ("hwmon/nct6793/in4", 12.0), // +12V
            ("hwmon/nct6793/in6", 2.0),  // CPU Graphics: x2
            ("hwmon/nct6793/in10", 2.0), // DRAM: x2
        ],
    },
};
