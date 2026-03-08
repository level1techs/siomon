use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs;
use std::path::PathBuf;

pub struct GpuSensorSource {
    nvidia: NvidiaState,
    amd_gpus: Vec<AmdGpu>,
}

enum NvidiaState {
    Unavailable,
    #[cfg(feature = "nvidia")]
    Active {
        lib: crate::platform::nvml::NvmlLibrary,
        gpus: Vec<NvidiaGpu>,
    },
}

struct NvidiaGpu {
    index: u32,
    name: String,
}

struct AmdGpu {
    index: u32,
    name: String,
    hwmon_path: PathBuf,
    busy_path: Option<PathBuf>,
    vram_used_path: Option<PathBuf>,
    #[allow(dead_code)]
    vram_total: Option<u64>,
}

impl GpuSensorSource {
    pub fn discover(no_nvidia: bool) -> Self {
        let nvidia = if no_nvidia {
            NvidiaState::Unavailable
        } else {
            discover_nvidia()
        };
        let amd_gpus = discover_amd();

        Self { nvidia, amd_gpus }
    }

    pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        #[cfg(feature = "nvidia")]
        if let NvidiaState::Active { ref lib, ref gpus } = self.nvidia {
            poll_nvidia(lib, gpus, &mut readings);
        }

        poll_amd(&self.amd_gpus, &mut readings);

        readings
    }
}

fn discover_nvidia() -> NvidiaState {
    #[cfg(feature = "nvidia")]
    {
        let lib = match crate::platform::nvml::NvmlLibrary::try_load() {
            Some(l) => l,
            None => return NvidiaState::Unavailable,
        };
        let count = lib.device_count().unwrap_or(0);
        let mut gpus = Vec::new();
        for i in 0..count {
            let name = lib.device_name(i).unwrap_or_else(|_| format!("GPU {i}"));
            gpus.push(NvidiaGpu { index: i, name });
        }
        NvidiaState::Active { lib, gpus }
    }
    #[cfg(not(feature = "nvidia"))]
    {
        NvidiaState::Unavailable
    }
}

fn discover_amd() -> Vec<AmdGpu> {
    let mut gpus = Vec::new();
    let mut idx = 0u32;

    for card_path in sysfs::glob_paths("/sys/class/drm/card[0-9]*") {
        let card_name = match card_path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.chars().skip(4).all(|c| c.is_ascii_digit()) => n,
            _ => continue,
        };

        let device_path = card_path.join("device");
        let vendor = sysfs::read_u64_optional(&device_path.join("vendor")).unwrap_or(0) as u16;
        if vendor != 0x1002 {
            continue;
        }

        let hwmon_pattern = format!("{}/hwmon/hwmon*", device_path.display());
        let hwmon_path = match sysfs::glob_paths(&hwmon_pattern).into_iter().next() {
            Some(p) => p,
            None => continue,
        };

        let name = sysfs::read_string_optional(&device_path.join("product_name"))
            .unwrap_or_else(|| format!("AMD GPU {card_name}"));

        let busy_path = {
            let p = device_path.join("gpu_busy_percent");
            if p.exists() { Some(p) } else { None }
        };
        let vram_used_path = {
            let p = device_path.join("mem_info_vram_used");
            if p.exists() { Some(p) } else { None }
        };
        let vram_total = sysfs::read_u64_optional(&device_path.join("mem_info_vram_total"));

        gpus.push(AmdGpu {
            index: idx,
            name,
            hwmon_path,
            busy_path,
            vram_used_path,
            vram_total,
        });
        idx += 1;
    }
    gpus
}

