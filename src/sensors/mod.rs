//! Real-time sensor polling sources.
//!
//! Most submodules implement [`SensorSource`] for a specific data source
//! (hwmon, IPMI, RAPL, Super I/O, I2C/PMBus, etc.). The [`poller`] module
//! orchestrates all discovered sources in a background thread.

pub mod aer;
pub mod alerts;
pub mod cpu_freq;
pub mod cpu_util;
pub mod disk_activity;
pub mod edac;
pub mod gpu_sensors;
pub mod hsmp;
pub mod hwmon;
pub mod i2c;
pub mod ipmi;
pub mod mce;
pub mod memory_util;
pub mod network_stats;
pub mod perf_cache;
pub mod poller;
pub mod rapl;
pub mod resctrl;
pub mod superio;

use crate::model::sensor::{SensorId, SensorReading};

/// Trait for real-time sensor polling sources.
///
/// Each implementor discovers hardware during construction (not part of the
/// trait, since discovery parameters vary per source) and then polls
/// repeatedly via `poll()`.
pub trait SensorSource: Send {
    /// Human-readable name for logging and timing stats (e.g., "hwmon", "ipmi").
    fn name(&self) -> &str;

    /// Read current sensor values. Returns an empty Vec if the source has
    /// no readings (hardware unavailable, no sensors discovered, etc.).
    fn poll(&mut self) -> Vec<(SensorId, SensorReading)>;
}
