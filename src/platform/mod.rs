//! OS and hardware I/O abstractions.
//!
//! Low-level interfaces to Linux subsystems: sysfs, procfs, MSR reads,
//! NVMe/SATA ioctls, direct port I/O, the `sinfo_io` kernel module,
//! NVML (NVIDIA), and Tegra devfreq/engine sensors.

pub mod msr;
pub mod nvme_ioctl;
#[cfg(feature = "nvidia")]
pub mod nvml;
pub mod port_io;
pub mod procfs;
pub mod sata_ioctl;
pub mod sinfo_io;
pub mod sysfs;
pub mod tegra;