#[cfg(feature = "nvidia")]
fn poll_nvidia(
    lib: &crate::platform::nvml::NvmlLibrary,
    gpus: &[NvidiaGpu],
    readings: &mut Vec<(SensorId, SensorReading)>,
) {
    for gpu in gpus {
        let chip = format!("gpu{}", gpu.index);

        if let Ok(temp) = lib.device_temperature(gpu.index) {
            let id = sid("nvml", &chip, "temperature");
            let label = format!("{} Temperature", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    temp as f64,
                    SensorUnit::Celsius,
                    SensorCategory::Temperature,
                ),
            ));
        }

        if let Ok(fan) = lib.device_fan_speed(gpu.index) {
            let id = sid("nvml", &chip, "fan_speed");
            let label = format!("{} Fan", gpu.name);
            readings.push((
                id,
                SensorReading::new(label, fan as f64, SensorUnit::Percent, SensorCategory::Fan),
            ));
        }

        if let Ok(watts) = lib.device_power_watts(gpu.index) {
            let id = sid("nvml", &chip, "power");
            let label = format!("{} Power", gpu.name);
            readings.push((
                id,
                SensorReading::new(label, watts, SensorUnit::Watts, SensorCategory::Power),
            ));
        }

        if let Ok(mhz) = lib.device_clock_mhz(gpu.index, crate::platform::nvml::NVML_CLOCK_GRAPHICS)
        {
            let id = sid("nvml", &chip, "core_clock");
            let label = format!("{} Core Clock", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    mhz as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ),
            ));
        }

        if let Ok(mhz) = lib.device_clock_mhz(gpu.index, crate::platform::nvml::NVML_CLOCK_MEM) {
            let id = sid("nvml", &chip, "mem_clock");
            let label = format!("{} Memory Clock", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    mhz as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ),
            ));
        }

        if let Ok(util) = lib.device_utilization(gpu.index) {
            let id = sid("nvml", &chip, "gpu_util");
            let label = format!("{} GPU Utilization", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    util.gpu as f64,
                    SensorUnit::Percent,
                    SensorCategory::Utilization,
                ),
            ));

            let id = sid("nvml", &chip, "mem_util");
            let label = format!("{} Memory Utilization", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    util.memory as f64,
                    SensorUnit::Percent,
                    SensorCategory::Utilization,
                ),
            ));
        }

        if let Ok(mem) = lib.device_memory_info(gpu.index) {
            let id = sid("nvml", &chip, "vram_used");
            let label = format!("{} VRAM Used", gpu.name);
            let mb = mem.used as f64 / (1024.0 * 1024.0);
            readings.push((
                id,
                SensorReading::new(label, mb, SensorUnit::Megabytes, SensorCategory::Memory),
            ));
        }
    }
}

fn poll_amd(gpus: &[AmdGpu], readings: &mut Vec<(SensorId, SensorReading)>) {
    for gpu in gpus {
        let chip = format!("amdgpu{}", gpu.index);

        if let Some(raw) = sysfs::read_u64_optional(&gpu.hwmon_path.join("temp1_input")) {
            let id = sid("amdgpu", &chip, "temperature");
            let label = format!("{} Temperature", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    raw as f64 / 1000.0,
                    SensorUnit::Celsius,
                    SensorCategory::Temperature,
                ),
            ));
        }

        if let Some(rpm) = sysfs::read_u64_optional(&gpu.hwmon_path.join("fan1_input")) {
            let id = sid("amdgpu", &chip, "fan");
            let label = format!("{} Fan", gpu.name);
            readings.push((
                id,
                SensorReading::new(label, rpm as f64, SensorUnit::Rpm, SensorCategory::Fan),
            ));
        }

        if let Some(uw) = sysfs::read_u64_optional(&gpu.hwmon_path.join("power1_average")) {
            let id = sid("amdgpu", &chip, "power");
            let label = format!("{} Power", gpu.name);
            readings.push((
                id,
                SensorReading::new(
                    label,
                    uw as f64 / 1_000_000.0,
                    SensorUnit::Watts,
                    SensorCategory::Power,
                ),
            ));
        }

        if let Some(ref path) = gpu.busy_path {
            if let Some(pct) = sysfs::read_u64_optional(path) {
                let id = sid("amdgpu", &chip, "gpu_util");
                let label = format!("{} GPU Utilization", gpu.name);
                readings.push((
                    id,
                    SensorReading::new(
                        label,
                        pct as f64,
                        SensorUnit::Percent,
                        SensorCategory::Utilization,
                    ),
                ));
            }
        }

        if let Some(ref path) = gpu.vram_used_path {
            if let Some(used) = sysfs::read_u64_optional(path) {
                let id = sid("amdgpu", &chip, "vram_used");
                let label = format!("{} VRAM Used", gpu.name);
                let mb = used as f64 / (1024.0 * 1024.0);
                readings.push((
                    id,
                    SensorReading::new(label, mb, SensorUnit::Megabytes, SensorCategory::Memory),
                ));
            }
        }
    }
}

fn sid(source: &str, chip: &str, sensor: &str) -> SensorId {
    SensorId {
        source: source.into(),
        chip: chip.into(),
        sensor: sensor.into(),
    }
}
