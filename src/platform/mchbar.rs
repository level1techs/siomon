//! Read-only access to the Intel host bridge MCHBAR MMIO window.
//!
//! MCHBAR's base comes from the host bridge's PCI config register at `0x48`
//! (64-bit; bit 0 enables the window, bits 63:15 hold the base). Registers
//! are read through a read-only `/dev/mem` mapping at their exact width, and
//! only `(offset, width)` pairs on the allowlist are ever read.
//!
//! MCHBAR layouts differ between generations, so the host bridge must be a
//! device listed in [`HOST_BRIDGES`] before `/dev/mem` is opened at all; an
//! unknown bridge is refused rather than guessed.
//!
//! Needs root (PCI config beyond `0x40`, and `/dev/mem`). Kernels built with
//! `CONFIG_IO_STRICT_DEVMEM` still allow it because the window is not
//! claimed exclusively by a driver.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// Host bridges whose MCHBAR register map is known: `(vendor, device, platform)`.
pub const HOST_BRIDGES: &[(u16, u16, &str)] = &[(0x8086, 0x7D1A, "Arrow Lake-S")];

const HOST_BRIDGE: &str = "/sys/bus/pci/devices/0000:00:00.0";
const MCHBAR_REG: u64 = 0x48;
const MCHBAR_BASE_MASK: u64 = !0x7FFF;
/// Size of the mapped window; covers every allowlisted register.
const WINDOW_LEN: usize = 0x20000;

pub const SA_PERF_STATUS: u64 = 0x5918;
pub const PP0_TEMPERATURE: u64 = 0x597C;
pub const THERM_STATUS_GT: u64 = 0x59C0;
pub const BCLK_FREQ: u64 = 0x5F60;
pub const MEMSS_BIOS_REQ: u64 = 0x13D08;
pub const MEMSS_BIOS_DATA: u64 = 0x13D10;

/// Per-channel timing register blocks (MC0/MC1 × channel 0/1). The
/// broadcast blocks alias channel 0 and are not read.
pub const TIMING_INSTANCES: &[(u64, &str)] = &[
    (0x0E000, "mc0 channel 0"),
    (0x0E800, "mc0 channel 1"),
    (0x1E000, "mc1 channel 0"),
    (0x1E800, "mc1 channel 1"),
];

pub const TC_PRE: u64 = 0x000;
pub const TC_RDRD: u64 = 0x00C;
pub const TC_RDWR: u64 = 0x010;
pub const TC_WRRD: u64 = 0x014;
pub const TC_WRWR: u64 = 0x018;
pub const TC_CAS: u64 = 0x070;
pub const SC_GS_CFG: u64 = 0x088;
pub const TC_ACT: u64 = 0x138;
pub const TC_RFTP: u64 = 0x4A0;

/// Fixed registers that may be read, with their width in bytes.
const FIXED_REGS: &[(u64, usize)] = &[
    (SA_PERF_STATUS, 8),
    (PP0_TEMPERATURE, 4),
    (THERM_STATUS_GT, 4),
    (BCLK_FREQ, 8),
    (MEMSS_BIOS_REQ, 4),
    (MEMSS_BIOS_DATA, 4),
];

/// Registers inside each timing instance, with their width in bytes.
const TIMING_REGS: &[(u64, usize)] = &[
    (TC_PRE, 8),
    (TC_RDRD, 4),
    (TC_RDWR, 4),
    (TC_WRRD, 4),
    (TC_WRWR, 4),
    (TC_CAS, 4),
    (SC_GS_CFG, 8),
    (TC_ACT, 8),
    (TC_RFTP, 8),
];

/// Whether `offset` may be read at `width` bytes.
pub fn is_allowed(offset: u64, width: usize) -> bool {
    FIXED_REGS.contains(&(offset, width))
        || TIMING_INSTANCES.iter().any(|&(base, _)| {
            offset
                .checked_sub(base)
                .is_some_and(|rel| TIMING_REGS.contains(&(rel, width)))
        })
}

/// Whether the host bridge is a supported device. Reads sysfs only.
pub fn host_bridge_supported() -> Option<&'static str> {
    let read = |name: &str| {
        crate::platform::sysfs::read_u64_optional(&Path::new(HOST_BRIDGE).join(name))
            .map(|v| v as u16)
    };
    let (vendor, device) = (read("vendor")?, read("device")?);
    HOST_BRIDGES
        .iter()
        .find(|&&(v, d, _)| v == vendor && d == device)
        .map(|&(_, _, name)| name)
}

