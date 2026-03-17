#[cfg(unix)]
use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
pub struct DiskActivitySource {
    prev_stats: HashMap<String, DiskStat>,
    prev_time: Instant,
}

#[cfg(unix)]
#[derive(Clone)]
struct DiskStat {
    read_sectors: u64,
    write_sectors: u64,
}

#[cfg(unix)]
impl DiskActivitySource {
    pub fn discover() -> Self {
        let prev_stats = parse_diskstats();
        Self {
            prev_stats,
            prev_time: Instant::now(),
        }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let current = parse_diskstats();
        let now = Instant::now();
        let elapsed_secs = now.duration_since(self.prev_time).as_secs_f64();
        let mut readings = Vec::new();

        if elapsed_secs <= 0.0 {
            self.prev_stats = current;
            self.prev_time = now;
            return readings;
        }

        for (device, cur) in &current {
            let Some(prev) = self.prev_stats.get(device) else {
                continue;
            };

            let read_delta = cur.read_sectors.saturating_sub(prev.read_sectors);
            let write_delta = cur.write_sectors.saturating_sub(prev.write_sectors);

            // Each sector is 512 bytes; convert to MB/s
            let read_mbps = (read_delta as f64 * 512.0) / (1_048_576.0 * elapsed_secs);
            let write_mbps = (write_delta as f64 * 512.0) / (1_048_576.0 * elapsed_secs);

            let read_id = SensorId {
                source: "disk".into(),
                chip: device.clone(),
                sensor: "read_mbps".into(),
            };
            let read_label = format!("{device} Read");
            readings.push((
                read_id,
                SensorReading::new(
                    read_label,
                    read_mbps,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));

            let write_id = SensorId {
                source: "disk".into(),
                chip: device.clone(),
                sensor: "write_mbps".into(),
            };
            let write_label = format!("{device} Write");
            readings.push((
                write_id,
                SensorReading::new(
                    write_label,
                    write_mbps,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));
        }

        self.prev_stats = current;
        self.prev_time = now;
        readings
    }
}

#[cfg(unix)]
impl crate::sensors::SensorSource for DiskActivitySource {
    fn name(&self) -> &str {
        "disk"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        DiskActivitySource::poll(self)
    }
}

#[cfg(unix)]
fn parse_diskstats() -> HashMap<String, DiskStat> {
    let mut stats = HashMap::new();
    let Ok(content) = fs::read_to_string("/proc/diskstats") else {
        return stats;
    };

    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // /proc/diskstats has at least 14 fields:
        // major minor name reads_completed _ sectors_read _ _ writes_completed _ sectors_written ...
        if fields.len() < 14 {
            continue;
        }

        let device = fields[2];

        // Filter to real block devices: skip partitions (e.g. sda1), loop, ram, dm devices
        if !is_real_block_device(device) {
            continue;
        }

        let read_sectors: u64 = match fields[5].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let write_sectors: u64 = match fields[9].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        stats.insert(
            device.to_string(),
            DiskStat {
                read_sectors,
                write_sectors,
            },
        );
    }

    stats
}

#[cfg(unix)]
fn is_real_block_device(name: &str) -> bool {
    // Skip loop, ram, and dm- devices
    if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("dm-") {
        return false;
    }

    // Skip partitions: sd*N, nvme*pN, vd*N, hd*N
    if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") {
        // "sda" is valid, "sda1" is a partition
        let suffix = &name[3..];
        if suffix.chars().any(|c| c.is_ascii_digit()) && name.len() > 3 {
            // Check if trailing portion is all digits (partition number)
            let alpha_part: String = name
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let num_part = &name[alpha_part.len()..];
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        }
    }

    if name.starts_with("nvme") {
        // "nvme0n1" is valid, "nvme0n1p1" is a partition
        if name.contains('p') {
            let last_p = name.rfind('p').unwrap();
            let after_p = &name[last_p + 1..];
            // If everything after the last 'p' is digits, it's a partition
            if !after_p.is_empty() && after_p.chars().all(|c| c.is_ascii_digit()) {
                // But make sure there's an 'n' before the 'p' (nvme0n1p1 pattern)
                let before_p = &name[..last_p];
                if before_p.contains('n') {
                    return false;
                }
            }
        }
    }

    // Verify device exists in /sys/block/
    Path::new(&format!("/sys/block/{name}")).exists()
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(not(unix))]
pub struct DiskActivitySource {
    disks: sysinfo::Disks,
    prev: std::collections::HashMap<String, (u64, u64)>,
    last_time: std::time::Instant,
}

#[cfg(not(unix))]
impl DiskActivitySource {
    pub fn discover() -> Self {
        use sysinfo::Disks;
        let disks = Disks::new_with_refreshed_list();
        // Initialize prev from current usage snapshot (will be 0 on first poll which is fine)
        let prev = disks
            .list()
            .iter()
            .map(|d| {
                let name = d.name().to_string_lossy().to_string();
                let usage = d.usage();
                (name, (usage.total_read_bytes, usage.total_written_bytes))
            })
            .collect();
        Self {
            disks,
            prev,
            last_time: std::time::Instant::now(),
        }
    }
}

#[cfg(not(unix))]
impl crate::sensors::SensorSource for DiskActivitySource {
    fn name(&self) -> &str {
        "disk"
    }

    fn poll(
        &mut self,
    ) -> Vec<(
        crate::model::sensor::SensorId,
        crate::model::sensor::SensorReading,
    )> {
        use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
        self.disks.refresh(false);
        let elapsed = self.last_time.elapsed().as_secs_f64().max(0.001);
        self.last_time = std::time::Instant::now();
        let mut results = Vec::new();

        for disk in self.disks.list() {
            let name = disk.name().to_string_lossy().to_string();
            let usage = disk.usage();
            let read = usage.total_read_bytes;
            let written = usage.total_written_bytes;
            let (prev_read, prev_written) =
                self.prev.get(&name).copied().unwrap_or((read, written));

            let read_rate = (read.saturating_sub(prev_read)) as f64 / elapsed / 1_048_576.0;
            let write_rate = (written.saturating_sub(prev_written)) as f64 / elapsed / 1_048_576.0;
            self.prev.insert(name.clone(), (read, written));

            results.push((
                SensorId {
                    source: "disk".to_string(),
                    chip: name.clone(),
                    sensor: "read_mbps".to_string(),
                },
                SensorReading::new(
                    format!("{name} Read"),
                    read_rate,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));
            results.push((
                SensorId {
                    source: "disk".to_string(),
                    chip: name.clone(),
                    sensor: "write_mbps".to_string(),
                },
                SensorReading::new(
                    format!("{name} Write"),
                    write_rate,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));
        }
        results
    }
}
