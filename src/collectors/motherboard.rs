use crate::model::motherboard::{BiosInfo, MotherboardInfo};
use crate::parsers::smbios;
use crate::platform::sysfs;
use std::path::Path;

pub fn collect() -> MotherboardInfo {
    let dmi = Path::new("/sys/class/dmi/id");

    let uefi_boot = Path::new("/sys/firmware/efi").exists();
    let secure_boot = detect_secure_boot();

    let chipset = detect_chipset();

    // Read what we can from sysfs (some fields require root).
    let mut info = MotherboardInfo {
        manufacturer: sysfs::read_string_optional(&dmi.join("board_vendor")),
        product_name: sysfs::read_string_optional(&dmi.join("board_name")),
        version: sysfs::read_string_optional(&dmi.join("board_version")),
        serial_number: sysfs::read_string_optional(&dmi.join("board_serial")),
        system_vendor: sysfs::read_string_optional(&dmi.join("sys_vendor")),
        system_product: sysfs::read_string_optional(&dmi.join("product_name")),
        system_family: sysfs::read_string_optional(&dmi.join("product_family")),
        system_sku: sysfs::read_string_optional(&dmi.join("product_sku")),
        system_uuid: sysfs::read_string_optional(&dmi.join("product_uuid")),
        chassis_type: sysfs::read_u64_optional(&dmi.join("chassis_type"))
            .map(|code| chassis_type_name(code as u8)),
        bios: BiosInfo {
            vendor: sysfs::read_string_optional(&dmi.join("bios_vendor")),
            version: sysfs::read_string_optional(&dmi.join("bios_version")),
            date: sysfs::read_string_optional(&dmi.join("bios_date")),
            release: sysfs::read_string_optional(&dmi.join("bios_release")),
            uefi_boot,
            secure_boot,
        },
        chipset,
        me_version: crate::collectors::me::collect().and_then(|me| me.firmware_version),
    };

    // Supplement any missing fields from the raw SMBIOS tables.
    if let Some(smbios_data) = smbios::parse() {
        supplement_from_smbios(&mut info, &smbios_data);
    }

    info
}

/// Fill in `None` fields using parsed SMBIOS data.  Fields already populated
/// from sysfs are left untouched.
fn supplement_from_smbios(info: &mut MotherboardInfo, data: &smbios::SmbiosData) {
    if let Some(ref bb) = data.baseboard {
        if info.manufacturer.is_none() {
            info.manufacturer = bb.manufacturer.clone();
        }
        if info.product_name.is_none() {
            info.product_name = bb.product.clone();
        }
        if info.version.is_none() {
            info.version = bb.version.clone();
        }
        if info.serial_number.is_none() {
            info.serial_number = bb.serial_number.clone();
        }
    }

    if let Some(ref sys) = data.system {
        if info.system_vendor.is_none() {
            info.system_vendor = sys.manufacturer.clone();
        }
        if info.system_product.is_none() {
            info.system_product = sys.product_name.clone();
        }
        if info.system_family.is_none() {
            info.system_family = sys.family.clone();
        }
        if info.system_sku.is_none() {
            info.system_sku = sys.sku_number.clone();
        }
        if info.system_uuid.is_none() {
            info.system_uuid = sys.uuid.clone();
        }
    }

    if let Some(ref bios) = data.bios {
        if info.bios.vendor.is_none() {
            info.bios.vendor = bios.vendor.clone();
        }
        if info.bios.version.is_none() {
            info.bios.version = bios.version.clone();
        }
        if info.bios.date.is_none() {
            info.bios.date = bios.release_date.clone();
        }
        if info.bios.release.is_none()
            && let (Some(major), Some(minor)) = (bios.major_release, bios.minor_release)
        {
            // Match the sysfs format: "major.minor"
            info.bios.release = Some(format!("{major}.{minor}"));
        }
    }
}

fn detect_secure_boot() -> Option<bool> {
    for entry in sysfs::glob_paths("/sys/firmware/efi/efivars/SecureBoot-*") {
        if let Ok(data) = std::fs::read(&entry) {
            // EFI variable: first 4 bytes are attributes, 5th byte is the value
            if data.len() >= 5 {
                return Some(data[4] == 1);
            }
        }
    }
    None
}

fn detect_chipset() -> Option<String> {
    // The host bridge at 00:00.0 identifies the chipset
    let vendor_path = Path::new("/sys/bus/pci/devices/0000:00:00.0/vendor");
    let device_path = Path::new("/sys/bus/pci/devices/0000:00:00.0/device");
    let vid = sysfs::read_u64_optional(vendor_path)? as u16;
    let did = sysfs::read_u64_optional(device_path)? as u16;

    if let Some(device) = pci_ids::Device::from_vid_pid(vid, did) {
        Some(device.name().to_string())
    } else {
        Some(format!("{:04x}:{:04x}", vid, did))
    }
}

pub struct MotherboardCollector;

impl crate::collectors::Collector for MotherboardCollector {
    fn name(&self) -> &str {
        "motherboard"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.motherboard = collect();
    }
}

fn chassis_type_name(code: u8) -> String {
    match code {
        1 => "Other",
        2 => "Unknown",
        3 => "Desktop",
        4 => "Low Profile Desktop",
        5 => "Pizza Box",
        6 => "Mini Tower",
        7 => "Tower",
        8 => "Portable",
        9 => "Laptop",
        10 => "Notebook",
        11 => "Hand Held",
        12 => "Docking Station",
        13 => "All in One",
        14 => "Sub Notebook",
        15 => "Space-Saving",
        16 => "Lunch Box",
        17 => "Main Server Chassis",
        18 => "Expansion Chassis",
        19 => "Sub Chassis",
        20 => "Bus Expansion Chassis",
        21 => "Peripheral Chassis",
        22 => "RAID Chassis",
        23 => "Rack Mount Chassis",
        24 => "Sealed-case PC",
        25 => "Multi-system Chassis",
        26 => "Compact PCI",
        27 => "Advanced TCA",
        28 => "Blade",
        29 => "Blade Enclosure",
        30 => "Tablet",
        31 => "Convertible",
        32 => "Detachable",
        33 => "IoT Gateway",
        34 => "Embedded PC",
        35 => "Mini PC",
        36 => "Stick PC",
        _ => "Unknown",
    }
    .to_string()
}
