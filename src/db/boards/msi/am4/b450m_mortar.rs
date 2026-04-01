use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["7b89"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "MSI B450M MORTAR (AMD AM4, NCT6797D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6797/in0", "Vcore"),
        ("hwmon/nct6797/in1", "+5V"),
        ("hwmon/nct6797/in2", "AVCC"),
        ("hwmon/nct6797/in3", "+3.3V"),
        ("hwmon/nct6797/in4", "+12V"),
        ("hwmon/nct6797/in7", "+3.3V Standby"),
        ("hwmon/nct6797/in8", "Vbat"),
        ("hwmon/nct6797/in9", "CPU 1.8V"),
        ("hwmon/nct6797/in10", "CPU VDDP"),
        ("hwmon/nct6797/in12", "Vcore SoC"),
        ("hwmon/nct6797/in13", "DRAM"),
        ("hwmon/nct6797/in14", "+5V Standby"),
        ("hwmon/nct6797/fan2", "CPU Fan"),
        ("hwmon/nct6797/fan3", "System Fan 1"),
        ("hwmon/nct6797/fan4", "System Fan 2"),
        ("hwmon/nct6797/fan5", "System Fan 3"),
        ("hwmon/nct6797/temp1", "Super I/O"),
        ("hwmon/nct6797/temp2", "SoC VRM"),
        ("hwmon/nct6797/temp3", "CPU VRM"),
        ("hwmon/nct6797/temp5", "Chipset"),
        ("hwmon/nct6797/temp7", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6797/in1", 5.0),   // +5V: (12/3)+1
            ("hwmon/nct6797/in4", 12.0),  // +12V: (220/20)+1
            ("hwmon/nct6797/in13", 2.0),  // DRAM: x2
            ("hwmon/nct6797/in14", 3.33), // 5VSB: (768/330)+1
        ],
    },
};
