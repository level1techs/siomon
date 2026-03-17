use crate::model::usb::{UsbDevice, UsbSpeed};

// ── Unix (Linux sysfs) implementation ──────────────────────────────────────

#[cfg(unix)]
use crate::platform::sysfs;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
pub fn collect() -> Vec<UsbDevice> {
    let mut devices = Vec::new();

    for entry in sysfs::glob_paths("/sys/bus/usb/devices/*") {
        let name = match entry.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // Skip interfaces (entries containing ":")
        if name.contains(':') {
            continue;
        }

        if let Some(device) = collect_device(&name, &entry) {
            devices.push(device);
        }
    }

    devices.sort_by(|a, b| {
        a.bus
            .cmp(&b.bus)
            .then_with(|| a.port_path.cmp(&b.port_path))
    });
    devices
}

#[cfg(unix)]
fn collect_device(name: &str, path: &Path) -> Option<UsbDevice> {
    let vendor_id = read_hex_u16(path, "idVendor")?;
    let product_id = read_hex_u16(path, "idProduct")?;

    let bus = sysfs::read_u64_optional(&path.join("busnum"))? as u8;
    let devnum = sysfs::read_u64_optional(&path.join("devnum"))? as u16;
    let port_path =
        sysfs::read_string_optional(&path.join("devpath")).unwrap_or_else(|| "0".into());

    let manufacturer = sysfs::read_string_optional(&path.join("manufacturer"));
    let product = sysfs::read_string_optional(&path.join("product"));
    let serial_number = sysfs::read_string_optional(&path.join("serial"));
    let usb_version = sysfs::read_string_optional(&path.join("version")).map(|s| s.trim().into());

    let device_class = sysfs::read_string_optional(&path.join("bDeviceClass"))
        .and_then(|s| u8::from_str_radix(&s, 16).ok())
        .unwrap_or(0);

    let speed = sysfs::read_string_optional(&path.join("speed"))
        .map(|s| classify_speed(&s))
        .unwrap_or(UsbSpeed::Unknown("unknown".into()));

    let max_power_ma =
        sysfs::read_string_optional(&path.join("bMaxPower")).and_then(|s| parse_max_power(&s));

    Some(UsbDevice {
        bus,
        port_path,
        devnum,
        vendor_id,
        product_id,
        manufacturer,
        product,
        serial_number,
        usb_version,
        device_class,
        speed,
        max_power_ma,
        sysfs_id: name.to_string(),
    })
}

#[cfg(unix)]
fn read_hex_u16(path: &Path, attr: &str) -> Option<u16> {
    sysfs::read_string_optional(&path.join(attr)).and_then(|s| u16::from_str_radix(&s, 16).ok())
}

// ── Windows (WMI via PowerShell) implementation ────────────────────────────

#[cfg(windows)]
pub fn collect() -> Vec<UsbDevice> {
    collect_via_powershell().unwrap_or_default()
}

