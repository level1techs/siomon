use crate::model::pci::PciDevice;
use pci_ids::FromId;

// ── Unix (Linux sysfs) implementation ──────────────────────────────────────

#[cfg(unix)]
use crate::model::gpu::PcieLinkInfo;
#[cfg(unix)]
use crate::model::pci::AerCounters;
#[cfg(unix)]
use crate::platform::sysfs;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
pub fn collect() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for entry in sysfs::glob_paths("/sys/bus/pci/devices/*") {
        if let Some(dev) = collect_device(&entry) {
            devices.push(dev);
        }
    }

    devices.sort_by(|a, b| a.address.cmp(&b.address));
    devices
}

#[cfg(unix)]
fn collect_device(path: &Path) -> Option<PciDevice> {
    let address = path.file_name()?.to_string_lossy().to_string();
    let (domain, bus, device, function) = parse_bdf(&address)?;

    let vendor_id = sysfs::read_u64_optional(&path.join("vendor"))? as u16;
    let device_id = sysfs::read_u64_optional(&path.join("device"))? as u16;
    let subsystem_vendor_id =
        sysfs::read_u64_optional(&path.join("subsystem_vendor")).map(|v| v as u16);
    let subsystem_device_id =
        sysfs::read_u64_optional(&path.join("subsystem_device")).map(|v| v as u16);
    let class_code = sysfs::read_u64_optional(&path.join("class")).unwrap_or(0) as u32;
    let revision = sysfs::read_u64_optional(&path.join("revision")).unwrap_or(0) as u8;

    let driver = sysfs::read_link_basename(&path.join("driver"));
    let irq = sysfs::read_u32_optional(&path.join("irq"));
    let numa_node =
        sysfs::read_string_optional(&path.join("numa_node")).and_then(|s| s.parse::<i32>().ok());
    let enabled = sysfs::read_u64_optional(&path.join("enable"))
        .map(|v| v == 1)
        .unwrap_or(true);

    let pcie_link = collect_pcie_link(path);
    let aer = collect_aer(path);

    let (vendor_name, device_name) = resolve_pci_names(vendor_id, device_id);
    let (class_name, subclass_name) = resolve_class_names(class_code);

    Some(PciDevice {
        address,
        domain,
        bus,
        device,
        function,
        vendor_id,
        device_id,
        subsystem_vendor_id,
        subsystem_device_id,
        revision,
        class_code,
        vendor_name,
        device_name,
        class_name,
        subclass_name,
        driver,
        irq,
        numa_node,
        pcie_link,
        enabled,
        aer,
    })
}

#[cfg(unix)]
fn collect_pcie_link(path: &Path) -> Option<PcieLinkInfo> {
    let current_speed = sysfs::read_string_optional(&path.join("current_link_speed"));
    let max_speed = sysfs::read_string_optional(&path.join("max_link_speed"));
    let current_width = sysfs::read_string_optional(&path.join("current_link_width"))
        .and_then(|s| s.parse::<u8>().ok());
    let max_width = sysfs::read_string_optional(&path.join("max_link_width"))
        .and_then(|s| s.parse::<u8>().ok());

    if current_speed.is_none() && max_speed.is_none() {
        return None;
    }

    Some(PcieLinkInfo {
        current_gen: current_speed.as_deref().map(pcie_speed_to_gen),
        current_width,
        max_gen: max_speed.as_deref().map(pcie_speed_to_gen),
        max_width,
        current_speed,
        max_speed,
    })
}

/// Read AER error totals from sysfs aer_dev_* files.
///
/// Each file contains lines like "TOTAL_ERR_COR 0". We extract the TOTAL_ line.
/// Returns None if AER files don't exist (older kernels, non-PCIe devices).
#[cfg(unix)]
fn collect_aer(path: &Path) -> Option<AerCounters> {
    let corr = parse_aer_total(&path.join("aer_dev_correctable"));
    let nonfatal = parse_aer_total(&path.join("aer_dev_nonfatal"));
    let fatal = parse_aer_total(&path.join("aer_dev_fatal"));

    // Only return if at least one file was readable
    if corr.is_none() && nonfatal.is_none() && fatal.is_none() {
        return None;
    }

    Some(AerCounters {
        correctable: corr.unwrap_or(0),
        nonfatal: nonfatal.unwrap_or(0),
        fatal: fatal.unwrap_or(0),
    })
}

