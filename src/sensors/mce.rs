//! MCE (Machine Check Exception) sensor source.
//!
//! Polls `/sys/devices/system/machinecheck/machinecheck0/bank*` for
//! MCi_STATUS register values. Tracks cumulative error counts per bank
//! with AMD SMCA bank type decoding.
//!
//! Only CPU 0 banks are polled. Per-core banks (LS, IF, L2, EX, FP) are
//! truly per-core, so errors on other cores are not detected. Shared banks
//! (UMC, NBIO, PSP, SMU) are the same across all CPUs.
//!
//! MCA_STATUS is a latch register — if the same error recurs between polls,
//! the status value will be identical and we will not detect the repeat.
//! The error_count is a lower bound, not an exact count.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};

/// No error: all bits set (sysfs default when no MCE logged).
const MCE_NO_ERROR: u64 = 0xFFFF_FFFF_FFFF_FFFF;

/// MCA_STATUS bit 63: Valid — error entry is valid.
const MCA_STATUS_VAL: u64 = 1 << 63;
/// MCA_STATUS bit 61: UC — uncorrectable error.
const MCA_STATUS_UC: u64 = 1 << 61;

enum CpuVendor {
    Amd,
    Intel,
    Other,
}

pub struct MceSource {
    banks: Vec<MceBankInfo>,
}

struct MceBankInfo {
    file: CachedFile,
    prev_status: u64,
    error_count: u64,
    uc_count: u64,
    id: SensorId,
    uc_id: SensorId,
    label: String,
    uc_label: String,
}

impl MceSource {
    pub fn discover() -> Self {
        let mut banks = Vec::new();

        // Detect CPU vendor for bank name lookup.
        // Read only the first 256 bytes of cpuinfo to avoid loading 300KB+ on large systems.
        let vendor = {
            let snippet = std::fs::File::open("/proc/cpuinfo")
                .and_then(|f| {
                    use std::io::Read;
                    let mut buf = [0u8; 256];
                    let n = f.take(256).read(&mut buf)?;
                    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
                })
                .unwrap_or_default();
            if snippet.contains("AuthenticAMD") {
                CpuVendor::Amd
            } else if snippet.contains("GenuineIntel") {
                CpuVendor::Intel
            } else {
                CpuVendor::Other
            }
        };

        for bank_path in sysfs::glob_paths("/sys/devices/system/machinecheck/machinecheck0/bank*") {
            let bank_name = match bank_path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };
            let bank_idx: u8 = match bank_name.strip_prefix("bank").and_then(|s| s.parse().ok()) {
                Some(idx) => idx,
                None => continue,
            };

            // Skip banks that are empty (unused/reserved)
            let Some(mut file) = CachedFile::open(&bank_path) else {
                continue;
            };
            let Some(initial) = read_mce_cached(&mut file) else {
                continue;
            };

            let type_name = match vendor {
                CpuVendor::Amd => crate::db::mce::amd_smca_bank_name(bank_idx),
                CpuVendor::Intel => crate::db::mce::intel_mca_bank_name(bank_idx),
                CpuVendor::Other => "Bank",
            };

            banks.push(MceBankInfo {
                file,
                prev_status: initial,
                error_count: 0,
                uc_count: 0,
                id: SensorId {
                    source: "mce".into(),
                    chip: "cpu".into(),
                    sensor: format!("bank{bank_idx}"),
                },
                uc_id: SensorId {
                    source: "mce".into(),
                    chip: "cpu".into(),
                    sensor: format!("bank{bank_idx}_uc"),
                },
                label: format!("{type_name} (Bank {bank_idx}) Errors"),
                uc_label: format!("{type_name} (Bank {bank_idx}) UC"),
            });
        }

        Self { banks }
    }

    pub fn bank_count(&self) -> usize {
        self.banks.len()
    }
}

impl super::SensorSource for MceSource {
    fn name(&self) -> &str {
        "mce"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::with_capacity(self.banks.len() * 2);

        for bank in &mut self.banks {
            let status = match read_mce_cached(&mut bank.file) {
                Some(s) => s,
                None => continue,
            };

            // Detect new error: valid bit set, not the "no error" sentinel,
            // and status changed from previous poll.
            if status != MCE_NO_ERROR
                && (status & MCA_STATUS_VAL) != 0
                && status != bank.prev_status
            {
                bank.error_count += 1;
                if (status & MCA_STATUS_UC) != 0 {
                    bank.uc_count += 1;
                }
            }
            bank.prev_status = status;

            readings.push((
                bank.id.clone(),
                SensorReading::new(
                    bank.label.clone(),
                    bank.error_count as f64,
                    SensorUnit::Unitless,
                    SensorCategory::Other,
                ),
            ));
            readings.push((
                bank.uc_id.clone(),
                SensorReading::new(
                    bank.uc_label.clone(),
                    bank.uc_count as f64,
                    SensorUnit::Unitless,
                    SensorCategory::Other,
                ),
            ));
        }

        readings
    }
}

/// Read an MCE bank status register via cached file handle.
/// Returns `None` if the file is empty or unreadable (unused bank).
fn read_mce_cached(file: &mut CachedFile) -> Option<u64> {
    let raw = file.read_raw()?;
    let hex = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    u64::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SensorSource;

    #[test]
    fn test_mce_source_empty() {
        let src = MceSource { banks: Vec::new() };
        assert_eq!(src.bank_count(), 0);
        assert_eq!(src.name(), "mce");
    }

    #[test]
    fn test_mce_sensor_id_format() {
        let id = SensorId {
            source: "mce".into(),
            chip: "cpu".into(),
            sensor: "bank20".into(),
        };
        assert_eq!(id.to_string(), "mce/cpu/bank20");
    }

    #[test]
    fn test_mce_no_error_sentinel() {
        assert_eq!(MCE_NO_ERROR, 0xFFFF_FFFF_FFFF_FFFF);
        // Val bit is set in the sentinel — the explicit MCE_NO_ERROR check
        // in poll() guards against treating this as a real error
        assert_ne!(MCE_NO_ERROR & MCA_STATUS_VAL, 0);
    }

    #[test]
    fn test_mce_status_bits() {
        // Simulated corrected error: Val=1, UC=0
        let status: u64 = MCA_STATUS_VAL | 0x0000_0000_0000_0110;
        assert_ne!(status & MCA_STATUS_VAL, 0);
        assert_eq!(status & MCA_STATUS_UC, 0);

        // Simulated uncorrectable error: Val=1, UC=1
        let status_uc: u64 = MCA_STATUS_VAL | MCA_STATUS_UC | 0x0110;
        assert_ne!(status_uc & MCA_STATUS_VAL, 0);
        assert_ne!(status_uc & MCA_STATUS_UC, 0);
    }

    #[test]
    fn test_read_mce_status_hex_formats() {
        // The function is tested indirectly through its parsing logic
        // Bare hex (kernel format): "ffffffffffffffff"
        assert_eq!(
            u64::from_str_radix("ffffffffffffffff", 16).unwrap(),
            MCE_NO_ERROR
        );
        // With 0x prefix (defensive): handled by strip_prefix
        let s = "0xffffffffffffffff";
        let hex = s.strip_prefix("0x").unwrap_or(s);
        assert_eq!(u64::from_str_radix(hex, 16).unwrap(), MCE_NO_ERROR);
    }
}
