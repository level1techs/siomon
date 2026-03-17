//! NVMe SMART/Health data on Windows via IOCTL_STORAGE_QUERY_PROPERTY.
//!
//! Reads the SMART/Health Information Log Page (0x02) from an NVMe controller
//! using `DeviceIoControl` with `IOCTL_STORAGE_QUERY_PROPERTY` on
//! `\\.\PhysicalDriveN`.

use crate::model::storage::SmartData;

use std::mem;
use std::ptr;

use winapi::shared::minwindef::{DWORD, FALSE};
use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};

// ---------- NVMe SMART log (NVMe spec, 512 bytes) ----------

/// NVMe SMART/Health Information (Log Page 0x02) - 512 bytes.
///
/// Same layout on every OS; defined by the NVMe specification.
#[repr(C)]
pub struct NvmeSmartLog {
    pub critical_warning: u8,
    pub temperature: [u8; 2],
    pub avail_spare: u8,
    pub spare_thresh: u8,
    pub percent_used: u8,
    pub endu_grp_crit_warn_sumry: u8,
    pub rsvd7: [u8; 25],
    pub data_units_read: [u8; 16],
    pub data_units_written: [u8; 16],
    pub host_reads: [u8; 16],
    pub host_writes: [u8; 16],
    pub ctrl_busy_time: [u8; 16],
    pub power_cycles: [u8; 16],
    pub power_on_hours: [u8; 16],
    pub unsafe_shutdowns: [u8; 16],
    pub media_errors: [u8; 16],
    pub num_err_log_entries: [u8; 16],
    pub warning_temp_time: u32,
    pub critical_comp_time: u32,
    pub temp_sensor: [u16; 8],
    pub rsvd_tail: [u8; 296],
}

// ---------- Windows storage protocol structures ----------

/// `IOCTL_STORAGE_QUERY_PROPERTY` = CTL_CODE(IOCTL_STORAGE_BASE, 0x0500, METHOD_BUFFERED, FILE_ANY_ACCESS)
const IOCTL_STORAGE_QUERY_PROPERTY: DWORD = 0x002D1400;

/// `StorageDeviceProtocolSpecificProperty` (property ID 50).
const PROPERTY_ID_DEVICE: u32 = 50;
/// `StorageAdapterProtocolSpecificProperty` (property ID 49).
const PROPERTY_ID_ADAPTER: u32 = 49;

/// `ProtocolTypeNvme` = 3.
const STORAGE_PROTOCOL_TYPE_NVME: u32 = 3;

/// `NVMeDataTypeLogPage` = 2.
const NVME_DATA_TYPE_LOG_PAGE: u32 = 2;

/// NVMe SMART/Health log page identifier.
const NVME_LOG_PAGE_SMART: u32 = 2;

/// Size of the SMART log in bytes.
const SMART_LOG_SIZE: u32 = 512;

/// `STORAGE_PROTOCOL_SPECIFIC_DATA` — 40-byte struct matching the Windows SDK
/// definition exactly (7 named fields + Reserved[3]).
///
/// Using the wrong size causes ERROR_INVALID_FUNCTION on some NVMe drivers.
#[repr(C)]
#[derive(Clone, Copy)]
struct StorageProtocolSpecificData {
    protocol_type: u32,
    data_type: u32,
    protocol_data_request_value: u32,
    protocol_data_request_sub_value: u32,
    protocol_data_offset: u32,
    protocol_data_length: u32,
    fixed_protocol_return_data: u32,
    reserved: [u32; 3],
}

/// Combined query header + protocol-specific data.
///
/// Matches the Windows `STORAGE_PROPERTY_QUERY` layout where
/// `AdditionalParameters[1]` is replaced with a full
/// `STORAGE_PROTOCOL_SPECIFIC_DATA`.
#[repr(C)]
#[derive(Clone, Copy)]
struct StoragePropertyQuery {
    property_id: u32,
    query_type: u32, // 0 = PropertyStandardQuery
    protocol_specific: StorageProtocolSpecificData,
}

/// Output buffer layout: the Windows driver writes the
/// `STORAGE_PROTOCOL_DATA_DESCRIPTOR` header followed by the raw NVMe log
/// page bytes.
///
/// `STORAGE_PROTOCOL_DATA_DESCRIPTOR`:
///   - Version (DWORD)
///   - Size    (DWORD)
///   - ProtocolSpecificData (STORAGE_PROTOCOL_SPECIFIC_DATA)
///     then the actual log page payload at the offset indicated by
///     `ProtocolSpecificData.ProtocolDataOffset`.
///
/// We allocate a buffer large enough for the descriptor header + 512 bytes.
const PROTOCOL_SPECIFIC_DATA_SIZE: usize = mem::size_of::<StorageProtocolSpecificData>();
const DATA_DESCRIPTOR_HEADER: usize = 8; // Version + Size
const OUTPUT_BUF_SIZE: usize =
    DATA_DESCRIPTOR_HEADER + PROTOCOL_SPECIFIC_DATA_SIZE + SMART_LOG_SIZE as usize;

// ---------- helpers ----------

/// Convert the NVMe SMART temperature field (Kelvin, little-endian) to Celsius.
pub fn nvme_smart_temperature_celsius(log: &NvmeSmartLog) -> i32 {
    let kelvin = u16::from_le_bytes(log.temperature) as i32;
    kelvin - 273
}

/// Convert a 16-byte little-endian field to u128.
pub fn nvme_smart_read_u128(bytes: &[u8; 16]) -> u128 {
    u128::from_le_bytes(*bytes)
}

