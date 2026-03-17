//! SATA SMART data on Windows via `IOCTL_SMART_RCV_DRIVE_DATA`.
//!
//! Sends an ATA SMART READ DATA command through the Windows SMART IOCTL
//! interface to read the 512-byte SMART data page from SATA/ATA drives.

use crate::model::storage::SmartData;

use std::mem;
use std::ptr;

use winapi::shared::minwindef::{DWORD, FALSE};
use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};

// ---------- SMART IOCTL constants ----------

/// `SMART_RCV_DRIVE_DATA` = CTL_CODE(IOCTL_DISK_BASE, 0x0022, METHOD_BUFFERED, FILE_READ_ACCESS)
const SMART_RCV_DRIVE_DATA: DWORD = 0x0007_C088;

/// ATA SMART READ DATA sub-command.
const SMART_READ_DATA: u8 = 0xD0;

/// ATA SMART command opcode.
const ATA_SMART: u8 = 0xB0;

/// SMART data page size in bytes.
const SMART_DATA_SIZE: usize = 512;

/// Maximum number of SMART attribute entries in the data page.
const MAX_SMART_ATTRS: usize = 30;

/// Size of each SMART attribute entry in bytes.
const SMART_ATTR_SIZE: usize = 12;

// ---------- Windows SMART structures ----------

/// `IDEREGS` — ATA task-file register values.
#[repr(C)]
#[derive(Clone, Copy)]
struct IdRegs {
    features: u8,
    sector_count: u8,
    sector_number: u8,
    cyl_low: u8,
    cyl_high: u8,
    drive_head: u8,
    command: u8,
    reserved: u8,
}

/// `SENDCMDINPARAMS` — input structure for `SMART_RCV_DRIVE_DATA`.
///
/// The real Windows struct has a trailing `BYTE bBuffer[1]` flexible array,
/// but for a SMART READ DATA command the input buffer is unused so we omit it.
#[repr(C)]
#[derive(Clone, Copy)]
struct SendCmdInParams {
    buffer_size: DWORD,
    ir_drive_regs: IdRegs,
    drive_number: u8,
    reserved: [u8; 3],
    reserved2: [DWORD; 4],
    // bBuffer[1] — not needed for input
}

/// `DRIVERSTATUS` — returned by the driver.
#[repr(C)]
#[derive(Clone, Copy)]
struct DriverStatus {
    error: u8,
    ide_status: u8,
    reserved: [u8; 2],
    reserved2: [DWORD; 2],
}

/// Fixed-size output buffer: `SENDCMDOUTPARAMS` header + 512 bytes of SMART
/// data.
///
/// The real `SENDCMDOUTPARAMS` has:
///   - buffer_size (DWORD)
///   - driver_status (DRIVERSTATUS)
///   - bBuffer[1] (flexible)
///
/// We pre-allocate room for the full 512-byte payload.
#[repr(C)]
struct SendCmdOutParams {
    buffer_size: DWORD,
    driver_status: DriverStatus,
    buffer: [u8; SMART_DATA_SIZE],
}

// ---------- ATA SMART attribute parsing ----------

/// A single SMART attribute entry parsed from the 12-byte on-disk format.
#[derive(Debug, Clone, Default)]
pub struct AtaSmartAttribute {
    pub id: u8,
    pub flags: u16,
    pub current_value: u8,
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

// ---------- Win32 helpers ----------

/// Encode a Rust `&str` as a null-terminated wide (UTF-16) string for Win32.
fn to_wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Read SATA SMART data from `\\.\PhysicalDriveN`.
///
/// Returns `None` if the drive cannot be opened (permissions, non-SATA) or the
/// SMART IOCTL fails.
pub fn read_sata_smart(drive_number: u32) -> Option<AtaSmartData> {
    let path = format!("\\\\.\\PhysicalDrive{}", drive_number);
    let wide_path = to_wide(&path);

    unsafe {
        let handle = CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            let err = std::io::Error::last_os_error();
            log::debug!("Failed to open {} for SMART: {}", path, err);
            return None;
        }

        // Build the SMART READ DATA command
        let in_params = SendCmdInParams {
            buffer_size: SMART_DATA_SIZE as DWORD,
            ir_drive_regs: IdRegs {
                features: SMART_READ_DATA,
                sector_count: 1,
                sector_number: 0,
                cyl_low: 0x4F,  // SMART signature
                cyl_high: 0xC2, // SMART signature
                drive_head: 0xA0 | ((drive_number as u8 & 1) << 4),
                command: ATA_SMART,
                reserved: 0,
            },
            drive_number: drive_number as u8,
            reserved: [0; 3],
            reserved2: [0; 4],
        };

        let mut out_params: SendCmdOutParams = mem::zeroed();
        let mut bytes_returned: DWORD = 0;

        let ok = DeviceIoControl(
            handle,
            SMART_RCV_DRIVE_DATA,
            &in_params as *const SendCmdInParams as *mut _,
            mem::size_of::<SendCmdInParams>() as DWORD,
            &mut out_params as *mut SendCmdOutParams as *mut _,
            mem::size_of::<SendCmdOutParams>() as DWORD,
            &mut bytes_returned,
            ptr::null_mut(),
        );

        CloseHandle(handle);

        if ok == FALSE {
            log::debug!("SMART_RCV_DRIVE_DATA failed on {}", path);
            return None;
        }

        // Check for driver-level errors
        if out_params.driver_status.error != 0 {
            log::debug!(
                "SMART driver error on {}: error={} status={}",
                path,
                out_params.driver_status.error,
                out_params.driver_status.ide_status,
            );
            return None;
        }

        Some(AtaSmartData::from_bytes(&out_params.buffer))
    }
}
