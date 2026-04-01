use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["x370", "gaming k4"],
    exclude_substrings: &[],
    match_vendor: &["asrock"],
    description: "ASRock X370 Gaming K4 (AMD AM4, NCT6779D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6779/in0", "Vcore"),
        ("hwmon/nct6779/in1", "VPPM"),
        ("hwmon/nct6779/in2", "AVCC"),
        ("hwmon/nct6779/in3", "+3.3V"),
        ("hwmon/nct6779/in4", "+12V"),
        ("hwmon/nct6779/in5", "Vcore SoC"),
        ("hwmon/nct6779/in6", "DRAM"),
        ("hwmon/nct6779/in7", "+3.3V Standby"),
        ("hwmon/nct6779/in8", "Vbat"),
        ("hwmon/nct6779/in10", "Mem VTT"),
        ("hwmon/nct6779/in11", "Chipset 1.05V"),
        ("hwmon/nct6779/in12", "+5V"),
        ("hwmon/nct6779/in13", "CPU VDDP"),
        ("hwmon/nct6779/in14", "+1.8V"),
        ("hwmon/nct6779/fan1", "Chassis Fan 3"),
        ("hwmon/nct6779/fan2", "CPU Fan 1"),
        ("hwmon/nct6779/fan3", "CPU Fan 2"),
        ("hwmon/nct6779/fan4", "Chassis Fan 1"),
        ("hwmon/nct6779/fan5", "Chassis Fan 2"),
        ("hwmon/nct6779/temp1", "Motherboard"),
        ("hwmon/nct6779/temp2", "CPU"),
        ("hwmon/nct6779/temp3", "Auxiliary"),
        ("hwmon/nct6779/temp4", "VRM"),
        ("hwmon/nct6779/temp7", "CPU Tctl"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6779/in0", 2.0),  // Vcore: x2
            ("hwmon/nct6779/in1", 2.0),  // VPPM: x2
            ("hwmon/nct6779/in4", 6.6),  // +12V: (56/10)+1
            ("hwmon/nct6779/in12", 3.0), // +5V: (20/10)+1
        ],
    },
};
