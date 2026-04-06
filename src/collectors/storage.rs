use crate::model::storage::{NvmeDetails, SmartData, StorageDevice, StorageInterface};
use crate::platform::{nvme_ioctl, sata_ioctl, sysfs};
use std::path::Path;

pub fn collect(storage_exclude: &[String]) -> Vec<StorageDevice> {
    let mut devices = Vec::new();

    for entry in sysfs::glob_paths("/sys/class/block/*") {
        let name = match entry.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // Skip virtual/pseudo devices by configurable prefix list
        if storage_exclude
            .iter()
            .any(|prefix| name.starts_with(prefix.as_str()))
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

pub struct StorageCollector;

impl crate::collectors::Collector for StorageCollector {
    fn name(&self) -> &str {
        "storage"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        let defaults = crate::config::GeneralConfig::default().storage_exclude;
        info.storage = collect(&defaults);
    }
}

fn is_partition(block_path: &Path) -> bool {
    block_path.join("partition").exists()
}

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
    let smart_path = format!("/dev/{}", name);
    let smart = sata_ioctl::read_sata_smart(Path::new(&smart_path))
        .map(|ata| sata_ioctl::sata_smart_to_smart_data(&ata));

    (interface, model, serial, firmware, smart)
}

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

fn detect_interface(block_path: &Path) -> StorageInterface {
    let dev_path = block_path.join("device");

    // Check for USB storage
    let uevent = sysfs::read_string_optional(&dev_path.join("uevent")).unwrap_or_default();
    if uevent.contains("usb") {
        return StorageInterface::USB;
    }

    // Check for virtio
    let driver = sysfs::read_link_basename(&dev_path.join("driver"));
    if let Some(ref d) = driver
        && d.contains("virtio")
    {
        return StorageInterface::VirtIO;
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
