//! IPMI BMC sensor source via native /dev/ipmi0 ioctl (ipmi-rs crate).
//!
//! Provides access to BMC-managed sensors including DIMM temperatures,
//! per-CCD voltages, PSU telemetry, and labeled fan RPMs. Works on
//! server and workstation boards with a BMC (requires /dev/ipmi0 + root).
//!
//! BMC round-trips are slow (~30-50ms per sensor, 800ms+ total). To avoid
//! blocking the main poll loop, IPMI runs in its own background thread on
//! a 5-second cycle and exposes readings via shared state.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use ipmi_rs::connection::CompletionErrorCode;
use ipmi_rs::sensor_event::{GetSensorReading, ThresholdReading};
use ipmi_rs::storage::sdr::record::{IdentifiableSensor, InstancedSensor, RecordContents};
use ipmi_rs::storage::sdr::{SensorType, Unit};
use ipmi_rs::{File as IpmiFile, Ipmi, IpmiError};

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

/// How often the background thread polls the BMC.
const IPMI_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Pre-loaded SDR entry for a full (threshold) sensor.
struct SdrEntry {
    id: SensorId,
    label: String,
    category: SensorCategory,
    unit: SensorUnit,
    /// Sensor key bytes for GetSensorReading command.
    key: ipmi_rs::storage::sdr::record::SensorKey,
    /// Linearization parameters from the SDR record.
    m: i16,
    b: i16,
    result_exponent: i8,
    b_exponent: i8,
}

type SharedReadings = Arc<RwLock<Vec<(SensorId, SensorReading)>>>;

pub struct IpmiSource {
    readings: SharedReadings,
    available: bool,
    _stop: Option<Arc<AtomicBool>>,
}

