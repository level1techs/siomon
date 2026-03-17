//! ACPI thermal zone temperature sensor on Windows via WMI.
//!
//! Queries `MSAcpi_ThermalZoneTemperature` in the `root\WMI` namespace
//! using PowerShell's `Get-CimInstance`. The `CurrentTemperature` field
//! is in tenths of Kelvin and is converted to Celsius:
//!
//!   celsius = (raw / 10.0) - 273.15
//!
//! Discovery is attempted once; systems without ACPI thermal zones (or
//! without admin access) will simply report zero sensors.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

pub struct AcpiThermalSource {
    zones: Vec<ThermalZone>,
}

struct ThermalZone {
    name: String,
}

impl AcpiThermalSource {
    pub fn discover() -> Self {
        let zones = query_zones().unwrap_or_default();
        if zones.is_empty() {
            log::debug!("ACPI thermal: no zones found");
        } else {
            log::info!("ACPI thermal: discovered {} zone(s)", zones.len());
        }
        Self { zones }
    }
}

impl crate::sensors::SensorSource for AcpiThermalSource {
    fn name(&self) -> &str {
        "acpi_thermal"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        if self.zones.is_empty() {
            return vec![];
        }
        query_temperatures(&self.zones)
    }
}

/// Run the PowerShell WMI query and return parsed JSON entries.
fn run_wmi_query() -> Option<Vec<serde_json::Value>> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance -Namespace root/WMI -ClassName MSAcpi_ThermalZoneTemperature \
             -ErrorAction SilentlyContinue | \
             Select-Object InstanceName,CurrentTemperature | \
             ConvertTo-Json -Compress",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        log::debug!(
            "ACPI thermal PowerShell exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    // PowerShell returns a bare object (not an array) when there is only one result.
    // Normalise to always be a Vec.
    if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<serde_json::Value>>(trimmed).ok()
    } else {
        serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .map(|v| vec![v])
    }
}

/// Extract a short zone name from an InstanceName like
/// `ACPI\ThermalZone\TZ00_0` -> `TZ00`.
fn short_zone_name(instance_name: &str) -> String {
    // Take the last path segment and strip any trailing underscore + digits.
    let segment = instance_name
        .rsplit(&['\\', '/'][..])
        .next()
        .unwrap_or(instance_name);

    // Strip trailing `_<digits>` suffix (e.g. "TZ00_0" -> "TZ00")
    if let Some(pos) = segment.rfind('_') {
        let after = &segment[pos + 1..];
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
            return segment[..pos].to_string();
        }
    }

    segment.to_string()
}

/// Discover which thermal zones exist.
fn query_zones() -> Option<Vec<ThermalZone>> {
    let entries = run_wmi_query()?;
    let mut zones = Vec::new();

    for entry in &entries {
        if let Some(instance) = entry.get("InstanceName").and_then(|v| v.as_str()) {
            zones.push(ThermalZone {
                name: short_zone_name(instance),
            });
        }
    }

    if zones.is_empty() { None } else { Some(zones) }
}

/// Query current temperatures and return sensor readings.
fn query_temperatures(zones: &[ThermalZone]) -> Vec<(SensorId, SensorReading)> {
    let entries = match run_wmi_query() {
        Some(e) => e,
        None => return vec![],
    };

    let mut readings = Vec::new();

    for (entry, zone) in entries.iter().zip(zones.iter()) {
        let raw = match entry.get("CurrentTemperature") {
            Some(v) => match v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)) {
                Some(r) => r,
                None => continue,
            },
            None => continue,
        };

        let celsius = (raw / 10.0) - 273.15;

        // Filter out unrealistic values.
        if !(-50.0..=150.0).contains(&celsius) {
            log::debug!(
                "ACPI thermal {}: ignoring unrealistic value {:.1} C (raw={})",
                zone.name,
                celsius,
                raw
            );
            continue;
        }

        readings.push((
            SensorId {
                source: "acpi_thermal".into(),
                chip: "acpi".into(),
                sensor: zone.name.to_lowercase(),
            },
            SensorReading::new(
                format!("ACPI {}", zone.name),
                celsius,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
        ));
    }

    readings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SensorSource;

    #[test]
    fn test_source_name() {
        let src = AcpiThermalSource { zones: vec![] };
        assert_eq!(src.name(), "acpi_thermal");
    }

    #[test]
    fn test_short_zone_name_typical() {
        assert_eq!(short_zone_name(r"ACPI\ThermalZone\TZ00_0"), "TZ00");
    }

    #[test]
    fn test_short_zone_name_forward_slash() {
        assert_eq!(short_zone_name("ACPI/ThermalZone/TZ01_0"), "TZ01");
    }

    #[test]
    fn test_short_zone_name_no_suffix() {
        assert_eq!(short_zone_name(r"ACPI\ThermalZone\THRM"), "THRM");
    }

    #[test]
    fn test_short_zone_name_bare() {
        assert_eq!(short_zone_name("TZ00_0"), "TZ00");
    }

    #[test]
    fn test_short_zone_name_no_underscore() {
        assert_eq!(short_zone_name("TZ00"), "TZ00");
    }

    #[test]
    fn test_kelvin_to_celsius_conversion() {
        // 3132 tenths of Kelvin = 313.2 K = 40.05 C
        let raw = 3132.0_f64;
        let celsius = (raw / 10.0) - 273.15;
        assert!((celsius - 40.05).abs() < 0.01);
    }

    #[test]
    fn test_kelvin_to_celsius_boiling() {
        // 3732 tenths of Kelvin = 373.2 K = 100.05 C
        let raw = 3732.0_f64;
        let celsius = (raw / 10.0) - 273.15;
        assert!((celsius - 100.05).abs() < 0.01);
    }

    #[test]
    fn test_poll_empty_zones() {
        let mut src = AcpiThermalSource { zones: vec![] };
        assert!(src.poll().is_empty());
    }

    #[test]
    fn test_sensor_id_format() {
        let id = SensorId {
            source: "acpi_thermal".into(),
            chip: "acpi".into(),
            sensor: "tz00".into(),
        };
        assert_eq!(id.to_string(), "acpi_thermal/acpi/tz00");
    }

    #[test]
    fn test_parse_single_result() {
        let json = r#"{"InstanceName":"ACPI\\ThermalZone\\TZ00_0","CurrentTemperature":3132}"#;
        let val: serde_json::Value = serde_json::from_str(json).unwrap();
        let entries = vec![val];

        assert_eq!(entries.len(), 1);
        let instance = entries[0]["InstanceName"].as_str().unwrap();
        assert_eq!(short_zone_name(instance), "TZ00");
    }

    #[test]
    fn test_parse_array_result() {
        let json = r#"[
            {"InstanceName":"ACPI\\ThermalZone\\TZ00_0","CurrentTemperature":3132},
            {"InstanceName":"ACPI\\ThermalZone\\TZ01_0","CurrentTemperature":3232}
        ]"#;
        let entries: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        assert_eq!(entries.len(), 2);
    }
}
