//! Intel per-core and package telemetry from read-only MSRs.
//!
//! Adds what hwmon and cpufreq do not expose: the effective clock (average
//! including idle), the active clock and active time (while not idle), the
//! core ratio, the requested core and uncore VID, thermal headroom to the
//! throttle temperature, whether and why cores are throttling, and the PL4
//! (peak power) limit. Temperatures themselves are left to `coretemp`.
//!
//! Throttling comes from the architectural live status bits of
//! `IA32_THERM_STATUS`: thermal (at the throttle temperature, PROCHOT or
//! critical), power limit (running below the requested clock because of a
//! power limit) and current limit. Sticky "since boot" log bits are not
//! reported.
//!
//! All reads go through the allowlisted, read-only
//! [`MsrDevice`](crate::platform::msr::MsrDevice). Reading `/dev/cpu/N/msr`
//! runs RDMSR on CPU N through an IPI, which wakes that CPU: instantaneous
//! fields (VID, ratio) of an idle core describe the operating point it wakes
//! into, and the wakeups add a little active time on an idle system. To keep
//! this small, each poll reads four MSRs per CPU (APERF, MPERF, PERF_STATUS,
//! THERM_STATUS) plus a few per package; configuration registers are read
//! once at startup.
//!
//! VID values are the CPU's *requested* voltage (SVID request), not a
//! measured rail. Board-sensed Vcore comes from the Super I/O via hwmon.
//!
//! Needs the `msr` module and root or `CAP_SYS_RAWIO`; without them the
//! source is silently empty.

use crate::model::sensor::{SensorId, SensorReading};

#[cfg(target_arch = "x86_64")]
mod inner {
    use std::path::Path;
    use std::time::Instant;

    use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
    use crate::platform::msr::{self, MsrDevice, bits};
    use crate::platform::sysfs;

    /// Model-specific MSR fields that the SDM marks reserved, enabled only on
    /// parts where they have been verified (same set as the upstream
    /// turbostat CoreVID/UncVID proposal).
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(super) struct ModelFeatures {
        /// `IA32_PERF_STATUS[47:32]` is the requested core VID (1/8192 V).
        pub core_vid: bool,
        /// `MSR_UNCORE_PERF_STATUS[31:16]` is the requested uncore VID.
        pub uncore_vid: bool,
        /// `MSR 0x601[14:0]` is PL4 in watts (RAPL power unit from
        /// `MSR 0x606`). Measured on Arrow Lake-S only; the register is a
        /// current limit in some older SDM tables, so other parts stay off.
        pub pl4: bool,
    }

    pub(super) fn model_features(family: u32, model: u32) -> ModelFeatures {
        if family != 6 {
            return ModelFeatures::default();
        }
        match model {
            // Raptor Lake, Raptor Lake-P, Raptor Lake-S
            0xB7 | 0xBA | 0xBF => ModelFeatures {
                core_vid: true,
                uncore_vid: false,
                pl4: false,
            },
            // Arrow Lake-S
            0xC6 => ModelFeatures {
                core_vid: true,
                uncore_vid: true,
                pl4: true,
            },
            _ => ModelFeatures::default(),
        }
    }

    // ── Pure decoders ───────────────────────────────────────────────────

    /// Requested core VID from `IA32_PERF_STATUS`, or `None` when zero.
    pub(super) fn core_vid_volts(perf_status: u64) -> Option<f64> {
        vid_volts(bits(perf_status, 47, 32))
    }

    /// Current core ratio from `IA32_PERF_STATUS[15:8]`, or `None` when zero.
    pub(super) fn core_ratio(perf_status: u64) -> Option<u64> {
        Some(bits(perf_status, 15, 8)).filter(|&r| r != 0)
    }

    /// Requested uncore VID from `MSR_UNCORE_PERF_STATUS[31:16]`.
    pub(super) fn uncore_vid_volts(uncore_status: u64) -> Option<f64> {
        vid_volts(bits(uncore_status, 31, 16))
    }

    fn vid_volts(code: u64) -> Option<f64> {
        (code != 0).then(|| code as f64 / 8192.0)
    }

    /// TjMAX in °C from `MSR_TEMPERATURE_TARGET[23:16]`.
    pub(super) fn tjmax(temp_target: u64) -> Option<u64> {
        Some(bits(temp_target, 23, 16)).filter(|&t| t != 0)
    }

