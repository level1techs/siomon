//! IPMI BMC sensor source on Windows via `ipmitool` CLI.
//!
//! On Linux the IPMI source talks to `/dev/ipmiN` directly using the
//! `ipmi-rs` crate.  That crate requires Unix ioctls, so on Windows we
//! shell out to `ipmitool sensor list` which speaks to the Microsoft
//! IPMI driver (`\\.\IPMI`) internally.
//!
//! Like the Linux variant, BMC round-trips are slow, so polling runs in
//! a background thread on a 5-second cycle and exposes readings via
//! shared state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

/// How often the background thread polls the BMC.
const IPMI_POLL_INTERVAL: Duration = Duration::from_secs(5);

type SharedReadings = Arc<RwLock<Vec<(SensorId, SensorReading)>>>;

pub struct IpmiWinSource {
    readings: SharedReadings,
    available: bool,
    _stop: Option<Arc<AtomicBool>>,
}

impl IpmiWinSource {
    pub fn discover() -> Self {
        // Probe: run ipmitool once to see if it is installed and the IPMI
        // driver is reachable.  This blocks briefly at startup but avoids
        // spawning a useless thread when IPMI is not available.
        let probe = std::process::Command::new("ipmitool")
            .args(["sensor", "list"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        let initial = match probe {
            Ok(ref output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let parsed = parse_ipmitool_output(&text);
                if parsed.is_empty() {
                    log::debug!("IPMI: ipmitool returned no parseable sensors");
                    return Self::unavailable();
                }
                log::info!("IPMI: discovered {} sensors via ipmitool", parsed.len());
                parsed
            }
            Ok(ref output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::debug!(
                    "IPMI: ipmitool exited with {}: {}",
                    output.status,
                    stderr.lines().next().unwrap_or("(no stderr)")
                );
                return Self::unavailable();
            }
            Err(e) => {
                log::debug!("IPMI: ipmitool not found or failed to execute: {e}");
                return Self::unavailable();
            }
        };

        let readings: SharedReadings = Arc::new(RwLock::new(initial));
        let stop = Arc::new(AtomicBool::new(false));

        // Spawn background poller
        let bg_readings = readings.clone();
        let bg_stop = stop.clone();
        thread::spawn(move || {
            ipmi_background_loop(bg_readings, bg_stop);
        });

        Self {
            readings,
            available: true,
            _stop: Some(stop),
        }
    }

    fn unavailable() -> Self {
        Self {
            readings: Arc::new(RwLock::new(Vec::new())),
            available: false,
            _stop: None,
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }
}

impl crate::sensors::SensorSource for IpmiWinSource {
    fn name(&self) -> &str {
        "ipmi"
    }

    /// Returns the latest readings from the background thread -- never blocks
    /// on the ipmitool subprocess.
    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        self.readings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for IpmiWinSource {
    fn drop(&mut self) {
        if let Some(ref stop) = self._stop {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

// -- Background thread -------------------------------------------------------

fn ipmi_background_loop(readings: SharedReadings, stop: Arc<AtomicBool>) {
    log::debug!("IPMI background poller started");

    while !stop.load(Ordering::Relaxed) {
        thread::sleep(IPMI_POLL_INTERVAL);
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let output = std::process::Command::new("ipmitool")
            .args(["sensor", "list"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let new = parse_ipmitool_output(&text);
                match readings.write() {
                    Ok(mut state) => *state = new,
                    Err(e) => *e.into_inner() = new,
                }
            }
        }
    }

    log::debug!("IPMI background poller stopped");
}

// -- Parser -------------------------------------------------------------------

/// Parse `ipmitool sensor list` output.
///
/// Each line is pipe-delimited with 10 fields:
/// ```text
/// Name             | Value      | Unit       | Status | LNR  | LC   | LNC  | UNC  | UC   | UNR
/// CPU Temp         | 45.000     | degrees C  | ok     | na   | 0.0  | 5.0  | 90.0 | 95.0 | na
/// ```
fn parse_ipmitool_output(text: &str) -> Vec<(SensorId, SensorReading)> {
    let mut results = Vec::new();

    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            continue;
        }

        let name = parts[0];
        let value_str = parts[1];
        let unit_str = parts[2];

        // Skip sensors whose reading is "na" or otherwise non-numeric.
        let value: f64 = match value_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        if !value.is_finite() {
            continue;
        }

        let (unit, category) = map_unit(unit_str);

        let sensor_name = name.to_lowercase().replace(' ', "_");

        let id = SensorId {
            source: "ipmi".into(),
            chip: "bmc".into(),
            sensor: sensor_name,
        };

        results.push((
            id,
            SensorReading::new(name.to_string(), value, unit, category),
        ));
    }

    results
}

/// Map the unit string from `ipmitool sensor list` to our model types.
fn map_unit(unit_str: &str) -> (SensorUnit, SensorCategory) {
    match unit_str {
        "degrees C" => (SensorUnit::Celsius, SensorCategory::Temperature),
        "RPM" => (SensorUnit::Rpm, SensorCategory::Fan),
        "Volts" => (SensorUnit::Volts, SensorCategory::Voltage),
        "Watts" => (SensorUnit::Watts, SensorCategory::Power),
        "Amps" => (SensorUnit::Amps, SensorCategory::Current),
        "percent" => (SensorUnit::Percent, SensorCategory::Utilization),
        _ => (SensorUnit::Unitless, SensorCategory::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OUTPUT: &str = "\
CPU Temp         | 45.000     | degrees C  | ok    | na        | 0.000     | 5.000     | 90.000    | 95.000    | na
System Temp      | 33.000     | degrees C  | ok    | na        | 0.000     | 5.000     | 80.000    | 85.000    | na
DIMMA1 Temp      | 37.000     | degrees C  | ok    | na        | 0.000     | 5.000     | 80.000    | 85.000    | na
Fan1             | 2100.000   | RPM        | ok    | na        | 300.000   | 500.000   | na        | na        | na
Fan2             | 0.000      | RPM        | ok    | na        | 300.000   | 500.000   | na        | na        | na
12V              | 12.192     | Volts      | ok    | na        | 10.200    | 10.800    | 13.200    | 13.800    | na
5VCC             | 5.088      | Volts      | ok    | na        | 4.250     | 4.500     | 5.500     | 5.750     | na
VBAT             | 3.216      | Volts      | ok    | na        | 2.550     | 2.700     | na        | na        | na
PSU Power        | 185.000    | Watts      | ok    | na        | na        | na        | 900.000   | na        | na
Chassis Intru    | na         |            | na    | na        | na        | na        | na        | na        | na";

    #[test]
    fn parse_temperatures() {
        let results = parse_ipmitool_output(SAMPLE_OUTPUT);
        let temps: Vec<_> = results
            .iter()
            .filter(|(_, r)| r.category == SensorCategory::Temperature)
            .collect();
        assert_eq!(temps.len(), 3);
        assert_eq!(temps[0].1.label, "CPU Temp");
        assert!((temps[0].1.current - 45.0).abs() < 0.001);
        assert_eq!(temps[0].1.unit, SensorUnit::Celsius);
    }

    #[test]
    fn parse_fans() {
        let results = parse_ipmitool_output(SAMPLE_OUTPUT);
        let fans: Vec<_> = results
            .iter()
            .filter(|(_, r)| r.category == SensorCategory::Fan)
            .collect();
        assert_eq!(fans.len(), 2);
        assert_eq!(fans[0].1.label, "Fan1");
        assert!((fans[0].1.current - 2100.0).abs() < 0.001);
        assert_eq!(fans[0].1.unit, SensorUnit::Rpm);
    }

    #[test]
    fn parse_voltages() {
        let results = parse_ipmitool_output(SAMPLE_OUTPUT);
        let volts: Vec<_> = results
            .iter()
            .filter(|(_, r)| r.category == SensorCategory::Voltage)
            .collect();
        assert_eq!(volts.len(), 3);
        assert_eq!(volts[0].0.sensor, "12v");
        assert!((volts[0].1.current - 12.192).abs() < 0.001);
    }

    #[test]
    fn parse_power() {
        let results = parse_ipmitool_output(SAMPLE_OUTPUT);
        let power: Vec<_> = results
            .iter()
            .filter(|(_, r)| r.category == SensorCategory::Power)
            .collect();
        assert_eq!(power.len(), 1);
        assert_eq!(power[0].1.label, "PSU Power");
        assert!((power[0].1.current - 185.0).abs() < 0.001);
    }

    #[test]
    fn skip_na_readings() {
        let results = parse_ipmitool_output(SAMPLE_OUTPUT);
        // "Chassis Intru" has "na" value -- should be skipped
        assert!(results.iter().all(|(id, _)| id.sensor != "chassis_intru"));
    }

    #[test]
    fn sensor_ids_are_lowercase_underscored() {
        let results = parse_ipmitool_output(SAMPLE_OUTPUT);
        for (id, _) in &results {
            assert_eq!(id.source, "ipmi");
            assert_eq!(id.chip, "bmc");
            assert!(!id.sensor.contains(' '));
            assert_eq!(id.sensor, id.sensor.to_lowercase());
        }
    }

    #[test]
    fn map_unit_known() {
        assert_eq!(
            map_unit("degrees C"),
            (SensorUnit::Celsius, SensorCategory::Temperature)
        );
        assert_eq!(map_unit("RPM"), (SensorUnit::Rpm, SensorCategory::Fan));
        assert_eq!(
            map_unit("Volts"),
            (SensorUnit::Volts, SensorCategory::Voltage)
        );
        assert_eq!(
            map_unit("Watts"),
            (SensorUnit::Watts, SensorCategory::Power)
        );
        assert_eq!(
            map_unit("Amps"),
            (SensorUnit::Amps, SensorCategory::Current)
        );
    }

    #[test]
    fn map_unit_unknown() {
        assert_eq!(
            map_unit("something_else"),
            (SensorUnit::Unitless, SensorCategory::Other)
        );
    }

    #[test]
    fn empty_input() {
        assert!(parse_ipmitool_output("").is_empty());
    }

    #[test]
    fn malformed_lines() {
        let input = "this is not valid\n| also bad\n";
        assert!(parse_ipmitool_output(input).is_empty());
    }
}
