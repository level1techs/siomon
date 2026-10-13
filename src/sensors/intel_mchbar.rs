//! Intel MCHBAR live telemetry (Arrow Lake-S, `--direct-io`).
//!
//! Reads through the allowlisted, read-only [`Mchbar`] mapping:
//! - CPU and SoC reference clocks (BCLK), measured by the PLL (`BCLK_FREQ`)
//! - CPU cores (IA, `PP0_TEMPERATURE`) and iGPU (GT, `THERM_STATUS_GT`)
//!   temperatures, separate from the per-core and package sensors
//! - the requested System Agent voltage (`SA_PERF_STATUS`)
//!
//! The SA VID field is 0 when the SA rail is not under SVID control, e.g.
//! when the BIOS sets a fixed (override) SA voltage. A zero field is not
//! published; the board-sensed VCCSA rail is the measured value.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::mchbar::{self, Mchbar};
use crate::platform::sysfs;

/// CPU and SoC BCLK in MHz from `BCLK_FREQ` (`[63:32]`/`[31:0]` in kHz).
/// Out-of-range halves (0, all-ones, outside 50-500 MHz) are `None`.
pub(crate) fn bclk_mhz(raw: u64) -> (Option<f64>, Option<f64>) {
    let half = |khz: u64| {
        let mhz = khz as f64 / 1000.0;
        (khz != 0 && khz != 0xFFFF_FFFF && (50.0..=500.0).contains(&mhz)).then_some(mhz)
    };
    (half(raw >> 32), half(raw & 0xFFFF_FFFF))
}

/// CPU cores (IA) temperature in °C from `PP0_TEMPERATURE[7:0]`.
pub(crate) fn ia_temp(raw: u32) -> Option<f64> {
    let byte = raw & 0xFF;
    (raw != u32::MAX && byte != 0 && byte <= 125).then_some(byte as f64)
}

/// iGPU (GT) temperature from `THERM_STATUS_GT`: TjMAX minus the `[22:16]`
/// readout, valid only with bit 31 set.
pub(crate) fn gt_temp(raw: u32, tjmax: f64) -> Option<f64> {
    if raw == u32::MAX || raw >> 31 == 0 {
        return None;
    }
    let temp = tjmax - ((raw >> 16) & 0x7F) as f64;
    (0.0..=125.0).contains(&temp).then_some(temp)
}

/// Requested System Agent voltage from `SA_PERF_STATUS[55:40]` (1/8192 V).
pub(crate) fn sa_vid(raw: u64) -> Option<f64> {
    let code = (raw >> 40) & 0xFFFF;
    (code != 0 && code != 0xFFFF).then(|| code as f64 / 8192.0)
}

/// TjMAX in °C from the coretemp package sensor's critical temperature.
fn coretemp_tjmax() -> Option<f64> {
    sysfs::glob_paths("/sys/class/hwmon/hwmon*")
        .into_iter()
        .find(|dir| sysfs::read_string_optional(&dir.join("name")).as_deref() == Some("coretemp"))
        .and_then(|dir| sysfs::read_u64_optional(&dir.join("temp1_crit")))
        .map(|mc| mc as f64 / 1000.0)
        .filter(|&t| t > 0.0)
}

pub struct IntelMchbarSource {
    mchbar: Option<Mchbar>,
    /// TjMAX from coretemp; the GT temperature is not published without it
    /// rather than assuming a default.
    tjmax: Option<f64>,
}

impl IntelMchbarSource {
    pub fn discover() -> Self {
        let mchbar = Mchbar::open();
        let tjmax = mchbar.as_ref().and_then(|_| coretemp_tjmax());
        Self { mchbar, tjmax }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let Some(mchbar) = &self.mchbar else {
            return Vec::new();
        };
        let mut readings = Vec::new();
        let mut push = |sensor: &str, label: &str, value: f64, unit, category| {
            readings.push((
                SensorId {
                    source: "mchbar".into(),
                    chip: "arl".into(),
                    sensor: sensor.into(),
                },
                SensorReading::new(label.into(), value, unit, category),
            ));
        };

        let (cpu_bclk, soc_bclk) = mchbar
            .read64(mchbar::BCLK_FREQ)
            .map(bclk_mhz)
            .unwrap_or((None, None));
        if let Some(mhz) = cpu_bclk {
            push(
                "bclk",
                "CPU BCLK",
                mhz,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            );
        }
        if let Some(mhz) = soc_bclk {
            push(
                "soc_bclk",
                "SoC BCLK",
                mhz,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            );
        }

        if let Some(t) = mchbar.read32(mchbar::PP0_TEMPERATURE).and_then(ia_temp) {
            push(
                "ia_temp",
                "CPU Cores (IA)",
                t,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            );
        }

        if let (Some(raw), Some(tj)) = (mchbar.read32(mchbar::THERM_STATUS_GT), self.tjmax)
            && let Some(t) = gt_temp(raw, tj)
        {
            push(
                "gt_temp",
                "iGPU (GT)",
                t,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            );
        }

        if let Some(volts) = mchbar.read64(mchbar::SA_PERF_STATUS).and_then(sa_vid) {
            push(
                "sa_vid",
                "SA VID",
                volts,
                SensorUnit::Volts,
                SensorCategory::Voltage,
            );
        }

        readings
    }
}

impl crate::sensors::SensorSource for IntelMchbarSource {
    fn name(&self) -> &str {
        "intel_mchbar"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        IntelMchbarSource::poll(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Raw words recorded on a Core Ultra 9 285K / Z890 Steel Legend WiFi.

    #[test]
    fn test_bclk_halves() {
        let (cpu, soc) = bclk_mhz(0x0001_86A0_0001_86A3);
        assert_eq!(cpu, Some(100.0));
        assert!((soc.unwrap() - 100.003).abs() < 1e-9);
        assert_eq!(bclk_mhz(0), (None, None));
        assert_eq!(bclk_mhz(u64::MAX), (None, None));
        // 1 MHz is outside the plausible range
        assert_eq!(bclk_mhz(0x0000_03E8_0000_03E8), (None, None));
    }

    #[test]
    fn test_ia_temp() {
        assert_eq!(ia_temp(0x28), Some(40.0));
        assert_eq!(ia_temp(0), None);
        assert_eq!(ia_temp(0xFF), None);
        assert_eq!(ia_temp(u32::MAX), None);
    }

    #[test]
    fn test_gt_temp_uses_live_tjmax() {
        // 0x884D0000: valid, readout 0x4D = 77 -> 105 - 77 = 28 °C
        assert_eq!(gt_temp(0x884D_0000, 105.0), Some(28.0));
        assert_eq!(gt_temp(0x884D_0000, 100.0), Some(23.0));
        assert_eq!(gt_temp(0x004A_0000, 105.0), None);
        assert_eq!(gt_temp(u32::MAX, 105.0), None);
    }

    #[test]
    fn test_sa_vid_absent_in_fixed_mode() {
        // SA rail on a fixed BIOS voltage: VID field 0
        assert_eq!(sa_vid(0x0000_0048_0000_0000), None);
        assert!((sa_vid(0x0026_0000_0000_0000).unwrap() - 1.1875).abs() < 1e-9);
    }
}
