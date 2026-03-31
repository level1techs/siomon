use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::db::boards::{BoardTemplate, Platform};
use crate::model::sensor::{SensorId, SensorReading};
use crate::sensors::{
    SensorSource, cpu_freq, cpu_util, disk_activity, gpu_sensors, hwmon, network_stats, rapl,
    superio,
};

pub type SensorState = Arc<RwLock<HashMap<SensorId, SensorReading>>>;

pub fn new_state() -> SensorState {
    Arc::new(RwLock::new(HashMap::new()))
}

#[derive(Debug, Clone, Default)]
pub struct PollStats {
    pub cycle_duration_ms: u64,
    pub source_durations: HashMap<String, u64>, // name -> ms
}

pub type PollStatsState = Arc<RwLock<PollStats>>;

pub fn new_poll_stats() -> PollStatsState {
    Arc::new(RwLock::new(PollStats::default()))
}

pub struct Poller {
    state: SensorState,
    poll_stats: PollStatsState,
    interval: Duration,
    no_nvidia: bool,
    direct_io: bool,
    label_overrides: HashMap<String, String>,
    voltage_scaling: HashMap<String, f64>,
    storage_exclude: Vec<String>,
    platform: Platform,
    board: Option<&'static BoardTemplate>,
}

impl Poller {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state: SensorState,
        poll_stats: PollStatsState,
        interval_ms: u64,
        no_nvidia: bool,
        direct_io: bool,
        label_overrides: HashMap<String, String>,
        voltage_scaling: HashMap<String, f64>,
        storage_exclude: Vec<String>,
        platform: Platform,
        board: Option<&'static BoardTemplate>,
    ) -> Self {
        Self {
            state,
            poll_stats,
            interval: Duration::from_millis(interval_ms),
            no_nvidia,
            direct_io,
            label_overrides,
            voltage_scaling,
            storage_exclude,
            platform,
            board,
        }
    }

    /// Run the polling loop in a background thread. Returns a handle to stop it.
    pub fn spawn(self) -> PollerHandle {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();

        let handle = thread::spawn(move || {
            self.run(stop_clone);
        });

        PollerHandle {
            stop,
            _handle: handle,
        }
    }

    fn run(self, stop: Arc<std::sync::atomic::AtomicBool>) {
        let mut sources = discover_all_sources(
            self.no_nvidia,
            self.direct_io,
            &self.label_overrides,
            &self.voltage_scaling,
            &self.storage_exclude,
            self.platform,
            self.board,
        );

        log::info!("Sensor poller started: {} sources", sources.len());

        let mut durations: HashMap<String, u64> = HashMap::new();
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let cycle_start = Instant::now();
            let mut new_readings: Vec<(SensorId, SensorReading)> = Vec::new();
            durations.clear();

            for source in &mut sources {
                let t = Instant::now();
                new_readings.extend(source.poll());
                *durations.entry(source.name().to_string()).or_default() +=
                    t.elapsed().as_millis() as u64;
            }

            let cycle_ms = cycle_start.elapsed().as_millis() as u64;

            // Log warning for slow poll cycles
            if cycle_ms > 500 {
                let slow: Vec<String> = durations
                    .iter()
                    .filter(|&(_, &ms)| ms > 100)
                    .map(|(name, ms)| format!("{name}: {ms}ms"))
                    .collect();
                log::warn!(
                    "Slow poll cycle: {}ms [{}]",
                    cycle_ms,
                    if slow.is_empty() {
                        "no single source >100ms".into()
                    } else {
                        slow.join(", ")
                    }
                );
            }

            // Update shared state
            if let Ok(mut state) = self.state.write() {
                for (id, new_reading) in new_readings {
                    if let Some(existing) = state.get_mut(&id) {
                        existing.update(new_reading.current);
                    } else {
                        state.insert(id, new_reading);
                    }
                }
            }

            // Update poll stats
            if let Ok(mut stats) = self.poll_stats.write() {
                stats.cycle_duration_ms = cycle_ms;
                stats.source_durations.clone_from(&durations);
            }

            thread::sleep(self.interval);
        }
    }
}

pub struct PollerHandle {
    stop: Arc<std::sync::atomic::AtomicBool>,
    _handle: thread::JoinHandle<()>,
}

impl PollerHandle {
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for PollerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Discover all sensor sources and return them as trait objects.
///
/// Encapsulates per-source construction and logging. Called by both the
/// continuous poller and the one-shot snapshot.
fn join_or_log<T>(result: std::thread::Result<T>, name: &str) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown");
            log::error!("{name} sensor discovery panicked: {msg}");
            None
        }
    }
}

