use crate::db::boards::{
    ASUS_NCT6798_HWMON_SCALING, BoardTemplate, Ddr5BusConfig, DimmSlotLabel, FEAT_DDR5,
    FeatureRequirements, HwmonConfig, Platform, Requirement,
};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["wrx90e"],
    exclude_substrings: &[],
    match_vendor: &[],
    description: "ASUS Pro WS WRX90E-SAGE SE (AMD TRX50, NCT6798D)",
    platform: Platform::Generic,

    // WRX90E has its own label set (not AM5)
    base_labels: None,
    sensor_labels: &[
        ("hwmon/nct6798/in0", "Vcore"),
        ("hwmon/nct6798/in1", "+5V"),
        ("hwmon/nct6798/in2", "+3.3V"),
        ("hwmon/nct6798/in3", "+3.3V Standby"),
        ("hwmon/nct6798/in4", "+12V"),
        ("hwmon/nct6798/in5", "VIN5"),
        ("hwmon/nct6798/in6", "VIN6"),
        ("hwmon/nct6798/in7", "+3.3V AUX"),
        ("hwmon/nct6798/in8", "Vbat"),
        ("hwmon/nct6798/temp1", "SYSTIN"),
        ("hwmon/nct6798/temp2", "CPUTIN"),
        ("hwmon/nct6798/temp3", "AUXTIN0"),
        ("hwmon/nct6798/fan1", "CPU Fan"),
        ("hwmon/nct6798/fan2", "Chassis Fan 1"),
        ("hwmon/nct6798/fan3", "Chassis Fan 2"),
        ("hwmon/nct6798/fan4", "Chassis Fan 3"),
        ("hwmon/nct6798/fan5", "Chassis Fan 4"),
        ("hwmon/nct6798/fan6", "Chassis Fan 5"),
        ("hwmon/nct6798/fan7", "AIO Pump"),
    ],

    nct_voltage_scaling: Some(&voltage_scaling::ASUS_WRX90E_SAGE),

    // 8-channel DDR5, dual-rank 96GB DIMMs (16 ranks total)
    // Rank numbering is non-contiguous: csrow0 = channels 0,2,3,5,6,8,9,11
    //                                   csrow1 = second rank of same DIMMs
    dimm_labels: &[
        // csrow 0: first rank of each physical DIMM
        DimmSlotLabel {
            mc: 0,
            rank: 0,
            label: "P0 Channel A DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 2,
            label: "P0 Channel B DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 3,
            label: "P0 Channel C DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 5,
            label: "P0 Channel D DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 6,
            label: "P0 Channel E DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 8,
            label: "P0 Channel F DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 9,
            label: "P0 Channel G DIMM 0",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 11,
            label: "P0 Channel H DIMM 0",
        },
        // csrow 1: second rank of each physical DIMM
        DimmSlotLabel {
            mc: 0,
            rank: 12,
            label: "P0 Channel A DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 14,
            label: "P0 Channel B DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 15,
            label: "P0 Channel C DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 17,
            label: "P0 Channel D DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 18,
            label: "P0 Channel E DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 20,
            label: "P0 Channel F DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 21,
            label: "P0 Channel G DIMM 0 R1",
        },
        DimmSlotLabel {
            mc: 0,
            rank: 23,
            label: "P0 Channel H DIMM 0 R1",
        },
    ],
    ddr5_bus_config: Some(&Ddr5BusConfig {
        i2c_buses: &[1, 2],
        slots_per_bus: 4,
    }),
    requirements: FeatureRequirements {
        entries: &[(
            FEAT_DDR5,
            &[Requirement::MinBiosVersion {
                version: 1317,
                hint: "Update ASUS WRX90E BIOS to >= 1317 to expose DDR5 I2C SPD addresses.",
            }],
        )],
    },
    hwmon: HwmonConfig {
        voltage_scaling: ASUS_NCT6798_HWMON_SCALING,
    },
};
