//! GPU information collector.
//!
//! On Linux, enumerates DRM cards from sysfs, reads PCI properties, then
//! enriches with vendor-specific data (NVML for NVIDIA, hwmon/sysfs for AMD,
//! basic sysfs for Intel).
//!
//! On Windows, discovers GPUs via NVML (NVIDIA) and uses
//! EnumDisplayDevices/EnumDisplaySettings for display output enumeration.

#[cfg(all(unix, feature = "nvidia"))]
use std::collections::HashMap;
#[cfg(feature = "nvidia")]
use std::ffi::CStr;
#[cfg(unix)]
use std::path::{Path, PathBuf};

use crate::model::gpu::{DisplayOutput, GpuInfo, GpuVendor, PcieLinkInfo};
#[cfg(unix)]
use crate::platform::sysfs::{
    glob_paths, read_link_basename, read_string_optional, read_u32_optional, read_u64_optional,
};

// PCI vendor IDs
const PCI_VENDOR_NVIDIA: u16 = 0x10de;
#[cfg(unix)]
const PCI_VENDOR_AMD: u16 = 0x1002;
#[cfg(unix)]
const PCI_VENDOR_INTEL: u16 = 0x8086;

// ===========================================================================
// Unix (Linux) collect
// ===========================================================================

/// Collect information about all GPUs visible through DRM (Linux).
///
/// When `no_nvidia` is `true`, NVML is not loaded even if the `nvidia`
/// feature is compiled in.
#[cfg(unix)]
pub fn collect(no_nvidia: bool) -> Vec<GpuInfo> {
    let _ = no_nvidia; // Used only with the `nvidia` feature.

    let card_paths = discover_drm_cards();
    if card_paths.is_empty() {
        return Vec::new();
    }

    // Optionally load NVML once for all NVIDIA cards.
    #[cfg(feature = "nvidia")]
    let nvml = if !no_nvidia {
        crate::platform::nvml::NvmlLibrary::try_load()
    } else {
        None
    };

    // Build a map from PCI bus address -> NVML device index for matching.
    #[cfg(feature = "nvidia")]
    let nvml_bus_map: HashMap<String, u32> = match &nvml {
        Some(lib) => build_nvml_bus_map(lib),
        None => HashMap::new(),
    };

    let mut gpus = Vec::new();
    for (card_idx, card_path) in card_paths.iter().enumerate() {
        let device_path = card_path.join("device");
        if !device_path.exists() {
            continue;
        }

        // Read PCI vendor/device IDs
        let vendor_id = match read_u64_optional(&device_path.join("vendor")) {
            Some(v) => v as u16,
            None => continue,
        };
        let device_id = match read_u64_optional(&device_path.join("device")) {
            Some(v) => v as u16,
            None => continue,
        };

        let vendor = match vendor_id {
            PCI_VENDOR_NVIDIA => GpuVendor::Nvidia,
            PCI_VENDOR_AMD => GpuVendor::Amd,
            PCI_VENDOR_INTEL => GpuVendor::Intel,
            _ => GpuVendor::Unknown(format!("{vendor_id:#06x}")),
        };

        let subsystem_vendor =
            read_u64_optional(&device_path.join("subsystem_vendor")).map(|v| v as u16);
        let subsystem_device =
            read_u64_optional(&device_path.join("subsystem_device")).map(|v| v as u16);

        // PCI bus address from the device symlink or directory name
        let pci_bus_address = read_link_basename(&device_path)
            .or_else(|| {
                device_path
                    .canonicalize()
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            })
            .unwrap_or_default();

        // Driver module name
        let driver_module = read_link_basename(&device_path.join("driver"));

        // PCIe link from sysfs
        let pcie_link = read_pcie_link(&device_path);

        // Fallback name from pci-ids database
        let fallback_name = pci_ids::Device::from_vid_pid(vendor_id, device_id)
            .map(|d| d.name().to_string())
            .unwrap_or_else(|| format!("{vendor_id:#06x}:{device_id:#06x}"));

        // Display outputs
        let drm_card_name = card_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let display_outputs = enumerate_display_outputs(&drm_card_name);

        // Build the base struct; vendor-specific code fills in the rest.
        let mut info = GpuInfo {
            index: card_idx as u32,
            vendor: vendor.clone(),
            name: fallback_name,
            architecture: None,
            pci_vendor_id: vendor_id,
            pci_device_id: device_id,
            pci_subsystem_vendor_id: subsystem_vendor,
            pci_subsystem_device_id: subsystem_device,
            pci_bus_address: pci_bus_address.clone(),
            drm_card_index: extract_card_index(&drm_card_name),
            vbios_version: None,
            driver_version: None,
            driver_module,
            vram_total_bytes: None,
            vram_type: None,
            vram_bus_width_bits: None,
            max_core_clock_mhz: None,
            max_memory_clock_mhz: None,
            compute_capability: None,
            shader_units: None,
            power_limit_watts: None,
            ecc_enabled: None,
            pcie_link,
            display_outputs,
        };

        // Vendor-specific enrichment
        match &vendor {
            GpuVendor::Nvidia =>
            {
                #[cfg(feature = "nvidia")]
                if let Some(ref lib) = nvml {
                    let nvml_idx = nvml_bus_map.get(&normalize_bus_addr(&pci_bus_address));
                    enrich_nvidia(&mut info, lib, nvml_idx.copied());
                }
            }
            GpuVendor::Amd => {
                enrich_amd(&mut info, &device_path, card_path);
            }
            GpuVendor::Intel => {
                enrich_intel(&mut info, &device_path, card_path);
            }
            GpuVendor::Unknown(_) => {}
        }

        gpus.push(info);
    }

    gpus
}