fn discover_all_sources(
    no_nvidia: bool,
    direct_io: bool,
    label_overrides: &HashMap<String, String>,
    voltage_scaling: &HashMap<String, f64>,
    storage_exclude: &[String],
    platform: Platform,
    board: Option<&'static BoardTemplate>,
) -> Vec<Box<dyn SensorSource>> {
    use std::thread;
    use std::time::Instant;

    // Run slow discoveries in parallel. IPMI (SDR walk + initial poll) and
    // GPU/NVML init are the biggest bottlenecks — 5-10s each on large systems.
    // By running them concurrently we cut startup from sum to max.
    let t = Instant::now();
    let sources: Vec<Box<dyn SensorSource>> = thread::scope(|s| {
        // Slow: IPMI SDR walk + initial poll (~5-10s on workstation BMCs)
        let h_ipmi = s.spawn(|| -> Box<dyn SensorSource> {
            let src = super::ipmi::IpmiSource::discover();
            log::info!("IPMI: {}", if src.is_available() { "yes" } else { "no" });
            Box::new(src)
        });

        // Slow: NVML library load + init + device enumeration (~2-5s)
        let h_gpu = s.spawn(move || -> Box<dyn SensorSource> {
            Box::new(gpu_sensors::GpuSensorSource::discover(no_nvidia))
        });

        // Slow: HSMP device open + protocol probe
        let h_hsmp = s.spawn(|| -> Box<dyn SensorSource> {
            let src = super::hsmp::HsmpSource::discover();
            log::info!("HSMP: {}", if src.is_available() { "yes" } else { "no" });
            Box::new(src)
        });

        // Moderate: hwmon sysfs enumeration
        let h_hwmon = s.spawn(|| -> Box<dyn SensorSource> {
            let src = hwmon::HwmonSource::discover(label_overrides, voltage_scaling);
            log::info!(
                "hwmon: {} chips, {} sensors",
                src.chip_count(),
                src.sensor_count()
            );
            Box::new(src)
        });

        // Direct I/O sources (Super I/O, I2C) — only when --direct-io is set
        let h_direct_io = if direct_io {
            Some(s.spawn(|| -> Vec<Box<dyn SensorSource>> {
                let mut dio_sources: Vec<Box<dyn SensorSource>> = Vec::new();

                let chips = superio::chip_detect::detect_all();
                let mut nct_count = 0;
                let mut ite_count = 0;
                for chip in chips {
                    let nct_s = superio::nct67xx::Nct67xxSource::new(chip.clone(), label_overrides);
                    if nct_s.is_supported() {
                        nct_count += 1;
                        dio_sources.push(Box::new(nct_s));
                        continue;
                    }
                    let ite_s = superio::ite87xx::Ite87xxSource::new(chip);
                    if ite_s.is_supported() {
                        ite_count += 1;
                        dio_sources.push(Box::new(ite_s));
                    }
                }
                if nct_count > 0 || ite_count > 0 {
                    log::info!(
                        "Super I/O: {} nct chips, {} ite chips",
                        nct_count,
                        ite_count
                    );
                }

                let buses = crate::sensors::i2c::bus_scan::enumerate_smbus_adapters();
                dio_sources.push(Box::new(
                    crate::sensors::i2c::spd5118::Spd5118Source::discover(&buses),
                ));
                dio_sources.push(Box::new(crate::sensors::i2c::pmbus::PmbusSource::discover(
                    &buses,
                )));
                let ddr5_config = board.and_then(|b| b.ddr5_bus_config);
                let ddr5_reqs = board
                    .map(|b| b.requirements.get(crate::db::boards::FEAT_DDR5))
                    .unwrap_or(&[]);
                dio_sources.push(Box::new(
                    crate::sensors::i2c::ddr5_temp::Ddr5TempSource::discover(
                        ddr5_config,
                        ddr5_reqs,
                    ),
                ));
                log::info!("I2C: enabled ({} buses)", buses.len());

                dio_sources
            }))
        } else {
            None
        };

        // Fast: these are trivial sysfs reads, run on the main thread while waiting
        let mut result: Vec<Box<dyn SensorSource>> = vec![
            Box::new(cpu_freq::CpuFreqSource::discover()),
            Box::new(cpu_util::CpuUtilSource::discover()),
            Box::new(rapl::RaplSource::discover()),
            Box::new(disk_activity::DiskActivitySource::discover(storage_exclude)),
            Box::new(network_stats::NetworkStatsSource::discover()),
            Box::new(super::edac::EdacSource::discover()),
            Box::new(super::aer::AerSource::discover()),
            Box::new(super::mce::MceSource::discover()),
            Box::new(super::memory_util::MemoryUtilSource::discover()),
        ];

        // Tegra platform sources (devfreq GPU, hardware engines)
        if platform == Platform::Tegra {
            let gpu_src = crate::platform::tegra::DevfreqGpuSource::discover();
            result.push(Box::new(gpu_src));
            let eng_src = crate::platform::tegra::TegraEngineSource::discover();
            result.push(Box::new(eng_src));
            log::info!("Tegra platform detected, added devfreq GPU + engine sources");
        }

        // Collect parallel results — log and skip any that panicked
        result.extend(join_or_log(h_hwmon.join(), "hwmon"));
        result.extend(join_or_log(h_gpu.join(), "gpu"));
        result.extend(join_or_log(h_hsmp.join(), "hsmp"));
        result.extend(join_or_log(h_ipmi.join(), "ipmi"));

        if let Some(h) = h_direct_io {
            if let Some(dio) = join_or_log(h.join(), "direct-io") {
                result.extend(dio);
            }
        }

        result
    });

    log::info!(
        "Sensor discovery: {} sources in {}ms",
        sources.len(),
        t.elapsed().as_millis()
    );
    sources
}

/// Take a one-shot snapshot of all sensors (single poll cycle).
pub fn snapshot(
    no_nvidia: bool,
    direct_io: bool,
    label_overrides: &HashMap<String, String>,
    voltage_scaling: &HashMap<String, f64>,
    storage_exclude: &[String],
    platform: Platform,
    board: Option<&'static BoardTemplate>,
) -> HashMap<SensorId, SensorReading> {
    let mut sources = discover_all_sources(
        no_nvidia,
        direct_io,
        label_overrides,
        voltage_scaling,
        storage_exclude,
        platform,
        board,
    );

    // Short sleep for delta-based sources to have meaningful deltas
    thread::sleep(Duration::from_millis(250));

    let mut map = HashMap::new();
    for source in &mut sources {
        for (id, reading) in source.poll() {
            map.insert(id, reading);
        }
    }
    map
}
