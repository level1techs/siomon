use crate::db::boards::{BoardTemplate, FeatureRequirements, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["p4242"], // DMI board_name for NVIDIA DGX Spark
    exclude_substrings: &[],
    match_any: &[],
    description: "NVIDIA DGX Spark (GB10 Grace Blackwell)",
    // Generic, not Tegra: DGX Spark runs standard Ubuntu (no L4T),
    // uses NVML for GPU, and has no devfreq/debugfs engine clocks.
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
};
