//! AMD GPU sensor source using the ADL (AMD Display Library) on Windows.
//!
//! Dynamically loads `atiadlxx.dll` and reads temperature, fan speed, etc.
//! from each active AMD adapter via the Overdrive 6 API.
//! Gracefully returns no sensors if the DLL is absent (no AMD GPU / driver).

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::adl::AdlLibrary;

/// Discovered AMD GPU adapter.
struct AdlGpu {
    adapter_index: i32,
    name: String,
    /// Our local ordinal (0, 1, 2...) for SensorId chip naming.
    ordinal: u32,
}

pub struct AdlGpuSensorSource {
    lib: Option<AdlLibrary>,
    gpus: Vec<AdlGpu>,
}

impl AdlGpuSensorSource {
    /// Attempt to load ADL and enumerate active AMD adapters.
    ///
    /// Returns a source with zero GPUs if the DLL is not found or
    /// no active AMD adapters are present.
    pub fn discover() -> Self {
        let lib = match AdlLibrary::try_load() {
            Some(l) => l,
            None => {
                log::debug!("ADL: atiadlxx.dll not found or init failed, skipping AMD GPU sensors");
                return Self {
                    lib: None,
                    gpus: Vec::new(),
                };
            }
        };

        let count = lib.adapter_count().unwrap_or(0);
        if count <= 0 {
            log::info!("ADL: no adapters found");
            return Self {
                lib: Some(lib),
                gpus: Vec::new(),
            };
        }

        let infos = match lib.adapter_info(count) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("ADL: failed to get adapter info: {e}");
                return Self {
                    lib: Some(lib),
                    gpus: Vec::new(),
                };
            }
        };

        let mut gpus = Vec::new();
        let mut seen_bus = std::collections::HashSet::new();
        let mut ordinal = 0u32;

        for info in &infos {
            // ADL can report multiple entries per physical GPU (one per display output).
            // De-duplicate by PCI bus/device/function.
            let bus_key = (info.bus_number, info.device_number, info.function_number);
            if !seen_bus.insert(bus_key) {
                continue;
            }

            // Skip inactive adapters.
            if !lib.adapter_active(info.adapter_index).unwrap_or(false) {
                continue;
            }

            let name = {
                let raw = AdlLibrary::read_c_string_pub(&info.adapter_name);
                if raw.is_empty() {
                    format!("AMD GPU {ordinal}")
                } else {
                    raw
                }
            };

            log::info!(
                "ADL: found adapter {} \"{}\" on PCI {:02x}:{:02x}.{:x}",
                info.adapter_index,
                name,
                info.bus_number,
                info.device_number,
                info.function_number,
            );

            gpus.push(AdlGpu {
                adapter_index: info.adapter_index,
                name,
                ordinal,
            });
            ordinal += 1;
        }

        log::info!("ADL: {} active AMD GPU(s) discovered", gpus.len());

        Self {
            lib: Some(lib),
            gpus,
        }
    }
}

impl crate::sensors::SensorSource for AdlGpuSensorSource {
    fn name(&self) -> &str {
        "adl"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let lib = match self.lib.as_ref() {
            Some(l) => l,
            None => return Vec::new(),
        };

        let mut readings = Vec::new();

        for gpu in &self.gpus {
            let chip = format!("gpu{}", gpu.ordinal);

            // Temperature
            if let Some(temp_c) = lib.temperature_celsius(gpu.adapter_index) {
                let id = sid("adl", &chip, "temperature");
                let label = format!("{} Temperature", gpu.name);
                readings.push((
                    id,
                    SensorReading::new(
                        label,
                        temp_c,
                        SensorUnit::Celsius,
                        SensorCategory::Temperature,
                    ),
                ));
            }

            // Fan speed (percentage)
            if let Some(fan_pct) = lib.fan_speed_percent(gpu.adapter_index) {
                let id = sid("adl", &chip, "fan_speed");
                let label = format!("{} Fan", gpu.name);
                readings.push((
                    id,
                    SensorReading::new(label, fan_pct, SensorUnit::Percent, SensorCategory::Fan),
                ));
            }

            // Fan speed (RPM)
            if let Some(fan_rpm) = lib.fan_speed_rpm(gpu.adapter_index) {
                if fan_rpm > 0.0 {
                    let id = sid("adl", &chip, "fan_rpm");
                    let label = format!("{} Fan RPM", gpu.name);
                    readings.push((
                        id,
                        SensorReading::new(label, fan_rpm, SensorUnit::Rpm, SensorCategory::Fan),
                    ));
                }
            }
        }

        readings
    }
}

fn sid(source: &str, chip: &str, sensor: &str) -> SensorId {
    SensorId {
        source: source.into(),
        chip: chip.into(),
        sensor: sensor.into(),
    }
}
