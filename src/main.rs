mod cli;
mod collectors;
mod config;
mod db;
mod error;
mod model;
mod output;
mod parsers;
mod platform;
mod sensors;

use chrono::Utc;
use clap::Parser;

use cli::{Cli, Commands, OutputFormat};
use model::system::SystemInfo;

fn main() {
    env_logger::init();

    let cli = Cli::parse();
    let config = config::SinfoConfig::load();

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

        // Start CSV logger thread if --log was specified
        let _csv_handle = start_csv_logger(cli, &state);

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

fn collect_all(cli: &Cli) -> SystemInfo {
    let cpus = collectors::cpu::collect().unwrap_or_else(|e| {
        log::warn!("CPU collection failed: {e}");
        Vec::new()
    });

    let memory = collectors::memory::collect();
    let motherboard = collectors::motherboard::collect();
    let gpus = collectors::gpu::collect(cli.no_nvidia);
    let storage = collectors::storage::collect();
    let network = collectors::network::collect(true);
    let pci_devices = collectors::pci::collect();
    let audio = collectors::audio::collect();
    let usb_devices = collectors::usb::collect();
    let batteries = collectors::battery::collect();

    let hostname =
        platform::sysfs::read_string_optional(std::path::Path::new("/proc/sys/kernel/hostname"))
            .unwrap_or_else(|| "unknown".into());

    let kernel_version =
        platform::sysfs::read_string_optional(std::path::Path::new("/proc/sys/kernel/osrelease"))
            .unwrap_or_else(|| "unknown".into());

    let os_name = read_os_name();

    SystemInfo {
        timestamp: Utc::now(),
        sinfo_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname,
        kernel_version,
        os_name,
        cpus,
        memory,
        motherboard,
        gpus,
        storage,
        network,
        audio,
        usb_devices,
        pci_devices,
        batteries,
        sensors: None,
    }
}

fn read_os_name() -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

/// Start a background thread that periodically writes sensor data to a CSV file.
///
/// Returns `None` if `--log` was not specified or the CSV feature is disabled.
/// The returned handle keeps the thread alive; dropping it signals the thread to stop.
#[cfg(feature = "csv")]
fn start_csv_logger(
    cli: &Cli,
    state: &sensors::poller::SensorState,
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
    let interval = std::time::Duration::from_millis(cli.interval);
    let handle = std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);
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
) -> Option<std::thread::JoinHandle<()>> {
    if _cli.log.is_some() {
        eprintln!("CSV logging not available — compile with the 'csv' feature");
    }
    None
}