impl IpmiSource {
    pub fn discover() -> Self {
        let dev_path = match find_ipmi_device() {
            Some(p) => p,
            None => {
                log::debug!("IPMI: no /dev/ipmiN device found");
                return Self::unavailable();
            }
        };

        let file = match IpmiFile::new(&dev_path, Duration::from_secs(2)) {
            Ok(f) => f,
            Err(e) => {
                log::debug!("IPMI: failed to open {}: {e}", dev_path.display());
                return Self::unavailable();
            }
        };

        let mut ipmi = Ipmi::new(file);

        // Load all SDR records and extract full (threshold) sensor entries
        let sdrs: Vec<_> = ipmi.sdrs().collect();
        let mut entries = Vec::new();

        for record in &sdrs {
            if let RecordContents::FullSensor(full) = &record.contents {
                let sensor_type = full.ty();
                let (category, unit) = match map_sensor_type(sensor_type) {
                    Some(cu) => cu,
                    None => continue,
                };

                let name = full.id_string().to_string();
                let sensor_name = name
                    .trim_start_matches(['+', '-'])
                    .to_lowercase()
                    .replace(' ', "_");

                let id = SensorId {
                    source: "ipmi".into(),
                    chip: "bmc".into(),
                    sensor: sensor_name,
                };

                entries.push(SdrEntry {
                    id,
                    label: name,
                    category,
                    unit,
                    key: *full.key_data(),
                    m: full.m,
                    b: full.b,
                    result_exponent: full.result_exponent,
                    b_exponent: full.b_exponent,
                });
            }
        }

        log::info!(
            "IPMI: loaded {} threshold sensors from {} SDR records via {}",
            entries.len(),
            sdrs.len(),
            dev_path.display()
        );

        if entries.is_empty() {
            return Self::unavailable();
        }

        // Do one synchronous poll before spawning the background thread
        // so that the first main-loop poll has data immediately.
        let initial = poll_bmc(&mut ipmi, &entries);

        let readings: SharedReadings = Arc::new(RwLock::new(initial));
        let stop = Arc::new(AtomicBool::new(false));

        // Spawn background poller
        let bg_readings = readings.clone();
        let bg_stop = stop.clone();
        thread::spawn(move || {
            ipmi_background_loop(ipmi, entries, bg_readings, bg_stop);
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

impl crate::sensors::SensorSource for IpmiSource {
    fn name(&self) -> &str {
        "ipmi"
    }

    /// Returns the latest readings from the background thread — never blocks on BMC I/O.
    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        self.readings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for IpmiSource {
    fn drop(&mut self) {
        if let Some(ref stop) = self._stop {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

// ── Background thread ───────────────────────────────────────────────────

fn ipmi_background_loop(
    mut ipmi: Ipmi<IpmiFile>,
    entries: Vec<SdrEntry>,
    readings: SharedReadings,
    stop: Arc<AtomicBool>,
) {
    log::debug!("IPMI background poller started ({} sensors)", entries.len());

    while !stop.load(Ordering::Relaxed) {
        thread::sleep(IPMI_POLL_INTERVAL);
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let new_readings = poll_bmc(&mut ipmi, &entries);

        match readings.write() {
            Ok(mut state) => *state = new_readings,
            Err(e) => *e.into_inner() = new_readings,
        }
    }

    log::debug!("IPMI background poller stopped");
}

/// Do one full round of BMC sensor reads. Called by both the initial
/// synchronous poll and the background thread.
fn poll_bmc(ipmi: &mut Ipmi<IpmiFile>, entries: &[SdrEntry]) -> Vec<(SensorId, SensorReading)> {
    let mut results = Vec::with_capacity(entries.len());

    for entry in entries {
        let raw = match ipmi.send_recv(GetSensorReading::for_sensor_key(&entry.key)) {
            Ok(v) => v,
            Err(IpmiError::Failed {
                completion_code: CompletionErrorCode::RequestedDatapointNotPresent,
                ..
            }) => continue,
            Err(_) => continue,
        };

        let threshold: ThresholdReading = (&raw).into();
        let raw_byte = match threshold.reading {
            Some(v) => v,
            None => continue,
        };

        let value = convert_reading(
            raw_byte,
            entry.m,
            entry.b,
            entry.result_exponent,
            entry.b_exponent,
        );

        if !value.is_finite() {
            continue;
        }

        results.push((
            entry.id.clone(),
            SensorReading::new(entry.label.clone(), value, entry.unit, entry.category),
        ));
    }

    results
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// IPMI sensor value linearization formula.
/// y = (m * raw + b * 10^b_exp) * 10^result_exp
fn convert_reading(raw: u8, m: i16, b: i16, result_exp: i8, b_exp: i8) -> f64 {
    let m = m as f64;
    let b = b as f64 * 10_f64.powi(b_exp as i32);
    let result_mul = 10_f64.powi(result_exp as i32);
    (m * raw as f64 + b) * result_mul
}

/// Find the first available IPMI device file.
fn find_ipmi_device() -> Option<std::path::PathBuf> {
    for i in 0..4 {
        let path = std::path::PathBuf::from(format!("/dev/ipmi{i}"));
        if path.exists() {
            return Some(path);
        }
    }
    let alt = Path::new("/dev/ipmi/0");
    if alt.exists() {
        return Some(alt.to_path_buf());
    }
    None
}

/// Map IPMI SensorType to our SensorCategory + SensorUnit.
/// Returns None for non-analog / discrete sensor types we don't display.
fn map_sensor_type(ty: &SensorType) -> Option<(SensorCategory, SensorUnit)> {
    match ty {
        SensorType::Temperature => Some((SensorCategory::Temperature, SensorUnit::Celsius)),
        SensorType::Voltage => Some((SensorCategory::Voltage, SensorUnit::Volts)),
        SensorType::Current => Some((SensorCategory::Current, SensorUnit::Amps)),
        SensorType::Fan => Some((SensorCategory::Fan, SensorUnit::Rpm)),
        SensorType::PowerSupply => Some((SensorCategory::Power, SensorUnit::Watts)),
        _ => None,
    }
}

/// Refine unit based on the IPMI SDR Unit field (overrides the SensorType default).
#[allow(dead_code)]
fn refine_unit(ipmi_unit: &Unit) -> Option<SensorUnit> {
    match ipmi_unit {
        Unit::DegreesCelsius => Some(SensorUnit::Celsius),
        Unit::Volt => Some(SensorUnit::Volts),
        Unit::Amp => Some(SensorUnit::Amps),
        Unit::Watt => Some(SensorUnit::Watts),
        Unit::RevolutionsPerMinute => Some(SensorUnit::Rpm),
        Unit::Hertz => Some(SensorUnit::Mhz),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_simple() {
        assert!((convert_reading(25, 1, 0, 0, 0) - 25.0).abs() < 0.001);
    }

    #[test]
    fn convert_with_multiplier() {
        assert!((convert_reading(50, 2, 0, 0, 0) - 100.0).abs() < 0.001);
    }

    #[test]
    fn convert_with_b_offset() {
        assert!((convert_reading(25, 1, 10, 0, 0) - 35.0).abs() < 0.001);
    }

    #[test]
    fn convert_with_exponents() {
        assert!((convert_reading(100, 1, 0, -2, 0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn convert_with_b_exponent() {
        assert!((convert_reading(10, 1, 5, 0, 1) - 60.0).abs() < 0.001);
    }

    #[test]
    fn map_temperature() {
        let (cat, unit) = map_sensor_type(&SensorType::Temperature).unwrap();
        assert_eq!(cat, SensorCategory::Temperature);
        assert_eq!(unit, SensorUnit::Celsius);
    }

    #[test]
    fn map_voltage() {
        let (cat, unit) = map_sensor_type(&SensorType::Voltage).unwrap();
        assert_eq!(cat, SensorCategory::Voltage);
        assert_eq!(unit, SensorUnit::Volts);
    }

    #[test]
    fn map_fan() {
        let (cat, unit) = map_sensor_type(&SensorType::Fan).unwrap();
        assert_eq!(cat, SensorCategory::Fan);
        assert_eq!(unit, SensorUnit::Rpm);
    }

    #[test]
    fn map_discrete_returns_none() {
        assert!(map_sensor_type(&SensorType::ChassisIntrusion).is_none());
        assert!(map_sensor_type(&SensorType::Processor).is_none());
        assert!(map_sensor_type(&SensorType::Memory).is_none());
    }

    #[test]
    fn find_device_path() {
        let _ = find_ipmi_device();
    }
}
