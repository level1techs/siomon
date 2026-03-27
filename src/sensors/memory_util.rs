use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use std::fs;

pub struct MemoryUtilSource;

impl MemoryUtilSource {
    pub fn discover() -> Self {
        Self
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let Some(info) = parse_meminfo() else {
            return Vec::new();
        };

        let mut readings = Vec::new();

        // RAM used = total - available (matches htop/free behavior)
        let ram_used_mb = (info.total_kb.saturating_sub(info.available_kb)) / 1024;
        let ram_total_mb = info.total_kb / 1024;
        let ram_util = if info.total_kb > 0 {
            (100.0 * (1.0 - (info.available_kb as f64 / info.total_kb as f64))).clamp(0.0, 100.0)
        } else {
            0.0
        };

        readings.push(reading(
            "ram_used",
            "RAM Used",
            ram_used_mb as f64,
            SensorUnit::Megabytes,
            SensorCategory::Memory,
        ));
        readings.push(reading(
            "ram_total",
            "RAM Total",
            ram_total_mb as f64,
            SensorUnit::Megabytes,
            SensorCategory::Memory,
        ));
        readings.push(reading(
            "ram_util",
            "RAM Utilization",
            ram_util,
            SensorUnit::Percent,
            SensorCategory::Utilization,
        ));

        // Swap (only if swap is configured)
        if info.swap_total_kb > 0 {
            let swap_used_kb = info.swap_total_kb.saturating_sub(info.swap_free_kb);
            let swap_used_mb = swap_used_kb / 1024;
            let swap_total_mb = info.swap_total_kb / 1024;
            let swap_util = 100.0 * (swap_used_kb as f64 / info.swap_total_kb as f64);

            readings.push(reading(
                "swap_used",
                "Swap Used",
                swap_used_mb as f64,
                SensorUnit::Megabytes,
                SensorCategory::Memory,
            ));
            readings.push(reading(
                "swap_total",
                "Swap Total",
                swap_total_mb as f64,
                SensorUnit::Megabytes,
                SensorCategory::Memory,
            ));
            readings.push(reading(
                "swap_util",
                "Swap Utilization",
                swap_util,
                SensorUnit::Percent,
                SensorCategory::Utilization,
            ));
        }

        // Cached + Buffers (useful for understanding "reclaimable" memory)
        let cached_mb = (info.cached_kb + info.buffers_kb) / 1024;
        readings.push(reading(
            "cached",
            "Cached + Buffers",
            cached_mb as f64,
            SensorUnit::Megabytes,
            SensorCategory::Memory,
        ));

        readings
    }
}

impl crate::sensors::SensorSource for MemoryUtilSource {
    fn name(&self) -> &str {
        "memory_util"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        MemoryUtilSource::poll(self)
    }
}

fn reading(
    sensor: &str,
    label: &str,
    value: f64,
    unit: SensorUnit,
    category: SensorCategory,
) -> (SensorId, SensorReading) {
    let id = SensorId {
        source: "memory".into(),
        chip: "system".into(),
        sensor: sensor.to_string(),
    };
    (
        id,
        SensorReading::new(label.to_string(), value, unit, category),
    )
}

struct MemInfo {
    total_kb: u64,
    available_kb: u64,
    cached_kb: u64,
    buffers_kb: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
}

fn parse_meminfo() -> Option<MemInfo> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;

    let mut total = 0u64;
    let mut available = 0u64;
    let mut cached = 0u64;
    let mut buffers = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let val: u64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);

        match key {
            "MemTotal:" => total = val,
            "MemAvailable:" => available = val,
            "Cached:" => cached = val,
            "Buffers:" => buffers = val,
            "SwapTotal:" => swap_total = val,
            "SwapFree:" => swap_free = val,
            _ => {}
        }
    }

    if total == 0 {
        return None;
    }

    Some(MemInfo {
        total_kb: total,
        available_kb: available,
        cached_kb: cached,
        buffers_kb: buffers,
        swap_total_kb: swap_total,
        swap_free_kb: swap_free,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poll_returns_readings() {
        let mut src = MemoryUtilSource::discover();
        let readings = src.poll();
        // On any Linux system, we should get at least RAM readings
        assert!(
            readings.len() >= 4,
            "expected at least 4 readings, got {}",
            readings.len()
        );

        // Check RAM util is in valid range
        let util = readings.iter().find(|(id, _)| id.sensor == "ram_util");
        assert!(util.is_some(), "ram_util reading missing");
        let (_, reading) = util.unwrap();
        assert!(reading.current >= 0.0 && reading.current <= 100.0);
    }

    #[test]
    fn test_sensor_ids_use_memory_source() {
        let mut src = MemoryUtilSource::discover();
        let readings = src.poll();
        for (id, _) in &readings {
            assert_eq!(id.source, "memory");
            assert_eq!(id.chip, "system");
        }
    }
}