/// Parse the TOTAL_ line from an AER counter file.
#[cfg(unix)]
pub(crate) fn parse_aer_total(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if line.starts_with("TOTAL_") {
            return line.split_whitespace().last()?.parse().ok();
        }
    }
    None
}

// ── Windows (WMI via PowerShell) implementation ────────────────────────────

#[cfg(windows)]
use crate::model::gpu::PcieLinkInfo;

#[cfg(windows)]
pub fn collect() -> Vec<PciDevice> {
    collect_via_powershell().unwrap_or_default()
}

/// Run a PowerShell command that emits one JSON array of PCI PnP entities
/// whose DeviceID starts with "PCI\\". Parse each row into a `PciDevice`.
#[cfg(windows)]
fn collect_via_powershell() -> Option<Vec<PciDevice>> {
    // The PowerShell script:
    //  1. Queries Win32_PnPEntity for DeviceID like 'PCI%'
    //  2. For each hit, also tries Win32_PnPSignedDriver to get the driver name
    //  3. Outputs compact JSON we can parse in Rust
    let ps_script = r#"
$devs = Get-CimInstance Win32_PnPEntity | Where-Object { $_.DeviceID -like 'PCI\*' }
$drivers = @{}
try {
    Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceID -like 'PCI\*' } | ForEach-Object {
        $drivers[$_.DeviceID] = $_.DriverName
    }
} catch {}
$result = @()
foreach ($d in $devs) {
    $obj = [ordered]@{
        DeviceID     = $d.DeviceID
        Name         = $d.Name
        Manufacturer = $d.Manufacturer
        Status       = $d.Status
        Driver       = $drivers[$d.DeviceID]
    }
    $result += $obj
}
$result | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .output()
        .ok()?;

    if !output.status.success() {
        log::debug!(
            "PCI PowerShell query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Some(Vec::new());
    }

    // PowerShell ConvertTo-Json returns a bare object (not array) when there's
    // exactly one result. Normalise to always be an array.
    let json_str = if stdout.starts_with('[') {
        stdout.to_string()
    } else {
        format!("[{}]", stdout)
    };

    let raw: Vec<WmiPciRow> = serde_json::from_str(&json_str).ok()?;

    let mut devices: Vec<PciDevice> = raw.iter().filter_map(wmi_row_to_pci).collect();

    // Attach PCIe link info via WinRing0 PCI config space reads (if available)
    attach_pcie_link_info(&mut devices);

    devices.sort_by(|a, b| a.address.cmp(&b.address));
    Some(devices)
}

/// Attempt to read PCIe link speed/width for every device using WinRing0.
/// Silently skips if the driver is not available.
#[cfg(windows)]
fn attach_pcie_link_info(devices: &mut [PciDevice]) {
    let wr = match crate::platform::winring0::WinRing0::try_load() {
        Some(wr) => wr,
        None => {
            log::debug!("WinRing0 not available – skipping PCIe link info");
            return;
        }
    };

    for dev in devices.iter_mut() {
        dev.pcie_link = read_pcie_link_winring0(wr, dev.bus, dev.device, dev.function);
    }
}

