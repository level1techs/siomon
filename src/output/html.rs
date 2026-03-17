#![cfg(feature = "html")]

use crate::model::system::SystemInfo;

pub fn print(info: &SystemInfo) {
    println!("<!DOCTYPE html>");
    println!("<html lang=\"en\">");
    println!("<head>");
    println!("<meta charset=\"UTF-8\">");
    println!("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">");
    println!("<title>sio Report — {}</title>", info.hostname);
    println!("<style>");
    println!("{}", CSS);
    println!("</style>");
    println!("</head>");
    println!("<body>");
    println!("<div class=\"container\">");

    // Header
    println!("<h1>sio System Report</h1>");
    println!("<div class=\"meta\">");
    println!("<span>Host: <strong>{}</strong></span>", info.hostname);
    println!("<span>Kernel: {}</span>", info.kernel_version);
    if let Some(ref os) = info.os_name {
        println!("<span>OS: {os}</span>");
    }
    println!(
        "<span>Generated: {}</span>",
        info.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("<span>sio v{}</span>", info.version);
    println!("</div>");

    // CPU
    for cpu in &info.cpus {
        println!("<h2>CPU</h2>");
        println!("<table>");
        row("Name", &cpu.brand);
        row("Vendor", &format!("{:?}", cpu.vendor));
        row(
            "ID",
            &format!(
                "Family {:#x} Model {:#x} Stepping {}",
                cpu.family, cpu.model, cpu.stepping
            ),
        );
        if let Some(ref cn) = cpu.codename {
            row("Codename", cn);
        }
        if let Some(ref mc) = cpu.microcode {
            row("Microcode", mc);
        }
        row(
            "Topology",
            &format!(
                "{} packages, {} cores, {} threads (SMT: {})",
                cpu.topology.packages,
                cpu.topology.physical_cores,
                cpu.topology.logical_processors,
                if cpu.topology.smt_enabled {
                    "on"
                } else {
                    "off"
                }
            ),
        );
        if let (Some(base), Some(boost)) = (cpu.base_clock_mhz, cpu.boost_clock_mhz) {
            row("Frequency", &format!("{base:.0} — {boost:.0} MHz"));
        }
        if let Some(ref drv) = cpu.scaling_driver {
            row("Scaling Driver", drv);
        }
        if let Some(ref c) = cpu.cache.l1d {
            row("L1d Cache", &format_cache(c));
        }
        if let Some(ref c) = cpu.cache.l1i {
            row("L1i Cache", &format_cache(c));
        }
        if let Some(ref c) = cpu.cache.l2 {
            row("L2 Cache", &format_cache(c));
        }
        if let Some(ref c) = cpu.cache.l3 {
            row("L3 Cache", &format_cache(c));
        }
        println!("</table>");

        if !cpu.vulnerabilities.is_empty() {
            println!("<h3>Vulnerabilities</h3>");
            println!("<table>");
            for v in &cpu.vulnerabilities {
                let class = if v.status.contains("Not affected") {
                    "val-good"
                } else if v.status.contains("Mitigat") {
                    "val-warn"
                } else {
                    "val-bad"
                };
                println!(
                    "<tr><td>{}</td><td class=\"{class}\">{}</td></tr>",
                    v.name, v.status
                );
            }
            println!("</table>");
        }
    }

    // Memory
    println!("<h2>Memory</h2>");
    println!("<table>");
    row("Total", &format_bytes(info.memory.total_bytes));
    row("Available", &format_bytes(info.memory.available_bytes));
    if info.memory.swap_total_bytes > 0 {
        row(
            "Swap",
            &format!(
                "{} total, {} free",
                format_bytes(info.memory.swap_total_bytes),
                format_bytes(info.memory.swap_free_bytes)
            ),
        );
    }
    println!("</table>");

    if !info.memory.dimms.is_empty() {
        println!("<h3>DIMMs</h3>");
        println!(
            "<table><tr><th>Locator</th><th>Size</th><th>Type</th><th>Speed</th><th>Manufacturer</th><th>Part Number</th></tr>"
        );
        for d in &info.memory.dimms {
            println!(
                "<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{} MT/s</td><td>{}</td><td>{}</td></tr>",
                d.locator,
                format_bytes(d.size_bytes),
                d.memory_type,
                d.configured_speed_mts.unwrap_or(0),
                d.manufacturer.as_deref().unwrap_or("-"),
                d.part_number.as_deref().unwrap_or("-"),
            );
        }
        println!("</table>");
    }

    // Motherboard
    println!("<h2>Motherboard</h2>");
    println!("<table>");
    let mb = &info.motherboard;
    if let Some(ref m) = mb.manufacturer {
        row(
            "Board",
            &format!("{} {}", m, mb.product_name.as_deref().unwrap_or("")),
        );
    }
    if let Some(ref v) = mb.bios.vendor {
        row(
            "BIOS",
            &format!(
                "{} {} ({})",
                v,
                mb.bios.version.as_deref().unwrap_or(""),
                mb.bios.date.as_deref().unwrap_or("")
            ),
        );
    }
    row(
        "Boot Mode",
        &format!(
            "{}{}",
            if mb.bios.uefi_boot { "UEFI" } else { "Legacy" },
            match mb.bios.secure_boot {
                Some(true) => " + Secure Boot",
                Some(false) => " (Secure Boot off)",
                None => "",
            }
        ),
    );
    if let Some(ref cs) = mb.chipset {
        row("Chipset", cs);
    }
    println!("</table>");

    // GPUs
    if !info.gpus.is_empty() {
        println!("<h2>GPUs</h2>");
        for gpu in &info.gpus {
            println!("<h3>[{}] {}</h3>", gpu.index, gpu.name);
            println!("<table>");
            row("Vendor", &format!("{:?}", gpu.vendor));
            if let Some(ref drv) = gpu.driver_module {
                let ver = gpu.driver_version.as_deref().unwrap_or("");
                row("Driver", &format!("{drv} {ver}"));
            }
            if let Some(vram) = gpu.vram_total_bytes {
                row("VRAM", &format_bytes(vram));
            }
            if let Some(mhz) = gpu.max_core_clock_mhz {
                let mem = gpu
                    .max_memory_clock_mhz
                    .map(|m| format!(", Mem {m} MHz"))
                    .unwrap_or_default();
                row("Max Clocks", &format!("Core {mhz} MHz{mem}"));
            }
            if let Some(ref link) = gpu.pcie_link {
                let g = link
                    .current_gen
                    .map(|g| format!("Gen{g}"))
                    .unwrap_or_default();
                let w = link
                    .current_width
                    .map(|w| format!(" x{w}"))
                    .unwrap_or_default();
                row("PCIe Link", &format!("{g}{w}"));
            }
            if let Some(w) = gpu.power_limit_watts {
                row("Power Limit", &format!("{w:.0} W"));
            }
            println!("</table>");
        }
    }

    // Storage
    if !info.storage.is_empty() {
        println!("<h2>Storage</h2>");
        println!(
            "<table><tr><th>Device</th><th>Model</th><th>Interface</th><th>Capacity</th><th>Firmware</th></tr>"
        );
        for d in &info.storage {
            println!(
                "<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td></tr>",
                if cfg!(unix) {
                    format!("/dev/{}", d.device_name)
                } else {
                    d.device_name.clone()
                },
                d.model.as_deref().unwrap_or("-"),
                d.interface,
                format_bytes(d.capacity_bytes),
                d.firmware_version.as_deref().unwrap_or("-"),
            );
        }
        println!("</table>");
    }

    // Network
    if !info.network.is_empty() {
        println!("<h2>Network</h2>");
        println!(
            "<table><tr><th>Interface</th><th>Driver</th><th>Speed</th><th>MAC</th><th>State</th></tr>"
        );
        for n in &info.network {
            println!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                n.name,
                n.driver.as_deref().unwrap_or("-"),
                n.speed_mbps
                    .map(|s| format!("{s} Mbps"))
                    .unwrap_or_else(|| "-".into()),
                n.mac_address.as_deref().unwrap_or("-"),
                n.operstate,
            );
        }
        println!("</table>");
    }

    // PCI
    println!("<h2>PCI Devices ({})</h2>", info.pci_devices.len());
    println!("<table><tr><th>Address</th><th>Device</th><th>Class</th><th>Driver</th></tr>");
    for d in &info.pci_devices {
        let name = d
            .device_name
            .as_deref()
            .or(d.vendor_name.as_deref())
            .unwrap_or("Unknown");
        let class = d
            .subclass_name
            .as_deref()
            .unwrap_or(d.class_name.as_deref().unwrap_or("-"));
        println!(
            "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            d.address,
            name,
            class,
            d.driver.as_deref().unwrap_or("-"),
        );
    }
    println!("</table>");

    println!("</div>");
    println!("</body>");
    println!("</html>");
}

fn row(label: &str, value: &str) {
    println!("<tr><td class=\"label\">{label}</td><td>{value}</td></tr>");
}

fn format_bytes(bytes: u64) -> String {
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

fn format_cache(c: &crate::model::cpu::CacheLevel) -> String {
    format!(
        "{} ({}-way, {} line)",
        format_bytes(c.size_bytes),
        c.ways,
        format_bytes(c.line_size_bytes as u64)
    )
}

const CSS: &str = r#"
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, sans-serif; background: #1a1a2e; color: #e0e0e0; padding: 2rem; }
.container { max-width: 1000px; margin: 0 auto; }
h1 { color: #00d4ff; margin-bottom: 0.5rem; font-size: 1.8rem; }
h2 { color: #00d4ff; margin: 1.5rem 0 0.5rem; padding-bottom: 0.3rem; border-bottom: 1px solid #333; font-size: 1.3rem; }
h3 { color: #888; margin: 0.8rem 0 0.3rem; font-size: 1.1rem; }
.meta { display: flex; gap: 1.5rem; flex-wrap: wrap; margin-bottom: 1rem; color: #888; font-size: 0.9rem; }
table { width: 100%; border-collapse: collapse; margin-bottom: 0.5rem; }
td, th { padding: 0.3rem 0.8rem; text-align: left; border-bottom: 1px solid #222; font-size: 0.9rem; }
th { background: #16213e; color: #aaa; font-weight: 600; }
td.label { color: #888; width: 180px; font-weight: 500; }
td.mono { font-family: "Cascadia Code", "Fira Code", monospace; font-size: 0.85rem; }
.val-good { color: #4caf50; }
.val-warn { color: #ff9800; }
.val-bad { color: #f44336; font-weight: bold; }
tr:hover { background: #16213e; }
"#;
