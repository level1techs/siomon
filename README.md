# siomon

A comprehensive Linux hardware information and real-time sensor monitoring tool. Single static binary, no runtime dependencies.

## Features

### Hardware Information (one-shot)
- **CPU** -- brand, microarchitecture codename, topology (packages/dies/cores/threads), cache hierarchy, feature flags (SSE through AVX-512, AMX), frequency, vulnerability details with mitigation status. Supports x86_64 (via CPUID) and aarch64 (via MIDR_EL1/procfs), including heterogeneous big.LITTLE configurations.
- **Memory** -- total/available/swap, per-DIMM details (manufacturer, part number, speed, ECC) via custom SMBIOS parser (no dmidecode dependency)
- **Motherboard** -- board vendor/model, BIOS version/date, UEFI/Secure Boot status, chipset identification, Intel ME firmware version
- **GPU** -- NVIDIA (via NVML), AMD (via amdgpu sysfs), Intel (via i915/xe sysfs); VRAM, clocks, power limit, PCIe link, display outputs, EDID monitor info
- **Storage** -- NVMe and SATA devices with model, serial, firmware, capacity; NVMe SMART health data (temperature, wear, hours, errors) via direct ioctl
- **Network** -- physical adapters with driver, MAC, link speed, IP addresses, NUMA node
- **Audio** -- HDA/USB audio devices with codec identification
- **USB** -- device tree with VID:PID, manufacturer, product, speed
- **Battery** -- charge status, wear level, cycle count, chemistry (laptops)
- **PCI** -- full bus enumeration with human-readable names from the PCI ID database (25,000+ devices)
- **PCIe** -- dedicated link analysis: negotiated vs max generation and width per device

### Real-time Sensor Monitoring (TUI)
- **hwmon** -- all kernel-exported sensors: temperatures, fan speeds, voltages, power, current
- **CPU** -- per-core frequency and utilization
- **GPU** -- temperature, fan speed, power draw, core/memory clocks, utilization, VRAM usage (NVIDIA via NVML, AMD via sysfs); Tegra integrated GPU frequency and load via devfreq
- **RAPL** -- CPU package power consumption
- **Tegra** -- hardware engine clocks and state (APE, DLA, VIC, NVENC, etc.) on NVIDIA Jetson platforms
- **Disk** -- per-device read/write throughput
- **Network** -- per-interface RX/TX throughput
- **Tracking** -- min/max/average for every sensor across the monitoring session
- **Collapsible groups** -- groups with 32+ sensors auto-collapse; toggle with Enter/Space; collapsed groups show summary min/max/avg
- **Alerts** -- configurable threshold alerts (`--alert "hwmon/nct6798/temp1 > 80 @30s"`)
- **CSV logging** -- record sensor data to file while monitoring (`--log sensors.csv`)
- **Board-specific labels** -- built-in label overrides for popular boards; user overrides via config file

### Output Formats
- Pretty-printed text summary (default)
- JSON (`-f json`)
- XML (`-f xml`)
- HTML report (`-f html`) -- self-contained dark-themed report with color-coded vulnerability status
- Per-section views (`sio cpu`, `sio gpu`, `sio storage`, `sio pcie`, etc.)
- Sensor snapshot (`sio sensors`)

### Configuration
- Config file at `~/.config/siomon/config.toml` for persistent preferences
- Sensor label overrides (built-in board mappings + user custom labels)

## Quick Start

```bash
# System summary
sio

# Specific sections
sio cpu
sio gpu
sio memory
sio storage
sio network
sio pci
sio pcie           # PCIe link details
sio audio
sio usb
sio battery
sio board

# JSON output (pipe to jq, store, etc.)
sio -f json
sio cpu -f json

# HTML report
sio -f html > report.html

# XML output
sio -f xml > report.xml

# One-shot sensor snapshot
sio sensors
sio sensors -f json

# Interactive TUI sensor monitor
sio -m

# TUI with custom polling interval (ms)
sio -m --interval 500

# TUI with CSV logging
sio -m --log sensors.csv

# Sensor alerts
sio -m --alert "hwmon/nct6798/temp1 > 80" --alert "hwmon/nct6798/fan1 < 100 @60s"

# Full access (SMART, DMI serials, MSR)
sudo sio
```

## TUI Keybindings

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit (or clear active filter if one is set) |
| `/` | Enter search/filter mode |
| `Up` / `Down` / `j` / `k` | Navigate between groups |
| `Enter` / `Space` | Toggle collapse/expand group |
| `c` | Collapse all groups |
| `e` | Expand all groups |
| `PageUp` / `PageDown` | Scroll 20 rows |
| `Home` / `End` | Jump to top/bottom |
| `Mouse scroll` | Scroll 3 lines |

**In filter mode** (after pressing `/`):

| Key | Action |
|-----|--------|
| _any character_ / `Space` | Append to search query |
| `Backspace` | Delete last character |
| `Enter` | Confirm filter and return to normal navigation |
| `Esc` | Clear filter and exit filter mode |

## Building

### Prerequisites

- Rust 1.85+ (edition 2024)
- Linux (kernel 4.x+ for full sysfs support; 5.x+ recommended)
- Standard build tools (`gcc` or `cc` for libc linking)

### Build

```bash
cargo build --release
```

The binary is at `./target/release/sio` (~5.3 MB with all features, statically linked PCI ID database).

### Cross-compilation

```bash
# For a different Linux target
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

## Runtime Dependencies

sio has **zero mandatory runtime dependencies**. Everything is read from kernel interfaces.

### Optional Runtime

| Component | What it enables | Package |
|-----------|----------------|---------|
| NVIDIA driver | GPU name, VRAM, clocks, temp, power, utilization | `libnvidia-compute` (provides `libnvidia-ml.so.1`) |
| `dmidecode` | Per-DIMM memory details (manufacturer, part number, timings) | `dmidecode` |
| `msr` kernel module | CPU TDP, turbo ratios, C-states, perf limiters | `modprobe msr` |
| `i2c-dev` kernel module | SPD/XMP memory timing data | `modprobe i2c-dev` |
| `drivetemp` kernel module | SATA drive temperatures via hwmon | `modprobe drivetemp` |

### Privilege Model

sio runs without root and gracefully degrades:

| Access Level | Available |
|-------------|-----------|
| **Non-root** | CPU info, hwmon sensors, GPU (NVML + sysfs), PCI/USB, network, disk basic info, DMI non-restricted fields |
| **Root / sudo** | + Full DMI (serials, UUID), SMART data, NVMe health, MSR access, RAPL power, SPD timings |

Fields requiring elevation show `[requires root]` or are omitted.

## Install

```bash
cargo install siomon
```

## License

MIT
