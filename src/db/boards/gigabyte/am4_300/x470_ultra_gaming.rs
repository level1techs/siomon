use crate::db::boards::{
    BoardTemplate, FeatureRequirements, GIGABYTE_IT8686_LABELS, HwmonConfig, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["x470", "ultra gaming"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte X470 AORUS Ultra Gaming (AMD AM4, IT8686 + IT8792)",
    platform: Platform::Generic,

    base_labels: Some(GIGABYTE_IT8686_LABELS),
    sensor_labels: &[
        ("hwmon/it8686/fan4", "SYS Fan 3"),
        ("hwmon/it8686/fan5", "CPU OPT"),
        // IT8792 (secondary)
        ("hwmon/it8792/in1", "DDR VTT"),
        ("hwmon/it8792/in2", "Chipset Core"),
        ("hwmon/it8792/in4", "CPU VDD 1.8V"),
        ("hwmon/it8792/in5", "DDR VPP"),
        ("hwmon/it8792/fan1", "SYS Fan 5 Pump"),
        ("hwmon/it8792/fan2", "SYS Fan 6 Pump"),
        ("hwmon/it8792/fan3", "SYS Fan 4"),
        ("hwmon/it8792/temp1", "PCIe x8"),
        ("hwmon/it8792/temp2", "EC Temp"),
        ("hwmon/it8792/temp3", "System 2"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8686/in1", 1.65),  // +3.3V: 33/20 divider
            ("hwmon/it8686/in2", 6.0),   // +12V: 120/20 divider
            ("hwmon/it8686/in3", 2.5),   // +5V: 50/20 divider
            ("hwmon/it8792/in5", 1.664), // DDR VPP: 208/125
        ],
    },
};
