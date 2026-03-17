//! Windows I/O port access via WinRing0.
//!
//! Provides the same `PortIo` interface as the Linux `/dev/port` backend,
//! backed by WinRing0x64.dll for direct IN/OUT instruction access.

use std::io;

pub struct PortIo;

impl PortIo {
    /// Open port I/O access via WinRing0. Returns `None` if the driver is not available.
    pub fn open() -> Option<Self> {
        // Check WinRing0 is available
        super::winring0::WinRing0::try_load()?;
        Some(Self)
    }

    /// Read a single byte from an I/O port.
    pub fn read_byte(&self, port: u16) -> io::Result<u8> {
        let w = super::winring0::WinRing0::try_load()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "WinRing0 not available"))?;
        Ok(w.read_io_port_byte(port))
    }

    /// Write a single byte to an I/O port.
    pub fn write_byte(&self, port: u16, val: u8) -> io::Result<()> {
        let w = super::winring0::WinRing0::try_load()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "WinRing0 not available"))?;
        w.write_io_port_byte(port, val);
        Ok(())
    }

    /// Write a byte to a port, then read a byte from another port.
    /// Common pattern for Super I/O address/data register pairs.
    pub fn write_read(&self, write_port: u16, write_val: u8, read_port: u16) -> io::Result<u8> {
        let w = super::winring0::WinRing0::try_load()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "WinRing0 not available"))?;
        w.write_io_port_byte(write_port, write_val);
        Ok(w.read_io_port_byte(read_port))
    }

    /// Check if direct I/O port access is available on this system.
    pub fn is_available() -> bool {
        super::winring0::WinRing0::try_load().is_some()
    }
}
