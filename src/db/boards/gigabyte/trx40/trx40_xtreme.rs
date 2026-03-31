use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["trx40", "xtreme"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte TRX40 AORUS XTREME (AMD TRX40, IT8688 + IT8792)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // IT8688 (primary)
        ("hwmon/it8688/in0", "Vcore"),
        ("hwmon/it8688/in1", "+3.3V"),
        ("hwmon/it8688/in2", "+12V"),
        ("hwmon/it8688/in3", "+5V"),
        ("hwmon/it8688/in4", "Vcore SoC"),
        ("hwmon/it8688/in5", "CPU VDDP"),
        ("hwmon/it8688/in6", "DRAM"),
        ("hwmon/it8688/fan1", "CPU Fan 1"),
        ("hwmon/it8688/fan2", "MOS Fan"),
        ("hwmon/it8688/fan3", "SYS Fan 2"),
        ("hwmon/it8688/fan4", "PCH Fan"),
        ("hwmon/it8688/fan5", "CPU Fan 2"),
        ("hwmon/it8688/temp1", "System"),
        ("hwmon/it8688/temp2", "VRM MOS"),
        ("hwmon/it8688/temp3", "PCH"),
        ("hwmon/it8688/temp4", "System 2"),
        ("hwmon/it8688/temp5", "CPU"),
        ("hwmon/it8688/temp6", "EC Temp"),
        // IT8792 (secondary)
        ("hwmon/it8792/fan1", "Pump Fan 5"),
        ("hwmon/it8792/fan2", "Pump Fan 6"),
        ("hwmon/it8792/fan3", "SYS Fan 4"),
        ("hwmon/it8792/temp1", "PCIe x16 Slot 2"),
        ("hwmon/it8792/temp2", "EC Temp 2"),
        ("hwmon/it8792/temp3", "PCIe x16 Slot 1"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8688/in1", 1.65), // +3.3V
            ("hwmon/it8688/in2", 6.0),  // +12V
            ("hwmon/it8688/in3", 2.5),  // +5V
        ],
    },
};