/// Convert NVMe data units (each unit = 1000 * 512 bytes = 512000 bytes) to
/// total bytes.
pub fn nvme_smart_data_bytes(data_units: u128) -> u128 {
    data_units.saturating_mul(512_000)
}

/// Convert an `NvmeSmartLog` to the common `SmartData` model struct.
pub fn nvme_smart_to_smart_data(log: &NvmeSmartLog) -> SmartData {
    let data_units_read = nvme_smart_read_u128(&log.data_units_read);
    let data_units_written = nvme_smart_read_u128(&log.data_units_written);

    SmartData {
        temperature_celsius: nvme_smart_temperature_celsius(log),
        available_spare_pct: log.avail_spare,
        available_spare_threshold_pct: log.spare_thresh,
        percentage_used: log.percent_used,
        data_units_read,
        data_units_written,
        host_read_commands: nvme_smart_read_u128(&log.host_reads),
        host_write_commands: nvme_smart_read_u128(&log.host_writes),
        controller_busy_time_minutes: nvme_smart_read_u128(&log.ctrl_busy_time),
        power_cycles: nvme_smart_read_u128(&log.power_cycles),
        power_on_hours: nvme_smart_read_u128(&log.power_on_hours),
        unsafe_shutdowns: nvme_smart_read_u128(&log.unsafe_shutdowns),
        media_errors: nvme_smart_read_u128(&log.media_errors),
        num_error_log_entries: nvme_smart_read_u128(&log.num_err_log_entries),
        warning_composite_temp_time_minutes: log.warning_temp_time,
        critical_composite_temp_time_minutes: log.critical_comp_time,
        critical_warning: log.critical_warning,
        total_bytes_read: nvme_smart_data_bytes(data_units_read),
        total_bytes_written: nvme_smart_data_bytes(data_units_written),
    }
}

/// Encode a Rust `&str` as a null-terminated wide (UTF-16) string for Win32.
fn to_wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Read the NVMe SMART/Health log from `\\.\PhysicalDriveN`.
///
/// Returns `None` if the drive cannot be opened (permissions, non-NVMe) or the
/// IOCTL fails.
pub fn read_nvme_smart(drive_number: u32) -> Option<NvmeSmartLog> {
    let path = format!("\\\\.\\PhysicalDrive{}", drive_number);
    let wide_path = to_wide(&path);

    unsafe {
        // Open with GENERIC_READ | GENERIC_WRITE — some NVMe drivers
        // require write access for the storage protocol IOCTL.
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
            log::debug!("Failed to open {}: {}", path, err);
            return None;
        }

        // Try device-level query first (PropertyId 50), then adapter-level (49).
        // Some NVMe drivers only support one or the other.
        let result = try_smart_query(handle, PROPERTY_ID_DEVICE, &path)
            .or_else(|| try_smart_query(handle, PROPERTY_ID_ADAPTER, &path));

        CloseHandle(handle);
        result
    }
}

/// Send IOCTL_STORAGE_QUERY_PROPERTY with the given property ID and parse the
/// SMART log from the response.
unsafe fn try_smart_query(
    handle: winapi::um::winnt::HANDLE,
    property_id: u32,
    path: &str,
) -> Option<NvmeSmartLog> {
    unsafe {
        let query = StoragePropertyQuery {
            property_id,
            query_type: 0, // PropertyStandardQuery
            protocol_specific: StorageProtocolSpecificData {
                protocol_type: STORAGE_PROTOCOL_TYPE_NVME,
                data_type: NVME_DATA_TYPE_LOG_PAGE,
                protocol_data_request_value: NVME_LOG_PAGE_SMART,
                protocol_data_request_sub_value: 0,
                protocol_data_offset: mem::size_of::<StorageProtocolSpecificData>() as u32,
                protocol_data_length: SMART_LOG_SIZE,
                fixed_protocol_return_data: 0,
                reserved: [0; 3],
            },
        };

        let mut output_buf = vec![0u8; OUTPUT_BUF_SIZE];
        let mut bytes_returned: DWORD = 0;

        let ok = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &query as *const StoragePropertyQuery as *mut _,
            mem::size_of::<StoragePropertyQuery>() as DWORD,
            output_buf.as_mut_ptr() as *mut _,
            OUTPUT_BUF_SIZE as DWORD,
            &mut bytes_returned,
            ptr::null_mut(),
        );

        if ok == FALSE {
            let err = std::io::Error::last_os_error();
            log::debug!(
                "NVMe SMART query (PropertyId={}) failed on {}: {}",
                property_id,
                path,
                err
            );
            return None;
        }

        if (bytes_returned as usize) < OUTPUT_BUF_SIZE {
            log::debug!(
                "Short read from {} ({} bytes, expected {})",
                path,
                bytes_returned,
                OUTPUT_BUF_SIZE
            );
            return None;
        }

        // The SMART log payload starts after the data descriptor header +
        // protocol-specific data structure.
        let log_offset = DATA_DESCRIPTOR_HEADER + PROTOCOL_SPECIFIC_DATA_SIZE;
        let log_bytes = &output_buf[log_offset..log_offset + SMART_LOG_SIZE as usize];

        // Copy into the NvmeSmartLog struct
        let mut log: NvmeSmartLog = mem::zeroed();
        ptr::copy_nonoverlapping(
            log_bytes.as_ptr(),
            &mut log as *mut NvmeSmartLog as *mut u8,
            SMART_LOG_SIZE as usize,
        );

        log::debug!(
            "NVMe SMART query (PropertyId={}) succeeded on {}",
            property_id,
            path
        );
        Some(log)
    } // unsafe
}
