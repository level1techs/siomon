//! DDR5 SPD EEPROM reader via SPD5118 hub on whitelisted I2C buses.
//!
//! On some boards the FCH's piix4_smbus intercepts SPD addresses 0x50–0x57,
//! returning garbled EEPROM data instead of SPD5118 management registers.
//! Dedicated I2C controllers (populated via ACPI) bypass this mux and give
//! clean access to the SPD5118 hubs.
//!
//! Boards opt in by setting `ddr5_bus_config` in their `BoardTemplate`
//! (see `db/boards/mod.rs`). Requires `--direct-io`.

use crate::db::boards::{Ddr5BusConfig, Requirement};
use crate::model::memory::SpdData;
use crate::sensors::i2c::bus_scan;
use crate::sensors::i2c::ddr5;
use crate::sensors::i2c::smbus_io::SmbusDevice;

/// SPD5118 management register addresses.
const MR0_DEVICE_TYPE: u8 = 0x00;
const MR11_PAGE_SELECT: u8 = 0x0B;

/// Expected MR0 value for SPD5118.
const SPD5118_DEVICE_ID: u8 = 0x51;

/// NVM region starts at register 0x80 (MemReg bit = 1).
const NVM_BASE: u8 = 0x80;

/// Each EEPROM page is 128 bytes, accessed at registers 0x80–0xFF.
const PAGE_SIZE: usize = 128;

/// Total EEPROM size: 8 pages × 128 bytes = 1024 bytes.
const EEPROM_SIZE: usize = 1024;

/// SPD address range for DDR5 DIMMs.
const SPD_ADDR_FIRST: u16 = 0x50;

/// Result of reading one DIMM's SPD EEPROM.
pub struct SpdDump {
    pub bus: u32,
    pub addr: u16,
    pub data: [u8; EEPROM_SIZE],
}

/// Discover and read SPD EEPROMs on whitelisted I2C buses.
///
/// Returns parsed `SpdData` paired with the serial number read from SPD
/// bytes 517–520 for matching against SMBIOS DIMM entries.
pub fn read_ddr5_spd(
    config: &Ddr5BusConfig,
    ddr5_requirements: &[Requirement],
) -> Vec<(String, SpdData)> {
    let buses = bus_scan::enumerate_buses();
    let dw_buses = ddr5::filter_buses(config, &buses);

    if dw_buses.is_empty() {
        log::debug!("SPD: no whitelisted I2C buses found");
        return Vec::new();
    }

    let mut results = Vec::new();

    for &bus in &dw_buses {
        log::debug!("SPD: scanning bus {} (slots {})", bus, config.slots_per_bus);
        for addr in SPD_ADDR_FIRST..(SPD_ADDR_FIRST + config.slots_per_bus) {
            match read_spd_eeprom(bus, addr) {
                Some(dump) => {
                    let serial = parse_spd_serial(&dump.data);
                    log::debug!(
                        "SPD: read EEPROM from bus {} addr {:#04x} serial={}",
                        bus,
                        addr,
                        serial
                    );
                    if let Some(mut parsed) = parse_ddr5_spd(&dump.data) {
                        parsed.i2c_bus = Some(bus);
                        parsed.i2c_addr = Some(addr);
                        results.push((serial, parsed));
                    }
                }
                None => {
                    log::debug!("SPD: no readable SPD5118 at bus {} addr {:#04x}", bus, addr);
                }
            }
        }
    }

    if results.is_empty() {
        let bios = crate::db::boards::diagnostics::read_bios_version();
        let hints = crate::db::boards::diagnostics::probe_failure_hints(
            "SPD",
            ddr5_requirements,
            bios.as_deref(),
        );
        for hint in &hints {
            log::warn!("SPD: {}", hint);
        }
    }

    results
}

