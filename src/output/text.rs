use crate::model::system::SystemInfo;

pub fn print_summary(info: &SystemInfo) {
    println!("  sio - System Information");
    println!("  ========================");
    if unsafe { libc::geteuid() } != 0 {
        println!("  (run as root for SMART data, DMI serials, and MSR access)");
    }
    println!();

    // System
    println!("  Hostname:        {}", info.hostname);
    println!("  Kernel:          {}", info.kernel_version);
    if let Some(ref os) = info.os_name {
        println!("  OS:              {os}");
    }
    println!();

    // CPU
    for cpu in &info.cpus {
        println!("── CPU ─────────────────────────────────────────────────────────");
        println!("  Name:            {}", cpu.brand);
        println!(
            "  Vendor:          {:?}  Family: {:#x}  Model: {:#x}  Stepping: {}",
            cpu.vendor, cpu.family, cpu.model, cpu.stepping
        );
        if let Some(ref cn) = cpu.codename {
            println!("  Codename:        {cn}");
        }
        if let Some(ref mc) = cpu.microcode {
            println!("  Microcode:       {mc}");
        }
        println!(
            "  Topology:        {} packages, {} cores, {} threads",
            cpu.topology.packages, cpu.topology.physical_cores, cpu.topology.logical_processors
        );
        if cpu.topology.smt_enabled {
            println!(
                "  SMT:             Enabled ({} threads/core)",
                cpu.topology.threads_per_core
            );
        }
        if cpu.topology.dies_per_package > 1 {
            println!("  Dies/Package:    {}", cpu.topology.dies_per_package);
        }
        if let (Some(base), Some(boost)) = (cpu.base_clock_mhz, cpu.boost_clock_mhz) {
            println!("  Frequency:       {base:.0} — {boost:.0} MHz");
        }
        if let Some(ref drv) = cpu.scaling_driver {
            println!("  Scaling Driver:  {drv}");
        }
        print_cache("  L1d", &cpu.cache.l1d);
        print_cache("  L1i", &cpu.cache.l1i);
        print_cache("  L2", &cpu.cache.l2);
        print_cache("  L3", &cpu.cache.l3);

        let feat_strs = format_features(&cpu.features);
        if !feat_strs.is_empty() {
            println!("  Features:        {feat_strs}");
        }
        if !cpu.vulnerabilities.is_empty() {
            println!("  Vulnerabilities:");
            for vuln in &cpu.vulnerabilities {
                println!("    {}: {}", vuln.name, vuln.status);
            }
        }
        println!();
    }

    // Memory
    println!("── Memory ──────────────────────────────────────────────────────");
    println!(
        "  Total:           {}",
        format_bytes(info.memory.total_bytes)
    );
    println!(
        "  Available:       {}",
        format_bytes(info.memory.available_bytes)
    );
    if info.memory.swap_total_bytes > 0 {
        println!(
            "  Swap:            {} total, {} free",
            format_bytes(info.memory.swap_total_bytes),
            format_bytes(info.memory.swap_free_bytes)
        );
    }
    for dimm in &info.memory.dimms {
        println!(
            "  DIMM {}: {} {:?} @ {} MT/s{}",
            dimm.locator,
            format_bytes(dimm.size_bytes),
            dimm.memory_type,
            dimm.configured_speed_mts.unwrap_or(0),
            if dimm.ecc { " ECC" } else { "" }
        );
        if let Some(ref mfg) = dimm.manufacturer {
            print!("    Mfg: {mfg}");
        }
        if let Some(ref pn) = dimm.part_number {
            print!("  Part: {pn}");
        }
        if dimm.manufacturer.is_some() || dimm.part_number.is_some() {
            println!();
        }
    }
    println!();

    // Motherboard
    println!("── Motherboard ─────────────────────────────────────────────────");
    let mb = &info.motherboard;
    if let Some(ref mfg) = mb.manufacturer {
        print!("  Board:           {mfg}");
        if let Some(ref name) = mb.product_name {
            print!(" {name}");
        }
        println!();
    }
    if let Some(ref v) = mb.bios.vendor {
        print!("  BIOS:            {v}");
        if let Some(ref ver) = mb.bios.version {
            print!(" {ver}");
        }
        if let Some(ref date) = mb.bios.date {
            print!(" ({date})");
        }
        println!();
    }
    println!(
        "  Boot Mode:       {}{}",
        if mb.bios.uefi_boot { "UEFI" } else { "Legacy" },
        match mb.bios.secure_boot {
            Some(true) => " + Secure Boot",
            Some(false) => " (Secure Boot off)",
            None => "",
        }
    );
    if let Some(ref cs) = mb.chipset {
        println!("  Chipset:         {cs}");
    }
    if let Some(ref me) = mb.me_version {
        println!("  ME Firmware:     {me}");
    }
    // Show detected Super I/O chip if direct I/O is available
    if unsafe { libc::geteuid() } == 0 {
        let chips = crate::sensors::superio::chip_detect::detect_all();
        for chip in &chips {
            let driver_status =
                if crate::sensors::superio::chip_detect::is_kernel_driver_loaded(&chip.chip) {
                    " (kernel driver loaded)"
                } else {
                    ""
                };
            println!(
                "  Super I/O:       {} (ID: {:#06x}) at base {:#06x}{}",
                chip.chip, chip.chip_id, chip.hwm_base, driver_status
            );
        }
    }
    println!();

    // GPU
    if !info.gpus.is_empty() {
        println!("── GPU ─────────────────────────────────────────────────────────");
        for gpu in &info.gpus {
            println!("  [{}] {}", gpu.index, gpu.name);
            println!("  Vendor:          {:?}", gpu.vendor);
            if let Some(ref drv) = gpu.driver_module {
                print!("  Driver:          {drv}");
                if let Some(ref ver) = gpu.driver_version {
                    print!(" ({ver})");
                }
                println!();
            }
            if let Some(vram) = gpu.vram_total_bytes {
                println!("  VRAM:            {}", format_bytes(vram));
            }
            if let Some(mhz) = gpu.max_core_clock_mhz {
                print!("  Max Clocks:      Core {mhz} MHz");
                if let Some(mem) = gpu.max_memory_clock_mhz {
                    print!("  Mem {mem} MHz");
                }
                println!();
            }
            if let Some(ref link) = gpu.pcie_link {
                let pcie_gen = link
                    .current_gen
                    .map(|g| format!("Gen{g}"))
                    .unwrap_or_default();
                let width = link
                    .current_width
                    .map(|w| format!("x{w}"))
                    .unwrap_or_default();
                println!("  PCIe Link:       {pcie_gen} {width}");
            }
            if let Some(w) = gpu.power_limit_watts {
                println!("  Power Limit:     {w:.0} W");
            }
            if !gpu.display_outputs.is_empty() {
                for o in &gpu.display_outputs {
                    let mut desc = format!("{}-{}: {}", o.connector_type, o.index, o.status);
                    if let Some(ref name) = o.monitor_name {
                        desc.push_str(&format!(" ({name}"));
                        if let Some(ref res) = o.resolution {
                            desc.push_str(&format!(", {res}"));
                        }
                        desc.push(')');
                    }
                    println!("  Output:          {desc}");
                }
            }
            println!();
        }
    }

    // Storage
    if !info.storage.is_empty() {
        println!("── Storage ─────────────────────────────────────────────────────");
        for dev in &info.storage {
            let model = dev.model.as_deref().unwrap_or("Unknown");
            let serial = dev
                .serial_number
                .as_deref()
                .map(|s| format!(" [{s}]"))
                .unwrap_or_default();
            println!(
                "  /dev/{}: {} ({:?}) {}{}",
                dev.device_name,
                model,
                dev.interface,
                format_bytes(dev.capacity_bytes),
                serial,
            );
            if let Some(ref nvme) = dev.nvme {
                // Find matching PCI device for PCIe link info
                let pcie_info = info
                    .pci_devices
                    .iter()
                    .find(|p| {
                        p.driver.as_deref() == Some("nvme")
                            && p.device_name
                                .as_ref()
                                .map(|n| {
                                    dev.model.as_ref().is_some_and(|m| {
                                        n.contains(m.split_whitespace().next().unwrap_or(""))
                                    })
                                })
                                .unwrap_or(false)
                    })
                    .or_else(|| {
                        // Match by looking for nvme controller path
                        info.pci_devices.iter().find(|p| {
                            p.driver.as_deref() == Some("nvme")
                                && dev.sysfs_path.contains(&p.address)
                        })
                    })
                    .and_then(|p| p.pcie_link.as_ref());

                let pcie_str = pcie_info
                    .map(|l| {
                        let g = l.current_gen.map(|g| format!("Gen{g}")).unwrap_or_default();
                        let w = l.current_width.map(|w| format!("x{w}")).unwrap_or_default();
                        format!("  PCIe: {g} {w}")
                    })
                    .unwrap_or_default();

                print!(
                    "    {} {}{}",
                    nvme.transport,
                    nvme.controller_type.as_deref().unwrap_or("io"),
                    pcie_str
                );
                println!();
            }
            if let Some(ref smart) = dev.smart {
                print!("    SMART: {}°C", smart.temperature_celsius);
                if smart.percentage_used > 0 {
                    print!(", {}% used", smart.percentage_used);
                }
                print!(", {} hours", smart.power_on_hours);
                if smart.total_bytes_written > 0 {
                    print!(", {} written", format_bytes_u128(smart.total_bytes_written));
                }
                println!();
            }
        }
        println!();
    }

    // Network
    if !info.network.is_empty() {
        println!("── Network ─────────────────────────────────────────────────────");
        for nic in &info.network {
            let speed = nic
                .speed_mbps
                .map(|s| {
                    if s >= 1000 {
                        format!("{} Gbps", s / 1000)
                    } else {
                        format!("{s} Mbps")
                    }
                })
                .unwrap_or_else(|| "N/A".into());
            let driver = nic.driver.as_deref().unwrap_or("unknown");

            // Try to find PCI device name for this NIC
            let pci_name = nic.pci_bus_address.as_ref().and_then(|addr| {
                info.pci_devices
                    .iter()
                    .find(|p| p.address == *addr)
                    .and_then(|p| p.device_name.as_deref())
            });

            if let Some(pci_name) = pci_name {
                println!("  {}: {} ({}) [{}]", nic.name, speed, driver, nic.operstate);
                println!("    {pci_name}");
            } else {
                println!("  {}: {} ({}) [{}]", nic.name, speed, driver, nic.operstate);
            }
            if let Some(ref mac) = nic.mac_address {
                println!("    MAC: {mac}");
            }
            for ip in &nic.ip_addresses {
                println!("    {}: {}/{}", ip.family, ip.address, ip.prefix_len);
            }
        }
        println!();
    }

    // Audio
    if !info.audio.is_empty() {
        println!("── Audio ───────────────────────────────────────────────────────");
        for dev in &info.audio {
            print!("  [{}] {}", dev.card_index, dev.card_long_name);
            if let Some(ref codec) = dev.codec {
                print!(" ({codec})");
            }
            println!();
        }
        println!();
    }

    // USB summary
    if !info.usb_devices.is_empty() {
        // Only show non-hub devices
        let real_devices: Vec<_> = info
            .usb_devices
            .iter()
            .filter(|d| d.device_class != 9) // Filter out hubs (class 9)
            .collect();
        if !real_devices.is_empty() {
            println!(
                "── USB Devices ({}) ─────────────────────────────────────────────",
                real_devices.len()
            );
            for dev in &real_devices {
                let name = dev
                    .product
                    .as_deref()
                    .or(dev.manufacturer.as_deref())
                    .unwrap_or("Unknown");
                println!(
                    "  {:04x}:{:04x} {} [{:?}]",
                    dev.vendor_id, dev.product_id, name, dev.speed
                );
            }
            println!();
        }
    }

    // Battery
    if !info.batteries.is_empty() {
        println!("── Battery ─────────────────────────────────────────────────────");
        for bat in &info.batteries {
            let pct = bat
                .capacity_percent
                .map(|p| format!("{p}%"))
                .unwrap_or_else(|| "N/A".into());
            println!("  {}: {:?} ({})", bat.name, bat.status, pct);
            if let Some(wear) = bat.wear_percent {
                println!("    Wear: {:.1}%", wear * 100.0);
            }
        }
        println!();
    }

    // PCI summary — only show interesting devices, not bridges/host bridges
    let interesting_pci: Vec<_> = info
        .pci_devices
        .iter()
        .filter(|d| {
            let class = (d.class_code >> 16) & 0xFF;
            // Skip host bridges (0x06), PCI bridges (0x06 subclass 0x04)
            class != 0x06 && class != 0x08
        })
        .collect();
    if !interesting_pci.is_empty() {
        println!(
            "── PCI Devices ({} of {}) ──────────────────────────────────────",
            interesting_pci.len(),
            info.pci_devices.len()
        );
        for dev in &interesting_pci {
            let name = dev
                .device_name
                .as_deref()
                .or(dev.vendor_name.as_deref())
                .unwrap_or("Unknown");
            let class = dev
                .subclass_name
                .as_deref()
                .unwrap_or(dev.class_name.as_deref().unwrap_or("Unknown"));
            let driver = dev.driver.as_deref().unwrap_or("-");
            let pcie = dev
                .pcie_link
                .as_ref()
                .map(|l| {
                    let g = l.current_gen.map(|g| format!("Gen{g}")).unwrap_or_default();
                    let w = l.current_width.map(|w| format!("x{w}")).unwrap_or_default();
                    format!(" {g}{w}")
                })
                .unwrap_or_default();
            println!(
                "  {} {} [{}] ({}{})",
                dev.address, name, class, driver, pcie
            );
        }
    }
}

