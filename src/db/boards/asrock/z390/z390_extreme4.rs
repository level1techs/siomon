use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["z390", "extreme4"],
    exclude_substrings: &[],
    match_vendor: &["asrock"],
    description: "ASRock Z390 Extreme4 (Intel Z390, NCT6791D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6791/in0", "Vcore"),
        ("hwmon/nct6791/in1", "+5V"),
        ("hwmon/nct6791/in2", "AVCC"),
        ("hwmon/nct6791/in3", "+3.3V"),
        ("hwmon/nct6791/in4", "+12V"),
        ("hwmon/nct6791/in6", "PCH 1.0V"),
        ("hwmon/nct6791/in7", "+3.3V Standby"),
        ("hwmon/nct6791/in8", "Vbat"),
        ("hwmon/nct6791/in9", "VCCST"),
        ("hwmon/nct6791/in11", "VCCIO"),
        ("hwmon/nct6791/in12", "DRAM"),
        ("hwmon/nct6791/in13", "DRAM VPP"),
        ("hwmon/nct6791/in14", "VCCSA"),
        ("hwmon/nct6791/fan1", "Chassis Fan 3"),
        ("hwmon/nct6791/fan2", "CPU Fan 1"),
        ("hwmon/nct6791/fan3", "CPU Fan 2"),
        ("hwmon/nct6791/fan4", "Chassis Fan 1"),
        ("hwmon/nct6791/fan5", "Chassis Fan 2"),
        ("hwmon/nct6791/temp1", "Motherboard"),
        ("hwmon/nct6791/temp2", "CPU"),
        ("hwmon/nct6791/temp7", "CPU Core"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6791/in0", 2.0),  // Vcore: x2
            ("hwmon/nct6791/in1", 3.0),  // +5V: (20/10)+1
            ("hwmon/nct6791/in4", 12.0), // +12V: x12
            ("hwmon/nct6791/in13", 2.0), // DRAM VPP: x2
        ],
    },
};
