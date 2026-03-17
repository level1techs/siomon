//! SMBus host controller access on Windows via WinRing0.
//!
//! Drives the AMD FCH (Fusion Controller Hub) SMBus host controller directly
//! through port I/O, bypassing the need for a Linux-style `/dev/i2c-N` device.
//!
//! The AMD FCH SMBus controller is at PCI Bus 0, Device 0x14, Function 0.
//! The SMBus base I/O address is read from PCI config register 0x90.
//!
//! Host controller register offsets from base:
//! - +0x00: SMB_STS   (status)
//! - +0x02: SMB_CTL   (control / protocol start)
//! - +0x03: SMB_CMD   (command byte / register address)
//! - +0x04: SMB_ADDR  (slave address << 1 | R/W bit)
//! - +0x05: SMB_DATA0 (data byte 0 / low byte)
//! - +0x06: SMB_DATA1 (data byte 1 / high byte)

use crate::platform::winring0::WinRing0;

/// Status register bits.
const SMB_STS_HOST_BUSY: u8 = 0x01;
const SMB_STS_INTR: u8 = 0x02;
const SMB_STS_DEV_ERR: u8 = 0x04;
const SMB_STS_BUS_COL: u8 = 0x08;
const SMB_STS_FAILED: u8 = 0x10;

/// SMBus protocol codes written to SMB_CTL to start a transaction.
const PROTOCOL_BYTE_DATA: u8 = 0x48; // Byte data read/write
const PROTOCOL_WORD_DATA: u8 = 0x4C; // Word data read/write

/// Register offsets from the SMBus base I/O address.
const REG_STS: u16 = 0x00;
const REG_CTL: u16 = 0x02;
const REG_CMD: u16 = 0x03;
const REG_ADDR: u16 = 0x04;
const REG_DATA0: u16 = 0x05;
const REG_DATA1: u16 = 0x06;

/// AMD vendor ID in PCI configuration space.
const AMD_VENDOR_ID: u16 = 0x1022;

/// PCI location of the AMD FCH SMBus controller.
const FCH_PCI_BUS: u8 = 0;
const FCH_PCI_DEV: u8 = 0x14;
const FCH_PCI_FUNC: u8 = 0;

/// PCI config offset where the SMBus base address is stored on AMD FCH.
/// The base address is in bits [15:4], with bits [3:0] reserved.
const FCH_SMBUS_BAR_OFFSET: u8 = 0x90;

/// Maximum iterations for busy-wait loops.
const BUSY_WAIT_ITERS: u32 = 5_000;

/// Microsecond delay between busy-wait polls.
const POLL_DELAY_US: u64 = 10;

/// Handle to an AMD FCH SMBus host controller.
///
/// Holds the base I/O port address and provides byte-data and word-data
/// SMBus read operations via WinRing0 port I/O.
pub struct SmbusController {
    base: u16,
}

impl SmbusController {
    /// Detect the AMD FCH SMBus controller and return its handle.
    ///
    /// Returns `None` if:
    /// - WinRing0 is not available
    /// - The PCI device at bus 0 / dev 0x14 / func 0 is not AMD
    /// - The SMBus base address register reads as zero
    pub fn detect_amd_fch() -> Option<Self> {
        let wr = WinRing0::try_load()?;

        // Read vendor/device ID from PCI config offset 0x00
        let vendor_device = wr.read_pci_config_dword(FCH_PCI_BUS, FCH_PCI_DEV, FCH_PCI_FUNC, 0x00);
        let vendor = (vendor_device & 0xFFFF) as u16;
        if vendor != AMD_VENDOR_ID {
            log::debug!(
                "SMBus: PCI 0:{:02x}.0 vendor {:#06x} is not AMD",
                FCH_PCI_DEV,
                vendor
            );
            return None;
        }

        // Read SMBus base I/O address from PCI config offset 0x90.
        // Bits [15:4] contain the base address; bits [3:0] are reserved.
        let base_reg =
            wr.read_pci_config_dword(FCH_PCI_BUS, FCH_PCI_DEV, FCH_PCI_FUNC, FCH_SMBUS_BAR_OFFSET);
        let base = (base_reg & 0xFFF0) as u16;

        if base == 0 || base == 0xFFF0 {
            log::debug!("SMBus: AMD FCH BAR reads {:#06x}, no valid base", base_reg);
            return None;
        }

        log::info!("SMBus: AMD FCH controller at I/O base {:#06x}", base);
        Some(Self { base })
    }

    /// Return the I/O base address (for logging/diagnostics).
    pub fn base_address(&self) -> u16 {
        self.base
    }

