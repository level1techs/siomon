use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};
use std::path::Path;
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

fn parse_domain(dir: &Path) -> Option<String> {
    Some(
        dir.to_str()?
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>(),
    )
}

impl RaplSource {
    pub fn discover() -> Self {
        let mut domains = Vec::new();

        for dir in sysfs::glob_paths("/sys/class/powercap/intel-rapl:*") {
            // RAPL subdomains like intel-rapl:0:1 are included in the glob;
            // in case the name of the subdomain does not have a distinctive number like `dram-0` a number will get added based on the RAPL domain.
            let name_path = dir.join("name");
            let mut name = match sysfs::read_string_optional(&name_path) {
                Some(n) => n,
                None => continue,
            };

            if !name.chars().any(|c| c.is_ascii_digit())
                && let Some(rapl_domain) = parse_domain(&dir)
            {
                name = format!("{name}-{rapl_domain}");
            }

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