/// Read the full 1024-byte SPD EEPROM from one SPD5118 device.
///
/// Sequence: verify MR0 = 0x51, then for each page 0–7 write
/// MR11\[2:0\] and read 128 bytes from NVM (registers 0x80–0xFF).
/// Always restores page 0 when done.
fn read_spd_eeprom(bus: u32, addr: u16) -> Option<SpdDump> {
    let dev = match SmbusDevice::open(bus, addr) {
        Ok(dev) => dev,
        Err(e) => {
            log::debug!("SPD: open failed on bus {} addr {:#04x}: {}", bus, addr, e);
            return None;
        }
    };

    // Verify SPD5118 device type BEFORE any writes — writing to register 0x0B
    // on a non-SPD5118 device could corrupt EEPROM data.
    let mr0 = match dev.read_byte_data(MR0_DEVICE_TYPE) {
        Ok(mr0) => mr0,
        Err(e) => {
            log::debug!(
                "SPD: MR0 read failed on bus {} addr {:#04x}: {}",
                bus,
                addr,
                e
            );
            return None;
        }
    };
    if mr0 != SPD5118_DEVICE_ID {
        log::debug!(
            "SPD: MR0 mismatch on bus {} addr {:#04x}: got {:#04x}",
            bus,
            addr,
            mr0
        );
        return None;
    }

    // Now that we've confirmed this is an SPD5118, reset to page 0.
    // A prior aborted read may have left MR11 on a non-zero page.
    let _ = dev.write_byte_data(MR11_PAGE_SELECT, 0x00);

    let mut eeprom = [0u8; EEPROM_SIZE];
    let mut ok = true;

    for page in 0u8..8 {
        // Select page — only write bits [2:0], keeping bit 3 = 0.
        if dev.write_byte_data(MR11_PAGE_SELECT, page).is_err() {
            log::warn!(
                "SPD: failed to set page {} on bus {} addr {:#04x}",
                page,
                bus,
                addr
            );
            ok = false;
            break;
        }

        let offset = page as usize * PAGE_SIZE;

        // Read 128 bytes in 32-byte chunks (SMBus I2C block max).
        for chunk_start in (0u8..PAGE_SIZE as u8).step_by(32) {
            let reg = NVM_BASE + chunk_start;
            let remaining = (PAGE_SIZE as u8) - chunk_start;
            let len = remaining.min(32);

            match dev.read_i2c_block_data(reg, len) {
                Ok(bytes) => {
                    if bytes.len() != len as usize {
                        log::warn!(
                            "SPD: short read page {} offset {} on bus {} addr {:#04x}: got {} expected {}",
                            page,
                            chunk_start,
                            bus,
                            addr,
                            bytes.len(),
                            len
                        );
                        ok = false;
                        break;
                    }
                    let dst_start = offset + chunk_start as usize;
                    eeprom[dst_start..dst_start + len as usize].copy_from_slice(&bytes);
                }
                Err(e) => {
                    log::warn!(
                        "SPD: read error page {} offset {} on bus {} addr {:#04x}: {}",
                        page,
                        chunk_start,
                        bus,
                        addr,
                        e
                    );
                    ok = false;
                    break;
                }
            }
        }

        if !ok {
            break;
        }
    }

    // Always restore page 0 so volatile registers (temperature etc.) remain
    // accessible. Retry with exponential backoff — a stuck page is the worst
    // failure mode since it corrupts all subsequent reads until reboot.
    let mut restored = false;
    for delay_ms in [0, 1, 5, 10, 50] {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        if dev.write_byte_data(MR11_PAGE_SELECT, 0x00).is_ok() {
            // Verify by reading MR11 back — bits [2:0] should be 0.
            if dev
                .read_byte_data(MR11_PAGE_SELECT)
                .is_ok_and(|v| v & 0x07 == 0x00)
            {
                restored = true;
                break;
            }
        }
    }
    if !restored {
        log::error!(
            "SPD: failed to restore page 0 on bus {} addr {:#04x} after 5 attempts — \
             temperature reads may return garbage until page 0 is restored",
            bus,
            addr
        );
    }

    if !ok {
        return None;
    }

    Some(SpdDump {
        bus,
        addr,
        data: eeprom,
    })
}

