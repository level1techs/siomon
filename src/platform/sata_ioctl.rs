//! SATA SMART data reading via SG_IO ioctl + ATA PASS-THROUGH(12).
//!
//! Sends an ATA SMART READ DATA command through the SCSI Generic (SG_IO)
//! interface to read the 512-byte SMART data page from SATA devices.

use crate::model::storage::SmartData;
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::path::Path;

// SG_IO ioctl request number
const SG_IO: libc::Ioctl = 0x2285;

// SG_IO data transfer direction: from device to host
const SG_DXFER_FROM_DEV: i32 = -3;

// SMART data page size in bytes
const SMART_DATA_SIZE: usize = 512;

// Maximum number of SMART attribute entries in the data page
const MAX_SMART_ATTRS: usize = 30;

// Size of each SMART attribute entry in bytes
const SMART_ATTR_SIZE: usize = 12;

/// Linux SG_IO header structure for SCSI Generic I/O.
///
/// Pointer fields use `usize` to match native pointer width (4 bytes on
/// 32-bit, 8 bytes on 64-bit), matching the kernel's `void *` / `unsigned char *`.
#[repr(C)]
#[derive(Default)]
struct SgIoHdr {
    interface_id: i32,
    dxfer_direction: i32,
    cmd_len: u8,
    mx_sb_len: u8,
    iovec_count: u16,
    dxfer_len: u32,
    dxferp: usize,
    cmdp: usize,
    sbp: usize,
    timeout: u32,
    flags: u32,
    pack_id: i32,
    usr_ptr: usize,
    status: u8,
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8,
    host_status: u16,
    driver_status: u16,
    resid: i32,
    duration: u32,
    info: u32,
}

/// A single SMART attribute entry parsed from the 12-byte on-disk format.
#[derive(Debug, Clone, Default)]
pub struct AtaSmartAttribute {
    pub id: u8,
    #[allow(dead_code)] // Public API field for SMART threshold analysis
    pub flags: u16,
    #[allow(dead_code)] // Public API field for SMART threshold analysis
    pub current_value: u8,
    #[allow(dead_code)] // Public API field for SMART threshold analysis
    pub worst_value: u8,
    pub raw_value: [u8; 6],
}

impl AtaSmartAttribute {
    /// Parse a 12-byte SMART attribute entry.
    pub fn from_bytes(data: &[u8; SMART_ATTR_SIZE]) -> Self {
        Self {
            id: data[0],
            flags: u16::from_le_bytes([data[1], data[2]]),
            current_value: data[3],
            worst_value: data[4],
            raw_value: [data[5], data[6], data[7], data[8], data[9], data[10]],
        }
    }

    /// Return the raw value as a u48 (stored in a u64).
    pub fn raw_u48(&self) -> u64 {
        u64::from(self.raw_value[0])
            | (u64::from(self.raw_value[1]) << 8)
            | (u64::from(self.raw_value[2]) << 16)
            | (u64::from(self.raw_value[3]) << 24)
            | (u64::from(self.raw_value[4]) << 32)
            | (u64::from(self.raw_value[5]) << 40)
    }
}

/// Parsed SMART data from a SATA device (512-byte data page).
#[derive(Debug, Clone)]
pub struct AtaSmartData {
    #[allow(dead_code)] // Public API field for SMART version checking
    pub revision: u16,
    pub attributes: Vec<AtaSmartAttribute>,
}

impl AtaSmartData {
    /// Parse the 512-byte SMART data page.
    pub fn from_bytes(data: &[u8; SMART_DATA_SIZE]) -> Self {
        let revision = u16::from_le_bytes([data[0], data[1]]);
        let mut attributes = Vec::new();

        for i in 0..MAX_SMART_ATTRS {
            let offset = 2 + i * SMART_ATTR_SIZE;
            let entry: [u8; SMART_ATTR_SIZE] =
                data[offset..offset + SMART_ATTR_SIZE].try_into().unwrap();
            let attr = AtaSmartAttribute::from_bytes(&entry);
            // ID 0 means unused entry
            if attr.id != 0 {
                attributes.push(attr);
            }
        }

        Self {
            revision,
            attributes,
        }
    }