pub fn print_section_cpu(info: &SystemInfo) {
    for cpu in &info.cpus {
        println!("{:#?}", cpu);
    }
}

pub fn print_section_gpu(info: &SystemInfo) {
    for gpu in &info.gpus {
        println!("{:#?}", gpu);
    }
}

pub fn print_section_memory(info: &SystemInfo) {
    println!("{:#?}", info.memory);
}

pub fn print_section_storage(info: &SystemInfo) {
    for dev in &info.storage {
        println!("{:#?}", dev);
    }
}

pub fn print_section_network(info: &SystemInfo) {
    for nic in &info.network {
        println!("{:#?}", nic);
    }
}

pub fn print_section_pci(info: &SystemInfo) {
    for dev in &info.pci_devices {
        let name = dev.device_name.as_deref().unwrap_or("Unknown");
        let vendor = dev.vendor_name.as_deref().unwrap_or("Unknown");
        let class = dev.subclass_name.as_deref().unwrap_or("");
        let driver = dev.driver.as_deref().unwrap_or("-");
        println!(
            "{} {:04x}:{:04x} {} {} [{}] ({})",
            dev.address, dev.vendor_id, dev.device_id, vendor, name, class, driver
        );
    }
}

pub fn print_section_board(info: &SystemInfo) {
    println!("{:#?}", info.motherboard);
}

