//! Intel integrated/discrete GPU clocks and idle time from DRM sysfs.
//!
//! Supports both kernel drivers:
//! - **i915**: `/sys/class/drm/cardN/gt/gtM/{rps_act_freq_mhz,rps_cur_freq_mhz,rc6_residency_ms}`.
//!   From Meteor Lake on, gt0 is the render GT and gt1 the media GT.
//! - **xe**: `/sys/class/drm/cardN/device/tile0/gtM/{freq0/act_freq,freq0/cur_freq,gtidle/idle_residency_ms}`,
//!   with the GT type taken from `gtidle/name` (`gtM-rc` render, `gtM-mc` media).
//!
//! The actual clock is what the GT runs at and the requested clock is what
//! the driver asked for. The actual clock reads 0 while the GT is
//! power-gated; that is published as is, since the GT really is not
//! clocked (and a skipped reading would leave the last clock on screen).
//! Idle is the share of the interval the GT spent power-gated (RC6
//! residency).
//!
//! The integrated GPU (PCI `00:02.0`) is labelled "iGPU"; other Intel GPUs
//! are labelled by card.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs::{self, CachedFile};

struct GtEntry {
    chip: String,
    /// Display prefix, e.g. "iGPU Render".
    name: String,
    act_freq: Option<CachedFile>,
    cur_freq: Option<CachedFile>,
    rc6_ms: Option<CachedFile>,
    prev_rc6: Option<(u64, Instant)>,
}

pub struct IntelGpuSource {
    gts: Vec<GtEntry>,
}

/// GT role from an xe `gtidle/name` value such as `gt0-rc` or `gt1-mc`.
fn xe_gt_name(idle_name: &str) -> &'static str {
    if idle_name.ends_with("-mc") {
        "Media"
    } else {
        "Render"
    }
}

/// GT role for an i915 GT index: gt1 is the standalone media GT.
fn i915_gt_name(index: u32) -> &'static str {
    if index == 0 { "Render" } else { "Media" }
}

/// RC6 residency in percent from two `(residency_ms, timestamp)` samples.
fn rc6_pct(prev: (u64, Instant), cur: (u64, Instant)) -> Option<f64> {
    let d_ms = cur.0.checked_sub(prev.0)? as f64;
    let elapsed_ms = cur.1.checked_duration_since(prev.1)?.as_secs_f64() * 1000.0;
    (elapsed_ms > 0.0).then(|| (d_ms / elapsed_ms * 100.0).clamp(0.0, 100.0))
}

/// Label prefix for a card: "iGPU" for the integrated function at
/// `0000:00:02.0`, otherwise the card name.
fn device_prefix(pci_addr: Option<&str>, card: &str) -> String {
    if pci_addr == Some("0000:00:02.0") {
        "iGPU".into()
    } else {
        format!("Intel GPU {card}")
    }
}

fn gt_index(path: &Path) -> Option<u32> {
    path.file_name()?.to_str()?.strip_prefix("gt")?.parse().ok()
}

impl IntelGpuSource {
    pub fn discover() -> Self {
        let mut gts = Vec::new();

        for card_path in sysfs::glob_paths("/sys/class/drm/card[0-9]*") {
            let Some(card) = card_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !card[4..].chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let device = card_path.join("device");
            if sysfs::read_u64_optional(&device.join("vendor")) != Some(0x8086) {
                continue;
            }
            let prefix = device_prefix(sysfs::read_link_basename(&device).as_deref(), card);

            // i915
            for gt in sysfs::glob_paths(&format!("{}/gt/gt[0-9]*", card_path.display())) {
                let Some(index) = gt_index(&gt) else {
                    continue;
                };
                push_gt(
                    &mut gts,
                    format!("{card}-gt{index}"),
                    format!("{prefix} {}", i915_gt_name(index)),
                    gt.join("rps_act_freq_mhz"),
                    gt.join("rps_cur_freq_mhz"),
                    gt.join("rc6_residency_ms"),
                );
            }

            // xe
            for gt in sysfs::glob_paths(&format!("{}/tile[0-9]*/gt[0-9]*", device.display())) {
                let Some(index) = gt_index(&gt) else {
                    continue;
                };
                let name = sysfs::read_string_optional(&gt.join("gtidle/name"))
                    .map(|n| xe_gt_name(&n))
                    .unwrap_or("Render");
                push_gt(
                    &mut gts,
                    format!("{card}-gt{index}"),
                    format!("{prefix} {name}"),
                    gt.join("freq0/act_freq"),
                    gt.join("freq0/cur_freq"),
                    gt.join("gtidle/idle_residency_ms"),
                );
            }
        }

        if !gts.is_empty() {
            log::info!("Intel GPU: {} GTs", gts.len());
        }
        Self { gts }
    }