    /// Look up an attribute by its SMART ID.
    pub fn find_attr(&self, id: u8) -> Option<&AtaSmartAttribute> {
        self.attributes.iter().find(|a| a.id == id)
    }
}

/// Build the 12-byte ATA PASS-THROUGH(12) CDB for SMART READ DATA.
fn build_smart_read_cdb() -> [u8; 12] {
    [
        0xA1, // ATA PASS-THROUGH(12) opcode
        0x08, // protocol: PIO Data-In (4 << 1)
        0x2E, // T_DIR=1 (from device), BYT_BLOK=1, T_LENGTH=2 (sectors)
        0xD0, // feature: SMART READ DATA
        0x01, // sector count: 1
        0x00, // LBA low
        0x4F, // LBA mid: SMART signature
        0xC2, // LBA high: SMART signature
        0x00, // device
        0xB0, // command: SMART
        0x00, // reserved
        0x00, // reserved
    ]
}

/// Read SATA SMART data from a block device (e.g. `/dev/sda`).
///
/// Returns `None` if the device cannot be opened or the ioctl fails.
pub fn read_sata_smart(device_path: &Path) -> Option<AtaSmartData> {
    let file = OpenOptions::new()
        .read(true)
        .open(device_path)
        .map_err(|e| {
            log::debug!("Failed to open {}: {}", device_path.display(), e);
            e
        })
        .ok()?;

    let fd = file.as_raw_fd();

    let mut data_buf = [0u8; SMART_DATA_SIZE];
    let mut sense_buf = [0u8; 32];
    let cdb = build_smart_read_cdb();

    let mut hdr = SgIoHdr {
        interface_id: b'S' as i32,
        dxfer_direction: SG_DXFER_FROM_DEV,
        cmd_len: 12,
        mx_sb_len: sense_buf.len() as u8,
        dxfer_len: SMART_DATA_SIZE as u32,
        dxferp: data_buf.as_mut_ptr() as usize,
        cmdp: cdb.as_ptr() as usize,
        sbp: sense_buf.as_mut_ptr() as usize,
        timeout: 5000,
        ..Default::default()
    };

    let ret = unsafe { libc::ioctl(fd, SG_IO, &mut hdr as *mut SgIoHdr) };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        log::debug!(
            "SG_IO SMART ioctl failed on {}: {}",
            device_path.display(),
            err
        );
        return None;
    }

    // Check for SCSI errors
    if hdr.status != 0 || hdr.host_status != 0 || hdr.driver_status != 0 {
        log::debug!(
            "SG_IO SMART returned error status on {}: scsi={} host={} driver={}",
            device_path.display(),
            hdr.status,
            hdr.host_status,
            hdr.driver_status
        );
        return None;
    }

    Some(AtaSmartData::from_bytes(&data_buf))
}

