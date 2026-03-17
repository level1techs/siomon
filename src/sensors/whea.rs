//! WHEA (Windows Hardware Error Architecture) sensor source.
//!
//! Queries the Windows System event log for WHEA-Logger events using
//! `wevtutil`. Tracks cumulative error counts by event ID, reporting
//! deltas from the baseline established on first poll.
//!
//! Event IDs monitored:
//! - 17: Corrected hardware error
//! - 18: Fatal hardware error
//! - 19: Corrected machine check
//! - 47: Corrected PCIe error

use std::collections::HashMap;

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

/// WHEA event IDs and their human-readable descriptions.
const WHEA_EVENTS: &[(u32, &str)] = &[
    (17, "Corrected HW Error"),
    (18, "Fatal HW Error"),
    (19, "Corrected Machine Check"),
    (47, "Corrected PCIe Error"),
];

pub struct WheaSource {
    baseline: HashMap<u32, u64>,
    initialized: bool,
}

impl WheaSource {
    pub fn discover() -> Self {
        Self {
            baseline: HashMap::new(),
            initialized: false,
        }
    }
}

impl crate::sensors::SensorSource for WheaSource {
    fn name(&self) -> &str {
        "whea"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let counts = query_whea_counts();

        if !self.initialized {
            self.baseline = counts.clone();
            self.initialized = true;
        }

        let mut readings = Vec::with_capacity(WHEA_EVENTS.len());

        for &(event_id, label) in WHEA_EVENTS {
            let total = counts.get(&event_id).copied().unwrap_or(0);
            let base = self.baseline.get(&event_id).copied().unwrap_or(0);
            let delta = total.saturating_sub(base);

            readings.push((
                SensorId {
                    source: "whea".into(),
                    chip: "system".into(),
                    sensor: format!("event_{event_id}"),
                },
                SensorReading::new(
                    format!("WHEA {label}"),
                    delta as f64,
                    SensorUnit::Unitless,
                    SensorCategory::Other,
                ),
            ));
        }

        readings
    }
}

/// Run `wevtutil` to query WHEA-Logger events and count occurrences by event ID.
fn query_whea_counts() -> HashMap<u32, u64> {
    let mut counts: HashMap<u32, u64> = HashMap::new();

    let output = match std::process::Command::new("wevtutil")
        .args([
            "qe",
            "System",
            "/q:*[System[Provider[@Name='Microsoft-Windows-WHEA-Logger']]]",
            "/c:1000",
            "/f:text",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            log::debug!("wevtutil failed: {e}");
            return counts;
        }
    };

    if !output.status.success() {
        log::debug!(
            "wevtutil exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return counts;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_wevtutil_text(&stdout, &mut counts);

    counts
}

/// Parse wevtutil text-format output, extracting Event ID lines.
///
/// The text format emits blocks like:
/// ```text
/// Event[0]:
///   Log Name: System
///   Source: Microsoft-Windows-WHEA-Logger
///   Event ID: 17
///   ...
/// ```
fn parse_wevtutil_text(text: &str, counts: &mut HashMap<u32, u64>) {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(id_str) = trimmed.strip_prefix("Event ID:") {
            if let Ok(id) = id_str.trim().parse::<u32>() {
                *counts.entry(id).or_default() += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SensorSource;

    #[test]
    fn test_whea_source_name() {
        let src = WheaSource::discover();
        assert_eq!(src.name(), "whea");
    }

    #[test]
    fn test_whea_sensor_id_format() {
        let id = SensorId {
            source: "whea".into(),
            chip: "system".into(),
            sensor: "event_17".into(),
        };
        assert_eq!(id.to_string(), "whea/system/event_17");
    }

    #[test]
    fn test_parse_wevtutil_text_empty() {
        let mut counts = HashMap::new();
        parse_wevtutil_text("", &mut counts);
        assert!(counts.is_empty());
    }

    #[test]
    fn test_parse_wevtutil_text_sample() {
        let sample = "\
Event[0]:
  Log Name: System
  Source: Microsoft-Windows-WHEA-Logger
  Event ID: 17
  Level: Warning

Event[1]:
  Log Name: System
  Source: Microsoft-Windows-WHEA-Logger
  Event ID: 17
  Level: Warning

Event[2]:
  Log Name: System
  Source: Microsoft-Windows-WHEA-Logger
  Event ID: 47
  Level: Warning

Event[3]:
  Log Name: System
  Source: Microsoft-Windows-WHEA-Logger
  Event ID: 18
  Level: Error
";
        let mut counts = HashMap::new();
        parse_wevtutil_text(sample, &mut counts);
        assert_eq!(counts.get(&17), Some(&2));
        assert_eq!(counts.get(&47), Some(&1));
        assert_eq!(counts.get(&18), Some(&1));
        assert_eq!(counts.get(&19), None);
    }

    #[test]
    fn test_whea_baseline_delta() {
        // Simulate first poll establishing baseline, second poll showing delta
        let mut src = WheaSource {
            baseline: HashMap::new(),
            initialized: false,
        };

        // First poll: baseline gets set, deltas should all be 0
        // (Cannot call poll() directly as it shells out to wevtutil,
        // so we test the logic manually)
        let counts: HashMap<u32, u64> = [(17, 5), (18, 0), (47, 3)].into_iter().collect();
        src.baseline = counts.clone();
        src.initialized = true;

        // Simulate second poll with new counts
        let new_counts: HashMap<u32, u64> = [(17, 7), (18, 1), (47, 3)].into_iter().collect();
        for &(event_id, _) in WHEA_EVENTS {
            let total = new_counts.get(&event_id).copied().unwrap_or(0);
            let base = src.baseline.get(&event_id).copied().unwrap_or(0);
            let delta = total.saturating_sub(base);
            match event_id {
                17 => assert_eq!(delta, 2),
                18 => assert_eq!(delta, 1),
                19 => assert_eq!(delta, 0),
                47 => assert_eq!(delta, 0),
                _ => {}
            }
        }
    }
}
