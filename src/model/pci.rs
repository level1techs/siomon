use serde::{Deserialize, Serialize};

use super::gpu::PcieLinkInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PciDevice {
    pub address: String,
    pub domain: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
    pub revision: u8,
    pub class_code: u32,
    pub vendor_name: Option<String>,
    pub device_name: Option<String>,
    pub class_name: Option<String>,
    pub subclass_name: Option<String>,
    pub driver: Option<String>,
    pub irq: Option<u32>,
    pub numa_node: Option<i32>,
    pub pcie_link: Option<PcieLinkInfo>,
    pub enabled: bool,
    /// Interrupt information from /proc/interrupts.
    pub interrupts: Option<InterruptInfo>,
    /// AER (Advanced Error Reporting) error counters.
    pub aer: Option<AerCounters>,
}

/// Per-PCI-device interrupt information parsed from /proc/interrupts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptInfo {
    /// Interrupt type: "MSI", "MSI-X".
    pub mode: String,
    /// Trigger mode: "edge" or "level".
    pub trigger: String,
    /// Per-vector breakdown.
    pub vectors: Vec<IrqVector>,
    /// Total interrupt count across all vectors and CPUs.
    pub total_count: u64,
}

/// A single interrupt vector (one line in /proc/interrupts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrqVector {
    /// Linux IRQ number.
    pub irq: u32,
    /// Total interrupt count (sum across all CPUs).
    pub count: u64,
    /// Handler name (e.g., "nvidia", "nvme0q0").
    pub handler: String,
}

/// PCIe AER error counters from sysfs aer_dev_* files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AerCounters {
    pub correctable: u64,
    pub nonfatal: u64,
    pub fatal: u64,
}
