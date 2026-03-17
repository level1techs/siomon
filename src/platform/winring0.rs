//! WinRing0 kernel driver wrapper for direct hardware access on Windows.
//!
//! Loads WinRing0x64.dll at runtime via libloading. Provides:
//! - I/O port read/write (x86 IN/OUT instructions)
//! - MSR read (Model-Specific Registers)
//! - PCI config space read/write

use libloading::{Library, Symbol};
use std::sync::OnceLock;

static INSTANCE: OnceLock<Option<WinRing0>> = OnceLock::new();

pub struct WinRing0 {
    _lib: Library,
    // Function pointers
    read_io_port_byte: unsafe extern "system" fn(u16) -> u8,
    write_io_port_byte: unsafe extern "system" fn(u16, u8),
    read_msr: unsafe extern "system" fn(u32, *mut u32, *mut u32) -> i32,
    read_pci_config_dword: unsafe extern "system" fn(u32, u8) -> u32,
    write_pci_config_dword: unsafe extern "system" fn(u32, u8, u32),
}

/// WinRing0 PCI address encoding: bus << 8 | device << 3 | function
fn pci_address(bus: u8, dev: u8, func: u8) -> u32 {
    ((bus as u32) << 8) | ((dev as u32) << 3) | (func as u32)
}

impl WinRing0 {
    /// Try to load WinRing0x64.dll and initialize. Returns None if not available.
    pub fn try_load() -> Option<&'static Self> {
        INSTANCE.get_or_init(|| Self::load_inner().ok()).as_ref()
    }

    fn load_inner() -> Result<Self, Box<dyn std::error::Error>> {
        // Try multiple paths for the DLL
        let lib = unsafe {
            Library::new("WinRing0x64.dll")
                .or_else(|_| Library::new("WinRing0x64"))
                .or_else(|e| {
                    // Try next to the executable
                    let exe_dir = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|d| d.join("WinRing0x64.dll")));
                    if let Some(path) = exe_dir {
                        Library::new(path)
                    } else {
                        Err(e)
                    }
                })
        }?;

        // Load InitializeOls and call it
        let init: Symbol<unsafe extern "system" fn() -> i32> =
            unsafe { lib.get(b"InitializeOls\0")? };
        if unsafe { init() } == 0 {
            return Err("InitializeOls failed".into());
        }

        // Load function pointers
        let read_io_port_byte =
            *unsafe { lib.get::<unsafe extern "system" fn(u16) -> u8>(b"ReadIoPortByte\0")? };
        let write_io_port_byte =
            *unsafe { lib.get::<unsafe extern "system" fn(u16, u8)>(b"WriteIoPortByte\0")? };
        let read_msr = *unsafe {
            lib.get::<unsafe extern "system" fn(u32, *mut u32, *mut u32) -> i32>(b"Rdmsr\0")?
        };
        let read_pci_config_dword = *unsafe {
            lib.get::<unsafe extern "system" fn(u32, u8) -> u32>(b"ReadPciConfigDword\0")?
        };
        let write_pci_config_dword = *unsafe {
            lib.get::<unsafe extern "system" fn(u32, u8, u32)>(b"WritePciConfigDword\0")?
        };

        Ok(Self {
            _lib: lib,
            read_io_port_byte,
            write_io_port_byte,
            read_msr,
            read_pci_config_dword,
            write_pci_config_dword,
        })
    }

    pub fn read_io_port_byte(&self, port: u16) -> u8 {
        unsafe { (self.read_io_port_byte)(port) }
    }

    pub fn write_io_port_byte(&self, port: u16, val: u8) {
        unsafe { (self.write_io_port_byte)(port, val) }
    }

    /// Read a 64-bit MSR. Returns None on failure.
    pub fn read_msr(&self, index: u32) -> Option<u64> {
        let mut eax: u32 = 0;
        let mut edx: u32 = 0;
        let ok = unsafe { (self.read_msr)(index, &mut eax, &mut edx) };
        if ok != 0 {
            Some(((edx as u64) << 32) | (eax as u64))
        } else {
            None
        }
    }

    pub fn read_pci_config_dword(&self, bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
        let addr = pci_address(bus, dev, func);
        unsafe { (self.read_pci_config_dword)(addr, reg) }
    }

    pub fn write_pci_config_dword(&self, bus: u8, dev: u8, func: u8, reg: u8, val: u32) {
        let addr = pci_address(bus, dev, func);
        unsafe { (self.write_pci_config_dword)(addr, reg, val) }
    }
}

// Implement Send + Sync since the DLL functions are thread-safe
unsafe impl Send for WinRing0 {}
unsafe impl Sync for WinRing0 {}
