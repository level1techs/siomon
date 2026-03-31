use crate::db::boards::{
    BoardTemplate, Ddr5BusConfig, FEAT_DDR5, FeatureRequirements, HwmonConfig,
    NCT6799_HWMON_SCALING, Platform, Requirement,
};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["wrx90"],
    exclude_substrings: &["wrx90e"],
    match_vendor: &[],
    description: "ASRock WRX90 WS EVO (AMD TRX50, NCT6799D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // hwmon labels
        ("hwmon/nct6799/in0", "Vcore"),
        ("hwmon/nct6799/in1", "VDD_18"),
        ("hwmon/nct6799/in2", "+3.3V"),
        ("hwmon/nct6799/in3", "+3.3V Standby"),
        ("hwmon/nct6799/in4", "VDD_SOC"),
        ("hwmon/nct6799/in5", "VDD_18_2"),
        ("hwmon/nct6799/in7", "+3.3V AUX"),
        ("hwmon/nct6799/in8", "Vbat"),
        ("hwmon/nct6799/in9", "VTT"),
        ("hwmon/nct6799/in12", "VDD_SOC2"),
        ("hwmon/nct6799/in13", "VDDIO"),
        ("hwmon/nct6799/temp1", "System"),
        ("hwmon/nct6799/fan1", "CPU Fan 1"),
        ("hwmon/nct6799/fan2", "CPU Fan 2"),
        ("hwmon/nct6799/fan4", "Chassis Fan"),
        ("hwmon/nct6799/fan6", "MOS Fan 1"),
        ("hwmon/nct6799/fan7", "MOS Fan 2"),
        // superio labels (same chip, different source name with --direct-io)
        ("superio/nct6799/vin0", "Vcore"),
        ("superio/nct6799/vin1", "VDD_18"),
        ("superio/nct6799/vin2", "+3.3V"),
        ("superio/nct6799/vin3", "+3.3V Standby"),
        ("superio/nct6799/vin4", "VDD_SOC"),
        ("superio/nct6799/vin5", "VDD_18_2"),
        ("superio/nct6799/vin7", "+3.3V AUX"),
        ("superio/nct6799/fan1", "CPU Fan 1"),
        ("superio/nct6799/fan2", "CPU Fan 2"),
        ("superio/nct6799/fan4", "Chassis Fan"),
        ("superio/nct6799/fan6", "MOS Fan 1"),
        ("superio/nct6799/fan7", "MOS Fan 2"),
    ],

    // No NCT6799 voltage scaling data yet
    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: Some(&Ddr5BusConfig {
        i2c_buses: &[1, 2],
        slots_per_bus: 4,
    }),
    requirements: FeatureRequirements {
        entries: &[(
            FEAT_DDR5,
            &[Requirement::BiosSetting {
                description: "Enable I2C/SPD passthrough in BIOS Advanced settings",
            }],
        )],
    },
    hwmon: HwmonConfig {
        voltage_scaling: NCT6799_HWMON_SCALING,
    },
};