    /// TCC activation offset in °C from `MSR_TEMPERATURE_TARGET[29:24]`:
    /// throttling starts this many degrees below TjMAX. The field width
    /// differs between public sources (6 or 7 bits, different top bit);
    /// identical for offsets below 64 C.
    pub(super) fn tcc_offset(temp_target: u64) -> u64 {
        bits(temp_target, 29, 24)
    }

    /// Degrees below TjMAX from a core thermal status word, if the reading
    /// is valid (bit 31).
    pub(super) fn distance_to_tjmax(therm_status: u64) -> Option<u64> {
        (bits(therm_status, 31, 31) == 1).then(|| bits(therm_status, 22, 16))
    }

    /// Degrees below TjMAX from `IA32_PACKAGE_THERM_STATUS[22:16]`. Unlike
    /// the per-core register, bit 31 is reserved here (no valid bit), so the
    /// readout is taken as is, as coretemp does.
    pub(super) fn package_distance_to_tjmax(pkg_therm_status: u64) -> u64 {
        bits(pkg_therm_status, 22, 16)
    }

    /// Degrees left before throttling: the distance to TjMAX minus the TCC
    /// offset. Negative once the temperature is past the throttle point.
    pub(super) fn headroom(distance_to_tjmax: u64, tcc_offset: u64) -> f64 {
        distance_to_tjmax as f64 - tcc_offset as f64
    }