// ===========================================================================
// Windows collect
// ===========================================================================

/// Collect information about all GPUs (Windows).
///
/// Uses NVML for NVIDIA GPU discovery and enrichment, then attaches display
/// outputs enumerated via EnumDisplayDevices / EnumDisplaySettings.
#[cfg(windows)]
pub fn collect(no_nvidia: bool) -> Vec<GpuInfo> {
    let _ = no_nvidia; // Used only with the `nvidia` feature.

    let mut gpus = Vec::new();

    // Enumerate display outputs first — we will attach them to GPUs later.
    let display_data = enumerate_display_outputs_win();

    // Try NVML for NVIDIA GPUs.
    #[cfg(feature = "nvidia")]
    {
        if !no_nvidia {
            if let Some(lib) = crate::platform::nvml::NvmlLibrary::try_load() {
                let count = lib.device_count().unwrap_or(0);
                for idx in 0..count {
                    let name = lib
                        .device_name(idx)
                        .unwrap_or_else(|_| "NVIDIA GPU".to_string());

                    // PCI info
                    let (pci_bus_address, vendor_id, device_id, subsys_vendor, subsys_device) =
                        if let Ok(pci) = lib.device_pci_info(idx) {
                            let bus_id =
                                crate::platform::nvml::NvmlLibrary::read_c_string_from_pci(&pci);
                            let vid = (pci.pci_device_id & 0xFFFF) as u16;
                            let did = (pci.pci_device_id >> 16) as u16;
                            let svid = (pci.pci_subsystem_id & 0xFFFF) as u16;
                            let sdid = (pci.pci_subsystem_id >> 16) as u16;
                            (bus_id, vid, did, Some(svid), Some(sdid))
                        } else {
                            (String::new(), PCI_VENDOR_NVIDIA, 0, None, None)
                        };

                    let vram_total_bytes = lib.device_memory_info(idx).ok().map(|m| m.total);
                    let max_core_clock_mhz = lib
                        .device_max_clock_mhz(idx, crate::platform::nvml::NVML_CLOCK_GRAPHICS)
                        .ok();
                    let max_memory_clock_mhz = lib
                        .device_max_clock_mhz(idx, crate::platform::nvml::NVML_CLOCK_MEM)
                        .ok();
                    let power_limit_watts = lib.device_power_limit_watts(idx).ok();
                    let vbios_version = lib.device_vbios_version(idx).ok();
                    let driver_version = lib.driver_version().ok();

                    let pcie_link = match (lib.device_pcie_gen(idx), lib.device_pcie_width(idx)) {
                        (Ok(pcie_gen), Ok(width)) => Some(PcieLinkInfo {
                            current_gen: Some(pcie_gen as u8),
                            current_width: Some(width as u8),
                            max_gen: None,
                            max_width: None,
                            current_speed: None,
                            max_speed: None,
                        }),
                        _ => None,
                    };

                    let info = GpuInfo {
                        index: idx,
                        vendor: GpuVendor::Nvidia,
                        name,
                        architecture: None,
                        pci_vendor_id: vendor_id,
                        pci_device_id: device_id,
                        pci_subsystem_vendor_id: subsys_vendor,
                        pci_subsystem_device_id: subsys_device,
                        pci_bus_address: normalize_bus_addr(&pci_bus_address),
                        drm_card_index: None,
                        vbios_version,
                        driver_version,
                        driver_module: Some("nvlddmkm".to_string()),
                        vram_total_bytes,
                        vram_type: None,
                        vram_bus_width_bits: None,
                        max_core_clock_mhz,
                        max_memory_clock_mhz,
                        compute_capability: None,
                        shader_units: None,
                        power_limit_watts,
                        ecc_enabled: None,
                        pcie_link,
                        display_outputs: Vec::new(), // attached below
                    };

                    gpus.push(info);
                }
            }
        }
    }

    // Attach display outputs to GPUs.
    // If there is exactly one GPU, it gets all outputs.
    // If multiple, try to match by adapter description containing GPU vendor/name keywords.
    if gpus.len() == 1 {
        let all_outputs: Vec<DisplayOutput> = display_data
            .into_iter()
            .flat_map(|(_, outputs)| outputs)
            .collect();
        gpus[0].display_outputs = all_outputs;
    } else if gpus.len() > 1 {
        for (adapter_desc, outputs) in &display_data {
            let desc_lower = adapter_desc.to_lowercase();
            // Find the best matching GPU by checking if the adapter description
            // contains the GPU name or vendor-specific keywords.
            let mut matched = false;
            for gpu in gpus.iter_mut() {
                let gpu_name_lower = gpu.name.to_lowercase();
                let vendor_keyword = match &gpu.vendor {
                    GpuVendor::Nvidia => "nvidia",
                    GpuVendor::Amd => "amd",
                    GpuVendor::Intel => "intel",
                    GpuVendor::Unknown(_) => "",
                };
                if desc_lower.contains(&gpu_name_lower)
                    || desc_lower.contains(vendor_keyword)
                    || gpu_name_lower.contains(&desc_lower)
                {
                    gpu.display_outputs.extend(outputs.clone());
                    matched = true;
                    break;
                }
            }
            // If no match found, attach to the first GPU as fallback.
            if !matched {
                if let Some(first) = gpus.first_mut() {
                    first.display_outputs.extend(outputs.clone());
                }
            }
        }
    }

    gpus
}

