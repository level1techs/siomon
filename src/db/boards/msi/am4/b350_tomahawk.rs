use crate::db::boards::{
    BoardTemplate, FeatureRequirements, HwmonConfig, MSI_AM4_NCT6795_HWMON_SCALING,
    MSI_AM4_NCT6795_LABELS, Platform,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["7a34"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "MSI B350 TOMAHAWK (AMD AM4, NCT6795D)",
    platform: Platform::Generic,

    base_labels: Some(MSI_AM4_NCT6795_LABELS),
    sensor_labels: &[
        ("hwmon/nct6795/in9", "CPU 1.8V"),
        ("hwmon/nct6795/in10", "CPU VDDP"),
        ("hwmon/nct6795/in12", "CPU NB/SoC"),
        ("hwmon/nct6795/in13", "DRAM"),
        ("hwmon/nct6795/in14", "+5V Standby"),
        ("hwmon/nct6795/fan2", "CPU Fan"),
        ("hwmon/nct6795/fan3", "System Fan 1"),
        ("hwmon/nct6795/temp3", "CPU VRM"),
        ("hwmon/nct6795/temp5", "Chipset"),
        ("hwmon/nct6795/temp7", "CPU"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: MSI_AM4_NCT6795_HWMON_SCALING,
    },
};
