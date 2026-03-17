#[cfg(unix)]
use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
#[cfg(unix)]
use crate::platform::sysfs::{self, CachedFile};
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
pub struct NetworkStatsSource {
    interfaces: Vec<NetInterface>,
    prev_time: Instant,
}

#[cfg(unix)]
struct NetInterface {
    name: String,
    rx_file: CachedFile,
    tx_file: CachedFile,
    prev_rx: u64,
    prev_tx: u64,
    /// Sysfs speed file, re-read each poll to track link renegotiation.
    speed_file: Option<CachedFile>,
}

#[cfg(unix)]
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

            let speed_file = CachedFile::open(dir.join("speed"));

            interfaces.push(NetInterface {
                name: iface,
                rx_file,
                tx_file,
                prev_rx,
                prev_tx,
                speed_file,
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

            // Link speed in MiB/s (re-read each poll to track renegotiation).
            // Sysfs reports -1 (parsed as large u64) when link is down.
            if let Some(speed_mbit) = iface
                .speed_file
                .as_mut()
                .and_then(|f| f.read_u64())
                .filter(|&s| s > 0 && s < u32::MAX as u64)
            {
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

#[cfg(unix)]
impl crate::sensors::SensorSource for NetworkStatsSource {
    fn name(&self) -> &str {
        "network"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        NetworkStatsSource::poll(self)
    }
}

#[cfg(unix)]
fn is_physical_interface(dir: &Path, iface: &str) -> bool {
    // Skip loopback
    if iface == "lo" {
        return false;
    }

    // Virtual interfaces don't have a "device" symlink in sysfs
    // Physical NICs (PCI, USB) have /sys/class/net/{iface}/device -> ../../...
    dir.join("device").exists()
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(not(unix))]
pub struct NetworkStatsSource {
    networks: sysinfo::Networks,
    prev: std::collections::HashMap<String, (u64, u64)>,
    last_time: std::time::Instant,
}

#[cfg(not(unix))]
impl NetworkStatsSource {
    pub fn discover() -> Self {
        use sysinfo::Networks;
        let networks = Networks::new_with_refreshed_list();
        let prev = networks
            .iter()
            .map(|(n, d)| (n.clone(), (d.total_received(), d.total_transmitted())))
            .collect();
        Self {
            networks,
            prev,
            last_time: std::time::Instant::now(),
        }
    }
}

#[cfg(not(unix))]
impl crate::sensors::SensorSource for NetworkStatsSource {
    fn name(&self) -> &str {
        "network"
    }

    fn poll(
        &mut self,
    ) -> Vec<(
        crate::model::sensor::SensorId,
        crate::model::sensor::SensorReading,
    )> {
        use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
        self.networks.refresh(false);
        let elapsed = self.last_time.elapsed().as_secs_f64().max(0.001);
        self.last_time = std::time::Instant::now();
        let mut results = Vec::new();

        for (name, data) in self.networks.iter() {
            let rx = data.total_received();
            let tx = data.total_transmitted();
            let (prev_rx, prev_tx) = self.prev.get(name).copied().unwrap_or((rx, tx));
            let rx_rate = (rx.saturating_sub(prev_rx)) as f64 / elapsed / 1_048_576.0;
            let tx_rate = (tx.saturating_sub(prev_tx)) as f64 / elapsed / 1_048_576.0;
            self.prev.insert(name.clone(), (rx, tx));

            results.push((
                SensorId {
                    source: "net".to_string(),
                    chip: name.clone(),
                    sensor: "rx_mbps".to_string(),
                },
                SensorReading::new(
                    format!("{name} RX"),
                    rx_rate,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));
            results.push((
                SensorId {
                    source: "net".to_string(),
                    chip: name.clone(),
                    sensor: "tx_mbps".to_string(),
                },
                SensorReading::new(
                    format!("{name} TX"),
                    tx_rate,
                    SensorUnit::MegabytesPerSec,
                    SensorCategory::Throughput,
                ),
            ));
        }
        results
    }
}