// ---------------------------------------------------------------------------
// DRM card discovery (Linux only)
// ---------------------------------------------------------------------------

/// Returns sorted paths like `/sys/class/drm/card0`, `/sys/class/drm/card1`, etc.
#[cfg(unix)]
fn discover_drm_cards() -> Vec<PathBuf> {
    let mut paths = glob_paths("/sys/class/drm/card[0-9]*");
    // Filter out render nodes (card0-DP-1 etc) — we only want cardN
    paths.retain(|p| {
        p.file_name()
            .map(|n| {
                let s = n.to_string_lossy();
                s.starts_with("card") && s[4..].chars().all(|c| c.is_ascii_digit())
            })
            .unwrap_or(false)
    });
    paths.sort();
    paths
}

/// Extract the numeric card index from "card3" -> Some(3).
#[cfg(unix)]
fn extract_card_index(name: &str) -> Option<u32> {
    name.strip_prefix("card")
        .and_then(|s| s.parse::<u32>().ok())
}

// ---------------------------------------------------------------------------
// PCIe link speed parsing (Linux only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn read_pcie_link(device_path: &Path) -> Option<PcieLinkInfo> {
    let current_speed = read_string_optional(&device_path.join("current_link_speed"));
    let current_width = read_string_optional(&device_path.join("current_link_width"))
        .and_then(|s| parse_link_width(&s));
    let max_speed = read_string_optional(&device_path.join("max_link_speed"));
    let max_width = read_string_optional(&device_path.join("max_link_width"))
        .and_then(|s| parse_link_width(&s));

    let current_gen = current_speed.as_deref().and_then(parse_pcie_gen);
    let max_gen = max_speed.as_deref().and_then(parse_pcie_gen);

    if current_speed.is_none()
        && max_speed.is_none()
        && current_width.is_none()
        && max_width.is_none()
    {
        return None;
    }

    Some(PcieLinkInfo {
        current_gen,
        current_width,
        max_gen,
        max_width,
        current_speed,
        max_speed,
    })
}