    pub fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();
        for gt in &mut self.gts {
            let mut push = |sensor: &str, label: String, value: f64, unit, category| {
                readings.push((
                    SensorId {
                        source: "intel_gpu".into(),
                        chip: gt.chip.clone(),
                        sensor: sensor.into(),
                    },
                    SensorReading::new(label, value, unit, category),
                ));
            };

            if let Some(mhz) = gt.act_freq.as_mut().and_then(|f| f.read_u64()) {
                push(
                    "act_freq",
                    format!("{} Clock", gt.name),
                    mhz as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                );
            }
            if let Some(mhz) = gt.cur_freq.as_mut().and_then(|f| f.read_u64()) {
                push(
                    "cur_freq",
                    format!("{} Requested Clock", gt.name),
                    mhz as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                );
            }
            if let Some(ms) = gt.rc6_ms.as_mut().and_then(|f| f.read_u64()) {
                let now = (ms, Instant::now());
                if let Some(pct) = gt.prev_rc6.and_then(|prev| rc6_pct(prev, now)) {
                    // Not Utilization: a high value means idle, not busy.
                    push(
                        "idle",
                        format!("{} Idle", gt.name),
                        pct,
                        SensorUnit::Percent,
                        SensorCategory::Other,
                    );
                }
                gt.prev_rc6 = Some(now);
            }
        }
        readings
    }
}

fn push_gt(
    gts: &mut Vec<GtEntry>,
    chip: String,
    name: String,
    act: PathBuf,
    cur: PathBuf,
    rc6: PathBuf,
) {
    let act_freq = CachedFile::open(&act);
    let cur_freq = CachedFile::open(&cur);
    let mut rc6_ms = CachedFile::open(&rc6);
    if act_freq.is_none() && cur_freq.is_none() && rc6_ms.is_none() {
        return;
    }
    // Baseline now so the first poll already has an RC6 delta.
    let prev_rc6 = rc6_ms
        .as_mut()
        .and_then(|f| f.read_u64())
        .map(|ms| (ms, Instant::now()));
    gts.push(GtEntry {
        chip,
        name,
        act_freq,
        cur_freq,
        rc6_ms,
        prev_rc6,
    });
}

impl crate::sensors::SensorSource for IntelGpuSource {
    fn name(&self) -> &str {
        "intel_gpu"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        IntelGpuSource::poll(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_gt_names() {
        assert_eq!(xe_gt_name("gt0-rc"), "Render");
        assert_eq!(xe_gt_name("gt1-mc"), "Media");
        assert_eq!(i915_gt_name(0), "Render");
        assert_eq!(i915_gt_name(1), "Media");
    }

    #[test]
    fn test_device_prefix() {
        assert_eq!(device_prefix(Some("0000:00:02.0"), "card1"), "iGPU");
        assert_eq!(
            device_prefix(Some("0000:03:00.0"), "card2"),
            "Intel GPU card2"
        );
        assert_eq!(device_prefix(None, "card0"), "Intel GPU card0");
    }

    #[test]
    fn test_rc6_pct() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(1000);
        assert!((rc6_pct((1000, t0), (1998, t1)).unwrap() - 99.8).abs() < 0.001);
        // Counter reset (driver reload) yields no value rather than a bogus one
        assert_eq!(rc6_pct((2000, t0), (10, t1)), None);
    }

    #[test]
    fn test_gt_index() {
        assert_eq!(gt_index(Path::new("/sys/class/drm/card1/gt/gt1")), Some(1));
        assert_eq!(gt_index(Path::new("/sys/class/drm/card1/gt/foo")), None);
    }
}
