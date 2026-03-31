use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["z690", "pro"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte Z690 AORUS PRO (Intel LGA1700, IT8689 + IT87952)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // IT8689 (primary)
        ("hwmon/it8689/in0", "Vcore"),
        ("hwmon/it8689/in1", "+3.3V"),
        ("hwmon/it8689/in2", "+12V"),
        ("hwmon/it8689/in3", "+5V"),
        ("hwmon/it8689/in4", "CPU VAXG"),
        ("hwmon/it8689/in5", "CPU VCCIN AUX"),
        ("hwmon/it8689/in6", "DRAM"),
        ("hwmon/it8689/fan1", "CPU Fan"),
        ("hwmon/it8689/fan2", "SYS Fan 1"),
        ("hwmon/it8689/fan3", "SYS Fan 2"),
        ("hwmon/it8689/fan4", "SYS Fan 3"),
        ("hwmon/it8689/fan5", "CPU OPT"),
        ("hwmon/it8689/temp1", "System"),
        ("hwmon/it8689/temp4", "PCIe x16"),
        ("hwmon/it8689/temp5", "VRM MOS"),
        ("hwmon/it8689/temp6", "PCH"),
        // IT87952 (secondary)
        ("hwmon/it87952/in1", "DDR VTT"),
        ("hwmon/it87952/in2", "PCH 0.82V"),
        ("hwmon/it87952/in4", "CPU VCCSA"),
        ("hwmon/it87952/in5", "PCH 1.8V"),
        ("hwmon/it87952/fan1", "SYS Fan 5 Pump"),
        ("hwmon/it87952/fan2", "SYS Fan 6 Pump"),
        ("hwmon/it87952/fan3", "SYS Fan 4"),
        ("hwmon/it87952/temp1", "PCIe x4"),
        ("hwmon/it87952/temp3", "System 2"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8689/in1", 1.68), // +3.3V: (6.8/10)+1 divider
            ("hwmon/it8689/in2", 6.0),  // +12V
            ("hwmon/it8689/in3", 2.5),  // +5V
        ],
    },
};
