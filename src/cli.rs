use clap::{Parser, Subcommand, ValueEnum};

use crate::config::SiomonConfig;

#[derive(Parser, Debug)]
#[command(
    name = "sio",
    about = "Linux hardware information and sensor monitoring",
    version,
    author
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Output format
    #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text, global = true)]
    pub format: OutputFormat,

    /// Run in TUI (interactive) sensor monitor mode (press '/' to cross-filter)
    #[arg(short = 'm', long = "monitor", global = true)]
    pub tui: bool,

    /// Sensor polling interval in milliseconds
    #[arg(long, default_value_t = 1000, global = true)]
    pub interval: u64,

    /// Disable NVIDIA GPU detection
    #[arg(long, global = true)]
    pub no_nvidia: bool,

    /// Enable direct I/O port and I2C sensor reading (requires root)
    #[arg(long, global = true)]
    pub direct_io: bool,

    /// Show empty/unavailable fields
    #[arg(long, global = true)]
    pub show_empty: bool,

    /// Log sensor data to CSV file while monitoring
    #[arg(long, global = true)]
    pub log: Option<std::path::PathBuf>,

    /// Sensor alert rules (e.g., "hwmon/nct6798/temp1 > 80 @30s")
    #[arg(long = "alert", value_name = "RULE", global = true)]
    pub alerts: Vec<String>,

    /// Color mode
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true)]
    pub color: ColorMode,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// CPU information
    Cpu,
    /// GPU information
    Gpu,
    /// Memory information
    Memory,
    /// Storage device information
    Storage,
    /// Network adapter information
    Network,
    /// PCI device list
    Pci,
    /// USB device list
    Usb,
    /// Audio device information
    Audio,
    /// Battery information
    Battery,
    /// Motherboard and BIOS information
    Board,
    /// PCIe link details (speed, width, ASPM)
    Pcie,
    /// Sensor readings (one-shot snapshot)
    Sensors,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Xml,
    Html,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl Cli {
    /// Apply config file values for any CLI argument that wasn't explicitly set.
    pub fn apply_config(&mut self, config: &SiomonConfig, matches: &clap::ArgMatches) {

        if !self.is_explicitly_set("format", matches) {
            match config.general.format.as_str() {
                "json" => self.format = OutputFormat::Json,
                "xml" => self.format = OutputFormat::Xml,
                "html" => self.format = OutputFormat::Html,
                "text" => self.format = OutputFormat::Text,
                other => log::warn!("Unknown format in config: {other:?}"),
            }
        }

        if !self.is_explicitly_set("color", matches) {
            match config.general.color.as_str() {
                "auto" => self.color = ColorMode::Auto,
                "always" => self.color = ColorMode::Always,
                "never" => self.color = ColorMode::Never,
                other => log::warn!("Unknown color mode in config: {other:?}"),
            }
        }

        if !self.is_explicitly_set("interval", matches) {
            self.interval = config.general.poll_interval_ms;
        }

        if !self.is_explicitly_set("no_nvidia", matches) {
            self.no_nvidia = config.general.no_nvidia;
        }
    }

    /// Check if an argument was explicitly set on the command line (not just a default).
    /// Recursive to handle global arguments placed after subcommands.
    fn is_explicitly_set(&self, id: &str, matches: &clap::ArgMatches) -> bool {
        use clap::parser::ValueSource;

        if matches.value_source(id).is_some_and(|s| s != ValueSource::DefaultValue) {
            return true;
        }

        if let Some((_, sub_m)) = matches.subcommand() {
            return self.is_explicitly_set(id, sub_m);
        }

        false
    }
}
