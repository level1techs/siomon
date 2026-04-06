//! PCIe AER (Advanced Error Reporting) sensor source.
//!
//! Polls per-device AER error counters from sysfs so they appear in the
//! sensor poller with min/max/avg tracking and TUI visibility.

use std::path::PathBuf;

use crate::collectors::pci::parse_aer_total;
use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs;

pub struct AerSource {
    devices: Vec<AerDevice>,
}

struct AerDevice {
    correctable_path: PathBuf,
    nonfatal_path: PathBuf,
    fatal_path: PathBuf,
    cor_id: SensorId,
    nf_id: SensorId,
    fat_id: SensorId,
    cor_label: String,
    nf_label: String,
    fat_label: String,
}

impl AerSource {
    pub fn discover() -> Self {
        let mut devices = Vec::new();

        for corr_path in sysfs::glob_paths("/sys/bus/pci/devices/*/aer_dev_correctable") {
            let dev_dir = match corr_path.parent() {
                Some(p) => p,
                None => continue,
            };
            let bdf = match dev_dir.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };

            let nonfatal_path = dev_dir.join("aer_dev_nonfatal");
            let fatal_path = dev_dir.join("aer_dev_fatal");
            if !nonfatal_path.exists() || !fatal_path.exists() {
                continue;
            }

            let device_label = resolve_pci_label(dev_dir, &bdf);

            devices.push(AerDevice {
                correctable_path: corr_path,
                nonfatal_path,
                fatal_path,
                cor_id: SensorId {
                    source: "aer".into(),
                    chip: bdf.clone(),
                    sensor: "correctable".into(),
                },
                nf_id: SensorId {
                    source: "aer".into(),
                    chip: bdf.clone(),
                    sensor: "nonfatal".into(),
                },
                fat_id: SensorId {
                    source: "aer".into(),
                    chip: bdf.clone(),
                    sensor: "fatal".into(),
                },
                cor_label: format!("{device_label} Correctable"),
                nf_label: format!("{device_label} Nonfatal"),
                fat_label: format!("{device_label} Fatal"),
            });
        }

        Self { devices }
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

impl super::SensorSource for AerSource {
    fn name(&self) -> &str {
        "aer"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::with_capacity(self.devices.len() * 3);

        for dev in &self.devices {
            if let Some(total) = parse_aer_total(&dev.correctable_path) {
                readings.push((
                    dev.cor_id.clone(),
                    SensorReading::new(
                        dev.cor_label.clone(),
                        total as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Other,
                    ),
                ));
            }
            if let Some(total) = parse_aer_total(&dev.nonfatal_path) {
                readings.push((
                    dev.nf_id.clone(),
                    SensorReading::new(
                        dev.nf_label.clone(),
                        total as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Other,
                    ),
                ));
            }
            if let Some(total) = parse_aer_total(&dev.fatal_path) {
                readings.push((
                    dev.fat_id.clone(),
                    SensorReading::new(
                        dev.fat_label.clone(),
                        total as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Other,
                    ),
                ));
            }
        }

        readings
    }
}

fn resolve_pci_label(dev_dir: &std::path::Path, bdf: &str) -> String {
    let vid = sysfs::read_u64_optional(&dev_dir.join("vendor")).map(|v| v as u16);
    let did = sysfs::read_u64_optional(&dev_dir.join("device")).map(|v| v as u16);

    if let (Some(vid), Some(did)) = (vid, did)
        && let Some(dev) = pci_ids::Device::from_vid_pid(vid, did)
    {
        let name = dev.name();
        let short = if name.len() > 40 {
            match name.char_indices().nth(40) {
                Some((idx, _)) => &name[..idx],
                None => name,
            }
        } else {
            name
        };
        return format!("{short} [{bdf}]");
    }
    bdf.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SensorSource;

    #[test]
    fn test_aer_source_empty() {
        let src = AerSource {
            devices: Vec::new(),
        };
        assert_eq!(src.device_count(), 0);
        assert_eq!(src.name(), "aer");
    }

    #[test]
    fn test_aer_sensor_id_format() {
        let id = SensorId {
            source: "aer".into(),
            chip: "0000:11:00.0".into(),
            sensor: "correctable".into(),
        };
        assert_eq!(id.to_string(), "aer/0000:11:00.0/correctable");
    }
}