pub fn print_section_pcie(info: &SystemInfo) {
    let pcie_devices: Vec<_> = info
        .pci_devices
        .iter()
        .filter(|d| d.pcie_link.is_some())
        .collect();

    if pcie_devices.is_empty() {
        println!("No PCIe devices detected.");
        return;
    }

    println!(
        "── PCIe Link Status ({} devices) ───────────────────────────────",
        pcie_devices.len()
    );
    println!(
        "  {:<14} {:<40} {:>4} {:>4} {:>4} {:>4} {:>4} {:>6} {:>10} {:>6}",
        "Address", "Device", "CGen", "CWid", "MGen", "MWid", "NUMA", "AER", "IRQ", "Count"
    );

    for dev in &pcie_devices {
        let link = dev.pcie_link.as_ref().unwrap();
        let name = dev
            .device_name
            .as_deref()
            .or(dev.vendor_name.as_deref())
            .unwrap_or("Unknown");
        let name_trunc: String = name.chars().take(38).collect();

        let cur_gen = link
            .current_gen
            .map(|g| format!("{g}"))
            .unwrap_or_else(|| "-".into());
        let cur_wid = link
            .current_width
            .map(|w| format!("x{w}"))
            .unwrap_or_else(|| "-".into());
        let max_gen = link
            .max_gen
            .map(|g| format!("{g}"))
            .unwrap_or_else(|| "-".into());
        let max_wid = link
            .max_width
            .map(|w| format!("x{w}"))
            .unwrap_or_else(|| "-".into());

        let numa = match dev.numa_node {
            Some(n) if n >= 0 => format!("{n}"),
            // -1 means firmware didn't set proximity; show 0 if single-node system
            _ => "-".into(),
        };

        let aer = match &dev.aer {
            Some(a) => {
                let total = a.correctable + a.nonfatal + a.fatal;
                if total == 0 {
                    "ok".into()
                } else {
                    format!("{total}")
                }
            }
            None => "-".into(),
        };

        let driver = dev.driver.as_deref().unwrap_or("-");

        let (irq_mode, irq_count) = match &dev.interrupts {
            Some(info) => {
                let nvec = info.vectors.len();
                let mode = if nvec > 1 {
                    format!("{} x{nvec}", info.mode)
                } else {
                    info.mode.clone()
                };
                (mode, format_count(info.total_count))
            }
            None => ("-".into(), "-".into()),
        };

        println!(
            "  {:<14} {:<40} {:>4} {:>4} {:>4} {:>4} {:>4} {:>6} {:>10} {:>6}  ({})",
            dev.address,
            name_trunc,
            cur_gen,
            cur_wid,
            max_gen,
            max_wid,
            numa,
            aer,
            irq_mode,
            irq_count,
            driver
        );
    }
}

