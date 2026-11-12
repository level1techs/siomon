//! Intel ring clock and DRAM bandwidth from uncore perf counters, plus the
//! configured ring clock range.
//!
//! Every event is resolved through the kernel's sysfs PMU interface
//! (`/sys/bus/event_source/devices/<pmu>/{type,format,events}`), so no event
//! codes are hard-coded here:
//! - **Ring/LLC clock**: `uncore_cncu/clockticks/` as an interval average.
//!   On Arrow Lake-S this counter moves with load and reaches the ring
//!   maximum, so it is taken to be the ring clock (inferred, not confirmed).
//!   The ring, NGU and D2D clocks are separate settings; this is not the
//!   NGU or D2D clock.
//! - **DRAM bandwidth**: `uncore_imc_free_running_N/data_{read,write,total}/`
//!   with the kernel's `.scale` and `.unit`, summed over all memory
//!   controllers. Free-running counters are shared, so other perf users
//!   (`perf stat`, turbostat) do not take them away.
//! - **Ring clock range**: `intel_uncore_frequency` sysfs `min_freq_khz` /
//!   `max_freq_khz` (no privileges needed). These are the configured limits
//!   (MSR 0x620), not the live clock.
//!
//! Rates are counts divided by the time the kernel actually ran the
//! counter. If another perf session time-shares the ring clock counter,
//! the rate is still measured over the time it ran; below 10% of the
//! interval the reading is skipped for that poll.
//!
//! Perf counters need root or `CAP_PERFMON` when
//! `kernel.perf_event_paranoid` is above 0.

use std::path::Path;

use perf_event::events::Dynamic;
use perf_event::{Builder, Counter, CounterData};

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};

/// Minimum share of the interval a counter must have run.
const MIN_RUNNING: f64 = 0.1;

const PMU_ROOT: &str = "/sys/bus/event_source/devices";
const UNCORE_FREQ_ROOT: &str = "/sys/devices/system/cpu/intel_uncore_frequency";

/// Events per second between two samples, over the time the counter ran.
/// `None` if a value went backwards or it ran for too little of the interval.
fn rate(prev: (u64, u64, u64), cur: (u64, u64, u64)) -> Option<f64> {
    let (d_count, d_enabled, d_running) = (
        cur.0.checked_sub(prev.0)?,
        cur.1.checked_sub(prev.1)?,
        cur.2.checked_sub(prev.2)?,
    );
    if d_enabled == 0 || (d_running as f64) < d_enabled as f64 * MIN_RUNNING {
        return None;
    }
    Some(d_count as f64 / (d_running as f64 / 1e9))
}

/// First CPU of a PMU `cpumask` list such as `0` or `0-3,8`.
fn first_cpu(mask: &str) -> Option<usize> {
    mask.split([',', '-']).next()?.trim().parse().ok()
}

/// Factor converting a sysfs event scale/unit to megabytes (10^6 bytes).
fn megabytes_per_unit(unit: &str) -> Option<f64> {
    match unit {
        "MiB" => Some(1.048_576),
        "MB" => Some(1.0),
        "KiB" => Some(1.024e-3),
        "B" | "Bytes" => Some(1e-6),
        _ => None,
    }
}

struct EventCounter {
    counter: Counter,
    prev: Option<(u64, u64, u64)>,
}

impl EventCounter {
    /// Open the named sysfs event of `pmu` system-wide on the PMU's CPU.
    /// Returns the counter and the event's scale and unit, if any.
    fn open(pmu: &str, event: &str) -> Option<(Self, f64, Option<String>)> {
        let mut builder = Dynamic::builder(pmu).ok()?;
        builder.event(event).ok()?;
        let scale = builder.scale().ok().flatten().unwrap_or(1.0);
        let unit = builder.unit().ok().flatten();
        let cpu = sysfs::read_string_optional(&Path::new(PMU_ROOT).join(pmu).join("cpumask"))
            .and_then(|mask| first_cpu(&mask))
            .unwrap_or(0);
        let counter = Builder::new(builder.build().ok()?)
            .one_cpu(cpu)
            .any_pid()
            // Uncore PMUs reject privilege-level filters.
            .exclude_kernel(false)
            .exclude_hv(false)
            .enabled(true)
            .build()
            .map_err(|e| log::debug!("perf {pmu}/{event}/: {e}"))
            .ok()?;
        let mut ev = Self {
            counter,
            prev: None,
        };
        ev.prev = ev.sample();
        Some((ev, scale, unit))
    }

    fn sample(&mut self) -> Option<(u64, u64, u64)> {
        let data: CounterData = self.counter.read_full().ok()?;
        Some((
            data.count(),
            data.time_enabled()?.as_nanos() as u64,
            data.time_running()?.as_nanos() as u64,
        ))
    }

    /// Events per second since the previous sample.
    fn rate(&mut self) -> Option<f64> {
        let cur = self.sample()?;
        let r = self.prev.and_then(|prev| rate(prev, cur));
        self.prev = Some(cur);
        r
    }
}

/// A reading summed over one or more counters (one per memory controller).
struct Metric {
    id: SensorId,
    label: &'static str,
    unit: SensorUnit,
    category: SensorCategory,
    /// Converts summed events per second into the published unit.
    factor: f64,
    counters: Vec<EventCounter>,
}

/// A configured value read from sysfs each poll.
struct Limit {
    id: SensorId,
    label: &'static str,
    file: CachedFile,
}

