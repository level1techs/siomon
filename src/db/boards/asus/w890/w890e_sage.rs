use crate::db::boards::{BoardTemplate, Ddr5BusConfig, FeatureRequirements, HwmonConfig, Platform};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["pro ws", "w890", "sage"],
    exclude_substrings: &[],
    match_vendor: &["asus"],
    description: "ASUS Pro WS W890E-SAGE SE (Intel W890, NCT6799D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // The BMC/IPMI path exposes the real fan headers on this board.
        // Hide the NCT6799 direct tach block, which reports zeroes here.
        ("superio/nct6799/fan1", ""),
        ("superio/nct6799/fan2", ""),
        ("superio/nct6799/fan3", ""),
        ("superio/nct6799/fan4", ""),
        ("superio/nct6799/fan5", ""),
        ("superio/nct6799/fan6", ""),
        ("superio/nct6799/fan7", ""),
    ],

    // Derived from a live direct-I/O scan on Pro WS W890E-SAGE SE.
    // Buses 0 and 2 are bare Synopsys DesignWare adapters; bus 1 hosts
    // ACPI devices (ITE8800/MSFT8000) and is excluded from DDR5 probing.
    nct_voltage_scaling: Some(&voltage_scaling::ASUS_W890E_SAGE),
    ite_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: Some(&Ddr5BusConfig {
        i2c_buses: &[0, 2],
        slots_per_bus: 4,
    }),
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[],
    },
};
