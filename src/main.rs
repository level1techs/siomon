use chrono::Utc;
use clap::{CommandFactory, FromArgMatches};

use siomon::cli::{Cli, Commands, OutputFormat};
use siomon::model::system::SystemInfo;
#[cfg(unix)]
use siomon::platform;
use siomon::{collectors, config, db, output, sensors};

fn main() {
    env_logger::init();

    let matches = Cli::command().get_matches();
    let mut cli = Cli::from_arg_matches(&matches).expect("CLI parse error");
    let config = config::SiomonConfig::load();
    cli.apply_config(&config, &matches);

    // Build sensor label overrides from board name + config file
    let board_name = db::sensor_labels::read_board_name();
    let label_overrides =
        db::sensor_labels::load_labels(board_name.as_deref(), &config.sensor_labels);

    // TUI monitor mode
    if cli.tui {
        run_monitor(&cli, label_overrides);
        return;
    }

    // Sensor snapshot or one-shot commands
    if let Some(Commands::Sensors) = &cli.command {
        run_sensor_snapshot(&cli, &label_overrides);
        return;
    }

    // Standard hardware info collection
    let info = collect_all(&cli);

    let print_formatted = |info: &SystemInfo| match cli.format {
        #[cfg(feature = "json")]
        OutputFormat::Json => output::json::print(info),
        #[cfg(not(feature = "json"))]
        OutputFormat::Json => eprintln!("JSON output not available — compile with 'json' feature"),
        #[cfg(feature = "xml")]
        OutputFormat::Xml => output::xml::print(info),
        #[cfg(not(feature = "xml"))]
        OutputFormat::Xml => eprintln!("XML output not available — compile with 'xml' feature"),
        #[cfg(feature = "html")]
        OutputFormat::Html => output::html::print(info),
        #[cfg(not(feature = "html"))]
        OutputFormat::Html => eprintln!("HTML output not available — compile with 'html' feature"),
        OutputFormat::Text => output::text::print_summary(info),
    };

    match &cli.command {
        None => print_formatted(&info),
        Some(cmd) => {
            if cli.format != OutputFormat::Text {
                print_formatted(&info);
            } else {
                match cmd {
                    Commands::Cpu => output::text::print_section_cpu(&info),
                    Commands::Gpu => output::text::print_section_gpu(&info),
                    Commands::Memory => output::text::print_section_memory(&info),
                    Commands::Storage => output::text::print_section_storage(&info),
                    Commands::Network => output::text::print_section_network(&info),
                    Commands::Pci => output::text::print_section_pci(&info),
                    Commands::Board => output::text::print_section_board(&info),
                    Commands::Audio => output::text::print_section_audio(&info),
                    Commands::Usb => output::text::print_section_usb(&info),
                    Commands::Battery => output::text::print_section_battery(&info),
                    Commands::Pcie => output::text::print_section_pcie(&info),
                    Commands::Sensors => unreachable!(),
                }
            }
        }
    }
}

fn run_monitor(cli: &Cli, label_overrides: std::collections::HashMap<String, String>) {
    #[cfg(feature = "tui")]
    {
        let state = sensors::poller::new_state();
        let poll_stats = sensors::poller::new_poll_stats();
        let poller = sensors::poller::Poller::new(
            state.clone(),
            poll_stats.clone(),
            cli.interval,
            cli.no_nvidia,
            cli.direct_io,
            label_overrides,
        );
        let _handle = poller.spawn();

        // Give poller a moment to collect initial data
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Shared stop flag for background threads
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Start CSV logger thread if --log was specified
        let csv_handle = start_csv_logger(cli, &state, &stop);

        // Parse alert rules from CLI
        let alert_rules: Vec<_> = cli
            .alerts
            .iter()
            .filter_map(|s| {
                let rule = sensors::alerts::parse_alert_rule(s);
                if rule.is_none() {
                    eprintln!("Invalid alert rule: {s}");
                }
                rule
            })
            .collect();

        if let Err(e) = output::tui::run(state, poll_stats, cli.interval, alert_rules) {
            eprintln!("TUI error: {e}");
        }

        // Signal background threads to stop and wait for CSV flush
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = csv_handle {
            let _ = h.join();
        }
    }

    #[cfg(not(feature = "tui"))]
    {
        let _ = (cli, label_overrides);
        eprintln!("TUI not available — compile with the 'tui' feature");
    }
}

