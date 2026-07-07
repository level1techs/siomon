use clap::{Parser, Subcommand, ValueEnum};

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
