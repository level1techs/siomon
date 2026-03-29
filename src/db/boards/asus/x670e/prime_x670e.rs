use crate::db::boards::{ASUS_AM5_NCT6798_LABELS, BoardTemplate, FeatureRequirements, Platform};

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["prime"],
    exclude_substrings: &[],
    match_any: &["x670", "b650"],
    description: "ASUS PRIME X670E/B650 (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: &[("hwmon/nct6798/fan2", "Chassis Fan 1")],

    // No board-specific voltage scaling known
    nct_voltage_scaling: None,
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
};
