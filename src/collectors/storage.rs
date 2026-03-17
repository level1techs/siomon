#[cfg(unix)]
use crate::model::storage::{NvmeDetails, SmartData};
use crate::model::storage::{StorageDevice, StorageInterface};
#[cfg(unix)]
use crate::platform::sysfs;
#[cfg(unix)]
use crate::platform::{nvme_ioctl, sata_ioctl};
#[cfg(windows)]
use crate::platform::{nvme_win, sata_win};
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
pub fn collect() -> Vec<StorageDevice> {
    let mut devices = Vec::new();

    for entry in sysfs::glob_paths("/sys/class/block/*") {
        let name = match entry.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // Skip partitions, loop, dm, ram, zram devices
        if name.starts_with("loop")
            || name.starts_with("dm-")
            || name.starts_with("ram")
            || name.starts_with("zram")
            || name.starts_with("sr")
            || name.starts_with("nbd")
        {
            continue;
        }
        // Skip partitions (e.g. nvme0n1p1, sda1)
        if is_partition(&entry) {
            continue;
        }

        if let Some(dev) = collect_device(&name, &entry) {
            devices.push(dev);
        }
    }

    devices.sort_by(|a, b| a.device_name.cmp(&b.device_name));
    devices
}

#[cfg(not(unix))]
pub fn collect() -> Vec<StorageDevice> {
    use sysinfo::{DiskKind, Disks};
    let disks = Disks::new_with_refreshed_list();
    let mut devices: Vec<StorageDevice> = disks
        .list()
        .iter()
        .map(|disk| {
            // Use drive letter only (e.g. "C:") so display shows "C: ..."
            let mount = disk
                .mount_point()
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_string();
            // Volume label (e.g. "OS", "Data") as model; fall back to filesystem type
            let vol_name = disk.name().to_string_lossy().to_string();
            let fs = disk.file_system().to_string_lossy().to_string();
            let model = if vol_name.is_empty() { fs } else { vol_name };
            let rotational = matches!(disk.kind(), DiskKind::HDD);
            let interface = match disk.kind() {
                DiskKind::SSD => StorageInterface::NVMe,
                DiskKind::HDD => StorageInterface::SATA,
                _ => StorageInterface::Unknown("unknown".to_string()),
            };
            StorageDevice {
                device_name: mount,
                sysfs_path: String::new(),
                model: Some(model),
                serial_number: None,
                firmware_version: None,
                capacity_bytes: disk.total_space(),
                interface,
                rotational,
                logical_sector_size: 512,
                physical_sector_size: 512,
                nvme: None,
                smart: None,
            }
        })
        .collect();

    // Try to read SMART data from physical drives and attach to matching
    // logical drives.  Requires admin — skip entirely when not elevated to
    // avoid 16 wasted ACCESS_DENIED failures from CreateFile.
    #[cfg(windows)]
    if crate::platform::is_elevated() {
        attach_smart_data(&mut devices);
    } else {
        log::debug!("Storage: skipping SMART probe (not elevated)");
    }

    devices
}