fn run_sensor_snapshot(cli: &Cli, label_overrides: &std::collections::HashMap<String, String>) {
    let readings = sensors::poller::snapshot(cli.no_nvidia, cli.direct_io, label_overrides);
    let mut sorted: Vec<_> = readings.into_iter().collect();
    sorted.sort_by(|a, b| a.0.natural_cmp(&b.0));

    if cli.format == OutputFormat::Json {
        #[cfg(feature = "json")]
        {
            let map: std::collections::HashMap<String, _> = sorted
                .into_iter()
                .map(|(id, r)| (id.to_string(), r))
                .collect();
            match serde_json::to_string_pretty(&map) {
                Ok(json) => println!("{json}"),
                Err(e) => eprintln!("JSON error: {e}"),
            }
        }
        #[cfg(not(feature = "json"))]
        {
            let _ = sorted;
            eprintln!("JSON output not available — compile with 'json' feature");
        }
    } else {
        let mut last_chip = String::new();
        for (id, reading) in &sorted {
            let chip_key = format!("{}/{}", id.source, id.chip);
            if chip_key != last_chip {
                if !last_chip.is_empty() {
                    println!();
                }
                println!("── {} ──", chip_key);
                last_chip = chip_key;
            }
            println!(
                "  {:<35} {:>10.1} {}",
                reading.label, reading.current, reading.unit
            );
        }
    }
}

fn join_or_default<T: Default>(result: std::thread::Result<T>, name: &str) -> T {
    match result {
        Ok(v) => v,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown");
            log::error!("{name} collector panicked: {msg}");
            T::default()
        }
    }
}

fn collect_all(cli: &Cli) -> SystemInfo {
    let no_nvidia = cli.no_nvidia;

    #[cfg(unix)]
    let hostname =
        platform::sysfs::read_string_optional(std::path::Path::new("/proc/sys/kernel/hostname"))
            .unwrap_or_else(|| "unknown".into());
    #[cfg(not(unix))]
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into());

    #[cfg(unix)]
    let kernel_version =
        platform::sysfs::read_string_optional(std::path::Path::new("/proc/sys/kernel/osrelease"))
            .unwrap_or_else(|| "unknown".into());
    #[cfg(not(unix))]
    let kernel_version = read_windows_version();

    // Run all collectors in parallel — the slow ones (GPU/NVML, storage/SMART,
    // PCI enumeration) no longer block each other.
    let t = std::time::Instant::now();
    #[allow(unused_mut)]
    let (cpus, memory, mut motherboard, gpus, storage, network, pci, audio, usb, batteries) =
        std::thread::scope(|s| {
            let h_cpu = s.spawn(|| {
                collectors::cpu::collect().unwrap_or_else(|e| {
                    log::warn!("CPU collection failed: {e}");
                    Vec::new()
                })
            });
            let h_mem = s.spawn(collectors::memory::collect);
            let h_board = s.spawn(collectors::motherboard::collect);
            let h_gpu = s.spawn(move || collectors::gpu::collect(no_nvidia));
            let h_stor = s.spawn(collectors::storage::collect);
            let h_net = s.spawn(|| collectors::network::collect(true));
            let h_pci = s.spawn(collectors::pci::collect);
            let h_audio = s.spawn(collectors::audio::collect);
            let h_usb = s.spawn(collectors::usb::collect);
            let h_batt = s.spawn(collectors::battery::collect);

            (
                join_or_default(h_cpu.join(), "cpu"),
                join_or_default(h_mem.join(), "memory"),
                join_or_default(h_board.join(), "motherboard"),
                join_or_default(h_gpu.join(), "gpu"),
                join_or_default(h_stor.join(), "storage"),
                join_or_default(h_net.join(), "network"),
                join_or_default(h_pci.join(), "pci"),
                join_or_default(h_audio.join(), "audio"),
                join_or_default(h_usb.join(), "usb"),
                join_or_default(h_batt.join(), "battery"),
            )
        });
    log::info!(
        "Hardware collection completed in {}ms",
        t.elapsed().as_millis()
    );

    // On Windows, fill in the chipset from the PCI host bridge at 00:00.0.
    // Prefer a device whose class is Host Bridge (0x0600) or whose pci_ids
    // name contains "Root Complex" / "Host Bridge" keywords, since Windows
    // PCI enumeration may place multiple devices at the same address and the
    // class_code field may be zero (unavailable from WMI).
    #[cfg(not(unix))]
    {
        if motherboard.chipset.is_none() {
            let candidates: Vec<_> = pci.iter().filter(|d| d.address == "0000:00:00.0").collect();
            let host_bridge = candidates
                .iter()
                .find(|d| (d.class_code >> 8) == 0x0600)
                .or_else(|| {
                    candidates.iter().find(|d| {
                        d.device_name
                            .as_deref()
                            .map(|n| {
                                let lower = n.to_lowercase();
                                lower.contains("root complex") || lower.contains("host bridge")
                            })
                            .unwrap_or(false)
                    })
                })
                .or_else(|| candidates.first());
            if let Some(hb) = host_bridge {
                motherboard.chipset = hb.device_name.clone();
            }
        }
    }

    SystemInfo {
        timestamp: Utc::now(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        hostname,
        kernel_version,
        os_name: read_os_name(),
        cpus,
        memory,
        motherboard,
        gpus,
        storage,
        network,
        audio,
        usb_devices: usb,
        pci_devices: pci,
        batteries,
        sensors: None,
    }
}