/// Read the PCI Express Capability Structure from config space to extract
/// link speed and width information.
///
/// Walks the PCI Capabilities List starting at the pointer in offset 0x34.
/// When capability ID 0x10 (PCI Express) is found, reads:
///   - Link Capabilities register (cap_offset + 0x0C)
///   - Link Status register (cap_offset + 0x12, via dword at cap_offset + 0x10)
#[cfg(windows)]
fn read_pcie_link_winring0(
    wr: &crate::platform::winring0::WinRing0,
    bus: u8,
    dev: u8,
    func: u8,
) -> Option<PcieLinkInfo> {
    // Read the Status register (offset 0x06) to check if capabilities list exists.
    // Bit 4 of Status indicates "Capabilities List" is present.
    let status_dword = wr.read_pci_config_dword(bus, dev, func, 0x04);
    let status = (status_dword >> 16) as u16;
    if status == 0xFFFF {
        // Device not present
        return None;
    }
    if status & (1 << 4) == 0 {
        // No capabilities list
        return None;
    }

    // Read the capabilities pointer at offset 0x34
    let cap_ptr_dword = wr.read_pci_config_dword(bus, dev, func, 0x34);
    let mut cap_ptr = (cap_ptr_dword & 0xFF) as u8;

    // Cap pointer must be dword-aligned and within config space
    cap_ptr &= 0xFC;

    // Walk the capabilities linked list (with a safety bound to avoid infinite loops)
    let mut iterations = 0u32;
    while cap_ptr != 0 && iterations < 48 {
        iterations += 1;

        let cap_dword = wr.read_pci_config_dword(bus, dev, func, cap_ptr);
        let cap_id = (cap_dword & 0xFF) as u8;
        let next_ptr = ((cap_dword >> 8) & 0xFC) as u8; // mask to dword-aligned

        if cap_id == 0x10 {
            // PCI Express capability found

            // Link Capabilities at cap_ptr + 0x0C
            let link_cap_offset = cap_ptr.checked_add(0x0C)?;
            let link_cap = wr.read_pci_config_dword(bus, dev, func, link_cap_offset);
            let max_speed_code = (link_cap & 0xF) as u8;
            let max_width = ((link_cap >> 4) & 0x3F) as u8;

            // Link Status at cap_ptr + 0x12 (16-bit register)
            // Read dword at cap_ptr + 0x10, status is in upper 16 bits
            let link_ctrl_status_offset = cap_ptr.checked_add(0x10)?;
            let link_ctrl_status =
                wr.read_pci_config_dword(bus, dev, func, link_ctrl_status_offset);
            let link_status = (link_ctrl_status >> 16) as u16;
            let current_speed_code = (link_status & 0xF) as u8;
            let current_width = ((link_status >> 4) & 0x3F) as u8;

            // Ignore devices with no valid link info (all zeros or unexpected values)
            if current_speed_code == 0
                && current_width == 0
                && max_speed_code == 0
                && max_width == 0
            {
                return None;
            }

            return Some(PcieLinkInfo {
                current_gen: Some(current_speed_code),
                current_width: Some(current_width),
                max_gen: Some(max_speed_code),
                max_width: Some(max_width),
                current_speed: Some(pcie_speed_code_to_string(current_speed_code)),
                max_speed: Some(pcie_speed_code_to_string(max_speed_code)),
            });
        }

        cap_ptr = next_ptr;
    }

    None
}

/// Convert a PCIe Link Speed encoding (from Link Capabilities / Link Status)
/// to a human-readable string.
#[cfg(windows)]
fn pcie_speed_code_to_string(code: u8) -> String {
    match code {
        1 => "2.5 GT/s PCIe".to_string(),
        2 => "5.0 GT/s PCIe".to_string(),
        3 => "8.0 GT/s PCIe".to_string(),
        4 => "16.0 GT/s PCIe".to_string(),
        5 => "32.0 GT/s PCIe".to_string(),
        6 => "64.0 GT/s PCIe".to_string(),
        _ => format!("Unknown ({})", code),
    }
}

#[cfg(windows)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
struct WmiPciRow {
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    name: Option<String>,
    manufacturer: Option<String>,
    status: Option<String>,
    driver: Option<String>,
}

/// Parse a WMI PnP DeviceID like:
///   `PCI\VEN_10DE&DEV_2684&SUBSYS_16F11043&REV_A1\4&2283F625&0&0019`
/// into vendor/device/subsystem IDs and a synthetic BDF address.
#[cfg(windows)]
fn wmi_row_to_pci(row: &WmiPciRow) -> Option<PciDevice> {
    let device_id = row.device_id.as_deref()?;

    // Split on backslash: ["PCI", "VEN_xxxx&DEV_xxxx&...", "4&xxxxx&0&00BB"]
    let parts: Vec<&str> = device_id.split('\\').collect();
    if parts.len() < 3 {
        return None;
    }

    let ids_section = parts[1]; // "VEN_10DE&DEV_2684&SUBSYS_16F11043&REV_A1"
    let location_section = parts[2]; // "4&2283F625&0&0019"

    let vendor_id = extract_hex_u16(ids_section, "VEN_")?;
    let device_id_val = extract_hex_u16(ids_section, "DEV_")?;
    let subsystem_raw = extract_hex_u32(ids_section, "SUBSYS_");
    let subsystem_vendor_id = subsystem_raw.map(|v| (v & 0xFFFF) as u16);
    let subsystem_device_id = subsystem_raw.map(|v| (v >> 16) as u16);
    let revision = extract_hex_u8(ids_section, "REV_").unwrap_or(0);

    // Try to extract bus/device/function from the location part.
    // The last segment after the last '&' is often the bus+dev+fn encoded as
    // a hex number where bits [7:0] = (bus << 0)... Actually the encoding
    // varies. We'll try to parse the location info: the last &-separated
    // component is a hex BBDDFF style location code on many systems.
    let (bus, dev, func) = parse_location_info(location_section);
    let address = format!("0000:{:02x}:{:02x}.{:x}", bus, dev, func);

    let (vendor_name, device_name_resolved) = resolve_pci_names(vendor_id, device_id_val);
    let (class_name, subclass_name) = (None, None); // class code unavailable from WMI PnP

    let driver = row.driver.clone().or_else(|| row.name.clone());
    let enabled = row.status.as_deref() == Some("OK");

    Some(PciDevice {
        address,
        domain: 0,
        bus,
        device: dev,
        function: func,
        vendor_id,
        device_id: device_id_val,
        subsystem_vendor_id,
        subsystem_device_id,
        revision,
        class_code: 0, // Not available from Win32_PnPEntity
        vendor_name,
        device_name: device_name_resolved,
        class_name,
        subclass_name,
        driver,
        irq: None,
        numa_node: None,
        pcie_link: None,
        enabled,
        aer: None,
    })
}

