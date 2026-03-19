use crate::model::gpu::PcieLinkInfo;
use crate::model::pci::{AerCounters, InterruptInfo, IrqVector, PciDevice};
use crate::platform::sysfs;
use pci_ids::FromId;
use std::collections::HashMap;
use std::path::Path;

pub fn collect() -> Vec<PciDevice> {
    let irq_map = parse_proc_interrupts();
    let mut devices = Vec::new();

    for entry in sysfs::glob_paths("/sys/bus/pci/devices/*") {
        if let Some(dev) = collect_device(&entry, &irq_map) {
            devices.push(dev);
        }
    }

    devices.sort_by(|a, b| a.address.cmp(&b.address));
    devices
}

fn collect_device(path: &Path, irq_map: &HashMap<String, Vec<ParsedIrq>>) -> Option<PciDevice> {
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
    let interrupts = build_interrupt_info(&address, irq_map);
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
        interrupts,
        aer,
    })
}

// ---------------------------------------------------------------------------
// /proc/interrupts parser
// ---------------------------------------------------------------------------

struct ParsedIrq {
    irq: u32,
    count: u64,
    mode: String,
    trigger: String,
    handler: String,
}

/// Parse /proc/interrupts and group PCI interrupt lines by device address.
///
/// Matches lines with `PCI-MSI-<bdf>` or `PCI-MSIX-<bdf>` type fields.
/// Some ARM/GICv3 platforms format MSI differently (hwirq numbers instead of
/// BDF addresses) — those will silently produce no matches, which is safe.
fn parse_proc_interrupts() -> HashMap<String, Vec<ParsedIrq>> {
    let content = match std::fs::read_to_string("/proc/interrupts") {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    parse_interrupts_str(&content)
}

/// Try each prefix; return the remainder of the first match.
fn strip_any_prefix<'a>(s: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes.iter().find_map(|p| s.strip_prefix(p))
}

fn parse_interrupts_str(content: &str) -> HashMap<String, Vec<ParsedIrq>> {
    let mut map: HashMap<String, Vec<ParsedIrq>> = HashMap::new();
    let mut lines = content.lines();

    let header = match lines.next() {
        Some(h) => h,
        None => return map,
    };
    let cpu_count = header.split_whitespace().count();

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Need: IRQ + cpu_count counts + type + trigger + handler(s)
        if parts.len() < cpu_count + 4 {
            continue;
        }

        let irq_str = parts[0].strip_suffix(':').unwrap_or(parts[0]);
        let irq: u32 = match irq_str.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        let count: u64 = parts[1..=cpu_count]
            .iter()
            .filter_map(|s| s.parse::<u64>().ok())
            .sum();

        let type_field = parts[cpu_count + 1];

        // Match PCI-MSI/MSIX with optional IR- prefix (Intel IOMMU remapping)
        let (mode, pci_addr) =
            if let Some(addr) = strip_any_prefix(type_field, &["PCI-MSIX-", "IR-PCI-MSIX-"]) {
                ("MSI-X".to_string(), addr.to_string())
            } else if let Some(addr) = strip_any_prefix(type_field, &["PCI-MSI-", "IR-PCI-MSI-"]) {
                ("MSI".to_string(), addr.to_string())
            } else {
                continue;
            };

        let trigger_field = parts.get(cpu_count + 2).unwrap_or(&"");
        let trigger = trigger_field
            .rsplit('-')
            .next()
            .unwrap_or("edge")
            .to_string();

        let handler = parts[cpu_count + 3..].to_vec().join(" ");

        map.entry(pci_addr).or_default().push(ParsedIrq {
            irq,
            count,
            mode,
            trigger,
            handler,
        });
    }

    map
}

/// Build InterruptInfo for a PCI device from the pre-parsed interrupt map.
fn build_interrupt_info(
    address: &str,
    irq_map: &HashMap<String, Vec<ParsedIrq>>,
) -> Option<InterruptInfo> {
    let parsed = irq_map.get(address)?;
    if parsed.is_empty() {
        return None;
    }

    let mode = parsed[0].mode.clone();
    let trigger = parsed[0].trigger.clone();
    let total_count: u64 = parsed.iter().map(|p| p.count).sum();

    let vectors = parsed
        .iter()
        .map(|p| IrqVector {
            irq: p.irq,
            count: p.count,
            handler: p.handler.clone(),
        })
        .collect();

    Some(InterruptInfo {
        mode,
        trigger,
        vectors,
        total_count,
    })
}

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
        current_gen: current_speed.as_deref().and_then(pcie_speed_to_gen),
        current_width,
        max_gen: max_speed.as_deref().and_then(pcie_speed_to_gen),
        max_width,
        current_speed,
        max_speed,
    })
}