/// Probe `\\.\PhysicalDrive0..15` for SMART data and attach to devices that
/// don't already have it.  We match physical drives to logical sysinfo entries
/// by capacity (best-effort heuristic).
#[cfg(windows)]
fn attach_smart_data(devices: &mut [StorageDevice]) {
    use std::mem;
    use std::ptr;
    use winapi::shared::minwindef::{DWORD, FALSE};
    use winapi::um::fileapi::CreateFileW;
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::ioapiset::DeviceIoControl;
    use winapi::um::winioctl::IOCTL_DISK_GET_DRIVE_GEOMETRY_EX;
    use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ};

    /// Minimal `DISK_GEOMETRY_EX` — we only need the total `DiskSize`.
    #[repr(C)]
    struct DiskGeometryEx {
        geometry: DiskGeometry,
        disk_size: i64,
        // Data[1] follows but we don't need it.
    }

    #[repr(C)]
    struct DiskGeometry {
        cylinders: i64,
        media_type: u32,
        tracks_per_cylinder: DWORD,
        sectors_per_track: DWORD,
        bytes_per_sector: DWORD,
    }

    /// Encode a Rust `&str` as null-terminated UTF-16.
    fn to_wide(s: &str) -> Vec<u16> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let mut consecutive_missing = 0u32;
    for drive_num in 0u32..16 {
        // Stop probing after 2 consecutive "drive not found" results
        let path = format!("\\\\.\\PhysicalDrive{}", drive_num);
        let wide = to_wide(&path);
        let exists = unsafe {
            let h = winapi::um::fileapi::CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                winapi::um::fileapi::OPEN_EXISTING,
                0,
                ptr::null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                false
            } else {
                CloseHandle(h);
                true
            }
        };
        if !exists {
            consecutive_missing += 1;
            if consecutive_missing >= 2 {
                break;
            }
            continue;
        }
        consecutive_missing = 0;

        // Try NVMe SMART first
        if let Some(smart_log) = nvme_win::read_nvme_smart(drive_num) {
            let smart = nvme_win::nvme_smart_to_smart_data(&smart_log);
            log::debug!(
                "Storage: PhysicalDrive{} NVMe SMART OK (temp={}C, hours={:?})",
                drive_num,
                smart.temperature_celsius,
                smart.power_on_hours
            );
            if let Some(dev) = match_physical_to_logical(
                devices,
                drive_num,
                to_wide,
                IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
            ) {
                log::debug!("Storage: attached NVMe SMART to {}", dev.device_name);
                dev.smart = Some(smart);
                if dev.interface == StorageInterface::Unknown("unknown".to_string()) {
                    dev.interface = StorageInterface::NVMe;
                }
            } else {
                log::debug!(
                    "Storage: PhysicalDrive{} NVMe SMART read OK but no matching logical drive",
                    drive_num
                );
            }
            continue;
        }

        // Try SATA SMART
        if let Some(ata) = sata_win::read_sata_smart(drive_num) {
            let smart = sata_win::sata_smart_to_smart_data(&ata);
            log::debug!(
                "Storage: PhysicalDrive{} SATA SMART OK (temp={}C, hours={:?})",
                drive_num,
                smart.temperature_celsius,
                smart.power_on_hours
            );
            if let Some(dev) = match_physical_to_logical(
                devices,
                drive_num,
                to_wide,
                IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
            ) {
                log::debug!("Storage: attached SATA SMART to {}", dev.device_name);
                dev.smart = Some(smart);
                if dev.interface == StorageInterface::Unknown("unknown".to_string()) {
                    dev.interface = StorageInterface::SATA;
                }
            } else {
                log::debug!(
                    "Storage: PhysicalDrive{} SATA SMART read OK but no matching logical drive",
                    drive_num
                );
            }
        }
    }

    /// Find the logical device (from sysinfo) that best matches a physical
    /// drive by capacity.  Returns `None` if no unmatched device is close
    /// enough in size (<10 % difference).
    fn match_physical_to_logical(
        devices: &mut [StorageDevice],
        drive_num: u32,
        to_wide: fn(&str) -> Vec<u16>,
        ioctl_geom: DWORD,
    ) -> Option<&mut StorageDevice> {
        // Read the physical drive capacity via IOCTL_DISK_GET_DRIVE_GEOMETRY_EX.
        let phys_capacity = get_physical_drive_size(drive_num, to_wide, ioctl_geom)?;
        log::debug!(
            "Storage: PhysicalDrive{} raw capacity = {} bytes",
            drive_num,
            phys_capacity
        );

        // Find the device whose capacity is closest (and within 20%) that
        // doesn't already have SMART data.
        let mut best_idx: Option<usize> = None;
        let mut best_diff: u64 = u64::MAX;
        for (i, dev) in devices.iter().enumerate() {
            if dev.smart.is_some() {
                continue;
            }
            let cap = dev.capacity_bytes;
            let diff = phys_capacity.abs_diff(cap);
            let threshold = phys_capacity / 5; // ~20 %
            if diff < threshold && diff < best_diff {
                best_diff = diff;
                best_idx = Some(i);
            }
        }

        if best_idx.is_some() {
            return best_idx.map(|i| &mut devices[i]);
        }

        // Fallback: for spanned/RAID volumes the logical size exceeds any
        // single physical drive.  Attach SMART to the first device whose
        // logical capacity is LARGER than the physical drive (component of
        // a larger volume) and that doesn't already have SMART data.
        for (i, dev) in devices.iter().enumerate() {
            if dev.smart.is_some() {
                continue;
            }
            if dev.capacity_bytes > phys_capacity {
                log::debug!(
                    "Storage: fallback match — PhysicalDrive{} ({}B) is likely a component of {} ({}B)",
                    drive_num,
                    phys_capacity,
                    dev.device_name,
                    dev.capacity_bytes
                );
                return Some(&mut devices[i]);
            }
        }

        None
    }

    /// Query the raw size (in bytes) of `\\.\PhysicalDriveN` via
    /// `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX`.
    fn get_physical_drive_size(
        drive_num: u32,
        to_wide: fn(&str) -> Vec<u16>,
        ioctl_geom: DWORD,
    ) -> Option<u64> {
        let path = format!("\\\\.\\PhysicalDrive{}", drive_num);
        let wide = to_wide(&path);
        unsafe {
            let handle = CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                winapi::um::fileapi::OPEN_EXISTING,
                0,
                ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut geom: DiskGeometryEx = mem::zeroed();
            let mut returned: DWORD = 0;
            let ok = DeviceIoControl(
                handle,
                ioctl_geom,
                ptr::null_mut(),
                0,
                &mut geom as *mut DiskGeometryEx as *mut _,
                mem::size_of::<DiskGeometryEx>() as DWORD,
                &mut returned,
                ptr::null_mut(),
            );
            CloseHandle(handle);
            if ok == FALSE {
                return None;
            }
            Some(geom.disk_size as u64)
        }
    }
}