/// Parse a PCIe speed string like "16.0 GT/s PCIe" into a generation number.
#[cfg(unix)]
fn parse_pcie_gen(speed: &str) -> Option<u8> {
    // Extract the GT/s number
    let gts: f64 = speed.split_whitespace().next()?.parse().ok()?;
    // Map GT/s to PCIe generation
    match gts as u32 {
        2 => Some(1),  // 2.5 GT/s rounds to 2
        5 => Some(2),  // 5.0 GT/s
        8 => Some(3),  // 8.0 GT/s
        16 => Some(4), // 16.0 GT/s
        32 => Some(5), // 32.0 GT/s
        64 => Some(6), // 64.0 GT/s
        _ => {
            // More precise matching with the float
            if (2.4..2.6).contains(&gts) {
                Some(1)
            } else if (4.9..5.1).contains(&gts) {
                Some(2)
            } else if (7.9..8.1).contains(&gts) {
                Some(3)
            } else if (15.9..16.1).contains(&gts) {
                Some(4)
            } else if (31.9..32.1).contains(&gts) {
                Some(5)
            } else if (63.9..64.1).contains(&gts) {
                Some(6)
            } else {
                None
            }
        }
    }
}

/// Parse link width like "x16" or "16" into a u8.
#[cfg(unix)]
fn parse_link_width(s: &str) -> Option<u8> {
    let s = s.strip_prefix('x').unwrap_or(s);
    s.trim().parse::<u8>().ok()
}

// ---------------------------------------------------------------------------
// Display output enumeration (Linux — DRM sysfs)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn enumerate_display_outputs(card_name: &str) -> Vec<DisplayOutput> {
    let pattern = format!("/sys/class/drm/{card_name}-*");
    let mut outputs = Vec::new();

    for path in glob_paths(&pattern) {
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // Parse connector name: "card0-DP-1" -> type="DP", index=1
        let suffix = match name.strip_prefix(&format!("{card_name}-")) {
            Some(s) => s,
            None => continue,
        };

        // Only process known connector types
        let (connector_type, connector_index) = match parse_connector(suffix) {
            Some(v) => v,
            None => continue,
        };

        let status =
            read_string_optional(&path.join("status")).unwrap_or_else(|| "unknown".to_string());

        // Parse EDID for connected monitors
        let (monitor_name, resolution) = if status == "connected" {
            match crate::parsers::edid::parse_from_drm(&path) {
                Some(edid) => {
                    let res = match (edid.preferred_width, edid.preferred_height) {
                        (Some(w), Some(h)) => {
                            let hz = edid
                                .preferred_refresh_hz
                                .map(|r| format!(" @ {r:.0}Hz"))
                                .unwrap_or_default();
                            Some(format!("{w}x{h}{hz}"))
                        }
                        _ => None,
                    };
                    (edid.monitor_name, res)
                }
                None => (None, None),
            }
        } else {
            (None, None)
        };

        outputs.push(DisplayOutput {
            connector_type,
            index: connector_index,
            status,
            monitor_name,
            resolution,
        });
    }

    outputs.sort_by(|a, b| {
        a.connector_type
            .cmp(&b.connector_type)
            .then(a.index.cmp(&b.index))
    });
    outputs
}