    /// Wait for the SMBus host controller to become idle.
    ///
    /// Returns `true` if the bus became free, `false` on timeout.
    fn wait_idle(&self, wr: &WinRing0) -> bool {
        for _ in 0..BUSY_WAIT_ITERS {
            let status = wr.read_io_port_byte(self.base + REG_STS);
            if status & SMB_STS_HOST_BUSY == 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_micros(POLL_DELAY_US));
        }
        false
    }

    /// Wait for transaction completion after starting a protocol.
    ///
    /// Returns `Ok(())` on success (INTR bit set), or `Err(())` on NACK,
    /// bus collision, failure, or timeout.
    fn wait_completion(&self, wr: &WinRing0) -> Result<(), ()> {
        for _ in 0..BUSY_WAIT_ITERS {
            let status = wr.read_io_port_byte(self.base + REG_STS);

            if status & SMB_STS_INTR != 0 {
                // Success — clear status bits
                wr.write_io_port_byte(self.base + REG_STS, status);
                return Ok(());
            }

            if status & (SMB_STS_DEV_ERR | SMB_STS_BUS_COL | SMB_STS_FAILED) != 0 {
                // Error — clear status bits
                wr.write_io_port_byte(self.base + REG_STS, status);
                return Err(());
            }

            std::thread::sleep(std::time::Duration::from_micros(POLL_DELAY_US));
        }
        // Timeout — clear status
        wr.write_io_port_byte(self.base + REG_STS, 0xFF);
        Err(())
    }

    /// Read a single byte from `register` on the device at `slave_addr`.
    ///
    /// Uses the SMBus byte-data read protocol (command 0x48).
    /// `slave_addr` is the 7-bit I2C address (0x00..0x7F).
    pub fn read_byte_data(&self, slave_addr: u8, register: u8) -> Option<u8> {
        let wr = WinRing0::try_load()?;

        // Wait for bus idle
        if !self.wait_idle(wr) {
            log::trace!(
                "SMBus: timeout waiting for idle (addr {:#04x} reg {:#04x})",
                slave_addr,
                register
            );
            return None;
        }

        // Clear any pending status
        wr.write_io_port_byte(self.base + REG_STS, 0xFF);

        // Set slave address with read bit
        wr.write_io_port_byte(self.base + REG_ADDR, (slave_addr << 1) | 1);
        // Set command/register byte
        wr.write_io_port_byte(self.base + REG_CMD, register);
        // Start byte-data read
        wr.write_io_port_byte(self.base + REG_CTL, PROTOCOL_BYTE_DATA);

        // Wait for completion
        if self.wait_completion(wr).is_err() {
            return None;
        }

        // Read result
        Some(wr.read_io_port_byte(self.base + REG_DATA0))
    }

    /// Write a single byte to `register` on the device at `slave_addr`.
    ///
    /// Uses the SMBus byte-data write protocol (command 0x48).
    /// `slave_addr` is the 7-bit I2C address (0x00..0x7F).
    pub fn write_byte_data(&self, slave_addr: u8, register: u8, value: u8) -> Option<()> {
        let wr = WinRing0::try_load()?;

        if !self.wait_idle(wr) {
            return None;
        }

        // Clear status
        wr.write_io_port_byte(self.base + REG_STS, 0xFF);

        // Set slave address with write bit (bit 0 = 0)
        wr.write_io_port_byte(self.base + REG_ADDR, slave_addr << 1);
        // Set command/register byte
        wr.write_io_port_byte(self.base + REG_CMD, register);
        // Set data byte
        wr.write_io_port_byte(self.base + REG_DATA0, value);
        // Start byte-data write
        wr.write_io_port_byte(self.base + REG_CTL, PROTOCOL_BYTE_DATA);

        self.wait_completion(wr).ok()
    }

    /// Read a 16-bit word from `register` on the device at `slave_addr`.
    ///
    /// Uses the SMBus word-data read protocol (command 0x4C).
    /// Returns the word in little-endian order (DATA0 = low, DATA1 = high),
    /// matching the standard SMBus word-data semantics.
    pub fn read_word_data(&self, slave_addr: u8, register: u8) -> Option<u16> {
        let wr = WinRing0::try_load()?;

        if !self.wait_idle(wr) {
            return None;
        }

        // Clear status
        wr.write_io_port_byte(self.base + REG_STS, 0xFF);

        // Set slave address with read bit
        wr.write_io_port_byte(self.base + REG_ADDR, (slave_addr << 1) | 1);
        // Set command/register byte
        wr.write_io_port_byte(self.base + REG_CMD, register);
        // Start word-data read
        wr.write_io_port_byte(self.base + REG_CTL, PROTOCOL_WORD_DATA);

        if self.wait_completion(wr).is_err() {
            return None;
        }

        let low = wr.read_io_port_byte(self.base + REG_DATA0) as u16;
        let high = wr.read_io_port_byte(self.base + REG_DATA1) as u16;
        Some((high << 8) | low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_constants_are_correct() {
        // Byte data protocol: bits [4:2] = 010 (byte data), bit 6 = START, bit 3 = LAST_BYTE
        // 0x48 = 0100_1000
        assert_eq!(PROTOCOL_BYTE_DATA, 0x48);
        // Word data protocol: bits [4:2] = 011 (word data), bit 6 = START, bit 3 = LAST_BYTE
        // 0x4C = 0100_1100
        assert_eq!(PROTOCOL_WORD_DATA, 0x4C);
    }

    #[test]
    fn register_offsets_are_sequential() {
        assert_eq!(REG_STS, 0x00);
        assert_eq!(REG_CTL, 0x02);
        assert_eq!(REG_CMD, 0x03);
        assert_eq!(REG_ADDR, 0x04);
        assert_eq!(REG_DATA0, 0x05);
        assert_eq!(REG_DATA1, 0x06);
    }

    #[test]
    fn slave_address_encoding() {
        // 7-bit addr 0x50 -> write = 0xA0, read = 0xA1
        assert_eq!(0x50_u8 << 1, 0xA0);
        assert_eq!((0x50_u8 << 1) | 1, 0xA1);
    }

    #[test]
    fn detect_returns_none_without_winring0() {
        // Without WinRing0 loaded, detection should return None gracefully
        // (This test will pass in CI where the DLL is not present)
        let result = SmbusController::detect_amd_fch();
        // We can't assert None because the DLL might be present on the build machine,
        // but we verify it doesn't panic.
        let _ = result;
    }
}