// ---------------------------------------------------------------------------
// DDR5 SPD EEPROM parser (JESD400-5)
// ---------------------------------------------------------------------------

/// Parse a 1024-byte DDR5 SPD EEPROM into structured data.
fn parse_ddr5_spd(data: &[u8; EEPROM_SIZE]) -> Option<SpdData> {
    // Byte 2: Key Byte — DRAM Device Type. 0x12 = DDR5.
    if data[2] != 0x12 {
        log::debug!("SPD: byte 2 = {:#04x}, not DDR5 (0x12)", data[2]);
        return None;
    }

    let spd_revision = format!("{}.{}", data[1] >> 4, data[1] & 0x0F);

    let module_type = match data[3] & 0x0F {
        0x01 => "RDIMM",
        0x02 => "UDIMM",
        0x04 => "LRDIMM",
        0x09 => "DDIMM",
        0x0C => "SODIMM",
        _ => "Unknown",
    }
    .to_string();

    // Byte 4: First SDRAM density and package.
    let die_density_gb = match data[4] & 0x1F {
        0x01 => 1,
        0x02 => 2,
        0x03 => 4,
        0x04 => 8,
        0x05 => 16,
        0x06 => 32,
        0x07 => 64,
        _ => 0,
    };
    let die_per_package = match (data[4] >> 5) & 0x07 {
        0x00 => 1,
        0x01 => 2,
        0x02 => 4,
        0x03 => 8,
        0x04 => 16,
        _ => 1,
    };

    // Byte 5: SDRAM addressing — rows and columns.
    let column_bits = match data[5] & 0x1F {
        0x01 => 10,
        0x02 => 11,
        _ => 10,
    };
    let row_bits = match (data[5] >> 5) & 0x07 {
        0x00 => 16,
        0x01 => 17,
        0x02 => 18,
        _ => 16,
    };

    // Byte 6: First SDRAM I/O width.
    let device_width = match data[6] & 0x07 {
        0x00 => 4,
        0x01 => 8,
        0x02 => 16,
        0x03 => 32,
        _ => 8,
    };

    // Byte 7: Bank groups and banks per group.
    let bank_groups = match data[7] & 0x07 {
        0x00 => 1,
        0x01 => 2,
        0x02 => 4,
        0x03 => 8,
        _ => 4,
    };
    let banks_per_group = match (data[7] >> 5) & 0x07 {
        0x00 => 1,
        0x01 => 2,
        0x02 => 4,
        _ => 4,
    };

    // Timing parameters are stored as 16-bit LE values in picoseconds.
    // Byte 30: tAAmin, 32: tRCDmin, 34: tRPmin, 36: tRASmin, 38: tRCmin.
    let t_aa_ns = parse_timing_ps(data, 30, 31);
    let t_rcd_ns = parse_timing_ps(data, 32, 33);
    let t_rp_ns = parse_timing_ps(data, 34, 35);
    let t_ras_ns = parse_timing_ps(data, 36, 37);
    let t_rc_ns = parse_timing_ps(data, 38, 39);

    // CAS latencies supported: bytes 24–28 (5 bytes = 40 bits).
    // DDR5 CL range starts at CL 20.
    let cas_latencies = parse_cas_latencies(data);

    // Manufacturer info is at byte 512+ (page 4).
    let spd_manufacturer = parse_jedec_manufacturer(data);
    let spd_part_number = parse_part_number(data);

    Some(SpdData {
        spd_revision,
        die_density_gb,
        die_per_package,
        bank_groups,
        banks_per_group,
        column_bits,
        row_bits,
        device_width,
        module_type,
        t_aa_ns,
        t_rcd_ns,
        t_rp_ns,
        t_ras_ns,
        t_rc_ns,
        cas_latencies,
        spd_manufacturer,
        spd_part_number,
        i2c_bus: None,
        i2c_addr: None,
    })
}

