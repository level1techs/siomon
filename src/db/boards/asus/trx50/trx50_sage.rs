use crate::db::boards::{BoardTemplate, Ddr5BusConfig, FeatureRequirements, HwmonConfig, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["pro ws", "trx50", "sage"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS Pro WS TRX50-SAGE WIFI A (AMD TRX50)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[],

    nct_voltage_scaling: None,
    dimm_labels: &[],
    // DDR5 direct probing: I2C buses 0 and 1 are DesignWare controllers
    // connected to DIMM slots. Enables SPD EEPROM reads and per-DIMM
    // temperature sensors with --direct-io.
    ddr5_bus_config: Some(&Ddr5BusConfig {
        i2c_buses: &[0, 1],
        slots_per_bus: 4,
    }),
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[],
    },
};
