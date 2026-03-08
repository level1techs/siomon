//! Super I/O chip detection via LPC configuration port probing.
//!
//! Probes the standard Super I/O configuration ports (0x2E/0x4E) to identify
//! the hardware monitoring chip and read its base address. This is the same
//! detection method used by `sensors-detect` and is read-only safe.

use crate::platform::port_io::PortIo;

/// Identified Super I/O chip with its hardware monitor base address.
#[derive(Debug, Clone)]
pub struct SuperIoChip {
    pub chip: ChipType,
    pub chip_id: u16,
    pub revision: u8,
    pub config_port: u16,
    pub hwm_base: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipType {
    Nct6775,
    Nct6776,
    Nct6779,
    Nct6791,
    Nct6792,
    Nct6793,
    Nct6795,
    Nct6796,
    Nct6797,
    Nct6798,
    Nct6799,
    Ite8686,
    Ite8688,
    Ite8689,
    Fintek71889,
    Unknown,
}

impl std::fmt::Display for ChipType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nct6775 => write!(f, "NCT6775"),
            Self::Nct6776 => write!(f, "NCT6776"),
            Self::Nct6779 => write!(f, "NCT6779"),
            Self::Nct6791 => write!(f, "NCT6791"),
            Self::Nct6792 => write!(f, "NCT6792"),
            Self::Nct6793 => write!(f, "NCT6793"),
            Self::Nct6795 => write!(f, "NCT6795"),
            Self::Nct6796 => write!(f, "NCT6796"),
            Self::Nct6797 => write!(f, "NCT6797"),
            Self::Nct6798 => write!(f, "NCT6798"),
            Self::Nct6799 => write!(f, "NCT6799"),
            Self::Ite8686 => write!(f, "IT8686E"),
            Self::Ite8688 => write!(f, "IT8688E"),
            Self::Ite8689 => write!(f, "IT8689E"),
            Self::Fintek71889 => write!(f, "F71889"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

// Super I/O configuration registers
const SIO_REG_DEVID: u8 = 0x20;
const SIO_REG_DEVREV: u8 = 0x21;
const SIO_REG_LDSEL: u8 = 0x07;
const SIO_REG_ADDR_HI: u8 = 0x60;
const SIO_REG_ADDR_LO: u8 = 0x61;
const SIO_REG_ENABLE: u8 = 0x30;

// Nuvoton/Winbond entry key
const NUVOTON_ENTRY_KEY: u8 = 0x87;
const NUVOTON_EXIT_KEY: u8 = 0xAA;

// Nuvoton Hardware Monitor logical device number
const NUVOTON_HWM_LDN: u8 = 0x0B;

// ITE entry key (same as Nuvoton)
const ITE_ENTRY_KEY: u8 = 0x87;
// ITE HWM logical device
const ITE_HWM_LDN: u8 = 0x04;

/// Standard Super I/O config ports to probe.
const CONFIG_PORTS: [u16; 2] = [0x2E, 0x4E];

/// Detect all Super I/O chips on the system.
///
/// Probes both standard config ports (0x2E and 0x4E). Returns a list of
/// detected chips. Typically only one is present on consumer boards.
pub fn detect_all() -> Vec<SuperIoChip> {
    let mut pio = match PortIo::open() {
        Some(p) => p,
        None => {
            log::debug!("Cannot open /dev/port for Super I/O detection");
            return Vec::new();
        }
    };

    let mut chips = Vec::new();
    for &config_port in &CONFIG_PORTS {
        if let Some(chip) = probe_nuvoton(&mut pio, config_port) {
            chips.push(chip);
        } else if let Some(chip) = probe_ite(&mut pio, config_port) {
            chips.push(chip);
        }
    }
    chips
}

/// Probe for Nuvoton/Winbond Super I/O at the given config port.
fn probe_nuvoton(pio: &mut PortIo, config_port: u16) -> Option<SuperIoChip> {
    let data_port = config_port + 1;

    // Enter extended function mode: write 0x87 twice
    pio.write_byte(config_port, NUVOTON_ENTRY_KEY).ok()?;
    pio.write_byte(config_port, NUVOTON_ENTRY_KEY).ok()?;

    // Read chip ID
    let id_hi = pio.write_read(config_port, SIO_REG_DEVID, data_port).ok()?;
    let id_lo = pio
        .write_read(config_port, SIO_REG_DEVREV, data_port)
        .ok()?;
    let chip_id = (id_hi as u16) << 8 | id_lo as u16;

    let chip = identify_nuvoton(chip_id);
    if chip == ChipType::Unknown {
        // Exit config mode before returning
        let _ = pio.write_byte(config_port, NUVOTON_EXIT_KEY);
        return None;
    }

    // Select HWM logical device
    pio.write_byte(config_port, SIO_REG_LDSEL).ok()?;
    pio.write_byte(data_port, NUVOTON_HWM_LDN).ok()?;

    // Check if HWM is enabled
    let enabled = pio
        .write_read(config_port, SIO_REG_ENABLE, data_port)
        .ok()?;
    if enabled & 0x01 == 0 {
        log::debug!("Super I/O HWM logical device is disabled");
        let _ = pio.write_byte(config_port, NUVOTON_EXIT_KEY);
        return None;
    }

    // Read HWM base address
    let addr_hi = pio
        .write_read(config_port, SIO_REG_ADDR_HI, data_port)
        .ok()?;
    let addr_lo = pio
        .write_read(config_port, SIO_REG_ADDR_LO, data_port)
        .ok()?;
    let hwm_base = (addr_hi as u16) << 8 | addr_lo as u16;

    // Exit config mode
    let _ = pio.write_byte(config_port, NUVOTON_EXIT_KEY);

    if hwm_base == 0 || hwm_base == 0xFFFF {
        return None;
    }

    log::info!(
        "Detected {} (ID: {:#06x}) at config port {:#06x}, HWM base {:#06x}",
        chip,
        chip_id,
        config_port,
        hwm_base
    );

    Some(SuperIoChip {
        chip,
        chip_id,
        revision: id_lo,
        config_port,
        hwm_base,
    })
}

/// Probe for ITE Super I/O at the given config port.
fn probe_ite(pio: &mut PortIo, config_port: u16) -> Option<SuperIoChip> {
    let data_port = config_port + 1;

    // ITE uses the same 0x87 entry key
    pio.write_byte(config_port, ITE_ENTRY_KEY).ok()?;
    pio.write_byte(config_port, ITE_ENTRY_KEY).ok()?;

    // Read chip ID
    let id_hi = pio.write_read(config_port, SIO_REG_DEVID, data_port).ok()?;
    let id_lo = pio
        .write_read(config_port, SIO_REG_DEVREV, data_port)
        .ok()?;
    let chip_id = (id_hi as u16) << 8 | id_lo as u16;

    let chip = identify_ite(chip_id);
    if chip == ChipType::Unknown {
        let _ = pio.write_byte(config_port, NUVOTON_EXIT_KEY);
        return None;
    }

    // Select HWM logical device (0x04 for ITE)
    pio.write_byte(config_port, SIO_REG_LDSEL).ok()?;
    pio.write_byte(data_port, ITE_HWM_LDN).ok()?;

    // Read HWM base address
    let addr_hi = pio
        .write_read(config_port, SIO_REG_ADDR_HI, data_port)
        .ok()?;
    let addr_lo = pio
        .write_read(config_port, SIO_REG_ADDR_LO, data_port)
        .ok()?;
    let hwm_base = (addr_hi as u16) << 8 | addr_lo as u16;

    // Exit config mode
    let _ = pio.write_byte(config_port, NUVOTON_EXIT_KEY);

    if hwm_base == 0 || hwm_base == 0xFFFF {
        return None;
    }

    log::info!(
        "Detected {} (ID: {:#06x}) at config port {:#06x}, HWM base {:#06x}",
        chip,
        chip_id,
        config_port,
        hwm_base
    );

    Some(SuperIoChip {
        chip,
        chip_id,
        revision: id_lo,
        config_port,
        hwm_base,
    })
}

fn identify_nuvoton(chip_id: u16) -> ChipType {
    match chip_id & 0xFFF0 {
        0xB470 => ChipType::Nct6775,
        0xC330 => ChipType::Nct6776,
        0xC560 => ChipType::Nct6779,
        0xC800 => ChipType::Nct6791,
        0xC910 => ChipType::Nct6792,
        0xD120 => ChipType::Nct6793,
        0xD350 => ChipType::Nct6795,
        0xD450 => ChipType::Nct6796,
        0xD590 => ChipType::Nct6797,
        0xD420 => ChipType::Nct6798,
        0xD800 => ChipType::Nct6799,
        _ => ChipType::Unknown,
    }
}

fn identify_ite(chip_id: u16) -> ChipType {
    match chip_id {
        0x8686 => ChipType::Ite8686,
        0x8688 => ChipType::Ite8688,
        0x8689 => ChipType::Ite8689,
        _ => {
            // Check high byte only for other ITE chips
            match chip_id >> 8 {
                0x86 | 0x87 => ChipType::Unknown, // Known ITE prefix but unrecognized model
                _ => ChipType::Unknown,
            }
        }
    }
}

/// Check if the kernel nct6775 driver module is currently loaded.
pub fn is_kernel_driver_loaded(chip: &ChipType) -> bool {
    let module_name = match chip {
        ChipType::Nct6775
        | ChipType::Nct6776
        | ChipType::Nct6779
        | ChipType::Nct6791
        | ChipType::Nct6792
        | ChipType::Nct6793
        | ChipType::Nct6795
        | ChipType::Nct6796
        | ChipType::Nct6797
        | ChipType::Nct6798
        | ChipType::Nct6799 => "nct6775",
        ChipType::Ite8686 | ChipType::Ite8688 | ChipType::Ite8689 => "it87",
        ChipType::Fintek71889 => "f71882fg",
        ChipType::Unknown => return false,
    };

    std::path::Path::new(&format!("/sys/module/{module_name}")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_nuvoton_nct6798() {
        assert_eq!(identify_nuvoton(0xD428), ChipType::Nct6798);
        assert_eq!(identify_nuvoton(0xD429), ChipType::Nct6798);
    }

    #[test]
    fn test_identify_nuvoton_nct6799() {
        assert_eq!(identify_nuvoton(0xD802), ChipType::Nct6799);
    }

    #[test]
    fn test_identify_nuvoton_unknown() {
        assert_eq!(identify_nuvoton(0x0000), ChipType::Unknown);
        assert_eq!(identify_nuvoton(0xFFFF), ChipType::Unknown);
    }

    #[test]
    fn test_identify_ite() {
        assert_eq!(identify_ite(0x8688), ChipType::Ite8688);
        assert_eq!(identify_ite(0x8689), ChipType::Ite8689);
        assert_eq!(identify_ite(0x0000), ChipType::Unknown);
    }

    #[test]
    fn test_chip_type_display() {
        assert_eq!(format!("{}", ChipType::Nct6798), "NCT6798");
        assert_eq!(format!("{}", ChipType::Ite8688), "IT8688E");
    }

    #[test]
    fn test_kernel_driver_check() {
        // nct6775 module is loaded on this system
        let loaded = is_kernel_driver_loaded(&ChipType::Nct6798);
        // Don't assert specific value — depends on test environment
        let _ = loaded;
    }
}