fn read_os_name() -> Option<String> {
    #[cfg(unix)]
    {
        let content = std::fs::read_to_string("/etc/os-release").ok()?;
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                return Some(val.trim_matches('"').to_string());
            }
        }
        None
    }
    #[cfg(not(unix))]
    {
        // Read ProductName from the Windows registry (e.g. "Windows 11 Pro")
        let out = std::process::Command::new("reg")
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
                "/v",
                "ProductName",
            ])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok());
        if let Some(text) = out {
            for line in text.lines() {
                // Line looks like "    ProductName    REG_SZ    Windows 11 Pro"
                let parts: Vec<&str> = line.trim().splitn(3, "    ").collect();
                if parts.len() == 3 && parts[0] == "ProductName" {
                    let val = parts[2].trim().to_string();
                    if !val.is_empty() {
                        return Some(val);
                    }
                }
            }
        }
        Some("Windows".to_string())
    }
}

#[cfg(not(unix))]
fn read_windows_version() -> String {
    // Extract "10.0.26200.7840" from "Microsoft Windows [Version 10.0.26200.7840]"
    std::process::Command::new("cmd")
        .args(["/c", "ver"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            let s = s.trim();
            let inside = s.split('[').nth(1)?;
            let inside = inside.trim_end_matches(|c: char| c == ']' || c.is_whitespace());
            inside
                .strip_prefix("Version ")
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_else(|| "Windows".to_string())
}

/// Start a background thread that periodically writes sensor data to a CSV file.
///
/// Returns `None` if `--log` was not specified or the CSV feature is disabled.
/// The returned handle keeps the thread alive; dropping it signals the thread to stop.
#[cfg(feature = "csv")]
fn start_csv_logger(
    cli: &Cli,
    state: &sensors::poller::SensorState,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Option<std::thread::JoinHandle<()>> {
    let path = cli.log.as_ref()?;
    let mut logger = match output::csv::CsvLogger::new(path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to open CSV log file {}: {e}", path.display());
            return None;
        }
    };

    let state = state.clone();
    let stop = stop.clone();
    let interval = std::time::Duration::from_millis(cli.interval);
    let handle = std::thread::spawn(move || {
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(interval);
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if let Err(e) = logger.write_row(&state) {
                log::warn!("CSV write error: {e}");
                break;
            }
        }
    });
    Some(handle)
}

#[cfg(not(feature = "csv"))]
fn start_csv_logger(
    _cli: &Cli,
    _state: &sensors::poller::SensorState,
    _stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Option<std::thread::JoinHandle<()>> {
    if _cli.log.is_some() {
        eprintln!("CSV logging not available — compile with the 'csv' feature");
    }
    None
}
