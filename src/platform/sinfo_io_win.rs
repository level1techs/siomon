//! Windows HwmAccess for Super I/O hardware monitoring register reads.
//!
//! On Windows there is no sinfo_io kernel module, so we always use direct
//! port I/O via WinRing0. This is equivalent to the Linux `HwmAccess::DevPort`
//! fallback path — non-atomic bank-select + register-read sequences.

use super::port_io_win::PortIo;

/// Unified hardware monitoring register access (Windows variant).
///
/// Always uses WinRing0 port I/O — there is no atomic kernel module path
/// on Windows.
pub struct HwmAccess {
    pio: PortIo,
}

impl HwmAccess {
    /// Open the best available access method for the given HWM base address.
    ///
    /// On Windows this simply opens WinRing0 port I/O.
    pub fn open(_hwm_base: u16) -> Option<Self> {
        let pio = PortIo::open()?;
        Some(Self { pio })
    }

    /// Read a single banked register.
    ///
    /// Performs non-atomic bank-select + register-read via WinRing0 port I/O.
    /// Register encoding: high byte = bank, low byte = offset.
    pub fn read_register(&mut self, hwm_base: u16, reg: u16) -> Option<u8> {
        let bank = (reg >> 8) as u8;
        let offset = (reg & 0xFF) as u8;
        let addr_port = hwm_base + 5;
        let data_port = hwm_base + 6;

        self.pio.write_byte(addr_port, 0x4E).ok()?; // REG_BANK
        self.pio.write_byte(data_port, bank).ok()?;
        self.pio.write_byte(addr_port, offset).ok()?;
        self.pio.read_byte(data_port).ok()
    }

    /// Read up to 32 banked registers sequentially (not atomic).
    pub fn read_batch(&mut self, hwm_base: u16, regs: &[u16]) -> Option<Vec<u8>> {
        let mut values = Vec::with_capacity(regs.len());
        for &reg in regs {
            values.push(self.read_register(hwm_base, reg)?);
        }
        Some(values)
    }

    /// Returns true if using the atomic kernel module path.
    ///
    /// Always false on Windows — there is no sinfo_io kernel module.
    pub fn is_atomic(&self) -> bool {
        false
    }
}
