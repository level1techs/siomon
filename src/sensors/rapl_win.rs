//! RAPL power metering on Windows via MSR reads (WinRing0).
//!
//! On Linux, RAPL energy counters are exposed via sysfs at
//! `/sys/class/powercap/intel-rapl:*/energy_uj`.  On Windows, the same data
//! lives in Model-Specific Registers (MSRs) that are readable through
//! WinRing0.  The MSR addresses are the same on Intel and AMD Zen processors.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::msr_win;
use std::time::Instant;

// ── MSR addresses ──────────────────────────────────────────────────────────

/// RAPL power-unit MSR: bits 12:8 encode the Energy Status Unit (ESU).
const MSR_RAPL_POWER_UNIT: u32 = 0x606;

/// Package-level energy counter (32-bit wraparound).
const MSR_PKG_ENERGY_STATUS: u32 = 0x611;

/// DRAM energy counter.
const MSR_DRAM_ENERGY_STATUS: u32 = 0x619;

/// PP0 (core) energy counter.
const MSR_PP0_ENERGY_STATUS: u32 = 0x639;

// ── Types ──────────────────────────────────────────────────────────────────

struct RaplDomain {
    name: String,
    msr_addr: u32,
    prev_energy: u64, // raw 32-bit counter value
}

pub struct RaplWinSource {
    /// Joules per raw counter unit, derived from MSR_RAPL_POWER_UNIT.
    energy_unit: f64,
    domains: Vec<RaplDomain>,
    last_time: Instant,
}

// ── Implementation ─────────────────────────────────────────────────────────

impl RaplWinSource {
    /// Probe WinRing0 for RAPL MSR accessibility and return a source.
    ///
    /// If WinRing0 is not loaded or the power-unit MSR is unreadable, the
    /// returned source will have zero domains and `poll()` will be a no-op.
    pub fn discover() -> Self {
        let mut domains = Vec::new();
        let mut energy_unit = 0.0;

        // Read power-unit MSR to derive the energy scaling factor.
        if let Some(power_unit_raw) = msr_win::read_msr(MSR_RAPL_POWER_UNIT) {
            // Energy Status Unit = bits 12:8.  Joules-per-unit = 1 / 2^ESU.
            let esu = ((power_unit_raw >> 8) & 0x1F) as u32;
            energy_unit = 1.0 / (1u64 << esu) as f64;

            let candidates: &[(&str, u32)] = &[
                ("package-0", MSR_PKG_ENERGY_STATUS),
                ("dram", MSR_DRAM_ENERGY_STATUS),
                ("core", MSR_PP0_ENERGY_STATUS),
            ];

            for &(name, msr_addr) in candidates {
                if let Some(raw) = msr_win::read_msr(msr_addr) {
                    let energy = raw & 0xFFFF_FFFF; // 32-bit counter
                    domains.push(RaplDomain {
                        name: name.to_string(),
                        msr_addr,
                        prev_energy: energy,
                    });
                    log::info!("RAPL: discovered domain '{}' (MSR {:#x})", name, msr_addr);
                }
            }
        }

        if domains.is_empty() {
            log::debug!(
                "RAPL: no domains discovered (WinRing0 not available or MSRs not readable)"
            );
        }

        Self {
            energy_unit,
            domains,
            last_time: Instant::now(),
        }
    }
}

impl crate::sensors::SensorSource for RaplWinSource {
    fn name(&self) -> &str {
        "rapl"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        if self.domains.is_empty() {
            return vec![];
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f64();
        self.last_time = now;

        // Guard against near-zero elapsed time (first poll, or clock glitch).
        if elapsed < 0.001 {
            return vec![];
        }

        let mut results = Vec::with_capacity(self.domains.len());

        for domain in &mut self.domains {
            if let Some(raw) = msr_win::read_msr(domain.msr_addr) {
                let energy = raw & 0xFFFF_FFFF; // 32-bit energy counter

                // Handle 32-bit counter wraparound.
                let delta = if energy >= domain.prev_energy {
                    energy - domain.prev_energy
                } else {
                    (0xFFFF_FFFF - domain.prev_energy) + energy + 1
                };
                domain.prev_energy = energy;

                let joules = delta as f64 * self.energy_unit;
                let watts = joules / elapsed;

                let id = SensorId {
                    source: "cpu".into(),
                    chip: "rapl".into(),
                    sensor: domain.name.clone(),
                };
                let label = format!("RAPL {}", domain.name);
                let reading =
                    SensorReading::new(label, watts, SensorUnit::Watts, SensorCategory::Power);
                results.push((id, reading));
            }
        }

        results
    }
}