/// Extract a 16-bit hex value from a string like "VEN_10DE&DEV_2684"
/// given a prefix like "VEN_".
#[cfg(windows)]
fn extract_hex_u16(s: &str, prefix: &str) -> Option<u16> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];
    let hex_str: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u16::from_str_radix(&hex_str, 16).ok()
}

/// Extract a 32-bit hex value (e.g. for SUBSYS_16F11043).
#[cfg(windows)]
fn extract_hex_u32(s: &str, prefix: &str) -> Option<u32> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];
    let hex_str: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u32::from_str_radix(&hex_str, 16).ok()
}

/// Extract an 8-bit hex value (e.g. for REV_A1).
#[cfg(windows)]
fn extract_hex_u8(s: &str, prefix: &str) -> Option<u8> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];
    let hex_str: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u8::from_str_radix(&hex_str, 16).ok()
}

/// Parse the location section of a PCI DeviceID.
///
/// The instance ID (3rd backslash-separated segment) has the form
/// `<enumerator>&<parent_hash>&<port>&<location>` where `<location>` is a hex
/// number encoding `(bus << 8) | (device << 3) | function`.
///
/// Examples:
///   `3&2411E6FE&0&08`   -> location 0x08  -> bus=0, dev=1, fn=0
///   `4&1BB67788&0&0039`  -> location 0x39  -> bus=0, dev=7, fn=1
///   `4&21F346C1&0&0441`  -> location 0x441 -> bus=4, dev=8, fn=1
///
/// Some special DeviceIDs (like Intel multi-host NICs) use a long hex string
/// as the entire instance ID; we skip those gracefully.
#[cfg(windows)]
fn parse_location_info(location: &str) -> (u8, u8, u8) {
    let parts: Vec<&str> = location.split('&').collect();
    if parts.len() < 2 {
        // Not the expected format (e.g. a bare hex string like "0000C9FFFF...")
        return (0, 0, 0);
    }
    if let Some(last) = parts.last() {
        // The location code may have leading zeros, e.g. "0039" or "0441".
        if let Ok(val) = u32::from_str_radix(last, 16) {
            // Encoding: bus = val >> 8, devfn = val & 0xFF
            // devfn:    device = bits [7:3], function = bits [2:0]
            let bus = (val >> 8) as u8;
            let devfn = (val & 0xFF) as u8;
            let dev = (devfn >> 3) & 0x1F;
            let func = devfn & 0x07;
            return (bus, dev, func);
        }
    }
    (0, 0, 0)
}

// ── Shared helpers ─────────────────────────────────────────────────────────

#[allow(dead_code)]
fn parse_bdf(address: &str) -> Option<(u16, u8, u8, u8)> {
    // Format: "0000:00:00.0"
    let parts: Vec<&str> = address.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let domain = u16::from_str_radix(parts[0], 16).ok()?;
    let bus = u8::from_str_radix(parts[1], 16).ok()?;
    let df: Vec<&str> = parts[2].split('.').collect();
    if df.len() != 2 {
        return None;
    }
    let device = u8::from_str_radix(df[0], 16).ok()?;
    let function = u8::from_str_radix(df[1], 16).ok()?;
    Some((domain, bus, device, function))
}

