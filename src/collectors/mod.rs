//! One-shot hardware information collectors.
//!
//! Each submodule queries a specific hardware subsystem (CPU, GPU, memory, etc.)
//! and populates [`SystemInfo`] fields. All collectors run in parallel during a
//! full system scan.

pub mod audio;
pub mod battery;
pub mod cpu;
pub mod gpu;
pub mod me;
pub mod memory;
pub mod motherboard;
pub mod network;
pub mod pci;
pub mod storage;
pub mod usb;

use crate::model::system::SystemInfo;

/// Trait for one-shot hardware information collectors.
///
/// Each implementor queries a specific hardware subsystem and writes
/// results directly into the appropriate field(s) of `SystemInfo`.
pub trait Collector: Send {
    /// Human-readable name for logging (e.g., "cpu", "memory").
    fn name(&self) -> &str;

    /// Collect hardware information and store it in `info`.
    fn collect_into(&self, info: &mut SystemInfo);
}