/// Decode the MCHBAR base from the host bridge register, if enabled.
pub fn decode_base(mchbar_reg: u64) -> Option<u64> {
    let base = mchbar_reg & MCHBAR_BASE_MASK;
    (mchbar_reg & 1 == 1 && base != 0).then_some(base)
}

/// A read-only mapping of the MCHBAR window; unmapped on drop.
pub struct Mchbar {
    map: *const u8,
    platform: &'static str,
}

// SAFETY: the mapping is read-only, never aliased mutably, and lives until
// drop; moving it to the poller thread is sound.
unsafe impl Send for Mchbar {}

impl Mchbar {
    /// Map MCHBAR if the host bridge is supported and the caller is
    /// privileged. Returns `None` otherwise, without opening `/dev/mem` for
    /// an unsupported bridge.
    pub fn open() -> Option<Self> {
        let Some(platform) = host_bridge_supported() else {
            log::debug!("MCHBAR: host bridge not in the supported list");
            return None;
        };

        let config = File::open(Path::new(HOST_BRIDGE).join("config")).ok()?;
        let mut reg = [0u8; 8];
        config.read_exact_at(&mut reg, MCHBAR_REG).ok()?;
        let Some(base) = decode_base(u64::from_le_bytes(reg)) else {
            log::debug!("MCHBAR: window disabled or unreadable");
            return None;
        };

        let mem = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_SYNC)
            .open("/dev/mem")
            .ok()?;
        // SAFETY: a fresh PROT_READ mapping of a valid fd; the result is
        // checked against MAP_FAILED before use.
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                WINDOW_LEN,
                libc::PROT_READ,
                libc::MAP_SHARED,
                mem.as_raw_fd(),
                base as libc::off_t,
            )
        };
        if map == libc::MAP_FAILED {
            log::debug!(
                "MCHBAR: mmap of {base:#x} failed: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }
        log::info!("MCHBAR: {platform} window at {base:#x}");
        Some(Self {
            map: map as *const u8,
            platform,
        })
    }

    /// Platform name of the matched host bridge.
    pub fn platform(&self) -> &'static str {
        self.platform
    }

    /// Read a 32-bit register on the allowlist.
    pub fn read32(&self, offset: u64) -> Option<u32> {
        if !is_allowed(offset, 4) {
            log::debug!("MCHBAR {offset:#x}/4 is not on the read allowlist");
            return None;
        }
        // SAFETY: the offset is allowlisted, 4-byte aligned and inside the
        // mapped window; a volatile load issues exactly one 32-bit access.
        Some(unsafe { std::ptr::read_volatile(self.map.add(offset as usize) as *const u32) })
    }

    /// Read a 64-bit register on the allowlist.
    pub fn read64(&self, offset: u64) -> Option<u64> {
        if !is_allowed(offset, 8) {
            log::debug!("MCHBAR {offset:#x}/8 is not on the read allowlist");
            return None;
        }
        // SAFETY: as in `read32`, for an 8-byte aligned 64-bit register.
        Some(unsafe { std::ptr::read_volatile(self.map.add(offset as usize) as *const u64) })
    }
}

impl Drop for Mchbar {
    fn drop(&mut self) {
        // SAFETY: unmapping the exact region returned by mmap in `open`.
        unsafe {
            libc::munmap(self.map as *mut libc::c_void, WINDOW_LEN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_base() {
        // Recorded on a Z890 board: enabled window at 0xFEDC0000
        assert_eq!(decode_base(0x0000_0000_FEDC_0001), Some(0xFEDC_0000));
        assert_eq!(decode_base(0x0000_0000_FEDC_0000), None);
        assert_eq!(decode_base(0x1), None);
    }

    #[test]
    fn test_allowlist_exact_width() {
        assert!(is_allowed(BCLK_FREQ, 8));
        assert!(!is_allowed(BCLK_FREQ, 4));
        assert!(is_allowed(0x0E000 + TC_CAS, 4));
        assert!(is_allowed(0x1E800 + TC_RFTP, 8));
        // Broadcast blocks and unlisted offsets are refused
        assert!(!is_allowed(0x0F000 + TC_CAS, 4));
        assert!(!is_allowed(0x5998, 4));
    }

    #[test]
    fn test_allowlist_fits_window_and_is_aligned() {
        let all = FIXED_REGS.iter().copied().chain(
            TIMING_INSTANCES
                .iter()
                .flat_map(|&(b, _)| TIMING_REGS.iter().map(move |&(r, w)| (b + r, w))),
        );
        for (off, width) in all {
            assert!(off as usize + width <= WINDOW_LEN, "{off:#x}");
            assert_eq!(off % width as u64, 0, "{off:#x} misaligned");
        }
    }
}
