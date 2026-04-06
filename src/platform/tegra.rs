//! Tegra platform support (NVIDIA Jetson / DGX Spark).
//!
//! Tegra SoCs use `devfreq` for integrated GPU frequency and load, and expose
//! hardware engine clocks via debugfs. This module provides platform detection
//! and sensor sources for these Tegra-specific interfaces.

use std::path::{Path, PathBuf};

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs;

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

/// Returns true if this system is a Tegra/Jetson platform.
pub fn is_tegra() -> bool {
    Path::new("/etc/nv_tegra_release").exists()
}

// ---------------------------------------------------------------------------
// Devfreq GPU sensor source
// ---------------------------------------------------------------------------

/// Known devfreq GPU device names (from device tree `of_node/name`).
const KNOWN_GPU_NAMES: &[&str] = &["gv11b", "gp10b", "ga10b", "gb10b", "gpu"];

struct DevfreqGpu {
    name: String,
    devfreq_dir: PathBuf,
    load_path: Option<PathBuf>,
    max_freq_mhz: Option<u32>,
}

/// Sensor source for Tegra integrated GPUs via devfreq.
pub struct DevfreqGpuSource {
    gpus: Vec<DevfreqGpu>,
}

impl DevfreqGpuSource {
    pub fn discover() -> Self {
        let mut gpus = Vec::new();

        for entry in sysfs::glob_paths("/sys/class/devfreq/*") {
            let devfreq_name = entry
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Check the device tree node name first, then fall back to directory name.
            let of_name = sysfs::read_string_optional(&entry.join("device/of_node/name"));
            let is_gpu = match &of_name {
                Some(name) => {
                    KNOWN_GPU_NAMES.iter().any(|&known| name == known)
                        || name.to_lowercase().contains("gpu")
                }
                None => devfreq_name.to_lowercase().contains("gpu"),
            };
            if !is_gpu {
                continue;
            }

            let max_freq_mhz =
                sysfs::read_u64_optional(&entry.join("max_freq")).map(|hz| (hz / 1_000_000) as u32);

            let load_path = {
                let p = entry.join("device/load");
                if p.exists() { Some(p) } else { None }
            };

            let display_name = of_name.as_deref().unwrap_or("?");
            log::info!("Tegra devfreq GPU: {devfreq_name} ({display_name})");

            gpus.push(DevfreqGpu {
                name: devfreq_name,
                devfreq_dir: entry,
                load_path,
                max_freq_mhz,
            });
        }

        Self { gpus }
    }
}

impl crate::sensors::SensorSource for DevfreqGpuSource {
    fn name(&self) -> &str {
        "tegra-gpu"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for gpu in &self.gpus {
            let chip = &gpu.name;

            // Current frequency (Hz → MHz)
            if let Some(hz) = sysfs::read_u64_optional(&gpu.devfreq_dir.join("cur_freq")) {
                let mhz = (hz / 1_000_000) as f64;
                readings.push((
                    sid("tegra-gpu", chip, "frequency"),
                    SensorReading::new(
                        format!("{chip} Frequency"),
                        mhz,
                        SensorUnit::Mhz,
                        SensorCategory::Frequency,
                    ),
                ));
            }

            // GPU load — devfreq reports 0-1000 (promille), divide by 10 for percent
            if let Some(ref load_path) = gpu.load_path
                && let Some(raw) = sysfs::read_u64_optional(load_path)
            {
                let pct = raw as f64 / 10.0;
                readings.push((
                    sid("tegra-gpu", chip, "load"),
                    SensorReading::new(
                        format!("{chip} Load"),
                        pct,
                        SensorUnit::Percent,
                        SensorCategory::Utilization,
                    ),
                ));
            }

            // Max frequency (static, cached from discovery)
            if let Some(max_mhz) = gpu.max_freq_mhz {
                readings.push((
                    sid("tegra-gpu", chip, "max_frequency"),
                    SensorReading::new(
                        format!("{chip} Max Frequency"),
                        max_mhz as f64,
                        SensorUnit::Mhz,
                        SensorCategory::Frequency,
                    ),
                ));
            }
        }

        readings
    }
}

// ---------------------------------------------------------------------------
// Hardware engine sensor source
// ---------------------------------------------------------------------------

struct TegraEngine {
    name: String,
    path: PathBuf,
}

/// Sensor source for Tegra hardware engines (APE, DLA, VIC, NVENC, etc.)
/// via `/sys/kernel/debug/clk/`.
pub struct TegraEngineSource {
    engines: Vec<TegraEngine>,
}

/// Engine names to probe in debugfs clk tree.
const ENGINE_NAMES: &[&str] = &[
    "ape", "dla0", "dla1", "nvenc", "nvdec", "nvjpg", "ofa", "pva", "se", "vic",
];

impl TegraEngineSource {
    pub fn discover() -> Self {
        let clk_base = Path::new("/sys/kernel/debug/clk");
        let mut engines = Vec::new();

        if !clk_base.is_dir() {
            return Self { engines };
        }

        for &name in ENGINE_NAMES {
            let path = clk_base.join(name);
            if path.join("clk_rate").exists() {
                log::info!("Tegra engine: {name}");
                engines.push(TegraEngine {
                    name: name.to_uppercase(),
                    path,
                });
            }
        }

        Self { engines }
    }
}

impl crate::sensors::SensorSource for TegraEngineSource {
    fn name(&self) -> &str {
        "tegra-engine"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for engine in &self.engines {
            // Clock rate (Hz → MHz)
            if let Some(hz) = sysfs::read_u64_optional(&engine.path.join("clk_rate")) {
                let mhz = hz as f64 / 1_000_000.0;
                readings.push((
                    sid("tegra-engine", &engine.name, "frequency"),
                    SensorReading::new(
                        format!("{} Clock", engine.name),
                        mhz,
                        SensorUnit::Mhz,
                        SensorCategory::Frequency,
                    ),
                ));
            }

            // Enable count (1 = active)
            if let Some(count) = sysfs::read_u64_optional(&engine.path.join("clk_enable_count")) {
                let active = if count > 0 { 1.0 } else { 0.0 };
                readings.push((
                    sid("tegra-engine", &engine.name, "active"),
                    SensorReading::new(
                        format!("{} Active", engine.name),
                        active,
                        SensorUnit::Unitless,
                        SensorCategory::Other,
                    ),
                ));
            }
        }

        readings
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sid(source: &str, chip: &str, sensor: &str) -> SensorId {
    SensorId {
        source: source.into(),
        chip: chip.into(),
        sensor: sensor.into(),
    }
}