/// Read AER error totals from sysfs aer_dev_* files.
///
/// Each file contains lines like "TOTAL_ERR_COR 0". We extract the TOTAL_ line.
/// Returns None if AER files don't exist (older kernels, non-PCIe devices).
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
pub(crate) fn parse_aer_total(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if line.starts_with("TOTAL_") {
            return line.split_whitespace().last()?.parse().ok();
        }
    }
    None
}

/// Parse a PCIe speed string like "16.0 GT/s PCIe" into a generation number.
///
/// Parses the leading float and maps GT/s to generation. Returns None for
/// unrecognized or missing values.
pub(crate) fn pcie_speed_to_gen(speed: &str) -> Option<u8> {
    let gts: f64 = speed.split_whitespace().next()?.parse().ok()?;
    match gts.round() as u32 {
        3 | 2 => Some(1), // 2.5 GT/s
        5 => Some(2),     // 5.0 GT/s
        8 => Some(3),     // 8.0 GT/s
        16 => Some(4),    // 16.0 GT/s
        32 => Some(5),    // 32.0 GT/s
        64 => Some(6),    // 64.0 GT/s
        _ => {
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

fn resolve_pci_names(vid: u16, did: u16) -> (Option<String>, Option<String>) {
    let vendor_name = pci_ids::Vendor::from_id(vid).map(|v| v.name().to_string());
    let device_name = pci_ids::Device::from_vid_pid(vid, did).map(|d| d.name().to_string());
    (vendor_name, device_name)
}

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
        assert_eq!(pcie_speed_to_gen("2.5 GT/s PCIe"), Some(1));
        assert_eq!(pcie_speed_to_gen("5.0 GT/s PCIe"), Some(2));
        assert_eq!(pcie_speed_to_gen("8.0 GT/s PCIe"), Some(3));
        assert_eq!(pcie_speed_to_gen("16.0 GT/s PCIe"), Some(4));
        assert_eq!(pcie_speed_to_gen("32.0 GT/s PCIe"), Some(5));
        assert_eq!(pcie_speed_to_gen("64.0 GT/s PCIe"), Some(6));
    }

    #[test]
    fn test_pcie_speed_to_gen_unknown() {
        assert_eq!(pcie_speed_to_gen("unknown"), None);
        assert_eq!(pcie_speed_to_gen("Unknown"), None);
    }

    #[test]
    fn test_parse_interrupts_msi() {
        let input = "\
                   CPU0       CPU1
  29:          0       1316  PCI-MSI-0000:f0:01.1    0-edge      PCIe PME aerdrv
  30:          2          0  PCI-MSI-0000:f0:07.1    0-edge      PCIe PME
";
        let map = parse_interrupts_str(input);
        assert_eq!(map.len(), 2);

        let dev = &map["0000:f0:01.1"];
        assert_eq!(dev.len(), 1);
        assert_eq!(dev[0].irq, 29);
        assert_eq!(dev[0].count, 1316);
        assert_eq!(dev[0].mode, "MSI");
        assert_eq!(dev[0].trigger, "edge");
        assert!(dev[0].handler.contains("PCIe PME"));
    }

    #[test]
    fn test_parse_interrupts_msix_multi_vector() {
        let input = "\
                   CPU0       CPU1
  90:       1000       2000  PCI-MSIX-0000:21:00.0    0-edge      nvme1q0
 212:        500        500  PCI-MSIX-0000:21:00.0    1-edge      nvme1q1
";
        let map = parse_interrupts_str(input);
        let dev = &map["0000:21:00.0"];
        assert_eq!(dev.len(), 2);
        assert_eq!(dev[0].count, 3000);
        assert_eq!(dev[1].count, 1000);
        assert_eq!(dev[0].handler, "nvme1q0");
        assert_eq!(dev[1].handler, "nvme1q1");

        let info = build_interrupt_info("0000:21:00.0", &map).unwrap();
        assert_eq!(info.mode, "MSI-X");
        assert_eq!(info.vectors.len(), 2);
        assert_eq!(info.total_count, 4000);
    }

    #[test]
    fn test_parse_interrupts_ir_prefix() {
        let input = "\
                   CPU0
  42:       5000  IR-PCI-MSI-0000:00:1f.0    0-edge      i801_smbus
  43:       3000  IR-PCI-MSIX-0000:02:00.0    0-edge      eth0
";
        let map = parse_interrupts_str(input);
        assert_eq!(map["0000:00:1f.0"][0].mode, "MSI");
        assert_eq!(map["0000:02:00.0"][0].mode, "MSI-X");
    }

    #[test]
    fn test_parse_interrupts_skips_non_pci() {
        let input = "\
                   CPU0
 NMI:          5   Non-maskable interrupts
 LOC:    1234567   Local timer interrupts
  42:       5000  PCI-MSI-0000:00:1f.0    0-edge      device
";
        let map = parse_interrupts_str(input);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("0000:00:1f.0"));
    }

    #[test]
    fn test_parse_interrupts_empty() {
        let map = parse_interrupts_str("");
        assert!(map.is_empty());
    }
}
