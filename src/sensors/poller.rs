use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::sensor::{SensorId, SensorReading};
use crate::sensors::{
    cpu_freq, cpu_util, disk_activity, gpu_sensors, hwmon, network_stats, rapl, superio,
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

macro_rules! timed_poll {
    ($name:expr, $src:expr, $readings:expr, $durations:expr) => {{
        let t = Instant::now();
        $readings.extend($src.poll());
        $durations.insert($name.into(), t.elapsed().as_millis() as u64);
    }};
}

pub struct Poller {
    state: SensorState,
    poll_stats: PollStatsState,
    interval: Duration,
    no_nvidia: bool,
    direct_io: bool,
    label_overrides: HashMap<String, String>,
}

impl Poller {
    pub fn new(
        state: SensorState,
        poll_stats: PollStatsState,
        interval_ms: u64,
        no_nvidia: bool,
        direct_io: bool,
        label_overrides: HashMap<String, String>,
    ) -> Self {
        Self {
            state,
            poll_stats,
            interval: Duration::from_millis(interval_ms),
            no_nvidia,
            direct_io,
            label_overrides,
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
        // Discover all sensor sources
        let hwmon_src = hwmon::HwmonSource::discover(&self.label_overrides);
        let freq_src = cpu_freq::CpuFreqSource::discover();
        let mut util_src = cpu_util::CpuUtilSource::discover();
        let gpu_src = gpu_sensors::GpuSensorSource::discover(self.no_nvidia);
        let mut rapl_src = rapl::RaplSource::discover();
        let mut disk_src = disk_activity::DiskActivitySource::discover();
        let mut net_src = network_stats::NetworkStatsSource::discover();

        // Direct I/O sources (Super I/O, I2C) — only when --direct-io is set
        let (nct_src, ite_src) = if self.direct_io {
            let chips = superio::chip_detect::detect_all();
            let mut nct = Vec::new();
            let mut ite = Vec::new();
            for chip in chips {
                let nct_s = superio::nct67xx::Nct67xxSource::new(chip.clone());
                if nct_s.is_supported() {
                    nct.push(nct_s);
                    continue;
                }
                let ite_s = superio::ite87xx::Ite87xxSource::new(chip);
                if ite_s.is_supported() {
                    ite.push(ite_s);
                }
            }
            (nct, ite)
        } else {
            (Vec::new(), Vec::new())
        };

        let (i2c_src, pmbus_src) = if self.direct_io {
            let buses = crate::sensors::i2c::bus_scan::enumerate_smbus_adapters();
            let spd = crate::sensors::i2c::spd5118::Spd5118Source::discover(&buses);
            let pmbus = crate::sensors::i2c::pmbus::PmbusSource::discover(&buses);
            (Some(spd), Some(pmbus))
        } else {
            (None, None)
        };

        // HSMP — always try (don't require --direct-io)
        let hsmp_src = super::hsmp::HsmpSource::discover();

        // IPMI — native ioctl via ipmi-rs, fast enough for the main loop
        let mut ipmi_src = super::ipmi::IpmiSource::discover();

        log::info!(
            "Sensor poller started: {} hwmon chips, {} hwmon sensors, {} nct chips, {} ite chips, i2c: {}, ipmi: {}, hsmp: {}",
            hwmon_src.chip_count(),
            hwmon_src.sensor_count(),
            nct_src.len(),
            ite_src.len(),
            if i2c_src.is_some() { "yes" } else { "no" },
            if ipmi_src.is_available() { "yes" } else { "no" },
            if hsmp_src.is_available() { "yes" } else { "no" },
        );

        let mut durations: HashMap<String, u64> = HashMap::new();
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let cycle_start = Instant::now();
            let mut new_readings: Vec<(SensorId, SensorReading)> = Vec::new();
            durations.clear();

            // Collect from all fast sources
            timed_poll!("hwmon", hwmon_src, new_readings, durations);
            timed_poll!("cpufreq", freq_src, new_readings, durations);
            timed_poll!("cpu_util", util_src, new_readings, durations);
            timed_poll!("gpu", gpu_src, new_readings, durations);
            timed_poll!("rapl", rapl_src, new_readings, durations);
            timed_poll!("disk", disk_src, new_readings, durations);
            timed_poll!("network", net_src, new_readings, durations);

            // Direct I/O sources
            for sio in &nct_src {
                let t = Instant::now();
                new_readings.extend(sio.poll());
                *durations.entry("superio".into()).or_default() += t.elapsed().as_millis() as u64;
            }
            for sio in &ite_src {
                let t = Instant::now();
                new_readings.extend(sio.poll());
                *durations.entry("superio".into()).or_default() += t.elapsed().as_millis() as u64;
            }
            if let Some(ref i2c) = i2c_src {
                timed_poll!("i2c", i2c, new_readings, durations);
            }
            if let Some(ref pmbus) = pmbus_src {
                timed_poll!("pmbus", pmbus, new_readings, durations);
            }

            // HSMP and IPMI (both fast — direct ioctl)
            timed_poll!("hsmp", hsmp_src, new_readings, durations);
            timed_poll!("ipmi", ipmi_src, new_readings, durations);

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

/// Take a one-shot snapshot of all sensors (single poll cycle).
pub fn snapshot(
    no_nvidia: bool,
    direct_io: bool,
    label_overrides: &HashMap<String, String>,
) -> HashMap<SensorId, SensorReading> {
    let hwmon_src = hwmon::HwmonSource::discover(label_overrides);
    let freq_src = cpu_freq::CpuFreqSource::discover();
    let mut util_src = cpu_util::CpuUtilSource::discover();
    let gpu_src = gpu_sensors::GpuSensorSource::discover(no_nvidia);
    let mut rapl_src = rapl::RaplSource::discover();
    let mut disk_src = disk_activity::DiskActivitySource::discover();
    let mut net_src = network_stats::NetworkStatsSource::discover();

    // Short sleep for delta-based sources to have meaningful deltas
    thread::sleep(Duration::from_millis(250));

    let mut map = HashMap::new();
    for (id, reading) in hwmon_src.poll() {
        map.insert(id, reading);
    }
    for (id, reading) in freq_src.poll() {
        map.insert(id, reading);
    }
    for (id, reading) in util_src.poll() {
        map.insert(id, reading);
    }
    for (id, reading) in gpu_src.poll() {
        map.insert(id, reading);
    }
    for (id, reading) in rapl_src.poll() {
        map.insert(id, reading);
    }
    for (id, reading) in disk_src.poll() {
        map.insert(id, reading);
    }
    for (id, reading) in net_src.poll() {
        map.insert(id, reading);
    }

    // Direct I/O sources
    if direct_io {
        for chip in superio::chip_detect::detect_all() {
            let nct = superio::nct67xx::Nct67xxSource::new(chip.clone());
            if nct.is_supported() {
                for (id, reading) in nct.poll() {
                    map.insert(id, reading);
                }
                continue;
            }
            let ite = superio::ite87xx::Ite87xxSource::new(chip);
            if ite.is_supported() {
                for (id, reading) in ite.poll() {
                    map.insert(id, reading);
                }
            }
        }
        let buses = crate::sensors::i2c::bus_scan::enumerate_smbus_adapters();
        let i2c_src = crate::sensors::i2c::spd5118::Spd5118Source::discover(&buses);
        for (id, reading) in i2c_src.poll() {
            map.insert(id, reading);
        }
        let pmbus_src = crate::sensors::i2c::pmbus::PmbusSource::discover(&buses);
        for (id, reading) in pmbus_src.poll() {
            map.insert(id, reading);
        }
    }

    // IPMI and HSMP — always try
    let mut ipmi_src = super::ipmi::IpmiSource::discover();
    for (id, reading) in ipmi_src.poll() {
        map.insert(id, reading);
    }
    let hsmp_src = super::hsmp::HsmpSource::discover();
    for (id, reading) in hsmp_src.poll() {
        map.insert(id, reading);
    }

    map
}
