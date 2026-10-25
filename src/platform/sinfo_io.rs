#![allow(dead_code)] // Public API awaiting consumer integration in superio sources
//! FFI wrapper for the sinfo_io kernel module.
//!
//! Provides atomic banked register reads on Super I/O hardware monitoring
//! chips via `/dev/sinfo_io`. Falls back to `/dev/port` when the kernel
//! module is not loaded.
//!
//! The kernel module uses a spinlock with interrupts disabled to guarantee
//! that bank-select + register-read sequences are atomic, eliminating race
//! conditions with other software (or kernel drivers) accessing the same
//! I/O ports.

use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;

use super::port_io::PortIo;

// ── IOCTL definitions (mirrors kmod/sinfo_io/sinfo_io.h) ───────────────

const SINFO_IO_BATCH_MAX: usize = 32;

const SINFO_IO_SETUP: libc::Ioctl = libc::_IOW::<SinfoIoSetup>(b'S' as u32, 0x01);
const SINFO_IO_READ_REG: libc::Ioctl = libc::_IOWR::<SinfoIoReg>(b'S' as u32, 0x02);
const SINFO_IO_READ_BATCH: libc::Ioctl = libc::_IOWR::<SinfoIoBatch>(b'S' as u32, 0x03);

// ── Kernel-matching structs ─────────────────────────────────────────────

#[repr(C)]
struct SinfoIoSetup {
    hwm_base: u16,
    reserved: u16,
}

#[repr(C)]
struct SinfoIoReg {
    reg: u16,
    value: u8,
    status: u8,
}

#[repr(C)]
struct SinfoIoBatch {
    count: u8,
    reserved: [u8; 3],
    regs: [u16; SINFO_IO_BATCH_MAX],
    values: [u8; SINFO_IO_BATCH_MAX],
}

// ── SinfoIo handle ─────────────────────────────────────────────────────

/// Handle for atomic Super I/O register access via the sinfo_io kernel module.
pub struct SinfoIo {
    file: File,
}

impl SinfoIo {
    /// Open `/dev/sinfo_io` and configure the HWM base address.
    ///
    /// Returns `None` if the device doesn't exist, permission is denied,
    /// or the SETUP ioctl fails.
    pub fn open(hwm_base: u16) -> Option<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/sinfo_io")
            .ok()?;

        let setup = SinfoIoSetup {
            hwm_base,
            reserved: 0,
        };
        let ret = unsafe { libc::ioctl(file.as_raw_fd(), SINFO_IO_SETUP, &setup) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            log::debug!("sinfo_io SETUP failed for base 0x{hwm_base:04X}: {err}");
            return None;
        }

        log::info!("sinfo_io: opened with HWM base 0x{hwm_base:04X}");
        Some(Self { file })
    }

    /// Check if the sinfo_io kernel module is loaded.
    pub fn is_available() -> bool {
        std::path::Path::new("/dev/sinfo_io").exists()
    }

    /// Read a single banked register atomically.
    ///
    /// `reg` uses the standard encoding: high byte = bank, low byte = offset.
    pub fn read_register(&self, reg: u16) -> Option<u8> {
        let mut r = SinfoIoReg {
            reg,
            value: 0,
            status: 0xFF,
        };
        let ret = unsafe { libc::ioctl(self.file.as_raw_fd(), SINFO_IO_READ_REG, &mut r) };
        if ret < 0 || r.status != 0 {
            return None;
        }
        Some(r.value)
    }

    /// Read up to 32 banked registers in a single atomic operation.
    ///
    /// Returns a Vec of values in the same order as the input registers.
    /// Returns `None` if the ioctl fails.
    pub fn read_batch(&self, regs: &[u16]) -> Option<Vec<u8>> {
        if regs.is_empty() || regs.len() > SINFO_IO_BATCH_MAX {
            return None;
        }

        let mut batch = SinfoIoBatch {
            count: regs.len() as u8,
            reserved: [0; 3],
            regs: [0; SINFO_IO_BATCH_MAX],
            values: [0; SINFO_IO_BATCH_MAX],
        };
        batch.regs[..regs.len()].copy_from_slice(regs);

        let ret = unsafe { libc::ioctl(self.file.as_raw_fd(), SINFO_IO_READ_BATCH, &mut batch) };
        if ret < 0 {
            return None;
        }

        Some(batch.values[..regs.len()].to_vec())
    }
}