pub struct IntelUncoreSource {
    metrics: Vec<Metric>,
    limits: Vec<Limit>,
}

const DRAM_EVENTS: &[(&str, &str)] = &[
    ("data_read", "DRAM Read"),
    ("data_write", "DRAM Write"),
    ("data_total", "DRAM Total"),
];

fn id(chip: &str, sensor: &str) -> SensorId {
    SensorId {
        source: "uncore".into(),
        chip: chip.into(),
        sensor: sensor.into(),
    }
}

/// Names of registered PMUs starting with `prefix`, sorted.
fn pmus_with_prefix(prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(PMU_ROOT)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| {
            n.strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
        .collect();
    names.sort_by(|a, b| crate::model::sensor::natural_cmp_str(a, b));
    names
}

impl IntelUncoreSource {
    pub fn discover() -> Self {
        let mut metrics = Vec::new();

        if let Some((counter, _, _)) = EventCounter::open("uncore_cncu", "clockticks") {
            metrics.push(Metric {
                id: id("clocks", "ring_clock"),
                label: "Ring/LLC Clock",
                unit: SensorUnit::Mhz,
                category: SensorCategory::Frequency,
                factor: 1e-6,
                counters: vec![counter],
            });
        }

        let imcs = pmus_with_prefix("uncore_imc_free_running_");
        for &(event, label) in DRAM_EVENTS {
            let opened: Vec<_> = imcs
                .iter()
                .filter_map(|pmu| EventCounter::open(pmu, event))
                .collect();
            // Publish only when every controller is counted with a known unit.
            let Some(factor) = opened
                .first()
                .and_then(|(_, scale, unit)| Some(scale * megabytes_per_unit(unit.as_deref()?)?))
            else {
                continue;
            };
            if opened.is_empty() || opened.len() != imcs.len() {
                continue;
            }
            metrics.push(Metric {
                id: id("imc", event),
                label,
                unit: SensorUnit::MegabytesPerSec,
                category: SensorCategory::Throughput,
                factor,
                counters: opened.into_iter().map(|(c, _, _)| c).collect(),
            });
        }

        // The first domain only: one per client package, while server parts
        // list several dies and fabric clusters.
        let mut limits = Vec::new();
        if let Some(dir) = sysfs::glob_paths(&format!("{UNCORE_FREQ_ROOT}/*"))
            .into_iter()
            .next()
        {
            for (file, sensor, label) in [
                ("min_freq_khz", "ring_clock_min", "Ring Clock Min"),
                ("max_freq_khz", "ring_clock_max", "Ring Clock Max"),
            ] {
                if let Some(file) = CachedFile::open(dir.join(file)) {
                    limits.push(Limit {
                        id: id("ring_limits", sensor),
                        label,
                        file,
                    });
                }
            }
        }

        if !metrics.is_empty() {
            log::info!("Intel uncore: {} perf metrics", metrics.len());
        }
        Self { metrics, limits }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();
        for metric in &mut self.metrics {
            let rates: Vec<Option<f64>> = metric.counters.iter_mut().map(|c| c.rate()).collect();
            // A partial sum would under-report, so skip the poll instead.
            if rates.iter().any(Option::is_none) {
                continue;
            }
            let total: f64 = rates.into_iter().flatten().sum();
            readings.push((
                metric.id.clone(),
                SensorReading::new(
                    metric.label.into(),
                    total * metric.factor,
                    metric.unit,
                    metric.category,
                ),
            ));
        }
        for limit in &mut self.limits {
            if let Some(khz) = limit.file.read_u64().filter(|&k| k != 0) {
                readings.push((
                    limit.id.clone(),
                    SensorReading::new(
                        limit.label.into(),
                        khz as f64 / 1000.0,
                        SensorUnit::Mhz,
                        SensorCategory::Frequency,
                    ),
                ));
            }
        }
        readings
    }
}

impl crate::sensors::SensorSource for IntelUncoreSource {
    fn name(&self) -> &str {
        "intel_uncore"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        IntelUncoreSource::poll(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_uses_running_time() {
        // 1e9 ticks over 0.5 s of running time in a 1 s window -> 2 GHz
        let r = rate((0, 0, 0), (1_000_000_000, 1_000_000_000, 500_000_000)).unwrap();
        assert!((r - 2e9).abs() < 1.0);
    }

    #[test]
    fn test_rate_rejects_starved_and_backwards() {
        // ran for 5% of the interval
        assert_eq!(rate((0, 0, 0), (100, 1_000, 50)), None);
        assert_eq!(rate((200, 0, 0), (100, 1_000, 1_000)), None);
        assert_eq!(rate((0, 0, 0), (100, 0, 0)), None);
    }

    #[test]
    fn test_first_cpu() {
        assert_eq!(first_cpu("0"), Some(0));
        assert_eq!(first_cpu("4-7,12"), Some(4));
        assert_eq!(first_cpu(""), None);
    }

    #[test]
    fn test_megabytes_per_unit() {
        // Kernel free-running IMC events: scale 6.103515625e-5 MiB (64 bytes)
        let mb_per_count = 6.103_515_625e-5 * megabytes_per_unit("MiB").unwrap();
        assert!((mb_per_count - 64e-6).abs() < 1e-12);
        assert_eq!(megabytes_per_unit("furlongs"), None);
    }
}