    /// Live throttling reasons from the architectural status bits of
    /// `IA32_THERM_STATUS`.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(super) struct Throttle {
        /// Thermal status, PROCHOT/FORCEPR event or critical temperature
        /// (bits 0, 2, 4).
        pub thermal: bool,
        /// Running below the requested clock because of a power limit
        /// (bit 10).
        pub power: bool,
        /// Current-limit status (bit 12). Besides an electrical current
        /// limit, on Arrow Lake-S this bit was also seen set on all CPUs
        /// while the PL4 predicted-peak limit was binding (all-core 256-bit
        /// AVX2 load), so it does not by itself mean an IccMax limit.
        pub current: bool,
    }

    impl Throttle {
        pub fn from_status(therm_status: u64) -> Self {
            let bit = |n: u32| bits(therm_status, n, n) == 1;
            Self {
                thermal: bit(0) || bit(2) || bit(4),
                power: bit(10),
                current: bit(12),
            }
        }

        pub fn any(self) -> bool {
            self.thermal || self.power || self.current
        }
    }

    /// PL4 in watts from `MSR 0x601[14:0]` scaled by the RAPL power unit
    /// (`MSR 0x606[3:0]`, 1/2^n W), plus the lock bit (31). `None` when the
    /// field is 0 or the unit word is unavailable. Public sources disagree on
    /// the field width (13, 15 or 16 bits); all agree below 1024 W.
    pub(super) fn pl4_watts(pl4_word: u64, unit_word: Option<u64>) -> Option<(f64, bool)> {
        let raw = bits(pl4_word, 14, 0);
        let n = bits(unit_word?, 3, 0);
        (raw != 0).then(|| {
            (
                raw as f64 / f64::from(1u32 << n),
                bits(pl4_word, 31, 31) == 1,
            )
        })
    }

    /// APERF/MPERF/TSC sample of one logical CPU.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct Counters {
        /// Only read when the MPERF base clock is unknown.
        pub tsc: Option<u64>,
        pub aperf: u64,
        pub mperf: u64,
        pub at: Instant,
    }

    /// Interval averages derived from two counter samples.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(super) struct ClockStats {
        /// Average clock including idle time (ΔAPERF / Δt).
        pub effective_mhz: f64,
        /// Share of the interval spent active, i.e. in C0 (ΔMPERF over the
        /// MPERF ticks of the interval).
        pub active_pct: f64,
        /// Average clock while active (MPERF rate × ΔAPERF / ΔMPERF).
        pub active_mhz: Option<f64>,
    }

    /// Derive interval clocks. `mperf_mhz` is the rate MPERF ticks at in
    /// C0, the base (P1) clock from CPUID leaf 0x16. It can differ from the
    /// TSC rate (Arrow Lake: 3700 vs 3686.4 MHz), so the TSC rate is only a
    /// fallback. Returns `None` when a counter went backwards (CPU hotplug,
    /// resume) or no time elapsed.
    pub(super) fn clock_stats(
        prev: &Counters,
        cur: &Counters,
        mperf_mhz: Option<f64>,
    ) -> Option<ClockStats> {
        let secs = cur.at.checked_duration_since(prev.at)?.as_secs_f64();
        let d_aperf = cur.aperf.checked_sub(prev.aperf)?;
        let d_mperf = cur.mperf.checked_sub(prev.mperf)?;
        if secs <= 0.0 {
            return None;
        }
        let ref_mhz = match mperf_mhz {
            Some(mhz) => mhz,
            None => {
                let d_tsc = cur.tsc?.checked_sub(prev.tsc?)?;
                if d_tsc == 0 {
                    return None;
                }
                d_tsc as f64 / secs / 1e6
            }
        };
        let ref_ticks = ref_mhz * 1e6 * secs;
        Some(ClockStats {
            effective_mhz: d_aperf as f64 / secs / 1e6,
            active_pct: (d_mperf as f64 / ref_ticks * 100.0).min(100.0),
            active_mhz: (d_mperf > 0).then(|| ref_mhz * d_aperf as f64 / d_mperf as f64),
        })
    }

    /// Base (MPERF) clock in MHz from CPUID leaf 0x16, if reported.
    fn mperf_mhz() -> Option<f64> {
        raw_cpuid::CpuId::new()
            .get_processor_frequency_info()
            .map(|f| f.processor_base_frequency())
            .filter(|&mhz| mhz != 0)
            .map(f64::from)
    }

    // ── Source ──────────────────────────────────────────────────────────

    /// Hybrid core type of a logical CPU, used for labels.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum CoreKind {
        Performance,
        Efficient,
        Uniform,
    }

    pub(super) fn core_label(kind: CoreKind, cpu: u32) -> String {
        match kind {
            CoreKind::Performance => format!("P-Core {cpu}"),
            CoreKind::Efficient => format!("E-Core {cpu}"),
            CoreKind::Uniform => format!("Core {cpu}"),
        }
    }

    struct CpuEntry {
        cpu: u32,
        /// Index into `IntelMsrSource::packages`.
        package: usize,
        prefix: String,
        dev: MsrDevice,
        prev: Option<Counters>,
    }

    struct PackageEntry {
        id: u32,
        /// Index into `IntelMsrSource::cpus` of the CPU used for package reads.
        cpu_index: usize,
        /// TCC offset in °C, read once.
        tcc_offset: u64,
        /// Throttle temperature (TjMAX − TCC offset) in °C, read once.
        throttle_temp: Option<f64>,
    }

    pub struct IntelMsrSource {
        cpus: Vec<CpuEntry>,
        packages: Vec<PackageEntry>,
        features: ModelFeatures,
        aperfmperf: bool,
        mperf_mhz: Option<f64>,
    }

    impl IntelMsrSource {
        pub fn discover() -> Self {
            let empty = Self {
                cpus: Vec::new(),
                packages: Vec::new(),
                features: ModelFeatures::default(),
                aperfmperf: false,
                mperf_mhz: None,
            };

            let cpuinfo = crate::platform::procfs::parse_cpuinfo();
            let Some(first) = cpuinfo.first() else {
                return empty;
            };
            if first.get("vendor_id").map(String::as_str) != Some("GenuineIntel") {
                return empty;
            }
            let family = first
                .get("cpu family")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let model = first.get("model").and_then(|s| s.parse().ok()).unwrap_or(0);
            let flags: Vec<&str> = first
                .get("flags")
                .map(|f| f.split_whitespace().collect())
                .unwrap_or_default();
            let aperfmperf = flags.contains(&"aperfmperf");
            let mperf_mhz = mperf_mhz();

            let online = sysfs::read_string_optional(Path::new("/sys/devices/system/cpu/online"))
                .map(|s| sysfs::parse_cpulist(&s))
                .unwrap_or_default();
            let p_cores = read_cpulist("/sys/devices/cpu_core/cpus");
            let e_cores = read_cpulist("/sys/devices/cpu_atom/cpus");

            let mut cpus: Vec<CpuEntry> = Vec::new();
            let mut packages: Vec<PackageEntry> = Vec::new();
            for cpu in online {
                let Some(dev) = MsrDevice::open(cpu) else {
                    continue;
                };
                let kind = if p_cores.contains(&cpu) {
                    CoreKind::Performance
                } else if e_cores.contains(&cpu) {
                    CoreKind::Efficient
                } else {
                    CoreKind::Uniform
                };
                let pkg_id = sysfs::read_u32_optional(Path::new(&format!(
                    "/sys/devices/system/cpu/cpu{cpu}/topology/physical_package_id"
                )))
                .unwrap_or(0);
                let package = match packages.iter().position(|p| p.id == pkg_id) {
                    Some(i) => i,
                    None => {
                        let target = dev.read(msr::MSR_TEMPERATURE_TARGET);
                        let tcc = target.map(tcc_offset).unwrap_or(0);
                        packages.push(PackageEntry {
                            id: pkg_id,
                            cpu_index: cpus.len(),
                            tcc_offset: tcc,
                            throttle_temp: target
                                .and_then(tjmax)
                                .map(|tj| tj.saturating_sub(tcc) as f64),
                        });
                        packages.len() - 1
                    }
                };
                let mut entry = CpuEntry {
                    cpu,
                    package,
                    prefix: core_label(kind, cpu),
                    dev,
                    prev: None,
                };
                // Baseline now so the first poll already has a delta.
                if aperfmperf {
                    entry.prev = read_counters(&entry.dev, mperf_mhz.is_none());
                }
                cpus.push(entry);
            }

            if cpus.is_empty() {
                log::debug!("Intel MSR: /dev/cpu/*/msr not readable");
                return empty;
            }
            let features = model_features(family, model);
            log::info!(
                "Intel MSR: {} CPUs, {} packages, model {:#x} (core VID: {}, uncore VID: {})",
                cpus.len(),
                packages.len(),
                model,
                features.core_vid,
                features.uncore_vid
            );
            Self {
                cpus,
                packages,
                features,
                aperfmperf,
                mperf_mhz,
            }
        }

        pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            let mut readings = Vec::new();
            // Per package: cores throttling for (thermal, power, current).
            let mut counts = vec![(0u32, 0u32, 0u32); self.packages.len()];
            for entry in &mut self.cpus {
                let pkg = &self.packages[entry.package];
                let throttle = poll_cpu(
                    entry,
                    self.features,
                    self.aperfmperf.then_some(self.mperf_mhz),
                    pkg.tcc_offset,
                    &mut readings,
                );
                if let Some(t) = throttle {
                    let c = &mut counts[entry.package];
                    c.0 += u32::from(t.thermal);
                    c.1 += u32::from(t.power);
                    c.2 += u32::from(t.current);
                }
            }
            for (pkg, count) in self.packages.iter().zip(counts) {
                let dev = &self.cpus[pkg.cpu_index].dev;
                poll_package(pkg, dev, self.features, count, &mut readings);
            }
            readings
        }
    }

    fn read_cpulist(path: &str) -> Vec<u32> {
        sysfs::read_string_optional(Path::new(path))
            .map(|s| sysfs::parse_cpulist(&s))
            .unwrap_or_default()
    }

    fn read_counters(dev: &MsrDevice, with_tsc: bool) -> Option<Counters> {
        let at = Instant::now();
        let tsc = if with_tsc {
            Some(dev.read(msr::IA32_TIME_STAMP_COUNTER)?)
        } else {
            None
        };
        Some(Counters {
            tsc,
            aperf: dev.read(msr::IA32_APERF)?,
            mperf: dev.read(msr::IA32_MPERF)?,
            at,
        })
    }

    /// Poll one CPU. `aperfmperf` is `None` when the CPU lacks APERF/MPERF,
    /// otherwise the MPERF base clock if known. Returns the CPU's live
    /// throttling state, if its thermal status could be read.
    fn poll_cpu(
        entry: &mut CpuEntry,
        features: ModelFeatures,
        aperfmperf: Option<Option<f64>>,
        tcc_offset: u64,
        out: &mut Vec<(SensorId, SensorReading)>,
    ) -> Option<Throttle> {
        let cpu = entry.cpu;
        let prefix = &entry.prefix;

        if let Some(mperf_mhz) = aperfmperf
            && let Some(cur) = read_counters(&entry.dev, mperf_mhz.is_none())
        {
            if let Some(stats) = entry
                .prev
                .as_ref()
                .and_then(|p| clock_stats(p, &cur, mperf_mhz))
            {
                out.push(reading(
                    "eff_clock",
                    cpu,
                    format!("{prefix} Effective Clock"),
                    stats.effective_mhz,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ));
                out.push(reading(
                    "active",
                    cpu,
                    format!("{prefix} Active Time"),
                    stats.active_pct,
                    SensorUnit::Percent,
                    SensorCategory::Utilization,
                ));
                if let Some(mhz) = stats.active_mhz {
                    out.push(reading(
                        "active_clock",
                        cpu,
                        format!("{prefix} Active Clock"),
                        mhz,
                        SensorUnit::Mhz,
                        SensorCategory::Frequency,
                    ));
                }
            }
            entry.prev = Some(cur);
        }

        if let Some(perf_status) = entry.dev.read(msr::IA32_PERF_STATUS) {
            if let Some(ratio) = core_ratio(perf_status) {
                out.push(reading(
                    "ratio",
                    cpu,
                    format!("{prefix} Ratio"),
                    ratio as f64,
                    SensorUnit::Unitless,
                    SensorCategory::Other,
                ));
            }
            if features.core_vid
                && let Some(volts) = core_vid_volts(perf_status)
            {
                out.push(reading(
                    "vid",
                    cpu,
                    format!("{prefix} VID"),
                    volts,
                    SensorUnit::Volts,
                    SensorCategory::Voltage,
                ));
            }
        }

        let status = entry.dev.read(msr::IA32_THERM_STATUS)?;
        if let Some(distance) = distance_to_tjmax(status) {
            out.push(reading(
                "headroom",
                cpu,
                format!("{prefix} Thermal Headroom"),
                headroom(distance, tcc_offset),
                SensorUnit::Celsius,
                SensorCategory::Other,
            ));
        }
        let throttle = Throttle::from_status(status);
        out.push(reading(
            "throttling",
            cpu,
            format!("{prefix} Throttling"),
            f64::from(u8::from(throttle.any())),
            SensorUnit::Unitless,
            SensorCategory::Other,
        ));
        Some(throttle)
    }

    fn poll_package(
        pkg: &PackageEntry,
        dev: &MsrDevice,
        features: ModelFeatures,
        (thermal, power, current): (u32, u32, u32),
        out: &mut Vec<(SensorId, SensorReading)>,
    ) {
        let chip = format!("package{}", pkg.id);
        let mut push = |sensor: &str, label: &str, value: f64, unit, category| {
            out.push((
                SensorId {
                    source: "cpu".into(),
                    chip: chip.clone(),
                    sensor: sensor.into(),
                },
                SensorReading::new(label.into(), value, unit, category),
            ));
        };

        if let Some(t) = pkg.throttle_temp {
            push(
                "throttle_temp",
                "Throttle Temperature",
                t,
                SensorUnit::Celsius,
                SensorCategory::Other,
            );
        }
        if features.pl4
            && let Some((watts, locked)) = dev
                .read(msr::MSR_PL4_POWER_LIMIT)
                .and_then(|w| pl4_watts(w, dev.read(msr::MSR_RAPL_POWER_UNIT)))
        {
            push(
                "pl4",
                if locked {
                    "PL4 (MSR 0x601, locked)"
                } else {
                    "PL4 (MSR 0x601)"
                },
                watts,
                SensorUnit::Watts,
                SensorCategory::Power,
            );
        }

        if features.uncore_vid
            && let Some(volts) = dev
                .read(msr::MSR_UNCORE_PERF_STATUS)
                .and_then(uncore_vid_volts)
        {
            push(
                "uncore_vid",
                "Uncore VID",
                volts,
                SensorUnit::Volts,
                SensorCategory::Voltage,
            );
        }

        if let Some(status) = dev.read(msr::IA32_PACKAGE_THERM_STATUS) {
            push(
                "headroom",
                "Package Thermal Headroom",
                headroom(package_distance_to_tjmax(status), pkg.tcc_offset),
                SensorUnit::Celsius,
                SensorCategory::Other,
            );
        }

        for (sensor, label, cores) in [
            ("throttle_thermal", "Cores Thermal Throttling", thermal),
            ("throttle_power", "Cores Power Limited", power),
            ("throttle_current", "Cores Current Limited", current),
        ] {
            push(
                sensor,
                label,
                f64::from(cores),
                SensorUnit::Unitless,
                SensorCategory::Other,
            );
        }
    }

    fn reading(
        chip: &str,
        cpu: u32,
        label: String,
        value: f64,
        unit: SensorUnit,
        category: SensorCategory,
    ) -> (SensorId, SensorReading) {
        (
            SensorId {
                source: "cpu".into(),
                chip: chip.into(),
                sensor: format!("cpu{cpu}"),
            },
            SensorReading::new(label, value, unit, category),
        )
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod inner {
    use crate::model::sensor::{SensorId, SensorReading};

    pub struct IntelMsrSource;

    impl IntelMsrSource {
        pub fn discover() -> Self {
            Self
        }

        pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            Vec::new()
        }
    }
}

