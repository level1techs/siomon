use crate::db::boards::{BoardTemplate, FeatureRequirements, HwmonConfig, Platform};

// Rail names and divider ratios come from ASRock A-Tuning's board map
// (Z890STLLGDWIFI.xml) and were checked on a live board: in5 tracks the BIOS
// VCCSA setting, in6 the BIOS VDD_CPU setting, and the scaled +5V/+12V/+5VSB
// rails land within 2% of nominal.
//
// The only Super I/O on this board is an NCT6686D (ID 0xD441) at 0x2E. It is
// an EC-backed chip served by the in-tree `nct6683` driver (hwmon name
// `nct6686`); there is no NCT679x companion as on the Z890 Nova WiFi.
pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["z890", "steel legend"],
    exclude_substrings: &[],
    match_vendor: &["asrock"],
    description: "ASRock Z890 Steel Legend WiFi (Intel Z890, NCT6686D)",
    platform: Platform::Generic,

    base_labels: None,
    sensor_labels: &[
        // VCCCORE VR output as sensed by the board: the shared rail before
        // the per-core DLVRs, not a per-core voltage.
        ("hwmon/nct6686/in0", "Vcore"),
        // VIN0's electrical identity is unresolved (captures conflict), so
        // it is hidden rather than given a guessed name.
        ("hwmon/nct6686/in1", ""),
        ("hwmon/nct6686/in2", "+5V"),
        ("hwmon/nct6686/in3", "+12V"),
        ("hwmon/nct6686/in4", "+5VSB"),
        ("hwmon/nct6686/in5", "VCCSA"),
        ("hwmon/nct6686/in6", "VDD_CPU"),
        ("hwmon/nct6686/in7", "+3.3V"),
        ("hwmon/nct6686/in8", "VBAT"),
        // The EC's cached PECI reading of the hottest core.
        ("hwmon/nct6686/temp1", "CPU PECI"),
        ("hwmon/nct6686/fan1", "CPU_FAN1"),
        ("hwmon/nct6686/fan2", "CHA_FAN3"),
        ("hwmon/nct6686/fan3", "CPU_FAN2"),
        ("hwmon/nct6686/fan4", "CHA_FAN1"),
        ("hwmon/nct6686/fan5", "CHA_FAN2"),
        ("hwmon/nct6686/fan6", "CHA_FAN4"),
        ("hwmon/nct6686/fan7", "CHA_FAN5"),
        ("hwmon/nct6686/fan8", "AIO_PUMP"),
    ],

    nct_voltage_scaling: None,
    ite_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
    hwmon: HwmonConfig {
        voltage_scaling: &[
            ("hwmon/nct6686/in2", 5.02), // +5V   = 1.008V * 5.02 = 5.06V
            ("hwmon/nct6686/in3", 12.0), // +12V  = 0.992V * 12.0 = 11.90V
            ("hwmon/nct6686/in4", 3.32), // +5VSB = 1.520V * 3.32 = 5.05V
        ],
    },
};
