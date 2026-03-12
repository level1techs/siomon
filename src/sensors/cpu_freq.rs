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

        for path in sysfs::glob_paths("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")
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
