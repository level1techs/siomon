use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};
use std::collections::HashMap;
use std::path::Path;

/// Hwmon chip names belonging to GPU drivers. Labels from these chips are
/// auto-prefixed with "GPU " to avoid ambiguity with CPU-side names (e.g.
/// amdgpu's "PPT" label vs CPU Package Power Tracking).
const GPU_HWMON_CHIPS: &[&str] = &["amdgpu", "nouveau", "i915", "xe"];

fn is_gpu_hwmon_chip(chip_name: &str) -> bool {
    GPU_HWMON_CHIPS.contains(&chip_name)
}

/// Prefix a GPU hwmon label with "GPU " if it doesn't already start with "GPU".
fn gpu_prefix_label(label: String) -> String {
    if label.starts_with("GPU") {
        label
    } else {
        format!("GPU {label}")
    }
}

pub struct HwmonSource {
    chips: Vec<ChipSensors>,
}

struct ChipSensors {
    entries: Vec<SensorEntry>,
}

struct SensorEntry {
    id: SensorId,
    label: String,
    input_file: CachedFile,
    category: SensorCategory,
    unit: SensorUnit,
    divisor: f64,
}

impl HwmonSource {
    pub fn discover(label_overrides: &HashMap<String, String>) -> Self {
        let mut chips = Vec::new();

        for hwmon_dir in sysfs::glob_paths("/sys/class/hwmon/hwmon*") {
            let chip_name = sysfs::read_string_optional(&hwmon_dir.join("name"))
                .unwrap_or_else(|| "unknown".into());

            let mut entries = Vec::new();

            // Temperature sensors
            discover_type(
                &hwmon_dir,
                &chip_name,
                "temp",
                SensorCategory::Temperature,
                SensorUnit::Celsius,
                1000.0,
                label_overrides,
                &mut entries,
            );

            // Fan sensors
            discover_type(
                &hwmon_dir,
                &chip_name,
                "fan",
                SensorCategory::Fan,
                SensorUnit::Rpm,
                1.0,
                label_overrides,
                &mut entries,
            );

            // Voltage sensors
            discover_type(
                &hwmon_dir,
                &chip_name,
                "in",
                SensorCategory::Voltage,
                SensorUnit::Volts,
                1000.0,
                label_overrides,
                &mut entries,
            );

            // Power sensors
            discover_power(&hwmon_dir, &chip_name, label_overrides, &mut entries);

            // Current sensors
            discover_type(
                &hwmon_dir,
                &chip_name,
                "curr",
                SensorCategory::Current,
                SensorUnit::Amps,
                1000.0,
                label_overrides,
                &mut entries,
            );

            if !entries.is_empty() {
                chips.push(ChipSensors { entries });
            }
        }

        Self { chips }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for chip in &mut self.chips {
            for entry in &mut chip.entries {
                if let Some(raw) = entry.input_file.read_u64() {
                    // Some sensors report 0 when disconnected/absent
                    let value = raw as f64 / entry.divisor;
                    let reading =
                        SensorReading::new(entry.label.clone(), value, entry.unit, entry.category);
                    readings.push((entry.id.clone(), reading));
                }
            }
        }

        readings
    }

    pub fn chip_count(&self) -> usize {
        self.chips.len()
    }

    pub fn sensor_count(&self) -> usize {
        self.chips.iter().map(|c| c.entries.len()).sum()
    }
}

