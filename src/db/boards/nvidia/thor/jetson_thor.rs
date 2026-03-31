use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["jetson", "thor"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "NVIDIA Jetson AGX Thor (Tegra, Neoverse V3AE + Blackwell GPU)",
    platform: Platform::Tegra,

    base_labels: None,
    sensor_labels: &[
        ("hwmon/ina3221/in1", "VDD_GPU Voltage"),
        ("hwmon/ina3221/in2", "VDD_CPU_SOC_MSS Voltage"),
        ("hwmon/ina3221/in3", "VIN_SYS_5V0 Voltage"),
        ("hwmon/ina3221/curr1", "VDD_GPU Current"),
        ("hwmon/ina3221/curr2", "VDD_CPU_SOC_MSS Current"),
        ("hwmon/ina3221/curr3", "VIN_SYS_5V0 Current"),
        ("hwmon/ina238/power1", "Board Power"),
        ("hwmon/tmp451/temp1", "Board Temp"),
        ("hwmon/tmp451/temp2", "Board Temp 2"),
    ],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[],
    },
};