/// Query WMI for USB devices, combining Win32_USBControllerDevice /
/// Win32_PnPEntity to get all USB-attached PnP entities with VID/PID.
/// Also queries Win32_USBHub for USB version info to classify speed.
#[cfg(windows)]
fn collect_via_powershell() -> Option<Vec<UsbDevice>> {
    // We query Win32_PnPEntity for devices whose DeviceID starts with "USB\"
    // and also pick up USBSTOR, HID devices on USB, etc. by looking for
    // VID_ and PID_ in the DeviceID.
    // We also query Win32_USBHub to get USBVersion for hub-based speed
    // classification.
    let ps_script = r#"
$hubs = @{}
Get-CimInstance Win32_USBHub | ForEach-Object {
    $hubs[$_.DeviceID] = $_.USBVersion
}
$devs = Get-CimInstance Win32_PnPEntity | Where-Object {
    $_.DeviceID -match 'VID_[0-9A-Fa-f]{4}&PID_[0-9A-Fa-f]{4}'
}
$result = @()
$idx = 0
foreach ($d in $devs) {
    $cids = @()
    if ($d.CompatibleID -ne $null) { $cids = @($d.CompatibleID) }
    $hubVer = $null
    if ($hubs.ContainsKey($d.DeviceID)) { $hubVer = $hubs[$d.DeviceID] }
    $obj = [ordered]@{
        DeviceID     = $d.DeviceID
        Name         = $d.Name
        Manufacturer = $d.Manufacturer
        Status       = $d.Status
        Index        = $idx
        CompatibleID = $cids
        USBVersion   = $hubVer
    }
    $result += $obj
    $idx++
}
$result | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .output()
        .ok()?;

    if !output.status.success() {
        log::debug!(
            "USB PowerShell query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Some(Vec::new());
    }

    let json_str = if stdout.starts_with('[') {
        stdout.to_string()
    } else {
        format!("[{}]", stdout)
    };

    let raw: Vec<WmiUsbRow> = serde_json::from_str(&json_str).ok()?;

    let mut devices: Vec<UsbDevice> = raw.iter().filter_map(wmi_row_to_usb).collect();
    devices.sort_by(|a, b| {
        a.bus
            .cmp(&b.bus)
            .then_with(|| a.port_path.cmp(&b.port_path))
    });
    Some(devices)
}

#[cfg(windows)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
struct WmiUsbRow {
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    name: Option<String>,
    manufacturer: Option<String>,
    status: Option<String>,
    index: Option<u32>,
    #[serde(rename = "CompatibleID")]
    compatible_id: Option<Vec<String>>,
    #[serde(rename = "USBVersion")]
    usb_version: Option<String>,
}

/// Parse a WMI PnP DeviceID like:
///   `USB\VID_046D&PID_C52B\5&2F4E3A01&0&2`
/// into vendor_id / product_id and best-effort other fields.
#[cfg(windows)]
fn wmi_row_to_usb(row: &WmiUsbRow) -> Option<UsbDevice> {
    let device_id = row.device_id.as_deref()?;

    // Extract VID and PID using regex-style manual parsing
    let vendor_id = extract_vid(device_id)?;
    let product_id = extract_pid(device_id)?;

    // Try to extract serial number: the last backslash-separated segment
    // may be a serial number if it doesn't look like a generic instance ID.
    let parts: Vec<&str> = device_id.split('\\').collect();
    let serial_number = if parts.len() >= 3 {
        let last = parts[parts.len() - 1];
        // Generic instance IDs contain '&'; serial numbers typically don't.
        if last.contains('&') {
            None
        } else {
            Some(last.to_string())
        }
    } else {
        None
    };

    // Determine the bus type prefix (USB, HID, USBSTOR, etc.)
    let prefix = parts.first().copied().unwrap_or("");

    // Extract device class from CompatibleID strings.
    // CompatibleID entries look like "USB\Class_08&SubClass_06&Prot_50"
    // or "USB\Class_03". We parse the hex class code.
    let device_class = extract_device_class(row.compatible_id.as_deref());

    // Classify USB speed from multiple sources:
    // 1. Win32_USBHub.USBVersion if available
    // 2. CompatibleID hints (USB30_HUB, etc.)
    // 3. DeviceID pattern heuristics (USB3 prefix, etc.)
    let speed = classify_speed_windows(
        device_id,
        row.usb_version.as_deref(),
        row.compatible_id.as_deref(),
    );

    // USB version from Win32_USBHub query (only set for hub entries)
    let usb_version = row.usb_version.as_ref().map(|v| v.trim().to_string());

    let idx = row.index.unwrap_or(0);

    Some(UsbDevice {
        bus: 0,                        // Not available from WMI
        port_path: format!("{}", idx), // Synthetic
        devnum: idx as u16,
        vendor_id,
        product_id,
        manufacturer: row.manufacturer.clone(),
        product: row.name.clone(),
        serial_number,
        usb_version,
        device_class,
        speed,
        max_power_ma: None, // Not available from WMI
        sysfs_id: format!("{}\\{}", prefix, parts.get(1).unwrap_or(&"")),
    })
}

/// Extract USB device class from the CompatibleID array.
/// Entries like "USB\Class_08" or "USB\Class_08&SubClass_06&Prot_50"
/// contain the hex class code after "Class_".
/// Hub devices may not have Class_ entries but instead have
/// "USB\USB20_HUB" or "USB\USB30_HUB" — these map to class 0x09 (Hub).
#[cfg(windows)]
fn extract_device_class(compatible_ids: Option<&[String]>) -> u8 {
    let ids = match compatible_ids {
        Some(ids) => ids,
        None => return 0,
    };
    for id in ids {
        let upper = id.to_uppercase();
        if let Some(pos) = upper.find("USB\\CLASS_") {
            let hex_start = pos + "USB\\CLASS_".len();
            let hex: String = upper[hex_start..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if let Ok(class) = u8::from_str_radix(&hex, 16) {
                return class;
            }
        }
    }
    // Check for hub CompatibleID patterns (USB\USB20_HUB, USB\USB30_HUB)
    for id in ids {
        let upper = id.to_uppercase();
        if upper.contains("USB20_HUB") || upper.contains("USB30_HUB") {
            return 0x09; // Hub class
        }
    }
    0
}

/// Classify USB speed on Windows using available information:
/// 1. If Win32_USBHub provides a USBVersion string, use it.
/// 2. Check CompatibleID for USB30_HUB / USB20_HUB hints.
/// 3. Otherwise, infer from DeviceID patterns.
#[cfg(windows)]
fn classify_speed_windows(
    device_id: &str,
    usb_version: Option<&str>,
    compatible_ids: Option<&[String]>,
) -> UsbSpeed {
    // First: use USBVersion from Win32_USBHub if available
    if let Some(ver) = usb_version {
        let ver = ver.trim();
        return match ver {
            "1.00" | "1.10" => UsbSpeed::Full,
            "2.00" => UsbSpeed::High,
            "3.00" => UsbSpeed::Super,
            "3.10" => UsbSpeed::SuperPlus,
            "3.20" => UsbSpeed::SuperPlus2x2,
            _ => UsbSpeed::Unknown(format!("USB {}", ver)),
        };
    }

    // Second: check CompatibleID for hub speed hints.
    // USB\USB30_HUB -> SuperSpeed, USB\USB20_HUB -> High speed
    if let Some(ids) = compatible_ids {
        for id in ids {
            let upper = id.to_uppercase();
            if upper.contains("USB30_HUB") {
                return UsbSpeed::Super;
            }
        }
    }

    // Third: heuristic from DeviceID patterns.
    let upper = device_id.to_uppercase();

    // USB3 in the DeviceID prefix indicates SuperSpeed
    if upper.starts_with("USB3\\") {
        return UsbSpeed::Super;
    }

    // For plain "USB\" or "USBSTOR\" prefix, assume High speed (USB 2.0)
    // as a reasonable default for modern systems.
    if upper.starts_with("USB\\") || upper.starts_with("USBSTOR\\") {
        return UsbSpeed::High;
    }

    // HID devices on USB are often Low or Full speed, but we can't be sure.
    if upper.starts_with("HID\\") {
        return UsbSpeed::Full;
    }

    UsbSpeed::Unknown("N/A".into())
}

/// Extract VID_xxxx from a DeviceID string.
#[cfg(windows)]
fn extract_vid(s: &str) -> Option<u16> {
    let marker = "VID_";
    let start = s.find(marker)? + marker.len();
    let hex: String = s[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    u16::from_str_radix(&hex, 16).ok()
}

/// Extract PID_xxxx from a DeviceID string.
#[cfg(windows)]
fn extract_pid(s: &str) -> Option<u16> {
    let marker = "PID_";
    let start = s.find(marker)? + marker.len();
    let hex: String = s[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    u16::from_str_radix(&hex, 16).ok()
}

// ── Shared helpers ─────────────────────────────────────────────────────────

#[allow(dead_code)]
fn classify_speed(speed: &str) -> UsbSpeed {
    match speed {
        "1.5" => UsbSpeed::Low,
        "12" => UsbSpeed::Full,
        "480" => UsbSpeed::High,
        "5000" => UsbSpeed::Super,
        "10000" => UsbSpeed::SuperPlus,
        "20000" => UsbSpeed::SuperPlus2x2,
        other => UsbSpeed::Unknown(other.to_string()),
    }
}

#[cfg(unix)]
fn parse_max_power(s: &str) -> Option<u32> {
    // Formats: "500mA" or "0mA"
    s.strip_suffix("mA")
        .and_then(|v| v.trim().parse::<u32>().ok())
}

pub struct UsbCollector;

impl crate::collectors::Collector for UsbCollector {
    fn name(&self) -> &str {
        "usb"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.usb_devices = collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_speed() {
        assert_eq!(classify_speed("480"), UsbSpeed::High);
        assert_eq!(classify_speed("5000"), UsbSpeed::Super);
        assert_eq!(classify_speed("999"), UsbSpeed::Unknown("999".to_string()));
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_vid_pid() {
        assert_eq!(
            extract_vid(r"USB\VID_046D&PID_C52B\5&2F4E3A01&0&2"),
            Some(0x046D)
        );
        assert_eq!(
            extract_pid(r"USB\VID_046D&PID_C52B\5&2F4E3A01&0&2"),
            Some(0xC52B)
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_wmi_row_to_usb() {
        let row = WmiUsbRow {
            device_id: Some(r"USB\VID_046D&PID_C52B\5&2F4E3A01&0&2".to_string()),
            name: Some("Logitech Unifying Receiver".to_string()),
            manufacturer: Some("Logitech".to_string()),
            status: Some("OK".to_string()),
            index: Some(0),
            compatible_id: Some(vec![
                r"USB\Class_03&SubClass_01&Prot_01".to_string(),
                r"USB\Class_03".to_string(),
            ]),
            usb_version: None,
        };

        let dev = wmi_row_to_usb(&row).unwrap();
        assert_eq!(dev.vendor_id, 0x046D);
        assert_eq!(dev.product_id, 0xC52B);
        assert_eq!(dev.product.as_deref(), Some("Logitech Unifying Receiver"));
        assert_eq!(dev.device_class, 0x03); // HID
        assert_eq!(dev.speed, UsbSpeed::High); // USB\ prefix -> High
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_device_class() {
        assert_eq!(
            extract_device_class(Some(&[
                r"USB\Class_08&SubClass_06&Prot_50".to_string(),
                r"USB\Class_08".to_string(),
            ])),
            0x08 // Mass Storage
        );
        assert_eq!(
            extract_device_class(Some(&[r"USB\Class_E0".to_string()])),
            0xE0 // Wireless
        );
        assert_eq!(
            extract_device_class(Some(&[r"USB\USB20_HUB".to_string()])),
            0x09 // Hub
        );
        assert_eq!(
            extract_device_class(Some(&[r"USB\USB30_HUB".to_string()])),
            0x09 // Hub
        );
        assert_eq!(extract_device_class(None), 0);
        assert_eq!(extract_device_class(Some(&[])), 0);
    }

    #[cfg(windows)]
    #[test]
    fn test_classify_speed_windows() {
        // USBVersion takes precedence over everything
        assert_eq!(
            classify_speed_windows(r"USB\VID_1234&PID_5678\0", Some("3.00"), None),
            UsbSpeed::Super
        );
        assert_eq!(
            classify_speed_windows(r"USB\VID_1234&PID_5678\0", Some("2.00"), None),
            UsbSpeed::High
        );
        // CompatibleID USB30_HUB -> Super
        assert_eq!(
            classify_speed_windows(
                r"USB\VID_05E3&PID_0620\0",
                None,
                Some(&[r"USB\USB30_HUB".to_string()])
            ),
            UsbSpeed::Super
        );
        // CompatibleID USB20_HUB -> falls through to DeviceID heuristic (High)
        assert_eq!(
            classify_speed_windows(
                r"USB\VID_05E3&PID_0608\0",
                None,
                Some(&[r"USB\USB20_HUB".to_string()])
            ),
            UsbSpeed::High
        );
        // No USBVersion, no CompatibleID: infer from DeviceID
        assert_eq!(
            classify_speed_windows(r"USB3\VID_1234&PID_5678\0", None, None),
            UsbSpeed::Super
        );
        assert_eq!(
            classify_speed_windows(r"USB\VID_1234&PID_5678\0", None, None),
            UsbSpeed::High
        );
        assert_eq!(
            classify_speed_windows(r"HID\VID_1234&PID_5678\0", None, None),
            UsbSpeed::Full
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_wmi_row_with_usb_version() {
        let row = WmiUsbRow {
            device_id: Some(r"USB\VID_0BDA&PID_5411\1234".to_string()),
            name: Some("USB3 Hub".to_string()),
            manufacturer: Some("Realtek".to_string()),
            status: Some("OK".to_string()),
            index: Some(1),
            compatible_id: Some(vec![r"USB\Class_09".to_string()]),
            usb_version: Some("3.00".to_string()),
        };

        let dev = wmi_row_to_usb(&row).unwrap();
        assert_eq!(dev.device_class, 0x09); // Hub
        assert_eq!(dev.speed, UsbSpeed::Super);
        assert_eq!(dev.usb_version.as_deref(), Some("3.00"));
    }
}
