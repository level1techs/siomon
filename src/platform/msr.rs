//! Read-only MSR (Model-Specific Register) access via `/dev/cpu/N/msr`.
//!
//! Requires the `msr` kernel module and root or `CAP_SYS_RAWIO`. Every
//! device is opened read-only, so the kernel refuses any `WRMSR` through
//! these handles, and only registers on [`ALLOWED`] are ever read.
//!
//! The Intel OC mailbox (`0x150`) is deliberately absent and pinned by a
//! test: a mailbox "read" is the second half of a `WRMSR` transaction, so it
//! is never a passive source.

use std::fs::File;
use std::os::unix::fs::FileExt;

pub const IA32_TIME_STAMP_COUNTER: u32 = 0x10;
pub const IA32_MPERF: u32 = 0xE7;
pub const IA32_APERF: u32 = 0xE8;
pub const IA32_PERF_STATUS: u32 = 0x198;
pub const IA32_THERM_STATUS: u32 = 0x19C;
pub const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
pub const IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;
/// PL4 (peak power limit): bits [14:0] in RAPL power units (see
/// [`MSR_RAPL_POWER_UNIT`]), bit 31 = lock. Older SDM revisions call this
/// `VR_CURRENT_CONFIG` (a current); on Arrow Lake-S it was measured to be
/// watts (0xD48 = 425 W) and to be the enforced PL4.
pub const MSR_PL4_POWER_LIMIT: u32 = 0x601;
/// RAPL unit register: bits [3:0] give the power unit as 1/2^n W.
pub const MSR_RAPL_POWER_UNIT: u32 = 0x606;
pub const MSR_UNCORE_PERF_STATUS: u32 = 0x621;

/// Registers this crate may read. Any other address is refused before the
/// device is touched.
pub const ALLOWED: &[u32] = &[
    IA32_TIME_STAMP_COUNTER,
    IA32_MPERF,
    IA32_APERF,
    IA32_PERF_STATUS,
    IA32_THERM_STATUS,
    MSR_TEMPERATURE_TARGET,
    IA32_PACKAGE_THERM_STATUS,
    MSR_PL4_POWER_LIMIT,
    MSR_RAPL_POWER_UNIT,
    MSR_UNCORE_PERF_STATUS,
];

/// Registers that must never be added to [`ALLOWED`]: `0x150` is the Intel
/// OC mailbox, where even a "Get" command is a write.
pub const DENIED: &[u32] = &[0x150];

/// Whether `addr` is on the read allowlist.
pub fn is_allowed(addr: u32) -> bool {
    ALLOWED.contains(&addr) && !DENIED.contains(&addr)
}

/// A read-only handle to one logical CPU's MSR device.
pub struct MsrDevice {
    file: File,
}

impl MsrDevice {
    /// Open `/dev/cpu/{cpu}/msr` read-only.
    ///
    /// Returns `None` if the `msr` module is not loaded or permission is
    /// denied (root or `CAP_SYS_RAWIO` is required).
    pub fn open(cpu: u32) -> Option<Self> {
        let file = File::open(format!("/dev/cpu/{cpu}/msr")).ok()?;
        Some(Self { file })
    }

    /// Read one 64-bit MSR. Returns `None` for addresses not on the
    /// allowlist, or when the CPU does not implement the register (`EIO`).
    pub fn read(&self, addr: u32) -> Option<u64> {
        if !is_allowed(addr) {
            log::debug!("MSR {addr:#x} is not on the read allowlist");
            return None;
        }
        let mut buf = [0u8; 8];
        self.file.read_exact_at(&mut buf, addr as u64).ok()?;
        Some(u64::from_le_bytes(buf))
    }
}

/// Extract the inclusive bit field `hi..=lo` from `value`.
pub const fn bits(value: u64, hi: u32, lo: u32) -> u64 {
    let width = hi - lo + 1;
    if width >= 64 {
        value >> lo
    } else {
        (value >> lo) & ((1u64 << width) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oc_mailbox_is_never_allowed() {
        for addr in DENIED {
            assert!(!ALLOWED.contains(addr), "{addr:#x} must not be readable");
            assert!(!is_allowed(*addr));
        }
    }

    #[test]
    fn test_allowlist_has_no_duplicates() {
        let mut sorted = ALLOWED.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ALLOWED.len());
    }

    #[test]
    fn test_unknown_address_refused() {
        assert!(!is_allowed(0x1234));
        assert!(is_allowed(IA32_PERF_STATUS));
    }

    #[test]
    fn test_bits() {
        // IA32_PERF_STATUS: VID in 47:32, ratio in 15:8
        let raw: u64 = 0x0000_1A40_0000_3000;
        assert_eq!(bits(raw, 47, 32), 0x1A40);
        assert_eq!(bits(raw, 15, 8), 0x30);
        assert_eq!(bits(u64::MAX, 63, 0), u64::MAX);
        assert_eq!(bits(0x8000_0000_0000_0000, 63, 63), 1);
    }
}
