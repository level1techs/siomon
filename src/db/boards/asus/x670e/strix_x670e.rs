use crate::db::boards::{ASUS_AM5_NCT6798_LABELS, BoardTemplate, FeatureRequirements, Platform};
use crate::db::voltage_scaling;

pub static BOARD: BoardTemplate = BoardTemplate {
    match_substrings: &["strix"],
    exclude_substrings: &[],
    match_any: &["x670", "b650"],
    description: "ASUS ROG STRIX X670E/B650E (AMD AM5, NCT6798D)",
    platform: Platform::Generic,

    base_labels: Some(ASUS_AM5_NCT6798_LABELS),
    sensor_labels: &[
        ("hwmon/nct6798/fan2", "Chassis Fan 1"),
        ("hwmon/nct6798/fan3", "Chassis Fan 2"),
        ("hwmon/nct6798/fan4", "Chassis Fan 3"),
    ],

    nct_voltage_scaling: Some(&voltage_scaling::ASUS_AM5_NCT6798),
    dimm_labels: &[],
    ddr5_bus_config: None,
    requirements: FeatureRequirements::NONE,
};
