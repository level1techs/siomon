//! NVMe SMART/Health log reading via direct ioctl.
//!
//! Reads the SMART/Health Information log page (0x02) from an NVMe controller
//! using the `NVME_IOCTL_ADMIN_CMD` ioctl on `/dev/nvmeN`.

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// NVMe Admin Command structure passed to the ioctl.
#[repr(C)]
#[derive(Default)]
struct NvmeAdminCmd {
    opcode: u8,
    flags: u8,
    rsvd1: u16,
    nsid: u32,
    cdw2: u32,
    cdw3: u32,
    metadata: u64,
    addr: u64,
    metadata_len: u32,
    data_len: u32,
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
    cdw13: u32,
    cdw14: u32,
    cdw15: u32,
    timeout_ms: u32,
    result: u32,
}

/// NVMe SMART/Health Information (Log Page 0x02) - 512 bytes.
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

const NVME_IOCTL_ADMIN_CMD: libc::Ioctl = libc::_IOWR::<NvmeAdminCmd>(b'N' as u32, 0x41);

// NVMe admin command opcode for Get Log Page
const NVME_ADMIN_GET_LOG_PAGE: u8 = 0x02;

// SMART/Health log page identifier
const NVME_LOG_SMART: u32 = 0x02;

// Size of the SMART log in bytes
const SMART_LOG_SIZE: u32 = 512;

/// Read the NVMe SMART/Health log from a controller device (e.g. `/dev/nvme0`).
///
/// Returns `None` if the device cannot be opened or the ioctl fails.
pub fn read_nvme_smart(device_path: &Path) -> Option<NvmeSmartLog> {
    let file = OpenOptions::new()
        .read(true)
        .open(device_path)
        .map_err(|e| {
            log::debug!("Failed to open {}: {}", device_path.display(), e);
            e
        })
        .ok()?;

    let fd = file.as_raw_fd();

    // Allocate the output buffer on the heap, zeroed
    let mut log = Box::new(unsafe { std::mem::zeroed::<NvmeSmartLog>() });

    // Number of dwords minus 1
    let numdl = (SMART_LOG_SIZE / 4) - 1;

    let mut cmd = NvmeAdminCmd {
        opcode: NVME_ADMIN_GET_LOG_PAGE,
        nsid: 0xFFFF_FFFF,
        addr: log.as_mut() as *mut NvmeSmartLog as u64,
        data_len: SMART_LOG_SIZE,
        cdw10: NVME_LOG_SMART | (numdl << 16),
        ..Default::default()
    };

    let ret = unsafe { libc::ioctl(fd, NVME_IOCTL_ADMIN_CMD, &mut cmd as *mut NvmeAdminCmd) };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        log::debug!(
            "NVMe SMART ioctl failed on {}: {}",
            device_path.display(),
            err
        );
        return None;
    }

    Some(*log)
}

/// Convert the NVMe SMART temperature field (Kelvin, little-endian) to Celsius.
pub fn nvme_smart_temperature_celsius(log: &NvmeSmartLog) -> i32 {
    let kelvin = u16::from_le_bytes(log.temperature) as i32;
    kelvin - 273
}

/// Convert a 16-byte little-endian field to u128.
pub fn nvme_smart_read_u128(bytes: &[u8; 16]) -> u128 {
    u128::from_le_bytes(*bytes)
}

/// Convert NVMe data units (each unit = 1000 * 512 bytes = 512000 bytes) to total bytes.
pub fn nvme_smart_data_bytes(data_units: u128) -> u128 {
    data_units.saturating_mul(512_000)
}
