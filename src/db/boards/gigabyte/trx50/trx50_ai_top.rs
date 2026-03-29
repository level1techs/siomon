use crate::db::boards::{BoardTemplate, Ddr5BusConfig, FeatureRequirements, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["trx50", "ai top"],
    exclude_substrings: &[],
    match_any: &[],
    description: "Gigabyte TRX50 AI TOP (AMD TRX50)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: Some(&Ddr5BusConfig {
        i2c_buses: &[1, 2],
        slots_per_bus: 4,
    }),
    requirements: FeatureRequirements::NONE,
};
