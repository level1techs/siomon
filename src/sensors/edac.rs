//! EDAC (Error Detection and Correction) sensor source.
//!
//! Polls `/sys/devices/system/edac/mc/` for per-rank correctable and
//! uncorrectable memory error counters. Uses board template DIMM labels
//! to map EDAC rank IDs to physical slot names.

use crate::db::boards;
use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};

/// EDAC memory error counter source.
pub struct EdacSource {
    ranks: Vec<EdacRank>,
    /// Top-level MC summary counter paths.
    mc_totals: Vec<McTotal>,
}

struct EdacRank {
    ce_file: CachedFile,
    ue_file: CachedFile,
    ce_id: SensorId,
    ue_id: SensorId,
    ce_label: String,
    ue_label: String,
}

struct McTotal {
    ce_file: CachedFile,
    ue_file: CachedFile,
    ce_id: SensorId,
    ue_id: SensorId,
    ce_label: String,
    ue_label: String,
}

impl EdacSource {
    pub fn discover() -> Self {
        let board_name = crate::db::sensor_labels::read_board_name();
        let board = board_name.as_deref().and_then(boards::lookup_board);

        let mut ranks = Vec::new();
        let mut mc_totals = Vec::new();

        for mc_dir in sysfs::glob_paths("/sys/devices/system/edac/mc/mc*") {
            let mc_name = match mc_dir.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };
            let mc_idx: u8 = match mc_name.strip_prefix("mc").and_then(|s| s.parse().ok()) {
                Some(idx) => idx,
                None => continue,
            };

            let chip = format!("mc{mc_idx}");

            // Top-level MC summary counters
            let mc_ce = mc_dir.join("ce_count");
            let mc_ue = mc_dir.join("ue_count");
            if let (Some(ce_file), Some(ue_file)) =
                (CachedFile::open(&mc_ce), CachedFile::open(&mc_ue))
            {
                mc_totals.push(McTotal {
                    ce_file,
                    ue_file,
                    ce_id: SensorId {
                        source: "edac".into(),
                        chip: chip.clone(),
                        sensor: "total_ce".into(),
                    },
                    ue_id: SensorId {
                        source: "edac".into(),
                        chip: chip.clone(),
                        sensor: "total_ue".into(),
                    },
                    ce_label: format!("MC{mc_idx} Total CE"),
                    ue_label: format!("MC{mc_idx} Total UE"),
                });
            }

            // Per-rank counters
            for rank_dir in sysfs::glob_paths(&format!("{}/rank*", mc_dir.display())) {
                let rank_name = match rank_dir.file_name() {
                    Some(n) => n.to_string_lossy().to_string(),
                    None => continue,
                };
                let rank_idx: u16 =
                    match rank_name.strip_prefix("rank").and_then(|s| s.parse().ok()) {
                        Some(idx) => idx,
                        None => continue,
                    };

                let ce_path = rank_dir.join("dimm_ce_count");
                let ue_path = rank_dir.join("dimm_ue_count");

                let Some(ce_file) = CachedFile::open(&ce_path) else {
                    continue;
                };
                let Some(ue_file) = CachedFile::open(&ue_path) else {
                    continue;
                };

                // Resolve label: board template → sysfs dimm_label → generic
                let slot_label = board
                    .and_then(|b| {
                        b.dimm_labels
                            .iter()
                            .find(|d| d.mc == mc_idx && d.rank == rank_idx)
                            .map(|d| d.label.to_string())
                    })
                    .or_else(|| sysfs::read_string_optional(&rank_dir.join("dimm_label")))
                    .unwrap_or_else(|| format!("mc{mc_idx} rank{rank_idx}"));

                ranks.push(EdacRank {
                    ce_file,
                    ue_file,
                    ce_id: SensorId {
                        source: "edac".into(),
                        chip: chip.clone(),
                        sensor: format!("rank{rank_idx}_ce"),
                    },
                    ue_id: SensorId {
                        source: "edac".into(),
                        chip: chip.clone(),
                        sensor: format!("rank{rank_idx}_ue"),
                    },
                    ce_label: format!("{slot_label} CE"),
                    ue_label: format!("{slot_label} UE"),
                });
            }
        }

        Self { ranks, mc_totals }
    }

    pub fn rank_count(&self) -> usize {
        self.ranks.len()
    }
}

impl super::SensorSource for EdacSource {
    fn name(&self) -> &str {
        "edac"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        // Per-rank counters
        for rank in &mut self.ranks {
            if let Some(ce) = rank.ce_file.read_u64() {
                readings.push((
                    rank.ce_id.clone(),
                    SensorReading::new(
                        rank.ce_label.clone(),
                        ce as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Memory,
                    ),
                ));
            }
            if let Some(ue) = rank.ue_file.read_u64() {
                readings.push((
                    rank.ue_id.clone(),
                    SensorReading::new(
                        rank.ue_label.clone(),
                        ue as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Memory,
                    ),
                ));
            }
        }

        // MC-level totals
        for mc in &mut self.mc_totals {
            if let Some(ce) = mc.ce_file.read_u64() {
                readings.push((
                    mc.ce_id.clone(),
                    SensorReading::new(
                        mc.ce_label.clone(),
                        ce as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Memory,
                    ),
                ));
            }
            if let Some(ue) = mc.ue_file.read_u64() {
                readings.push((
                    mc.ue_id.clone(),
                    SensorReading::new(
                        mc.ue_label.clone(),
                        ue as f64,
                        SensorUnit::Unitless,
                        SensorCategory::Memory,
                    ),
                ));
            }
        }

        readings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SensorSource;

    #[test]
    fn test_edac_source_empty_when_no_edac() {
        let src = EdacSource {
            ranks: Vec::new(),
            mc_totals: Vec::new(),
        };
        assert_eq!(src.rank_count(), 0);
        assert_eq!(src.name(), "edac");
    }

    #[test]
    fn test_edac_sensor_id_format() {
        let id = SensorId {
            source: "edac".into(),
            chip: "mc0".into(),
            sensor: "rank5_ce".into(),
        };
        assert_eq!(id.to_string(), "edac/mc0/rank5_ce");
    }
}