pub use inner::IntelMsrSource;

impl crate::sensors::SensorSource for IntelMsrSource {
    fn name(&self) -> &str {
        "intel_msr"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        IntelMsrSource::poll(self)
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::inner::*;
    use std::time::{Duration, Instant};

    // Raw words captured on a Core Ultra 9 285K (Arrow Lake-S, model 0xC6).

    #[test]
    fn test_core_vid_and_ratio_pcore_idle() {
        let raw = 0x0000_1605_0000_0800;
        assert!((core_vid_volts(raw).unwrap() - 0.688).abs() < 0.0005);
        assert_eq!(core_ratio(raw), Some(8));
    }

    #[test]
    fn test_core_vid_and_ratio_ecore() {
        let raw = 0x0000_23EC_0000_3000;
        assert!((core_vid_volts(raw).unwrap() - 1.1226).abs() < 0.0005);
        assert_eq!(core_ratio(raw), Some(48));
    }

    #[test]
    fn test_zero_vid_is_absent_not_zero_volts() {
        assert_eq!(core_vid_volts(0x0000_0000_0000_2A00), None);
        assert_eq!(core_ratio(0), None);
    }

    #[test]
    fn test_uncore_vid() {
        assert!((uncore_vid_volts(0x26EB_0028).unwrap() - 1.2162).abs() < 0.0005);
    }

    #[test]
    fn test_tjmax_and_tcc_offset() {
        assert_eq!(tjmax(0x8069_1400), Some(105));
        assert_eq!(tcc_offset(0x8069_1400), 0);
        assert_eq!(tcc_offset(0x8A69_1400), 10);
        assert_eq!(tjmax(0), None);
    }

    #[test]
    fn test_distance_to_tjmax_requires_valid_bit() {
        // Core 0: readout 0x47 = 71 below TjMAX 105 -> 34 °C
        assert_eq!(distance_to_tjmax(0x8847_2000), Some(71));
        assert_eq!(distance_to_tjmax(0x0847_2000), None);
    }

    #[test]
    fn test_package_distance_has_no_valid_bit() {
        // IA32_PACKAGE_THERM_STATUS recorded at idle: readout 0x42 = 66
        assert_eq!(package_distance_to_tjmax(0x8842_0000), 66);
        assert_eq!(package_distance_to_tjmax(0x0842_0000), 66);
    }

    #[test]
    fn test_headroom_accounts_for_tcc_offset() {
        assert_eq!(headroom(71, 0), 71.0);
        assert_eq!(headroom(71, 10), 61.0);
        // Past the throttle point
        assert_eq!(headroom(4, 10), -6.0);
    }

    #[test]
    fn test_throttle_from_live_bits_only() {
        // Recorded at idle: only the current-limit *log* bit (13) is set
        let idle = Throttle::from_status(0x8847_2000);
        assert!(!idle.any());
        assert!(Throttle::from_status(0x1).thermal);
        assert!(Throttle::from_status(0x4).thermal); // PROCHOT
        assert!(Throttle::from_status(0x10).thermal); // critical
        assert!(Throttle::from_status(0x400).power);
        assert!(Throttle::from_status(0x1000).current);
        // Log bits alone (1, 3, 5, 11, 13) are not live throttling
        assert!(!Throttle::from_status(0x282A).any());
    }

    #[test]
    fn test_pl4_watts() {
        // 3400 * 0.125 W = 425 W, unlocked (unit word from the reference CPU)
        assert_eq!(pl4_watts(0xD48, Some(0xA0E03)), Some((425.0, false)));
        assert_eq!(pl4_watts(0x8000_0D48, Some(0xA0E03)), Some((425.0, true)));
        assert_eq!(pl4_watts(0, Some(0xA0E03)), None);
        assert_eq!(pl4_watts(0xD48, None), None);
    }

    #[test]
    fn test_model_features_gate() {
        let arl = model_features(6, 0xC6);
        assert!(arl.core_vid && arl.uncore_vid && arl.pl4);
        let rpl = model_features(6, 0xB7);
        assert!(rpl.core_vid && !rpl.uncore_vid && !rpl.pl4);
        // Arrow Lake-H/U and unknown models stay off until verified
        assert_eq!(model_features(6, 0xC5), ModelFeatures::default());
        assert_eq!(model_features(0x19, 0xC6), ModelFeatures::default());
    }

    fn counters(tsc: Option<u64>, aperf: u64, mperf: u64, at: Instant) -> Counters {
        Counters {
            tsc,
            aperf,
            mperf,
            at,
        }
    }

    #[test]
    fn test_clock_stats() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let prev = counters(Some(0), 0, 0, t0);
        // MPERF at a 3700 MHz base, TSC at 3686.4 MHz (Arrow Lake);
        // 25% active at 5000 MHz
        let cur = counters(Some(3_686_400_000), 1_250_000_000, 925_000_000, t1);
        let s = clock_stats(&prev, &cur, Some(3700.0)).unwrap();
        assert!((s.effective_mhz - 1250.0).abs() < 0.01);
        assert!((s.active_pct - 25.0).abs() < 0.01);
        assert!((s.active_mhz.unwrap() - 5000.0).abs() < 0.01);

        // Fully active: active clock equals effective clock
        let busy = counters(Some(3_686_400_000), 5_500_000_000, 3_700_000_000, t1);
        let s = clock_stats(&prev, &busy, Some(3700.0)).unwrap();
        assert!((s.active_mhz.unwrap() - s.effective_mhz).abs() < 0.01);
        assert!((s.active_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_clock_stats_tsc_fallback() {
        let t0 = Instant::now();
        let prev = counters(Some(0), 0, 0, t0);
        let cur = counters(
            Some(3_700_000_000),
            1_250_000_000,
            925_000_000,
            t0 + Duration::from_secs(1),
        );
        let s = clock_stats(&prev, &cur, None).unwrap();
        assert!((s.active_pct - 25.0).abs() < 0.01);
        assert!((s.active_mhz.unwrap() - 5000.0).abs() < 0.01);
    }

    #[test]
    fn test_clock_stats_rejects_backwards_counters() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let prev = counters(Some(100), 100, 100, t0);
        // TSC went backwards: only matters for the TSC fallback
        let tsc_back = counters(Some(50), 200, 200, t1);
        assert!(clock_stats(&prev, &tsc_back, None).is_none());
        assert!(clock_stats(&prev, &tsc_back, Some(3700.0)).is_some());
        // APERF went backwards (hotplug, resume): always rejected
        let aperf_back = counters(None, 50, 200, t1);
        assert!(clock_stats(&prev, &aperf_back, Some(3700.0)).is_none());
    }

    #[test]
    fn test_clock_stats_idle_has_no_active_clock() {
        let t0 = Instant::now();
        let prev = counters(Some(0), 0, 0, t0);
        let cur = counters(Some(1_000_000), 0, 0, t0 + Duration::from_millis(10));
        let s = clock_stats(&prev, &cur, Some(3700.0)).unwrap();
        assert_eq!(s.active_pct, 0.0);
        assert_eq!(s.active_mhz, None);
    }

    #[test]
    fn test_core_labels() {
        assert_eq!(core_label(CoreKind::Performance, 3), "P-Core 3");
        assert_eq!(core_label(CoreKind::Efficient, 8), "E-Core 8");
        assert_eq!(core_label(CoreKind::Uniform, 0), "Core 0");
    }
}
