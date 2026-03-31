use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};
use std::collections::HashMap;
use std::path::Path;

/// Hwmon chip names belonging to GPU drivers. Labels from these chips are
/// auto-prefixed with "GPU " to avoid ambiguity with CPU-side names (e.g.
/// amdgpu's "PPT" label vs CPU Package Power Tracking).
const GPU_HWMON_CHIPS: &[&str] = &["amdgpu", "nouveau", "i915", "xe"];

fn is_gpu_hwmon_chip(chip_name: &str) -> bool {
    GPU_HWMON_CHIPS.iter().any(|&gpu| {
        chip_name == gpu
            || chip_name
                .strip_prefix(gpu)
                .is_some_and(|rest| rest.starts_with('-'))
    })
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
    /// External voltage scaling multiplier (default 1.0). Applied after
    /// dividing by `divisor` to correct for board-level resistor dividers.
    multiplier: f64,
}

impl HwmonSource {
    pub fn discover(
        label_overrides: &HashMap<String, String>,
        voltage_scaling: &HashMap<String, f64>,
    ) -> Self {
        let mut chips = Vec::new();

        // First pass: collect hwmon dirs with their chip names to detect duplicates
        let hwmon_dirs: Vec<_> = sysfs::glob_paths("/sys/class/hwmon/hwmon*")
            .into_iter()
            .map(|dir| {
                let chip_name = sysfs::read_string_optional(&dir.join("name"))
                    .unwrap_or_else(|| "unknown".into());
                (dir, chip_name)
            })
            .collect();

        // Count occurrences of each chip name
        let mut name_counts: HashMap<String, usize> = HashMap::new();
        for (_, name) in &hwmon_dirs {
            *name_counts.entry(name.clone()).or_default() += 1;
        }

        // Compute display names and expand label overrides for disambiguated chips.
        // Board templates use unqualified names like "hwmon/jc42/temp1"; when a chip
        // is disambiguated to "jc42-9-0018", we copy matching overrides so they
        // still apply without changing discover_type/discover_power signatures.
        let hwmon_entries: Vec<_> = hwmon_dirs
            .into_iter()
            .map(|(dir, chip_name)| {
                let display_name = if name_counts[&chip_name] > 1 {
                    let suffix = sysfs::read_link_basename(&dir.join("device"))
                        .or_else(|| {
                            // Last resort: use hwmon sysfs index (unstable across
                            // reboots, but avoids collisions within a session)
                            dir.file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| "unknown".into());
                    format!("{chip_name}-{suffix}")
                } else {
                    chip_name.clone()
                };
                (dir, chip_name, display_name)
            })
            .collect();

        let effective_overrides = expand_overrides(label_overrides, &hwmon_entries);
        let effective_scaling = expand_overrides(voltage_scaling, &hwmon_entries);

        for (hwmon_dir, _, display_name) in &hwmon_entries {
            let mut entries = Vec::new();

            // Temperature sensors
            discover_type(
                hwmon_dir,
                display_name,
                "temp",
                SensorCategory::Temperature,
                SensorUnit::Celsius,
                1000.0,
                &effective_overrides,
                &mut entries,
            );

            // Fan sensors
            discover_type(
                hwmon_dir,
                display_name,
                "fan",
                SensorCategory::Fan,
                SensorUnit::Rpm,
                1.0,
                &effective_overrides,
                &mut entries,
            );

            // Voltage sensors
            discover_type(
                hwmon_dir,
                display_name,
                "in",
                SensorCategory::Voltage,
                SensorUnit::Volts,
                1000.0,
                &effective_overrides,
                &mut entries,
            );

            // Power sensors
            discover_power(hwmon_dir, display_name, &effective_overrides, &mut entries);

            // Current sensors
            discover_type(
                hwmon_dir,
                display_name,
                "curr",
                SensorCategory::Current,
                SensorUnit::Amps,
                1000.0,
                &effective_overrides,
                &mut entries,
            );

            // Apply voltage scaling multipliers from board template
            for entry in &mut entries {
                if entry.category == SensorCategory::Voltage {
                    if let Some(&mult) = effective_scaling.get(&entry.id.to_string()) {
                        entry.multiplier = mult;
                    }
                }
            }

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
                    let value = raw as f64 / entry.divisor * entry.multiplier;
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

/// Expand a sensor key → value map for disambiguated chip names. Board
/// templates use unqualified names like `hwmon/jc42/temp1`; when a chip is
/// disambiguated to `jc42-9-0018`, copy matching entries so they still apply.
/// Qualified entries (if any) take precedence via `or_insert`.
fn expand_overrides<V: Clone>(
    base: &HashMap<String, V>,
    entries: &[(std::path::PathBuf, String, String)],
) -> HashMap<String, V> {
    let mut expanded = base.clone();
    for (_, chip_name, display_name) in entries {
        if chip_name != display_name {
            let prefix = format!("hwmon/{chip_name}/");
            for (key, value) in base {
                if let Some(sensor) = key.strip_prefix(&prefix) {
                    expanded
                        .entry(format!("hwmon/{display_name}/{sensor}"))
                        .or_insert_with(|| value.clone());
                }
            }
        }
    }
    expanded
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
            multiplier: 1.0,
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
                multiplier: 1.0,
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
        // Disambiguated multi-GPU names must still match
        assert!(is_gpu_hwmon_chip("amdgpu-0000:41:00.0"));
        assert!(is_gpu_hwmon_chip("nouveau-0000:01:00.0"));
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

    #[test]
    fn test_expand_overrides_no_duplicates() {
        let base: HashMap<String, String> = [("hwmon/nct6798/temp1".into(), "SYSTIN".into())]
            .into_iter()
            .collect();
        // Unique chip name — display_name == chip_name, no expansion
        let entries = vec![(
            std::path::PathBuf::from("/sys/class/hwmon/hwmon0"),
            "nct6798".into(),
            "nct6798".into(),
        )];
        let result = expand_overrides(&base, &entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result["hwmon/nct6798/temp1"], "SYSTIN");
    }

    #[test]
    fn test_expand_overrides_with_duplicates() {
        let base: HashMap<String, String> = [("hwmon/jc42/temp1".into(), "DIMM Temp".into())]
            .into_iter()
            .collect();
        let entries = vec![
            (
                std::path::PathBuf::from("/sys/class/hwmon/hwmon0"),
                "jc42".into(),
                "jc42-9-0018".into(),
            ),
            (
                std::path::PathBuf::from("/sys/class/hwmon/hwmon1"),
                "jc42".into(),
                "jc42-9-0019".into(),
            ),
        ];
        let result = expand_overrides(&base, &entries);
        assert_eq!(result.len(), 3);
        assert_eq!(result["hwmon/jc42/temp1"], "DIMM Temp");
        assert_eq!(result["hwmon/jc42-9-0018/temp1"], "DIMM Temp");
        assert_eq!(result["hwmon/jc42-9-0019/temp1"], "DIMM Temp");
    }

    #[test]
    fn test_expand_overrides_qualified_takes_precedence() {
        let base: HashMap<String, String> = [
            ("hwmon/jc42/temp1".into(), "DIMM Temp".into()),
            ("hwmon/jc42-9-0018/temp1".into(), "DIMM A1".into()),
        ]
        .into_iter()
        .collect();
        let entries = vec![(
            std::path::PathBuf::from("/sys/class/hwmon/hwmon0"),
            "jc42".into(),
            "jc42-9-0018".into(),
        )];
        let result = expand_overrides(&base, &entries);
        // Qualified override takes precedence over expanded unqualified
        assert_eq!(result["hwmon/jc42-9-0018/temp1"], "DIMM A1");
    }
}