/// Format a large count with SI suffix (K, M, G).
fn format_count(n: u64) -> String {
    if n >= 999_950_000 {
        format!("{:.0}G", n as f64 / 1_000_000_000.0)
    } else if n >= 999_950 {
        format!("{:.0}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        format!("{n}")
    }
}

pub fn print_section_audio(info: &SystemInfo) {
    if info.audio.is_empty() {
        println!("No audio devices detected.");
        return;
    }
    println!("── Audio Devices ───────────────────────────────────────────────");
    for dev in &info.audio {
        println!("  [{}] {}", dev.card_index, dev.card_long_name);
        println!("    Driver: {}  Bus: {:?}", dev.driver, dev.bus_type);
        if let Some(ref codec) = dev.codec {
            println!("    Codec: {codec}");
        }
        if let Some(ref pci) = dev.pci_bus_address {
            println!("    PCI: {pci}");
        }
    }
}

pub fn print_section_usb(info: &SystemInfo) {
    if info.usb_devices.is_empty() {
        println!("No USB devices detected.");
        return;
    }
    println!(
        "── USB Devices ({}) ────────────────────────────────────────────",
        info.usb_devices.len()
    );
    for dev in &info.usb_devices {
        let name = dev
            .product
            .as_deref()
            .or(dev.manufacturer.as_deref())
            .unwrap_or("Unknown");
        println!(
            "  Bus {} Port {} {:04x}:{:04x} {} [{:?}]",
            dev.bus, dev.port_path, dev.vendor_id, dev.product_id, name, dev.speed
        );
    }
}

pub fn print_section_battery(info: &SystemInfo) {
    if info.batteries.is_empty() {
        println!("No batteries detected.");
        return;
    }
    println!("── Batteries ───────────────────────────────────────────────────");
    for bat in &info.batteries {
        println!("  {}: {:?}", bat.name, bat.status);
        if let Some(pct) = bat.capacity_percent {
            println!("    Charge: {pct}%");
        }
        if let Some(ref mfg) = bat.manufacturer {
            print!("    Manufacturer: {mfg}");
        }
        if let Some(ref model) = bat.model_name {
            print!("  Model: {model}");
        }
        if bat.manufacturer.is_some() || bat.model_name.is_some() {
            println!();
        }
        if let Some(cycles) = bat.cycle_count {
            println!("    Cycles: {cycles}");
        }
        if let Some(wear) = bat.wear_percent {
            println!("    Wear: {:.1}%", wear * 100.0);
        }
    }
}

fn print_cache(label: &str, cache: &Option<crate::model::cpu::CacheLevel>) {
    if let Some(c) = cache {
        println!(
            "{}: {} ({}-way, {} line)",
            label,
            format_bytes(c.size_bytes),
            c.ways,
            format_bytes(c.line_size_bytes as u64)
        );
    }
}

fn format_features(f: &crate::model::cpu::CpuFeatures) -> String {
    let mut v = Vec::new();
    macro_rules! check {
        ($field:ident, $name:expr) => {
            if f.$field {
                v.push($name);
            }
        };
    }
    check!(sse, "SSE");
    check!(sse2, "SSE2");
    check!(sse3, "SSE3");
    check!(ssse3, "SSSE3");
    check!(sse4_1, "SSE4.1");
    check!(sse4_2, "SSE4.2");
    check!(sse4a, "SSE4a");
    check!(avx, "AVX");
    check!(avx2, "AVX2");
    check!(avx512f, "AVX-512F");
    check!(avx512dq, "AVX-512DQ");
    check!(avx512bw, "AVX-512BW");
    check!(avx512vl, "AVX-512VL");
    check!(avx512cd, "AVX-512CD");
    check!(avx512vnni, "AVX-512VNNI");
    check!(avx512bf16, "AVX-512BF16");
    check!(avx_vnni, "AVX-VNNI");
    check!(amx_tile, "AMX-TILE");
    check!(amx_int8, "AMX-INT8");
    check!(amx_bf16, "AMX-BF16");
    check!(fma, "FMA3");
    check!(aes_ni, "AES-NI");
    check!(sha, "SHA");
    check!(bmi1, "BMI1");
    check!(bmi2, "BMI2");
    check!(popcnt, "POPCNT");
    check!(rdrand, "RDRAND");
    check!(rdseed, "RDSEED");
    check!(vmx, "VT-x");
    check!(svm, "AMD-V");
    check!(hypervisor, "HYPERVISOR");
    v.join(" ")
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_bytes_u128(bytes: u128) -> String {
    const TIB: u128 = 1024 * 1024 * 1024 * 1024;
    const GIB: u128 = 1024 * 1024 * 1024;
    const MIB: u128 = 1024 * 1024;

    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kib() {
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(10 * 1024), "10.0 KiB");
    }

    #[test]
    fn test_format_bytes_mib() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(256 * 1024 * 1024), "256.0 MiB");
    }

    #[test]
    fn test_format_bytes_gib() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GiB");
        assert_eq!(format_bytes(16 * 1024 * 1024 * 1024), "16.0 GiB");
    }

    #[test]
    fn test_format_bytes_tib() {
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 1024), "1.0 TiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024 * 1024), "2.0 TiB");
    }
}
