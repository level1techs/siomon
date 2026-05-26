use crate::db::boards::{
    BoardTemplate, Ddr5BusConfig, FeatureRequirements, HwmonConfig, NCT6799_HWMON_SCALING, Platform,
};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["pro ws", "trx50", "sage"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS Pro WS TRX50-SAGE WIFI A (AMD TRX50)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // hwmon labels
        ("hwmon/nct6799/in0", "CPU Core0"),
        ("hwmon/nct6799/in1", "+5V"),
        ("hwmon/nct6799/in3", "+3.3V"),
        ("hwmon/nct6799/in4", "+12V"),
        ("hwmon/nct6799/in10", "CPU VDDIO"),
        ("hwmon/nct6799/in12", "VDD_11_S3 / MC"),
        ("hwmon/nct6799/in15", "CPU VSOC"),
        ("hwmon/nct6799/temp1", "MotherBoard Temperature"),
        ("hwmon/nct6799/temp2", "CPU Temperature"),
        ("hwmon/nct6799/fan1", "Chassis Fan 1 Speed"),
        ("hwmon/nct6799/fan2", "CPU Fan Speed"),
        ("hwmon/nct6799/fan3", "Chassis Fan 2 Speed"),
        ("hwmon/nct6799/fan4", "Chassis Fan 3 Speed"),
        ("hwmon/nct6799/fan5", "Chassis Fan 4 Speed"),
        ("hwmon/nct6799/fan6", "Water Pump+ Speed"),
        // CPU optional + VRM fans are exposed on asusec, not NCT fan7.
        ("hwmon/nct6799/fan7", ""),
        // ASUS EC (asusec) labels
        ("hwmon/asusec/fan1", "CPU Optional Fan Speed"),
        ("hwmon/asusec/fan2", "VRM_W Heatsink Fan Speed"),
        ("hwmon/asusec/fan3", "VRM_E Heatsink Fan Speed"),
        ("hwmon/asusec/temp1", "CPU Temperature"),
        ("hwmon/asusec/temp2", "CPU Package Temperature"),
        ("hwmon/asusec/temp3", "T_Sensor Temperature"),
        ("hwmon/asusec/temp4", "VRM_E Temperature"),
        ("hwmon/asusec/temp5", "VRM_W Temperature"),
        // superio labels (same chip, different source name with --direct-io)
        ("superio/nct6799/cputin", "CPU Temperature"),
        ("superio/nct6799/systin", "MotherBoard Temperature"),
        ("superio/nct6799/fan1", "Chassis Fan 1 Speed"),
        ("superio/nct6799/fan2", "CPU Fan Speed"),
        ("superio/nct6799/fan3", "Chassis Fan 2 Speed"),
        ("superio/nct6799/fan4", "Chassis Fan 3 Speed"),
        ("superio/nct6799/fan5", "Chassis Fan 4 Speed"),
        ("superio/nct6799/fan6", "Water Pump+ Speed"),
        ("superio/nct6799/fan7", ""),
    ],

    nct_voltage_scaling: Some(&voltage_scaling::ASUS_TRX50_SAGE),
    ite_voltage_scaling: None,
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
        voltage_scaling: NCT6799_HWMON_SCALING,
    },
};