// ── HwmAccess: unified interface ────────────────────────────────────────

/// Unified hardware monitoring register access.
///
/// Prefers `/dev/sinfo_io` (atomic, exclusive port ownership) when available,
/// falls back to `/dev/port` (non-atomic, may race with kernel drivers).
pub enum HwmAccess {
    /// Atomic access via sinfo_io kernel module.
    KernelModule(SinfoIo),
    /// Direct port I/O via `/dev/port` (non-atomic fallback).
    DevPort(PortIo),
}

impl HwmAccess {
    /// Open the best available access method for the given HWM base address.
    ///
    /// Tries sinfo_io first, then falls back to /dev/port.
    pub fn open(hwm_base: u16) -> Option<Self> {
        if let Some(sio) = SinfoIo::open(hwm_base) {
            return Some(HwmAccess::KernelModule(sio));
        }
        log::debug!("sinfo_io unavailable, falling back to /dev/port");
        PortIo::open().map(HwmAccess::DevPort)
    }

    /// Read a single banked register.
    ///
    /// For sinfo_io: atomic (bank-select + read under spinlock).
    /// For /dev/port: non-atomic (separate write+read operations).
    pub fn read_register(&mut self, hwm_base: u16, reg: u16) -> Option<u8> {
        match self {
            HwmAccess::KernelModule(sio) => sio.read_register(reg),
            HwmAccess::DevPort(pio) => {
                let bank = (reg >> 8) as u8;
                let offset = (reg & 0xFF) as u8;
                let addr_port = hwm_base + 5;
                let data_port = hwm_base + 6;

                pio.write_byte(addr_port, 0x4E).ok()?; // REG_BANK
                pio.write_byte(data_port, bank).ok()?;
                pio.write_byte(addr_port, offset).ok()?;
                pio.read_byte(data_port).ok()
            }
        }
    }

    /// Read up to 32 banked registers.
    ///
    /// For sinfo_io: single atomic ioctl.
    /// For /dev/port: sequential reads (not atomic).
    pub fn read_batch(&mut self, hwm_base: u16, regs: &[u16]) -> Option<Vec<u8>> {
        match self {
            HwmAccess::KernelModule(sio) => sio.read_batch(regs),
            HwmAccess::DevPort(_) => {
                let mut values = Vec::with_capacity(regs.len());
                for &reg in regs {
                    values.push(self.read_register(hwm_base, reg)?);
                }
                Some(values)
            }
        }
    }

    /// Returns true if using the atomic kernel module path.
    pub fn is_atomic(&self) -> bool {
        matches!(self, HwmAccess::KernelModule(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_sizes() {
        assert_eq!(std::mem::size_of::<SinfoIoSetup>(), 4);
        assert_eq!(std::mem::size_of::<SinfoIoReg>(), 4);
        assert_eq!(std::mem::size_of::<SinfoIoBatch>(), 100);
    }

    #[test]
    fn test_sinfo_io_availability() {
        // Just verify the function doesn't panic
        let _ = SinfoIo::is_available();
    }

    #[test]
    fn test_hwm_access_open_returns_some_or_none() {
        // On any system this should either:
        // - Return Some(KernelModule) if sinfo_io is loaded and we're root
        // - Return Some(DevPort) if /dev/port is accessible and we're root
        // - Return None if neither device is accessible (non-root)
        // It must never panic.
        let result = HwmAccess::open(0x0290);
        if let Some(ref access) = result {
            // If we got access, is_atomic tells us which path was taken
            let _ = access.is_atomic();
        }
    }
}