/// Convert parsed SATA SMART attributes to the common `SmartData` struct.
///
/// SATA SMART attribute mapping:
/// - ID 5:   Reallocated Sectors Count -> media_errors
/// - ID 9:   Power-On Hours -> power_on_hours
/// - ID 12:  Power Cycle Count -> power_cycles
/// - ID 190/194: Temperature -> temperature_celsius (raw low byte)
/// - ID 197: Current Pending Sector Count -> added to media_errors
/// - ID 198: Uncorrectable Sector Count -> num_error_log_entries
/// - ID 241: Total LBAs Written -> total_bytes_written (* 512)
/// - ID 242: Total LBAs Read -> total_bytes_read (* 512)
pub fn sata_smart_to_smart_data(ata: &AtaSmartData) -> SmartData {
    let power_on_hours = ata.find_attr(9).map(|a| a.raw_u48()).unwrap_or(0);
    let power_cycles = ata.find_attr(12).map(|a| a.raw_u48()).unwrap_or(0);

    // Temperature: prefer ID 194, fall back to ID 190. Low byte of raw value.
    let temperature_celsius = ata
        .find_attr(194)
        .or_else(|| ata.find_attr(190))
        .map(|a| a.raw_value[0] as i32)
        .unwrap_or(0);

    // Media errors: reallocated sectors (ID 5) + current pending sectors (ID 197)
    let reallocated = ata.find_attr(5).map(|a| a.raw_u48()).unwrap_or(0);
    let pending = ata.find_attr(197).map(|a| a.raw_u48()).unwrap_or(0);
    let media_errors = reallocated.saturating_add(pending);

    let num_error_log_entries = ata.find_attr(198).map(|a| a.raw_u48()).unwrap_or(0);

    let total_lbas_written = ata.find_attr(241).map(|a| a.raw_u48()).unwrap_or(0);
    let total_lbas_read = ata.find_attr(242).map(|a| a.raw_u48()).unwrap_or(0);

    SmartData {
        temperature_celsius,
        available_spare_pct: 0,
        available_spare_threshold_pct: 0,
        percentage_used: 0,
        data_units_read: 0,
        data_units_written: 0,
        host_read_commands: 0,
        host_write_commands: 0,
        controller_busy_time_minutes: 0,
        power_cycles: u128::from(power_cycles),
        power_on_hours: u128::from(power_on_hours),
        unsafe_shutdowns: 0,
        media_errors: u128::from(media_errors),
        num_error_log_entries: u128::from(num_error_log_entries),
        warning_composite_temp_time_minutes: 0,
        critical_composite_temp_time_minutes: 0,
        critical_warning: 0,
        total_bytes_read: u128::from(total_lbas_read).saturating_mul(512),
        total_bytes_written: u128::from(total_lbas_written).saturating_mul(512),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sg_io_hdr_size() {
        let size = std::mem::size_of::<SgIoHdr>();
        // sg_io_hdr_t has 4 pointer-sized fields; rest is fixed.
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size, 88, "SgIoHdr size mismatch on 64-bit");
        #[cfg(target_pointer_width = "32")]
        assert_eq!(size, 68, "SgIoHdr size mismatch on 32-bit");
    }

    #[test]
    fn test_parse_smart_attribute() {
        // Build a 12-byte entry: ID=194 (temperature), raw value low byte = 42
        let mut entry = [0u8; 12];
        entry[0] = 194; // attribute ID
        entry[1] = 0x03; // flags low byte
        entry[2] = 0x00; // flags high byte
        entry[3] = 100; // current value
        entry[4] = 95; // worst value
        entry[5] = 42; // raw[0] = temperature
        entry[6] = 0; // raw[1]
        entry[7] = 0; // raw[2]
        entry[8] = 0; // raw[3]
        entry[9] = 0; // raw[4]
        entry[10] = 0; // raw[5]
        entry[11] = 0; // reserved

        let attr = AtaSmartAttribute::from_bytes(&entry);
        assert_eq!(attr.id, 194);
        assert_eq!(attr.flags, 0x0003);
        assert_eq!(attr.current_value, 100);
        assert_eq!(attr.worst_value, 95);
        assert_eq!(attr.raw_u48(), 42);
        assert_eq!(attr.raw_value[0], 42);
    }

    #[test]
    fn test_parse_smart_data_page() {
        let mut page = [0u8; 512];
        // Revision number = 1
        page[0] = 0x01;
        page[1] = 0x00;

        // Attribute 0 at offset 2: ID=9 (Power-On Hours), raw = 1234
        page[2] = 9;
        page[7] = 0xD2; // 1234 = 0x04D2
        page[8] = 0x04;

        // Attribute 1 at offset 14: ID=194 (Temperature), raw[0] = 35
        page[14] = 194;
        page[19] = 35;

        // Attribute 2 at offset 26: ID=241 (Total LBAs Written), raw = 100000
        page[26] = 241;
        let lbas: u64 = 100_000;
        page[31] = (lbas & 0xFF) as u8;
        page[32] = ((lbas >> 8) & 0xFF) as u8;
        page[33] = ((lbas >> 16) & 0xFF) as u8;

        let ata = AtaSmartData::from_bytes(&page);
        assert_eq!(ata.revision, 1);
        assert_eq!(ata.attributes.len(), 3);

        let poh = ata.find_attr(9).unwrap();
        assert_eq!(poh.raw_u48(), 1234);

        let temp = ata.find_attr(194).unwrap();
        assert_eq!(temp.raw_value[0], 35);

        let written = ata.find_attr(241).unwrap();
        assert_eq!(written.raw_u48(), 100_000);
    }

    #[test]
    fn test_sata_smart_to_smart_data() {
        let mut page = [0u8; 512];
        page[0] = 0x01; // revision

        // ID=5 (Reallocated Sectors): raw = 3
        let mut offset = 2;
        page[offset] = 5;
        page[offset + 5] = 3;

        // ID=9 (Power-On Hours): raw = 5000
        offset += SMART_ATTR_SIZE;
        page[offset] = 9;
        page[offset + 5] = (5000 & 0xFF) as u8;
        page[offset + 6] = ((5000 >> 8) & 0xFF) as u8;

        // ID=12 (Power Cycles): raw = 200
        offset += SMART_ATTR_SIZE;
        page[offset] = 12;
        page[offset + 5] = 200;

        // ID=194 (Temperature): raw[0] = 38
        offset += SMART_ATTR_SIZE;
        page[offset] = 194;
        page[offset + 5] = 38;

        // ID=197 (Current Pending Sectors): raw = 1
        offset += SMART_ATTR_SIZE;
        page[offset] = 197;
        page[offset + 5] = 1;

        // ID=198 (Uncorrectable Sector Count): raw = 2
        offset += SMART_ATTR_SIZE;
        page[offset] = 198;
        page[offset + 5] = 2;

        // ID=241 (Total LBAs Written): raw = 50000
        offset += SMART_ATTR_SIZE;
        page[offset] = 241;
        let lbas_w: u64 = 50_000;
        page[offset + 5] = (lbas_w & 0xFF) as u8;
        page[offset + 6] = ((lbas_w >> 8) & 0xFF) as u8;

        // ID=242 (Total LBAs Read): raw = 80000
        offset += SMART_ATTR_SIZE;
        page[offset] = 242;
        let lbas_r: u64 = 80_000;
        page[offset + 5] = (lbas_r & 0xFF) as u8;
        page[offset + 6] = ((lbas_r >> 8) & 0xFF) as u8;
        page[offset + 7] = ((lbas_r >> 16) & 0xFF) as u8;

        let ata = AtaSmartData::from_bytes(&page);
        let smart = sata_smart_to_smart_data(&ata);

        assert_eq!(smart.temperature_celsius, 38);
        assert_eq!(smart.power_on_hours, 5000);
        assert_eq!(smart.power_cycles, 200);
        // media_errors = reallocated(3) + pending(1) = 4
        assert_eq!(smart.media_errors, 4);
        assert_eq!(smart.num_error_log_entries, 2);
        assert_eq!(smart.total_bytes_written, u128::from(lbas_w) * 512);
        assert_eq!(smart.total_bytes_read, u128::from(lbas_r) * 512);
        // NVMe-specific fields should be zero for SATA
        assert_eq!(smart.available_spare_pct, 0);
        assert_eq!(smart.percentage_used, 0);
        assert_eq!(smart.critical_warning, 0);
    }

    #[test]
    fn test_smart_cdb() {
        let cdb = build_smart_read_cdb();
        assert_eq!(cdb[0], 0xA1); // ATA PASS-THROUGH(12)
        assert_eq!(cdb[3], 0xD0); // SMART READ DATA feature
        assert_eq!(cdb[6], 0x4F); // SMART signature LBA mid
        assert_eq!(cdb[7], 0xC2); // SMART signature LBA high
        assert_eq!(cdb[9], 0xB0); // SMART command
    }
}
