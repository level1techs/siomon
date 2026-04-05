//! L3 cache hit rate sensor via `perf_event_open(2)`.
//!
//! Opens per-CPU hardware cache counters for LLC read accesses and misses.
//! Each poll cycle computes the delta since the last read to derive a
//! hit rate percentage: `100 * (1 - misses / accesses)`.
//!
//! Requires `perf_event_paranoid ≤ 0` or `CAP_PERFMON` / `CAP_SYS_ADMIN`
//! for system-wide per-CPU monitoring. Returns no sensors if permissions
//! are insufficient or the hardware doesn't support LLC counters.

use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::FromRawFd;
use std::time::Instant;

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

/// `perf_event_attr` type field.
const PERF_TYPE_HW_CACHE: u32 = 3;

/// Cache identifiers.
const PERF_COUNT_HW_CACHE_LL: u64 = 2; // Last-level cache

/// Cache operation identifiers.
const PERF_COUNT_HW_CACHE_OP_READ: u64 = 0;

/// Cache result identifiers.
const PERF_COUNT_HW_CACHE_RESULT_ACCESS: u64 = 0;
const PERF_COUNT_HW_CACHE_RESULT_MISS: u64 = 1;

/// Build a `perf_event_open` config for LLC read accesses or misses.
fn ll_cache_config(result: u64) -> u64 {
    PERF_COUNT_HW_CACHE_LL | (PERF_COUNT_HW_CACHE_OP_READ << 8) | (result << 16)
}

/// Minimal `perf_event_attr` for counting events.
#[repr(C)]
#[derive(Clone)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period_or_freq: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events_or_watermark: u32,
    bp_type: u32,
    bp_addr_or_config1: u64,
    bp_len_or_config2: u64,
    branch_sample_type: u64,
    sample_regs_user: u64,
    sample_stack_user: u32,
    clockid: i32,
    sample_regs_intr: u64,
    aux_watermark: u32,
    sample_max_stack: u16,
    __reserved_2: u16,
    aux_sample_size: u32,
    __reserved_3: u32,
    sig_data: u64,
    config3: u64,
}

impl PerfEventAttr {
    fn new(config: u64) -> Self {
        Self {
            type_: PERF_TYPE_HW_CACHE,
            size: std::mem::size_of::<Self>() as u32,
            config,
            sample_period_or_freq: 0,
            sample_type: 0,
            read_format: 0,
            // PERF_FLAG_DISABLED (bit 0) = 0 — start counting immediately
            flags: 0,
            wakeup_events_or_watermark: 0,
            bp_type: 0,
            bp_addr_or_config1: 0,
            bp_len_or_config2: 0,
            branch_sample_type: 0,
            sample_regs_user: 0,
            sample_stack_user: 0,
            clockid: 0,
            sample_regs_intr: 0,
            aux_watermark: 0,
            sample_max_stack: 0,
            __reserved_2: 0,
            aux_sample_size: 0,
            __reserved_3: 0,
            sig_data: 0,
            config3: 0,
        }
    }
}

fn perf_event_open(attr: &PerfEventAttr, pid: i32, cpu: i32) -> io::Result<File> {
    let fd = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            attr as *const PerfEventAttr as *const libc::c_void,
            pid,
            cpu,
            -1i32, // group_fd
            0u64,  // flags
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }
}

fn read_counter(file: &mut File) -> u64 {
    let mut buf = [0u8; 8];
    if file.read_exact(&mut buf).is_ok() {
        u64::from_ne_bytes(buf)
    } else {
        0
    }
}

/// Per-CPU LLC counter pair.
struct CpuCounters {
    access_fd: File,
    miss_fd: File,
    prev_access: u64,
    prev_miss: u64,
    prev_time: Instant,
}

pub struct PerfCacheSource {
    cpus: Vec<(usize, CpuCounters)>,
}

impl PerfCacheSource {
    /// Try to open LLC counters on all online CPUs.
    /// Returns a source with no CPUs if permissions or hardware don't allow it.
    pub fn discover() -> Self {
        let num_cpus = num_online_cpus();
        let mut cpus = Vec::new();

        for cpu in 0..num_cpus {
            let access_attr =
                PerfEventAttr::new(ll_cache_config(PERF_COUNT_HW_CACHE_RESULT_ACCESS));
            let miss_attr = PerfEventAttr::new(ll_cache_config(PERF_COUNT_HW_CACHE_RESULT_MISS));

            // pid=-1, cpu=N → system-wide monitoring on this CPU
            let Ok(mut access_fd) = perf_event_open(&access_attr, -1, cpu as i32) else {
                if cpu == 0 {
                    log::debug!(
                        "perf_cache: cannot open LLC counters (perf_event_paranoid?), skipping"
                    );
                }
                return Self { cpus: Vec::new() };
            };
            let Ok(mut miss_fd) = perf_event_open(&miss_attr, -1, cpu as i32) else {
                return Self { cpus: Vec::new() };
            };

            let now = Instant::now();
            let prev_access = read_counter(&mut access_fd);
            let prev_miss = read_counter(&mut miss_fd);

            cpus.push((
                cpu,
                CpuCounters {
                    access_fd,
                    miss_fd,
                    prev_access,
                    prev_miss,
                    prev_time: now,
                },
            ));
        }

        if !cpus.is_empty() {
            log::info!("perf_cache: opened LLC counters on {} CPUs", cpus.len());
        }

        Self { cpus }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        if self.cpus.is_empty() {
            return Vec::new();
        }

        let now = Instant::now();
        let mut total_access_delta: u64 = 0;
        let mut total_miss_delta: u64 = 0;

        for (_cpu, counters) in &mut self.cpus {
            let access = read_counter(&mut counters.access_fd);
            let miss = read_counter(&mut counters.miss_fd);

            let access_delta = access.wrapping_sub(counters.prev_access);
            let miss_delta = miss.wrapping_sub(counters.prev_miss);

            total_access_delta += access_delta;
            total_miss_delta += miss_delta;

            counters.prev_access = access;
            counters.prev_miss = miss;
            counters.prev_time = now;
        }

        let mut readings = Vec::new();

        // Aggregate L3 hit rate across all CPUs
        let hit_rate = if total_access_delta > 0 {
            100.0 * (1.0 - total_miss_delta as f64 / total_access_delta as f64)
        } else {
            100.0 // No accesses → no misses → 100% hit rate
        };

        readings.push((
            SensorId {
                source: "perf".into(),
                chip: "cache".into(),
                sensor: "l3_hit_rate".into(),
            },
            SensorReading::new(
                "L3 Hit Rate".into(),
                hit_rate.clamp(0.0, 100.0),
                SensorUnit::Percent,
                SensorCategory::Utilization,
            ),
        ));

        readings
    }
}

impl crate::sensors::SensorSource for PerfCacheSource {
    fn name(&self) -> &str {
        "perf_cache"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        PerfCacheSource::poll(self)
    }
}

/// Count online CPUs from /sys/devices/system/cpu/online.
fn num_online_cpus() -> usize {
    std::fs::read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .and_then(|s| {
            // Parse "0-N" format
            let s = s.trim();
            if let Some((_start, end)) = s.split_once('-') {
                end.parse::<usize>().ok().map(|n| n + 1)
            } else {
                s.parse::<usize>().ok().map(|n| n + 1)
            }
        })
        .unwrap_or(1)
}