#[allow(clippy::too_many_arguments)]
fn discover_type(
    hwmon_dir: &Path,
    chip_name: &str,
    prefix: &str,
    category: SensorCategory,
    unit: SensorUnit,
    divisor: f64,
    label_overrides: &HashMap<String, String>,
    entries: &mut Vec<SensorEntry>,
) {
    let pattern = format!("{}/{prefix}*_input", hwmon_dir.display());
    for input_path in sysfs::glob_paths(&pattern) {
        let filename = match input_path.file_name().and_then(|n| n.to_str()) {
            Some(f) => f,
            None => continue,
        };

        // Extract index: "temp1_input" -> "1"
        let idx_str = &filename[prefix.len()..filename.len() - "_input".len()];
        let idx: u32 = match idx_str.parse() {
            Ok(i) => i,
            Err(_) => continue,
        };

        let sensor_name = format!("{prefix}{idx}");
        let id = SensorId {
            source: "hwmon".into(),
            chip: chip_name.into(),
            sensor: sensor_name,
        };

        // Check label overrides first, then fall back to sysfs label file.
        // GPU hwmon labels are auto-prefixed with "GPU " to avoid ambiguity.
        let label = if let Some(override_label) = label_overrides.get(&id.to_string()) {
            override_label.clone()
        } else {
            let raw = {
                let label_path = hwmon_dir.join(format!("{prefix}{idx}_label"));
                sysfs::read_string_optional(&label_path).unwrap_or_else(|| format!("{prefix}{idx}"))
            };
            if is_gpu_hwmon_chip(chip_name) {
                gpu_prefix_label(raw)
            } else {
                raw
            }
        };

        let Some(input_file) = CachedFile::open(&input_path) else {
            continue;
        };

        entries.push(SensorEntry {
            id,
            label,
            input_file,
            category,
            unit,
            divisor,
        });
    }
}

impl crate::sensors::SensorSource for HwmonSource {
    fn name(&self) -> &str {
        "hwmon"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        HwmonSource::poll(self)
    }
}

fn discover_power(
    hwmon_dir: &Path,
    chip_name: &str,
    label_overrides: &HashMap<String, String>,
    entries: &mut Vec<SensorEntry>,
) {
    // Power can be power*_input or power*_average
    for suffix in &["_input", "_average"] {
        let pattern = format!("{}/power*{suffix}", hwmon_dir.display());
        for path in sysfs::glob_paths(&pattern) {
            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(f) => f,
                None => continue,
            };

            let idx_str = &filename["power".len()..filename.len() - suffix.len()];
            let idx: u32 = match idx_str.parse() {
                Ok(i) => i,
                Err(_) => continue,
            };

            // Skip if we already have this index from _input
            let sensor_name = format!("power{idx}");
            if entries
                .iter()
                .any(|e| e.id.chip == chip_name && e.id.sensor == sensor_name)
            {
                continue;
            }

            let id = SensorId {
                source: "hwmon".into(),
                chip: chip_name.into(),
                sensor: sensor_name,
            };

            // Check label overrides first, then fall back to sysfs label file.
            // GPU hwmon labels are auto-prefixed with "GPU " to avoid ambiguity.
            let label = if let Some(override_label) = label_overrides.get(&id.to_string()) {
                override_label.clone()
            } else {
                let raw = {
                    let label_path = hwmon_dir.join(format!("power{idx}_label"));
                    sysfs::read_string_optional(&label_path).unwrap_or_else(|| id.sensor.clone())
                };
                if is_gpu_hwmon_chip(chip_name) {
                    gpu_prefix_label(raw)
                } else {
                    raw
                }
            };

            let Some(input_file) = CachedFile::open(&path) else {
                continue;
            };

            entries.push(SensorEntry {
                id,
                label,
                input_file,
                category: SensorCategory::Power,
                unit: SensorUnit::Watts,
                divisor: 1_000_000.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_gpu_hwmon_chip() {
        assert!(is_gpu_hwmon_chip("amdgpu"));
        assert!(is_gpu_hwmon_chip("nouveau"));
        assert!(is_gpu_hwmon_chip("i915"));
        assert!(is_gpu_hwmon_chip("xe"));
        assert!(!is_gpu_hwmon_chip("nct6798"));
        assert!(!is_gpu_hwmon_chip("coretemp"));
        assert!(!is_gpu_hwmon_chip("k10temp"));
    }

    #[test]
    fn test_gpu_prefix_label() {
        assert_eq!(gpu_prefix_label("PPT".into()), "GPU PPT");
        assert_eq!(gpu_prefix_label("edge".into()), "GPU edge");
        assert_eq!(gpu_prefix_label("power1".into()), "GPU power1");
        // Already prefixed — should not double-prefix
        assert_eq!(
            gpu_prefix_label("GPU Temperature".into()),
            "GPU Temperature"
        );
    }
}
