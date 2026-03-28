/// Command-line argument parsing and config application (clap).
pub mod cli;
/// One-shot hardware information collectors (`Collector` trait).
pub mod collectors;
/// Configuration file loading (`~/.config/siomon/config.toml`).
pub mod config;
/// Embedded lookup databases (CPU codenames, board templates, sensor labels).
pub mod db;
/// Error types (`SiomonError`, `NvmlError`).
pub mod error;
/// Shared data structures for hardware and sensor information.
pub mod model;
/// Output formatters (text, JSON, XML, HTML, CSV) and the TUI dashboard.
pub mod output;
/// Binary format parsers (SMBIOS, EDID).
pub mod parsers;
/// OS and hardware I/O abstractions (sysfs, procfs, MSR, NVMe/SATA ioctl, port I/O, NVML, Tegra).
pub mod platform;
/// Real-time sensor polling sources (`SensorSource` trait).
pub mod sensors;
