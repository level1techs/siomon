use crate::db::boards::{BoardTemplate, Ddr5BusConfig, FeatureRequirements, HwmonConfig, Platform};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["trx50", "ai top"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "Gigabyte TRX50 AI TOP (AMD TRX50, IT8689E + IT87952E)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // ── IT8689E (primary, hwmon path) ──
        ("hwmon/it8689/in0", "Vcore"),
        ("hwmon/it8689/in1", "+3.3V"),
        ("hwmon/it8689/in2", "+12V"),
        ("hwmon/it8689/in3", "+5V"),
        ("hwmon/it8689/in4", "Vcore SoC"),
        ("hwmon/it8689/in5", "CPU VDDP"),
        ("hwmon/it8689/in6", "DRAM"),
        ("hwmon/it8689/in7", "+3.3V Standby"),
        ("hwmon/it8689/in8", "Vbat"),
        ("hwmon/it8689/fan1", "CPU Fan"),
        ("hwmon/it8689/fan2", "SYS Fan 1"),
        ("hwmon/it8689/fan3", "SYS Fan 2"),
        ("hwmon/it8689/fan4", "SYS Fan 3"),
        ("hwmon/it8689/fan5", "CPU OPT"),
        ("hwmon/it8689/temp1", "System"),
        ("hwmon/it8689/temp2", "Chipset"),
        ("hwmon/it8689/temp3", "CPU"),
        ("hwmon/it8689/temp4", "PCIe x16"),
        ("hwmon/it8689/temp5", "VRM MOS"),
        ("hwmon/it8689/temp6", "Vcore SoC MOS"),
        // ── IT87952E (secondary, hwmon path) ──
        ("hwmon/it87952/in1", "DDR VTT"),
        ("hwmon/it87952/in2", "Chipset Core"),
        ("hwmon/it87952/in4", "CPU VDD 1.8V"),
        ("hwmon/it87952/in5", "PM CLDO12"),
        ("hwmon/it87952/fan1", "SYS Fan 5 Pump"),
        ("hwmon/it87952/fan2", "SYS Fan 6 Pump"),
        ("hwmon/it87952/fan3", "SYS Fan 4"),
        ("hwmon/it87952/temp1", "PCIe x8"),
        ("hwmon/it87952/temp3", "System 2"),
        // ── IT8689E (superio path, direct I/O) ──
        ("superio/it8689e/vin0", "Vcore"),
        ("superio/it8689e/vin1", "+3.3V"),
        ("superio/it8689e/vin2", "+12V"),
        ("superio/it8689e/vin3", "+5V"),
        ("superio/it8689e/vin4", "Vcore SoC"),
        ("superio/it8689e/vin5", "CPU VDDP"),
        ("superio/it8689e/vin6", "DRAM"),
        ("superio/it8689e/vin7", "+3.3V Standby"),
        ("superio/it8689e/vin8", "Vbat"),
        ("superio/it8689e/vin9", "VIN9"),
        ("superio/it8689e/fan1", "CPU Fan"),
        ("superio/it8689e/fan2", "SYS Fan 1"),
        ("superio/it8689e/fan3", "SYS Fan 2"),
        ("superio/it8689e/fan4", "SYS Fan 3"),
        ("superio/it8689e/fan5", "CPU OPT"),
        ("superio/it8689e/fan6", "SYS Fan 4"),
        ("superio/it8689e/systin", "System"),
        ("superio/it8689e/cputin", "CPU"),
        // ── IT87952E (superio path, direct I/O) ──
        ("superio/it87952e/vin0", "VIN0"),
        ("superio/it87952e/vin1", "DDR VTT"),
        ("superio/it87952e/vin2", "Chipset Core"),
        ("superio/it87952e/vin4", "CPU VDD 1.8V"),
        ("superio/it87952e/vin5", "PM CLDO12"),
        ("superio/it87952e/fan1", "SYS Fan 5 Pump"),
        ("superio/it87952e/fan2", "SYS Fan 6 Pump"),
        ("superio/it87952e/fan3", "SYS Fan 4"),
        ("superio/it87952e/temp1", "PCIe x8"),
        ("superio/it87952e/temp3", "System 2"),
    ],

    nct_voltage_scaling: None,
    ite_voltage_scaling: Some(&voltage_scaling::GIGABYTE_TRX50_AI_TOP_ITE),
    dimm_labels: &[],
    ddr5_bus_config: Some(&Ddr5BusConfig {
        i2c_buses: &[1, 2],
        slots_per_bus: 4,
    }),
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/it8689/in1", 1.65), // +3.3V: 33/20 divider
            ("hwmon/it8689/in2", 6.0),  // +12V: 120/20 divider
            ("hwmon/it8689/in3", 2.5),  // +5V: 50/20 divider
        ],
    },
};
