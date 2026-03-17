// ── Linux implementation ─────────────────────────────────────────────
#[cfg(unix)]
mod platform {
    use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
    use crate::platform::sysfs::{self, CachedFile};

    pub struct CpuFreqSource {
        cpus: Vec<CpuFreqEntry>,
    }

    struct CpuFreqEntry {
        id: SensorId,
        label: String,
        freq_file: CachedFile,
    }

    impl CpuFreqSource {
        pub fn discover() -> Self {
            let mut cpus = Vec::new();

            for path in
                sysfs::glob_paths("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")
            {
                // Extract CPU index from path: .../cpu{N}/cpufreq/...
                let cpu_dir = match path.parent().and_then(|p| p.parent()) {
                    Some(d) => d,
                    None => continue,
                };
                let dir_name = match cpu_dir.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let idx: u32 = match dir_name.strip_prefix("cpu").and_then(|s| s.parse().ok()) {
                    Some(i) => i,
                    None => continue,
                };

                let Some(freq_file) = CachedFile::open(&path) else {
                    continue;
                };

                cpus.push(CpuFreqEntry {
                    id: SensorId {
                        source: "cpu".into(),
                        chip: "cpufreq".into(),
                        sensor: format!("cpu{idx}"),
                    },
                    label: format!("Core {idx} Frequency"),
                    freq_file,
                });
            }

            cpus.sort_by(|a, b| a.id.natural_cmp(&b.id));

            Self { cpus }
        }

        pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            let mut readings = Vec::new();

            for entry in &mut self.cpus {
                let Some(khz) = entry.freq_file.read_u64() else {
                    continue;
                };
                let mhz = khz as f64 / 1000.0;

                let reading = SensorReading::new(
                    entry.label.clone(),
                    mhz,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                );
                readings.push((entry.id.clone(), reading));
            }

            readings
        }
    }

    impl crate::sensors::SensorSource for CpuFreqSource {
        fn name(&self) -> &str {
            "cpufreq"
        }

        fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            CpuFreqSource::poll(self)
        }
    }
}

// ── Windows implementation ──────────────────────────────────────────
#[cfg(windows)]
mod platform {
    use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

    /// Per-logical-processor frequency data returned by
    /// `CallNtPowerInformation(ProcessorInformation)`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcessorPowerInformation {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        mhz_limit: u32,
        max_idle_state: u32,
        current_idle_state: u32,
    }

    /// ProcessorInformation = 11 in the POWER_INFORMATION_LEVEL enum.
    const PROCESSOR_INFORMATION: i32 = 11;

    #[link(name = "powrprof")]
    unsafe extern "system" {
        fn CallNtPowerInformation(
            InformationLevel: i32,
            InputBuffer: *const std::ffi::c_void,
            InputBufferLength: u32,
            OutputBuffer: *mut std::ffi::c_void,
            OutputBufferLength: u32,
        ) -> i32;
    }

    pub struct CpuFreqSource {
        num_cpus: u32,
    }

    impl CpuFreqSource {
        pub fn discover() -> Self {
            use sysinfo::System;
            let mut sys = System::new();
            sys.refresh_cpu_all();
            let count = sys.cpus().len() as u32;
            log::info!("cpu_freq: discovered {count} logical CPUs (Windows)");
            Self { num_cpus: count }
        }

        pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            let count = self.num_cpus as usize;
            if count == 0 {
                return vec![];
            }

            let mut info: Vec<ProcessorPowerInformation> =
                vec![unsafe { std::mem::zeroed() }; count];
            let buf_size = (count * std::mem::size_of::<ProcessorPowerInformation>()) as u32;

            let status = unsafe {
                CallNtPowerInformation(
                    PROCESSOR_INFORMATION,
                    std::ptr::null(),
                    0,
                    info.as_mut_ptr() as *mut std::ffi::c_void,
                    buf_size,
                )
            };

            // STATUS_SUCCESS == 0
            if status != 0 {
                log::warn!("CallNtPowerInformation failed: NTSTATUS 0x{status:08X}");
                return vec![];
            }

            let mut results = Vec::with_capacity(count);
            for ppi in &info {
                results.push((
                    SensorId {
                        source: "cpu".into(),
                        chip: "cpufreq".into(),
                        sensor: format!("cpu{}", ppi.number),
                    },
                    SensorReading::new(
                        format!("Core {} Frequency", ppi.number),
                        ppi.current_mhz as f64,
                        SensorUnit::Mhz,
                        SensorCategory::Frequency,
                    ),
                ));
            }

            results
        }
    }

    impl crate::sensors::SensorSource for CpuFreqSource {
        fn name(&self) -> &str {
            "cpufreq"
        }

        fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            CpuFreqSource::poll(self)
        }
    }
}

pub use platform::CpuFreqSource;
