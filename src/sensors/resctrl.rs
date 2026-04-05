//! LLC occupancy and memory bandwidth monitoring via Linux resctrl.
//!
//! Reads from `/sys/fs/resctrl/mon_data/mon_L3_<domain>/` to provide:
//! - `llc_occupancy` — L3 cache occupancy in bytes
//! - `mbm_total_bytes` — total memory bandwidth (derived as rate from deltas)
//! - `mbm_local_bytes` — local NUMA bandwidth (derived as rate)
//!
//! Requires `CONFIG_X86_CPU_RESCTRL=y` and resctrl mounted:
//! `mount -t resctrl resctrl /sys/fs/resctrl`

use std::path::PathBuf;
use std::time::Instant;

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

/// A single L3 monitoring domain.
struct L3Domain {
    id: u32,
    path: PathBuf,
    prev_mbm_total: Option<u64>,
    prev_mbm_local: Option<u64>,
    prev_time: Instant,
}

pub struct ResctrlSource {
    domains: Vec<L3Domain>,
}

impl ResctrlSource {
    pub fn discover() -> Self {
        let base = PathBuf::from("/sys/fs/resctrl/mon_data");
        if !base.exists() {
            log::debug!("resctrl: /sys/fs/resctrl/mon_data not found");
            return Self {
                domains: Vec::new(),
            };
        }

        let mut domains = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(id_str) = name.strip_prefix("mon_L3_") {
                    if let Ok(id) = id_str.parse::<u32>() {
                        domains.push(L3Domain {
                            id,
                            path: entry.path(),
                            prev_mbm_total: None,
                            prev_mbm_local: None,
                            prev_time: Instant::now(),
                        });
                    }
                }
            }
        }

        domains.sort_by_key(|d| d.id);

        if !domains.is_empty() {
            log::info!("resctrl: found {} L3 monitoring domains", domains.len());
        }

        Self { domains }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();
        let now = Instant::now();

        for domain in &mut self.domains {
            let chip = format!("L3_{}", domain.id);

            // LLC occupancy (bytes)
            if let Some(occ) = read_u64_file(&domain.path.join("llc_occupancy")) {
                let occ_mb = occ as f64 / (1024.0 * 1024.0);
                readings.push((
                    SensorId {
                        source: "resctrl".into(),
                        chip: chip.clone(),
                        sensor: "llc_occupancy".into(),
                    },
                    SensorReading::new(
                        format!("L3_{} LLC Occupancy", domain.id),
                        occ_mb,
                        SensorUnit::Megabytes,
                        SensorCategory::Memory,
                    ),
                ));
            }

            // MBM total bandwidth (bytes → MB/s rate)
            if let Some(total) = read_u64_file(&domain.path.join("mbm_total_bytes")) {
                let elapsed = now.duration_since(domain.prev_time).as_secs_f64();
                if let Some(prev) = domain.prev_mbm_total {
                    if elapsed > 0.01 {
                        let delta = total.wrapping_sub(prev);
                        let mbps = delta as f64 / (1024.0 * 1024.0) / elapsed;
                        readings.push((
                            SensorId {
                                source: "resctrl".into(),
                                chip: chip.clone(),
                                sensor: "mbm_total".into(),
                            },
                            SensorReading::new(
                                format!("L3_{} MBM Total", domain.id),
                                mbps,
                                SensorUnit::MegabytesPerSec,
                                SensorCategory::Throughput,
                            ),
                        ));
                    }
                }
                domain.prev_mbm_total = Some(total);
            }

            // MBM local bandwidth
            if let Some(local) = read_u64_file(&domain.path.join("mbm_local_bytes")) {
                let elapsed = now.duration_since(domain.prev_time).as_secs_f64();
                if let Some(prev) = domain.prev_mbm_local {
                    if elapsed > 0.01 {
                        let delta = local.wrapping_sub(prev);
                        let mbps = delta as f64 / (1024.0 * 1024.0) / elapsed;
                        readings.push((
                            SensorId {
                                source: "resctrl".into(),
                                chip: chip.clone(),
                                sensor: "mbm_local".into(),
                            },
                            SensorReading::new(
                                format!("L3_{} MBM Local", domain.id),
                                mbps,
                                SensorUnit::MegabytesPerSec,
                                SensorCategory::Throughput,
                            ),
                        ));
                    }
                }
                domain.prev_mbm_local = Some(local);
            }

            domain.prev_time = now;
        }

        readings
    }
}

impl crate::sensors::SensorSource for ResctrlSource {
    fn name(&self) -> &str {
        "resctrl"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        ResctrlSource::poll(self)
    }
}

fn read_u64_file(path: &std::path::Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}