pub struct StorageCollector;

impl crate::collectors::Collector for StorageCollector {
    fn name(&self) -> &str {
        "storage"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.storage = collect();
    }
}

#[cfg(unix)]
fn is_partition(block_path: &Path) -> bool {
    block_path.join("partition").exists()
}

#[cfg(unix)]
fn collect_device(name: &str, block_path: &Path) -> Option<StorageDevice> {
    let size_sectors = sysfs::read_u64_optional(&block_path.join("size")).unwrap_or(0);
    if size_sectors == 0 {
        return None;
    }
    let capacity_bytes = size_sectors * 512;

    let rotational = sysfs::read_u64_optional(&block_path.join("queue/rotational"))
        .map(|v| v == 1)
        .unwrap_or(false);
    let logical_sector_size =
        sysfs::read_u32_optional(&block_path.join("queue/logical_block_size")).unwrap_or(512);
    let physical_sector_size =
        sysfs::read_u32_optional(&block_path.join("queue/physical_block_size")).unwrap_or(512);

    let (interface, model, serial, firmware, nvme, smart) = if name.starts_with("nvme") {
        let (iface, model, serial, fw, nvme_details) = collect_nvme(name, block_path);
        let smart = nvme_details.as_ref().and_then(|n| n.smart.clone());
        (iface, model, serial, fw, nvme_details, smart)
    } else {
        let (iface, model, serial, fw, smart) = collect_ata_scsi(name, block_path);
        (iface, model, serial, fw, None, smart)
    };

    Some(StorageDevice {
        device_name: name.to_string(),
        sysfs_path: block_path.to_string_lossy().to_string(),
        model,
        serial_number: serial,
        firmware_version: firmware,
        capacity_bytes,
        interface,
        rotational,
        logical_sector_size,
        physical_sector_size,
        nvme,
        smart,
    })
}

#[cfg(unix)]
fn collect_nvme(
    name: &str,
    block_path: &Path,
) -> (
    StorageInterface,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<NvmeDetails>,
) {
    // Extract controller name: nvme0n1 -> nvme0
    let ctrl_name = name.split('n').take(2).next().unwrap_or(name);
    // Try nvme0 first, then fallback patterns
    let ctrl_path_str = format!("/sys/class/nvme/{}", ctrl_name);
    let ctrl_path = Path::new(&ctrl_path_str);

    let model = sysfs::read_string_optional(&ctrl_path.join("model"))
        .or_else(|| sysfs::read_string_optional(&block_path.join("device/model")));
    let serial = sysfs::read_string_optional(&ctrl_path.join("serial"));
    let firmware = sysfs::read_string_optional(&ctrl_path.join("firmware_rev"));
    let transport =
        sysfs::read_string_optional(&ctrl_path.join("transport")).unwrap_or_else(|| "pcie".into());
    let controller_type = sysfs::read_string_optional(&ctrl_path.join("cntrltype"));
    let queue_count = sysfs::read_u32_optional(&ctrl_path.join("queue_count"));
    let subsystem_nqn = sysfs::read_string_optional(&ctrl_path.join("subsysnqn"));

    let controller_id = ctrl_name
        .strip_prefix("nvme")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let namespace_count =
        sysfs::glob_paths(&format!("/sys/class/nvme/{}/{}n*", ctrl_name, ctrl_name)).len() as u32;

    // Try reading SMART data via NVMe ioctl on the controller device
    let smart = read_nvme_smart_data(ctrl_name);

    let nvme_details = NvmeDetails {
        controller_id,
        nvme_version: None,
        transport,
        namespace_count: namespace_count.max(1),
        controller_type,
        queue_count,
        subsystem_nqn,
        smart,
    };

    (
        StorageInterface::NVMe,
        model,
        serial,
        firmware,
        Some(nvme_details),
    )
}

