use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};
use std::time::Instant;

pub struct RaplSource {
    domains: Vec<RaplDomain>,
}

struct RaplDomain {
    name: String,
    energy_file: CachedFile,
    max_energy: u64,
    prev_energy: u64,
    prev_time: Instant,
}

impl RaplSource {
    pub fn discover() -> Self {
        let mut domains = Vec::new();

        for dir in sysfs::glob_paths("/sys/class/powercap/intel-rapl:*") {
            // Skip sub-domains like intel-rapl:0:1 at top level; we enumerate them
            // separately via the glob which catches all levels.
            let name_path = dir.join("name");
            let name = match sysfs::read_string_optional(&name_path) {
                Some(n) => n,
                None => continue,
            };

            let energy_path = dir.join("energy_uj");
            let max_path = dir.join("max_energy_range_uj");

            let max_energy = match sysfs::read_u64_optional(&max_path) {
                Some(v) => v,
                None => continue,
            };

            let prev_energy = match sysfs::read_u64_optional(&energy_path) {
                Some(v) => v,
                None => continue,
            };

            let Some(energy_file) = CachedFile::open(&energy_path) else {
                continue;
            };

            domains.push(RaplDomain {
                name,
                energy_file,
                max_energy,
                prev_energy,
                prev_time: Instant::now(),
            });
        }

        Self { domains }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();
        let now = Instant::now();

        for domain in &mut self.domains {
            let Some(energy) = domain.energy_file.read_u64() else {
                continue;
            };

            let elapsed = now.duration_since(domain.prev_time);
            let elapsed_us = elapsed.as_micros() as f64;
            if elapsed_us <= 0.0 {
                domain.prev_energy = energy;
                domain.prev_time = now;
                continue;
            }

            // Handle counter wraparound
            let delta_uj = if energy >= domain.prev_energy {
                energy - domain.prev_energy
            } else {
                (domain.max_energy - domain.prev_energy) + energy
            };

            let watts = delta_uj as f64 / elapsed_us;

            let id = SensorId {
                source: "cpu".into(),
                chip: "rapl".into(),
                sensor: domain.name.clone(),
            };
            let label = format!("RAPL {}", domain.name);
            let reading =
                SensorReading::new(label, watts, SensorUnit::Watts, SensorCategory::Power);
            readings.push((id, reading));

            domain.prev_energy = energy;
            domain.prev_time = now;
        }

        readings
    }
}

impl crate::sensors::SensorSource for RaplSource {
    fn name(&self) -> &str {
        "rapl"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        RaplSource::poll(self)
    }
}