pub fn pcie_speed_to_gen(speed: &str) -> u8 {
    if speed.contains("64") {
        6
    } else if speed.contains("32") {
        5
    } else if speed.contains("16") {
        4
    } else if speed.contains("8") {
        3
    } else if speed.contains("5") {
        2
    } else if speed.contains("2.5") {
        1
    } else {
        0
    }
}

fn resolve_pci_names(vid: u16, did: u16) -> (Option<String>, Option<String>) {
    let vendor_name = pci_ids::Vendor::from_id(vid).map(|v| v.name().to_string());
    let device_name = pci_ids::Device::from_vid_pid(vid, did).map(|d| d.name().to_string());
    (vendor_name, device_name)
}

#[cfg(unix)]
fn resolve_class_names(class_code: u32) -> (Option<String>, Option<String>) {
    let class = ((class_code >> 16) & 0xFF) as u8;
    let subclass = ((class_code >> 8) & 0xFF) as u8;

    let class_name = pci_ids::Class::from_id(class).map(|c| c.name().to_string());
    let subclass_name =
        pci_ids::Subclass::from_cid_sid(class, subclass).map(|s| s.name().to_string());
    (class_name, subclass_name)
}

pub struct PciCollector;

impl crate::collectors::Collector for PciCollector {
    fn name(&self) -> &str {
        "pci"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.pci_devices = collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bdf() {
        let (domain, bus, device, function) = parse_bdf("0000:f1:00.0").unwrap();
        assert_eq!(domain, 0);
        assert_eq!(bus, 0xf1);
        assert_eq!(device, 0);
        assert_eq!(function, 0);
    }

    #[test]
    fn test_parse_bdf_nonzero_function() {
        let (domain, bus, device, function) = parse_bdf("0001:03:1a.3").unwrap();
        assert_eq!(domain, 1);
        assert_eq!(bus, 0x03);
        assert_eq!(device, 0x1a);
        assert_eq!(function, 3);
    }

    #[test]
    fn test_parse_bdf_invalid() {
        assert!(parse_bdf("invalid").is_none());
    }

    #[test]
    fn test_parse_bdf_missing_function() {
        assert!(parse_bdf("0000:00:00").is_none());
    }

    #[test]
    fn test_pcie_speed_to_gen() {
        assert_eq!(pcie_speed_to_gen("8.0 GT/s PCIe"), 3);
        assert_eq!(pcie_speed_to_gen("16.0 GT/s PCIe"), 4);
        assert_eq!(pcie_speed_to_gen("32.0 GT/s PCIe"), 5);
        assert_eq!(pcie_speed_to_gen("64.0 GT/s PCIe"), 6);
    }

    #[test]
    fn test_pcie_speed_to_gen_unknown() {
        assert_eq!(pcie_speed_to_gen("unknown"), 0);
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_hex_u16() {
        assert_eq!(extract_hex_u16("VEN_10DE&DEV_2684", "VEN_"), Some(0x10DE));
        assert_eq!(extract_hex_u16("VEN_10DE&DEV_2684", "DEV_"), Some(0x2684));
        assert_eq!(extract_hex_u16("VEN_10DE&DEV_2684", "FOO_"), None);
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_hex_u32() {
        assert_eq!(
            extract_hex_u32("SUBSYS_16F11043&REV_A1", "SUBSYS_"),
            Some(0x16F11043)
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_wmi_row_to_pci() {
        let row = WmiPciRow {
            device_id: Some(
                r"PCI\VEN_10DE&DEV_2684&SUBSYS_16F11043&REV_A1\4&2283F625&0&0019".to_string(),
            ),
            name: Some("NVIDIA GeForce RTX 4090".to_string()),
            manufacturer: Some("NVIDIA".to_string()),
            status: Some("OK".to_string()),
            driver: Some("nvlddmkm".to_string()),
        };

        let dev = wmi_row_to_pci(&row).unwrap();
        assert_eq!(dev.vendor_id, 0x10DE);
        assert_eq!(dev.device_id, 0x2684);
        assert_eq!(dev.revision, 0xA1);
        assert!(dev.enabled);
        assert_eq!(dev.driver.as_deref(), Some("nvlddmkm"));
    }
}