#[cfg(unix)]
fn collect_ata_scsi(
    name: &str,
    block_path: &Path,
) -> (
    StorageInterface,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<SmartData>,
) {
    let dev_path = block_path.join("device");
    let model = sysfs::read_string_optional(&dev_path.join("model")).or_else(|| {
        sysfs::read_string_optional(&dev_path.join("vendor")).map(|v| v.trim().to_string())
    });
    let serial = sysfs::read_string_optional(&dev_path.join("serial"));
    let firmware = sysfs::read_string_optional(&dev_path.join("rev"));

    let interface = detect_interface(block_path);

    // Try reading SATA SMART data via SG_IO ioctl
    let smart = read_sata_smart_data(name);

    (interface, model, serial, firmware, smart)
}

#[cfg(unix)]
fn read_nvme_smart_data(ctrl_name: &str) -> Option<SmartData> {
    let dev_path = format!("/dev/{}", ctrl_name);
    let log = nvme_ioctl::read_nvme_smart(Path::new(&dev_path))?;

    let data_units_read = nvme_ioctl::nvme_smart_read_u128(&log.data_units_read);
    let data_units_written = nvme_ioctl::nvme_smart_read_u128(&log.data_units_written);

    Some(SmartData {
        temperature_celsius: nvme_ioctl::nvme_smart_temperature_celsius(&log),
        available_spare_pct: log.avail_spare,
        available_spare_threshold_pct: log.spare_thresh,
        percentage_used: log.percent_used,
        data_units_read,
        data_units_written,
        host_read_commands: nvme_ioctl::nvme_smart_read_u128(&log.host_reads),
        host_write_commands: nvme_ioctl::nvme_smart_read_u128(&log.host_writes),
        controller_busy_time_minutes: nvme_ioctl::nvme_smart_read_u128(&log.ctrl_busy_time),
        power_cycles: nvme_ioctl::nvme_smart_read_u128(&log.power_cycles),
        power_on_hours: nvme_ioctl::nvme_smart_read_u128(&log.power_on_hours),
        unsafe_shutdowns: nvme_ioctl::nvme_smart_read_u128(&log.unsafe_shutdowns),
        media_errors: nvme_ioctl::nvme_smart_read_u128(&log.media_errors),
        num_error_log_entries: nvme_ioctl::nvme_smart_read_u128(&log.num_err_log_entries),
        warning_composite_temp_time_minutes: log.warning_temp_time,
        critical_composite_temp_time_minutes: log.critical_comp_time,
        critical_warning: log.critical_warning,
        total_bytes_read: nvme_ioctl::nvme_smart_data_bytes(data_units_read),
        total_bytes_written: nvme_ioctl::nvme_smart_data_bytes(data_units_written),
    })
}

#[cfg(unix)]
fn read_sata_smart_data(name: &str) -> Option<SmartData> {
    let smart_path = format!("/dev/{}", name);
    sata_ioctl::read_sata_smart(Path::new(&smart_path))
        .map(|ata| sata_ioctl::sata_smart_to_smart_data(&ata))
}

#[cfg(unix)]
fn detect_interface(block_path: &Path) -> StorageInterface {
    let dev_path = block_path.join("device");

    // Check for USB storage
    let uevent = sysfs::read_string_optional(&dev_path.join("uevent")).unwrap_or_default();
    if uevent.contains("usb") {
        return StorageInterface::USB;
    }

    // Check for virtio
    let driver = sysfs::read_link_basename(&dev_path.join("driver"));
    if let Some(ref d) = driver {
        if d.contains("virtio") {
            return StorageInterface::VirtIO;
        }
    }

    // Check for MMC
    let device_name = block_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if device_name.starts_with("mmcblk") {
        return StorageInterface::MMC;
    }

    StorageInterface::SATA
}
