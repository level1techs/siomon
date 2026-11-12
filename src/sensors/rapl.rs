use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};
use std::path::Path;
use std::time::Instant;

pub struct RaplSource {
    domains: Vec<RaplDomain>,
    limits: Vec<RaplLimit>,
}

/// A configured package power limit (`constraint_N_power_limit_uw`) or the
/// package thermal spec power (`constraint_0_max_power_uw`).
struct RaplLimit {
    id: SensorId,
    label: String,
    file: CachedFile,
}

struct RaplDomain {
    name: String,
    energy_file: CachedFile,
    max_energy: u64,
    prev_energy: u64,
    prev_time: Instant,
}

/// Map a powercap constraint name to Intel's power-limit name.
///
/// Only the MSR-backed `intel-rapl:*` zones are scanned, never the
/// `intel-rapl-mmio:*` zones. On Arrow Lake-S the MMIO zone's `peak_power`
/// (PL4) is a firmware copy that is not enforced, while the `intel-rapl:0`
/// zone has no PL4 constraint at all (the kernel does not manage PL4 there;
/// the enforced value is in MSR 0x601). A PL4 shown from here is whatever the
/// kernel reports for its zone, so treat it as the configured value.
fn limit_name(constraint: &str) -> Option<&'static str> {
    match constraint {
        "long_term" => Some("PL1"),
        "short_term" => Some("PL2"),
        "peak_power" => Some("PL4"),
        _ => None,
    }
}

/// Discover the configured power limits of a package domain.
fn discover_limits(dir: &Path, domain: &str, limits: &mut Vec<RaplLimit>) {
    for n in 0..3 {
        let Some(constraint) =
            sysfs::read_string_optional(&dir.join(format!("constraint_{n}_name")))
        else {
            continue;
        };
        let Some(pl) = limit_name(&constraint) else {
            continue;
        };
        // PL1's max_power is the package thermal spec power (base TDP).
        if pl == "PL1"
            && let Some(file) = CachedFile::open(dir.join(format!("constraint_{n}_max_power_uw")))
        {
            limits.push(RaplLimit {
                id: SensorId {
                    source: "cpu".into(),
                    chip: "rapl_limits".into(),
                    sensor: format!("{domain}-tdp"),
                },
                label: format!("RAPL {domain} Base TDP"),
                file,
            });
        }
        let Some(file) = CachedFile::open(dir.join(format!("constraint_{n}_power_limit_uw")))
        else {
            continue;
        };
        limits.push(RaplLimit {
            id: SensorId {
                source: "cpu".into(),
                chip: "rapl_limits".into(),
                sensor: format!("{domain}-{}", pl.to_lowercase()),
            },
            label: format!("RAPL {domain} {pl}"),
            file,
        });
    }
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
        let mut limits = Vec::new();

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

            if name.starts_with("package") {
                discover_limits(&dir, &name, &mut limits);
            }

            domains.push(RaplDomain {
                name,
                energy_file,
                max_energy,
                prev_energy,
                prev_time: Instant::now(),
            });
        }

        Self { domains, limits }
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

        for limit in &mut self.limits {
            // A zero limit means the constraint is not programmed.
            let Some(uw) = limit.file.read_u64().filter(|&uw| uw != 0) else {
                continue;
            };
            readings.push((
                limit.id.clone(),
                SensorReading::new(
                    limit.label.clone(),
                    uw as f64 / 1e6,
                    SensorUnit::Watts,
                    SensorCategory::Power,
                ),
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limit_names() {
        assert_eq!(limit_name("long_term"), Some("PL1"));
        assert_eq!(limit_name("short_term"), Some("PL2"));
        assert_eq!(limit_name("peak_power"), Some("PL4"));
        assert_eq!(limit_name("unknown"), None);
    }
}