/// Parse "DP-1" -> ("DP", 1), "HDMI-A-2" -> ("HDMI", 2), etc.
#[cfg(unix)]
fn parse_connector(s: &str) -> Option<(String, u32)> {
    // Known prefixes in DRM connector naming
    let known_types = [
        "DP", "HDMI-A", "HDMI-B", "VGA", "DVI-I", "DVI-D", "DVI-A", "eDP",
    ];

    for prefix in &known_types {
        if let Some(rest) = s.strip_prefix(prefix) {
            if let Some(idx_str) = rest.strip_prefix('-') {
                if let Ok(idx) = idx_str.parse::<u32>() {
                    // Normalize HDMI-A/HDMI-B to HDMI
                    let display_type = if prefix.starts_with("HDMI") {
                        "HDMI".to_string()
                    } else {
                        prefix.to_string()
                    };
                    return Some((display_type, idx));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Display output enumeration (Windows — EnumDisplayDevices/EnumDisplaySettings)
// ---------------------------------------------------------------------------

/// Enumerate display outputs on Windows using Win32 EnumDisplayDevices and
/// EnumDisplaySettings. Returns a list of `(adapter_description, outputs)` pairs.
#[cfg(windows)]
fn enumerate_display_outputs_win() -> Vec<(String, Vec<DisplayOutput>)> {
    use winapi::shared::minwindef::DWORD;
    use winapi::um::wingdi::{DEVMODEW, DISPLAY_DEVICEW};
    use winapi::um::winuser::{ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsW};

    let mut results = Vec::new();
    let mut adapter_idx: DWORD = 0;

    loop {
        let mut adapter: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        adapter.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as DWORD;

        if unsafe { EnumDisplayDevicesW(std::ptr::null(), adapter_idx, &mut adapter, 0) } == 0 {
            break;
        }
        adapter_idx += 1;

        // Skip non-active adapters (DISPLAY_DEVICE_ATTACHED_TO_DESKTOP = 0x1)
        if adapter.StateFlags & 0x00000001 == 0 {
            continue;
        }

        let adapter_name = wchar_to_string(&adapter.DeviceName);
        let adapter_desc = wchar_to_string(&adapter.DeviceString);

        // Enumerate monitors on this adapter
        let mut monitor_idx: DWORD = 0;
        let mut outputs = Vec::new();

        loop {
            let mut monitor: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
            monitor.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as DWORD;

            if unsafe {
                EnumDisplayDevicesW(adapter.DeviceName.as_ptr(), monitor_idx, &mut monitor, 0)
            } == 0
            {
                break;
            }

            let mon_name = wchar_to_string(&monitor.DeviceString);
            let is_active = monitor.StateFlags & 0x00000001 != 0; // DISPLAY_DEVICE_ACTIVE

            // Get resolution via EnumDisplaySettings
            let resolution = if is_active {
                let mut devmode: DEVMODEW = unsafe { std::mem::zeroed() };
                devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;

                if unsafe {
                    EnumDisplaySettingsW(
                        adapter.DeviceName.as_ptr(),
                        ENUM_CURRENT_SETTINGS,
                        &mut devmode,
                    )
                } != 0
                {
                    let width = devmode.dmPelsWidth;
                    let height = devmode.dmPelsHeight;
                    let freq = devmode.dmDisplayFrequency;
                    if width > 0 && height > 0 {
                        Some(format!("{}x{}@{}Hz", width, height, freq))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            outputs.push(DisplayOutput {
                connector_type: "Display".to_string(),
                index: monitor_idx,
                status: if is_active {
                    "connected".to_string()
                } else {
                    "disconnected".to_string()
                },
                monitor_name: if mon_name.is_empty() {
                    None
                } else {
                    Some(mon_name)
                },
                resolution,
            });

            monitor_idx += 1;
        }

        // If no monitor sub-devices found but adapter is active, create an
        // entry from the adapter itself.
        if outputs.is_empty() && adapter.StateFlags & 0x00000001 != 0 {
            let mut devmode: DEVMODEW = unsafe { std::mem::zeroed() };
            devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            let resolution = if unsafe {
                EnumDisplaySettingsW(
                    adapter.DeviceName.as_ptr(),
                    ENUM_CURRENT_SETTINGS,
                    &mut devmode,
                )
            } != 0
            {
                let width = devmode.dmPelsWidth;
                let height = devmode.dmPelsHeight;
                let freq = devmode.dmDisplayFrequency;
                if width > 0 && height > 0 {
                    Some(format!("{}x{}@{}Hz", width, height, freq))
                } else {
                    None
                }
            } else {
                None
            };

            outputs.push(DisplayOutput {
                connector_type: "Display".to_string(),
                index: 0,
                status: "connected".to_string(),
                monitor_name: if adapter_name.is_empty() {
                    None
                } else {
                    Some(adapter_desc.clone())
                },
                resolution,
            });
        }

        results.push((adapter_desc, outputs));
    }

    results
}

/// Convert a null-terminated wide character (UTF-16) slice to a Rust String.
#[cfg(windows)]
fn wchar_to_string(wchars: &[u16]) -> String {
    let len = wchars.iter().position(|&c| c == 0).unwrap_or(wchars.len());
    String::from_utf16_lossy(&wchars[..len])
}

// ---------------------------------------------------------------------------
// NVIDIA enrichment (via NVML) — shared between platforms
// ---------------------------------------------------------------------------

#[cfg(all(unix, feature = "nvidia"))]
fn build_nvml_bus_map(lib: &crate::platform::nvml::NvmlLibrary) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    let count = match lib.device_count() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("NVML device_count failed: {e}");
            return map;
        }
    };
    for idx in 0..count {
        if let Ok(pci) = lib.device_pci_info(idx) {
            let bus_id = crate::platform::nvml::NvmlLibrary::read_c_string_from_pci(&pci);
            map.insert(normalize_bus_addr(&bus_id), idx);
        }
    }
    map
}

#[cfg(all(unix, feature = "nvidia"))]
fn enrich_nvidia(
    info: &mut GpuInfo,
    lib: &crate::platform::nvml::NvmlLibrary,
    nvml_idx: Option<u32>,
) {
    let idx = match nvml_idx {
        Some(i) => i,
        None => return,
    };

    if let Ok(name) = lib.device_name(idx) {
        info.name = name;
    }

    if let Ok(mem) = lib.device_memory_info(idx) {
        info.vram_total_bytes = Some(mem.total);
    }

    info.max_core_clock_mhz = lib
        .device_max_clock_mhz(idx, crate::platform::nvml::NVML_CLOCK_GRAPHICS)
        .ok();
    info.max_memory_clock_mhz = lib
        .device_max_clock_mhz(idx, crate::platform::nvml::NVML_CLOCK_MEM)
        .ok();

    if let Ok(limit) = lib.device_power_limit_watts(idx) {
        info.power_limit_watts = Some(limit);
    }

    info.vbios_version = lib.device_vbios_version(idx).ok();
    info.driver_version = lib.driver_version().ok();

    // Override PCIe link info from NVML (more reliable than sysfs for
    // current negotiated link).
    if let (Ok(pcie_gen), Ok(width)) = (lib.device_pcie_gen(idx), lib.device_pcie_width(idx)) {
        if let Some(ref mut link) = info.pcie_link {
            link.current_gen = Some(pcie_gen as u8);
            link.current_width = Some(width as u8);
        } else {
            info.pcie_link = Some(PcieLinkInfo {
                current_gen: Some(pcie_gen as u8),
                current_width: Some(width as u8),
                max_gen: None,
                max_width: None,
                current_speed: None,
                max_speed: None,
            });
        }
    }
}

pub struct GpuCollector {
    pub no_nvidia: bool,
}

impl crate::collectors::Collector for GpuCollector {
    fn name(&self) -> &str {
        "gpu"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.gpus = collect(self.no_nvidia);
    }
}

/// Normalize a PCI bus address for comparison.
///
/// NVML returns "00000000:11:00.0" (8-digit domain), sysfs uses "0000:11:00.0"
/// (4-digit domain). Normalize to 4-digit domain lowercase.
fn normalize_bus_addr(addr: &str) -> String {
    let s = addr.trim().to_lowercase();
    // If domain is 8 digits (e.g. "00000000:11:00.0"), truncate to 4
    if s.len() > 12 {
        if let Some(first_colon) = s.find(':') {
            if first_colon > 4 {
                let domain = &s[..first_colon];
                // Take last 4 chars of domain
                let short_domain = &domain[domain.len().saturating_sub(4)..];
                return format!("{}{}", short_domain, &s[first_colon..]);
            }
        }
    }
    s
}

// ---------------------------------------------------------------------------
// AMD enrichment (Linux only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn enrich_amd(info: &mut GpuInfo, device_path: &Path, _card_path: &Path) {
    // Product name: prefer amdgpu's product_name over pci-ids fallback
    if let Some(name) = read_string_optional(&device_path.join("product_name")) {
        info.name = name;
    }

    // VRAM total
    info.vram_total_bytes = read_u64_optional(&device_path.join("mem_info_vram_total"));

    // VBIOS version
    info.vbios_version = read_string_optional(&device_path.join("vbios_version"));

    // Read max core clock from pp_dpm_sclk (highest entry with asterisk or last line)
    info.max_core_clock_mhz = read_amd_max_sclk(&device_path.join("pp_dpm_sclk"));

    // Power limit from hwmon
    if let Some(hwmon_path) = find_hwmon_path(device_path) {
        // power1_cap is in microwatts
        if let Some(uw) = read_u64_optional(&hwmon_path.join("power1_cap")) {
            info.power_limit_watts = Some(uw as f64 / 1_000_000.0);
        }
    }

    // Driver version from module version
    info.driver_version =
        read_string_optional(&device_path.join("driver").join("module").join("version"));
}

/// Parse pp_dpm_sclk to extract the maximum clock frequency.
///
/// Format example:
/// ```text
/// 0: 500Mhz
/// 1: 800Mhz
/// 2: 2100Mhz *
/// ```
/// Returns the highest MHz value found.
#[cfg(unix)]
fn read_amd_max_sclk(path: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut max_mhz: Option<u32> = None;
    for line in content.lines() {
        // Extract the MHz value: after ": " and before "Mhz" (case-insensitive)
        let parts: Vec<&str> = line.split_whitespace().collect();
        for part in &parts {
            if let Some(mhz_str) = part
                .strip_suffix("Mhz")
                .or_else(|| part.strip_suffix("MHz"))
                .or_else(|| part.strip_suffix("mhz"))
            {
                if let Ok(mhz) = mhz_str.parse::<u32>() {
                    max_mhz = Some(max_mhz.map_or(mhz, |prev: u32| prev.max(mhz)));
                }
            }
        }
    }
    max_mhz
}

/// Find the hwmon directory under a device path.
#[cfg(unix)]
fn find_hwmon_path(device_path: &Path) -> Option<PathBuf> {
    let pattern = format!("{}/hwmon/hwmon*", device_path.display());
    glob_paths(&pattern).into_iter().next()
}

// ---------------------------------------------------------------------------
// Intel enrichment (Linux only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn enrich_intel(info: &mut GpuInfo, device_path: &Path, card_path: &Path) {
    // Max GT frequency from DRM sysfs
    info.max_core_clock_mhz = read_u32_optional(&card_path.join("gt_max_freq_mhz"));

    // Driver version from module version
    info.driver_version =
        read_string_optional(&device_path.join("driver").join("module").join("version"));
}

// ---------------------------------------------------------------------------
// NVML PCI bus ID extraction helper
// ---------------------------------------------------------------------------

#[cfg(feature = "nvidia")]
impl crate::platform::nvml::NvmlLibrary {
    /// Extract the bus_id string from an NvmlPciInfo struct.
    pub(crate) fn read_c_string_from_pci(pci: &crate::platform::nvml::NvmlPciInfo) -> String {
        // SAFETY: NVML null-terminates bus_id.
        unsafe { CStr::from_ptr(pci.bus_id.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }
}