/// Parse a DDR5 timing value from two EEPROM bytes.
///
/// DDR5 SPD stores timing values as 16-bit little-endian unsigned integers
/// in picoseconds. Returns the value in nanoseconds.
fn parse_timing_ps(data: &[u8; EEPROM_SIZE], lo: usize, hi: usize) -> Option<f64> {
    if lo >= data.len() || hi >= data.len() {
        return None;
    }
    let ps = u16::from_le_bytes([data[lo], data[hi]]);
    if ps == 0 {
        return None;
    }
    Some(ps as f64 / 1000.0)
}

/// Parse supported CAS latencies from bytes 24–28.
///
/// DDR5 uses 5 bytes (40 bits) to indicate which CAS latencies are supported.
/// Bit 0 of byte 24 corresponds to CL 20, bit 1 to CL 22, etc. (even CLs only).
fn parse_cas_latencies(data: &[u8; EEPROM_SIZE]) -> Vec<u32> {
    let mut cls = Vec::new();
    for byte_idx in 0..5u32 {
        let b = data[24 + byte_idx as usize];
        for bit in 0..8u32 {
            if b & (1 << bit) != 0 {
                // Each bit represents an even CL starting at 20.
                let cl = 20 + (byte_idx * 8 + bit) * 2;
                cls.push(cl);
            }
        }
    }
    cls
}

/// Parse JEDEC manufacturer from SPD bytes 512–513 (module manufacturer).
///
/// DDR5 SPD module manufacturing section starts at byte 512 (page 4, offset 0):
/// - Byte 512: Bank continuation count with odd parity in bit 7
/// - Byte 513: Manufacturer ID within that bank (also with parity bit 7)
fn parse_jedec_manufacturer(data: &[u8; EEPROM_SIZE]) -> Option<String> {
    // Strip parity bits (bit 7) from both bytes.
    let bank = data[512] & 0x7F;
    let id = data[513] & 0x7F;
    if bank == 0 && id == 0 {
        return None;
    }
    Some(jedec_manufacturer_name(bank, id))
}

/// Parse module serial number from SPD bytes 517–520 (4-byte big-endian).
fn parse_spd_serial(data: &[u8; EEPROM_SIZE]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}",
        data[517], data[518], data[519], data[520]
    )
}

/// Parse module part number from SPD bytes 521–550 (30-character ASCII).
fn parse_part_number(data: &[u8; EEPROM_SIZE]) -> Option<String> {
    if data.len() < 551 {
        return None;
    }
    let part = &data[521..551];
    let s: String = part
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
        .collect::<String>()
        .trim()
        .to_string();
    if s.is_empty() || s.chars().all(|c| c == ' ' || c == '\0') {
        None
    } else {
        Some(s)
    }
}

