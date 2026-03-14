use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};
use std::path::Path;
use std::time::Instant;

pub struct NetworkStatsSource {
    interfaces: Vec<NetInterface>,
    prev_time: Instant,
}

struct NetInterface {
    name: String,
    rx_file: CachedFile,
    tx_file: CachedFile,
    prev_rx: u64,
    prev_tx: u64,
    /// Link speed in Mbit/s from sysfs, if available.
    link_speed_mbit: Option<u64>,
}

impl NetworkStatsSource {
    pub fn discover() -> Self {
        let mut interfaces = Vec::new();

        for dir in sysfs::glob_paths("/sys/class/net/*") {
            let iface = match dir.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            if !is_physical_interface(&dir, &iface) {
                continue;
            }

            let base = dir.join("statistics");
            let Some(mut rx_file) = CachedFile::open(base.join("rx_bytes")) else {
                continue;
            };
            let Some(mut tx_file) = CachedFile::open(base.join("tx_bytes")) else {
                continue;
            };

            let Some(prev_rx) = rx_file.read_u64() else {
                continue;
            };
            let Some(prev_tx) = tx_file.read_u64() else {
                continue;
            };

            // Link speed in Mbit/s (sysfs returns -1 as u32::MAX when link is down)
            let link_speed_mbit = sysfs::read_u64_optional(&dir.join("speed"))
                .filter(|&s| s > 0 && s < u32::MAX as u64);

            interfaces.push(NetInterface {
                name: iface,
                rx_file,
                tx_file,
                prev_rx,
                prev_tx,
                link_speed_mbit,
            });
        }

        Self {
            interfaces,
            prev_time: Instant::now(),
        }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let now = Instant::now();
        let elapsed_secs = now.duration_since(self.prev_time).as_secs_f64();
        let mut readings = Vec::new();

        if elapsed_secs <= 0.0 {
            self.prev_time = now;
            return readings;
        }

        for iface in &mut self.interfaces {
            let Some(rx) = iface.rx_file.read_u64() else {
                continue;
            };
            let Some(tx) = iface.tx_file.read_u64() else {
                continue;
            };

            let rx_delta = rx.saturating_sub(iface.prev_rx);
            let tx_delta = tx.saturating_sub(iface.prev_tx);

            let rx_mbps = (rx_delta as f64) / (1_048_576.0 * elapsed_secs);
            let tx_mbps = (tx_delta as f64) / (1_048_576.0 * elapsed_secs);

            let rx_id = SensorId {
                source: "net".into(),
                chip: iface.name.clone(),
                sensor: "rx_mbps".into(),
            };
            let rx_label = format!("{} RX", iface.name);
            readings.push((
                rx_id,
                SensorReading::new(
                    rx_label,
                    rx_mbps,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));

            let tx_id = SensorId {
                source: "net".into(),
                chip: iface.name.clone(),
                sensor: "tx_mbps".into(),
            };
            let tx_label = format!("{} TX", iface.name);
            readings.push((
                tx_id,
                SensorReading::new(
                    tx_label,
                    tx_mbps,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));

            // Link speed in MiB/s (Mbit/s × 1e6 / 8 / 1048576) to match rx/tx units
            if let Some(speed_mbit) = iface.link_speed_mbit {
                let speed_id = SensorId {
                    source: "net".into(),
                    chip: iface.name.clone(),
                    sensor: "link_speed".into(),
                };
                let speed_mibs = speed_mbit as f64 * 1_000_000.0 / 8.0 / 1_048_576.0;
                readings.push((
                    speed_id,
                    SensorReading::new(
                        format!("{} Link Speed", iface.name),
                        speed_mibs,
                        SensorUnit::MegabytesPerSec,
                        SensorCategory::Throughput,
                    ),
                ));
            }

            iface.prev_rx = rx;
            iface.prev_tx = tx;
        }

        self.prev_time = now;
        readings
    }
}

impl crate::sensors::SensorSource for NetworkStatsSource {
    fn name(&self) -> &str {
        "network"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        NetworkStatsSource::poll(self)
    }
}

fn is_physical_interface(dir: &Path, iface: &str) -> bool {
    // Skip loopback
    if iface == "lo" {
        return false;
    }

    // Virtual interfaces don't have a "device" symlink in sysfs
    // Physical NICs (PCI, USB) have /sys/class/net/{iface}/device -> ../../...
    dir.join("device").exists()
}
