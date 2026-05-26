use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["z890", "nova"],
    exclude_substrings: &[],
    match_vendor: &["asrock"],
    description: "ASRock Z890 Nova WiFi (Intel Z890, NCT6798D + NCT6686D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // Direct Super I/O labels confirmed from the BIOS H/W Monitor page.
        ("superio/nct6798/systin", "Motherboard"),
        ("superio/nct6798/peci_agent_0", "CPU PECI"),
        ("superio/nct6798/systin2", "System 2"),
        // The board fans live on the companion NCT6686D-class controller, not
        // the NCT6798 tach inputs. Hide the bogus zero-RPM direct fan block.
        ("superio/nct6798/fan1", ""),
        ("superio/nct6798/fan2", ""),
        ("superio/nct6798/fan3", ""),
        ("superio/nct6798/fan4", ""),
        ("superio/nct6798/fan5", ""),
        ("superio/nct6798/fan6", ""),
        ("superio/nct6798/fan7", ""),
        // If the kernel hwmon driver binds this chip, use the same naming.
        ("hwmon/nct6798/in0", "Vcore"),
        ("hwmon/nct6798/in1", "VIN1"),
        ("hwmon/nct6798/in2", "+3.30V"),
        ("hwmon/nct6798/in3", "+3.30V Standby"),
        ("hwmon/nct6798/in4", "+VNNAON"),
        ("hwmon/nct6798/in5", "VCCSA"),
        ("hwmon/nct6798/in6", "+0.82V PCH"),
        ("hwmon/nct6798/in7", "+3.30V AUX"),
        ("hwmon/nct6798/in8", "Vbat"),
        ("hwmon/nct6798/in9", "VTT"),
        ("hwmon/nct6798/in10", "+5.00V"),
        ("hwmon/nct6798/in11", "+12.00V"),
        ("hwmon/nct6798/in12", "VIN12"),
        ("hwmon/nct6798/in13", "ATX5VSB"),
        ("hwmon/nct6798/in14", "VDD2"),
        ("hwmon/nct6798/in15", "VIN15"),
        ("hwmon/nct6798/in16", "+VCC1.8V"),
        ("hwmon/nct6798/temp1", "Motherboard"),
        ("hwmon/nct6798/temp2", "CPU PECI"),
        ("hwmon/nct6798/temp3", "System 2"),
        // Older kernel/module naming guesses kept for compatibility.
        ("hwmon/nct6796/in0", "Vcore"),
        ("hwmon/nct6796/in1", "VIN1"),
        ("hwmon/nct6796/in2", "+3.30V"),
        ("hwmon/nct6796/in3", "+3.30V Standby"),
        ("hwmon/nct6796/in4", "+VNNAON"),
        ("hwmon/nct6796/in5", "VCCSA"),
        ("hwmon/nct6796/in6", "+0.82V PCH"),
        ("hwmon/nct6796/in7", "+3.30V AUX"),
        ("hwmon/nct6796/in8", "Vbat"),
        ("hwmon/nct6796/in9", "VTT"),
        ("hwmon/nct6796/in10", "+5.00V"),
        ("hwmon/nct6796/in11", "+12.00V"),
        ("hwmon/nct6796/in12", "VIN12"),
        ("hwmon/nct6796/in13", "ATX5VSB"),
        ("hwmon/nct6796/in14", "VDD2"),
        ("hwmon/nct6796/in15", "VIN15"),
        ("hwmon/nct6796/in16", "+VCC1.8V"),
        ("hwmon/nct6796/temp1", "Motherboard"),
        ("hwmon/nct6796/temp2", "CPU PECI"),
        ("hwmon/nct6796/temp3", "System 2"),
        // The Linux nct6683 driver covers NCT6686D-class embedded sensor chips,
        // but the exposed hwmon device name on this board is nct6686.
        ("hwmon/nct6686/fan1", "CPU Fan 1"),
        ("hwmon/nct6686/fan2", "CPU Fan 2"),
        ("hwmon/nct6686/fan3", "Chassis Fan 1"),
        ("hwmon/nct6686/fan4", "Chassis Fan 2"),
        ("hwmon/nct6686/fan5", "Chassis Fan 3"),
        ("hwmon/nct6686/fan6", "Chassis Fan 4"),
        ("hwmon/nct6686/fan7", "AIO Pump"),
        ("hwmon/nct6686/fan8", "Water Pump"),
        // Compatibility aliases for kernels exposing the older nct6683 name.
        ("hwmon/nct6683/fan1", "CPU Fan 1"),
        ("hwmon/nct6683/fan2", "CPU Fan 2"),
        ("hwmon/nct6683/fan3", "Chassis Fan 1"),
        ("hwmon/nct6683/fan4", "Chassis Fan 2"),
        ("hwmon/nct6683/fan5", "Chassis Fan 3"),
        ("hwmon/nct6683/fan6", "Chassis Fan 4"),
        ("hwmon/nct6683/fan7", "AIO Pump"),
        ("hwmon/nct6683/fan8", "Water Pump"),
    ],

    nct_voltage_scaling: Some(&voltage_scaling::ASROCK_Z890_NOVA),
    ite_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            // Derived from the BIOS H/W Monitor page and the direct NCT6798
            // readings captured on this board.
            ("hwmon/nct6798/in10", 13.2), // +5.00V = 0.384V * 13.2 = 5.07V
            ("hwmon/nct6798/in11", 13.5), // +12.00V = 0.896V * 13.5 = 12.096V
            ("hwmon/nct6798/in13", 8.0),  // ATX5VSB = 0.632V * 8.0 = 5.056V
            // Compatibility aliases if a kernel names the chip nct6796.
            ("hwmon/nct6796/in10", 13.2),
            ("hwmon/nct6796/in11", 13.5),
            ("hwmon/nct6796/in13", 8.0),
        ],
    },
};