/// Look up a JEDEC manufacturer by bank and ID byte.
///
/// Uses JEDEC JEP106 continuation code scheme: bank byte = number of
/// 0x7F continuation bytes before the actual manufacturer code.
fn jedec_manufacturer_name(bank: u8, id: u8) -> String {
    // Common DDR5 DIMM manufacturers (bank, id) -> name.
    match (bank, id) {
        // Bank 1 (no continuation)
        (0, 0x2C) => "Micron".into(),
        (0, 0xCE) => "Samsung".into(),
        (0, 0xAD) => "SK Hynix".into(),
        // Bank 2
        (1, 0x9E) => "Corsair".into(),
        (1, 0xEF) => "Team Group".into(),
        // Bank 3
        (2, 0x9B) => "Crucial".into(),
        (2, 0x04) => "G.Skill".into(),
        // Bank 5
        (4, 0xF1) => "Kingston".into(),
        (4, 0xCB) => "A-DATA".into(),
        // Bank 7 (6 continuation bytes)
        (6, 0x6D) => "V-Color".into(),
        _ => format!("JEDEC Bank {} ID {:#04x}", bank + 1, id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_parse() {
        let mut data = [0u8; EEPROM_SIZE];
        // 0x3E80 = 16000 ps = 16.0 ns (typical DDR5-4800 tAA)
        data[30] = 0x80;
        data[31] = 0x3E;
        let ns = parse_timing_ps(&data, 30, 31).unwrap();
        assert!((ns - 16.0).abs() < 0.001, "got {ns}");
    }

    #[test]
    fn test_timing_zero() {
        let data = [0u8; EEPROM_SIZE];
        assert!(parse_timing_ps(&data, 30, 31).is_none());
    }

    #[test]
    fn test_cas_latencies() {
        let mut data = [0u8; EEPROM_SIZE];
        // Bit 0 of byte 24 = CL 20
        // Bit 2 of byte 24 = CL 24
        data[24] = 0b00000101;
        let cls = parse_cas_latencies(&data);
        assert_eq!(cls, vec![20, 24]);
    }

    #[test]
    fn test_cas_latencies_higher() {
        let mut data = [0u8; EEPROM_SIZE];
        // Bit 0 of byte 25 = CL 36
        data[25] = 0b00000001;
        let cls = parse_cas_latencies(&data);
        assert_eq!(cls, vec![36]);
    }

    #[test]
    fn test_jedec_manufacturer() {
        assert_eq!(jedec_manufacturer_name(0, 0x2C), "Micron");
        assert_eq!(jedec_manufacturer_name(0, 0xCE), "Samsung");
        assert_eq!(jedec_manufacturer_name(6, 0x6D), "V-Color");
    }

    #[test]
    fn test_part_number_parse() {
        let mut data = [0u8; EEPROM_SIZE];
        let part = b"TRA596G60D436O                ";
        data[521..551].copy_from_slice(&part[..30]);
        let pn = parse_part_number(&data).unwrap();
        assert_eq!(pn, "TRA596G60D436O");
    }

    #[test]
    fn test_part_number_empty() {
        let data = [0u8; EEPROM_SIZE];
        assert!(parse_part_number(&data).is_none());
    }

    #[test]
    fn test_spd_serial() {
        let mut data = [0u8; EEPROM_SIZE];
        data[517] = 0x00;
        data[518] = 0x00;
        data[519] = 0x07;
        data[520] = 0xA1;
        assert_eq!(parse_spd_serial(&data), "000007A1");
    }

    #[test]
    fn test_jedec_manufacturer_parity() {
        // Test through parse_jedec_manufacturer to exercise parity stripping.
        // Bank byte 0x86 has parity bit set; actual bank = 6 → Bank 7 (V-Color).
        // ID byte 0xED has parity bit set; actual ID = 0x6D.
        let mut data = [0u8; EEPROM_SIZE];
        data[512] = 0x86; // bank with parity
        data[513] = 0xED; // ID with parity
        let vendor = parse_jedec_manufacturer(&data).unwrap();
        assert_eq!(vendor, "V-Color");
    }

    #[test]
    fn test_parse_ddr5_valid_header() {
        let mut data = [0u8; EEPROM_SIZE];
        data[1] = 0x10; // revision 1.0
        data[2] = 0x12; // DDR5
        data[3] = 0x01; // RDIMM
        data[4] = 0x05; // 16 Gb die
        data[5] = 0x21; // 17 rows, 10 cols
        data[6] = 0x00; // x4
        data[7] = 0x02; // 4 bank groups, 1 bank/group
        // Set a CAS latency bit so the vec isn't empty
        data[24] = 0x01; // CL 20

        let spd = parse_ddr5_spd(&data).unwrap();
        assert_eq!(spd.spd_revision, "1.0");
        assert_eq!(spd.module_type, "RDIMM");
        assert_eq!(spd.die_density_gb, 16);
        assert_eq!(spd.die_per_package, 1);
        assert_eq!(spd.row_bits, 17);
        assert_eq!(spd.column_bits, 10);
        assert_eq!(spd.device_width, 4);
        assert_eq!(spd.bank_groups, 4);
        assert!(spd.cas_latencies.contains(&20));
    }

    #[test]
    fn test_parse_non_ddr5_rejected() {
        let mut data = [0u8; EEPROM_SIZE];
        data[2] = 0x0C; // DDR4
        assert!(parse_ddr5_spd(&data).is_none());
    }
}
