use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["x370", "sli plus"],
    exclude_substrings: &[],
    match_vendor: &["micro-star"],
    description: "MSI X370 SLI PLUS (AMD AM4, NCT6795D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6795/in0", "Vcore"),
        ("hwmon/nct6795/in1", "CPU NB/SoC"),
        ("hwmon/nct6795/in2", "AVCC"),
        ("hwmon/nct6795/in3", "+5V"),
        ("hwmon/nct6795/in4", "+12V"),
        ("hwmon/nct6795/in6", "CLDO VDDP"),
        ("hwmon/nct6795/in7", "+3.3V Standby"),
        ("hwmon/nct6795/in8", "Vbat"),
        ("hwmon/nct6795/in9", "VTT"),
        ("hwmon/nct6795/in10", "CPU VDDP"),
        ("hwmon/nct6795/in11", "DRAM VREF"),
        ("hwmon/nct6795/in12", "VDD"),
        ("hwmon/nct6795/in13", "DRAM"),
        ("hwmon/nct6795/in14", "+5V Standby"),
        ("hwmon/nct6795/fan1", "Pump Fan"),
        ("hwmon/nct6795/fan2", "CPU Fan"),
        ("hwmon/nct6795/fan3", "System Fan 1"),
        ("hwmon/nct6795/fan4", "System Fan 2"),
        ("hwmon/nct6795/fan5", "System Fan 3"),
        ("hwmon/nct6795/fan6", "System Fan 4"),
        ("hwmon/nct6795/temp1", "Super I/O"),
        ("hwmon/nct6795/temp2", "SoC VRM"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6795/in3", 1.5),   // +5V: x1.5
            ("hwmon/nct6795/in4", 12.0),  // +12V: x12
            ("hwmon/nct6795/in13", 2.0),  // DRAM: x2
            ("hwmon/nct6795/in14", 3.33), // 5VSB: (768/330)+1
        ],
    },
};
