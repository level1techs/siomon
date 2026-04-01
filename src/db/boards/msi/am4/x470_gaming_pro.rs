use crate::db::boards::{
    BoardTemplate, FeatureRequirements, HwmonConfig, MSI_AM4_NCT6795_LABELS, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["7b79"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "MSI X470 GAMING PRO (AMD AM4, NCT6795D)",
    platform: Platform::Generic,

    base_labels: Some(MSI_AM4_NCT6795_LABELS),
    sensor_labels: &[
        ("hwmon/nct6795/in9", "CPU 1.8V"),
        ("hwmon/nct6795/in12", "CPU SoC"),
        ("hwmon/nct6795/in13", "DRAM"),
        ("hwmon/nct6795/fan1", "Pump Fan"),
        ("hwmon/nct6795/fan2", "CPU Fan"),
        ("hwmon/nct6795/fan3", "System Fan 1"),
        ("hwmon/nct6795/fan4", "System Fan 2"),
        ("hwmon/nct6795/fan5", "System Fan 3"),
        ("hwmon/nct6795/fan6", "System Fan 4"),
        ("hwmon/nct6795/temp3", "CPU VRM"),
        ("hwmon/nct6795/temp5", "Chipset"),
        ("hwmon/nct6795/temp7", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6795/in1", 5.04), // +5V: x5.04
            ("hwmon/nct6795/in4", 12.0), // +12V: x12
            ("hwmon/nct6795/in13", 2.0), // DRAM: x2
        ],
    },
};
