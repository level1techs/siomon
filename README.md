# siomon

A comprehensive Linux hardware information and real-time sensor monitoring tool. Single static binary, no runtime dependencies.

## Features

### Hardware Information (one-shot)
- **CPU** -- brand, microarchitecture codename, topology (packages/dies/cores/threads), cache hierarchy, feature flags (SSE through AVX-512, AMX), frequency, vulnerability details with mitigation status. Supports x86_64 (via CPUID) and aarch64 (via MIDR_EL1/procfs).
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
- **GPU** -- temperature, fan speed, power draw, core/memory clocks, utilization, VRAM usage (NVIDIA via NVML, AMD via sysfs)
- **RAPL** -- CPU package power consumption
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

### Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `tui` | on | Interactive terminal UI (ratatui + crossterm) |
| `nvidia` | on | NVIDIA GPU support via NVML dlopen |
| `json` | on | JSON output format |
| `csv` | on | CSV sensor logging |
| `xml` | off | XML output format |
| `html` | off | HTML report generation |

Build without optional features for a smaller binary:

```bash
# Minimal: text output only, no TUI, no NVIDIA
cargo build --release --no-default-features

# Text + JSON, no TUI
cargo build --release --no-default-features --features json
```

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

## Data Sources

sio reads directly from Linux kernel interfaces -- no lm-sensors or other userspace daemons required.

| Data | Source |
|------|--------|
| CPU identification | CPUID instruction (`raw-cpuid` crate) |
| CPU topology | `/sys/devices/system/cpu/cpu*/topology/` |
| CPU frequency | `/sys/devices/system/cpu/cpu*/cpufreq/` |
| CPU utilization | `/proc/stat` |
| CPU vulnerabilities | `/sys/devices/system/cpu/vulnerabilities/` |
| Memory | `/proc/meminfo` + SMBIOS Type 17 |
| Motherboard/BIOS | `/sys/class/dmi/id/` |
| Chipset | PCI host bridge at `0000:00:00.0` |
| UEFI/Secure Boot | `/sys/firmware/efi/` |
| GPU (NVIDIA) | NVML via `dlopen("libnvidia-ml.so.1")` |
| GPU (AMD) | `/sys/class/drm/card*/device/` + hwmon |
| GPU (Intel) | `/sys/class/drm/card*/` + hwmon |
| Storage | `/sys/class/block/` + `/sys/class/nvme/` |
| Network | `/sys/class/net/` + `getifaddrs()` |
| PCI devices | `/sys/bus/pci/devices/` + `pci.ids` (embedded) |
| Sensors (hwmon) | `/sys/class/hwmon/hwmon*/` |
| Power (RAPL) | `/sys/class/powercap/intel-rapl:*/` |
| Disk throughput | `/proc/diskstats` |
| Network throughput | `/sys/class/net/*/statistics/` |

## Project Structure

```
src/
  main.rs              -- CLI dispatch and orchestration
  cli.rs               -- clap argument definitions
  error.rs             -- Error types (SiomonError, SysfsError, MsrError, NvmlError)

  model/               -- Data structures (serde Serialize/Deserialize)
    system.rs          -- SystemInfo top-level container
    cpu.rs             -- CpuInfo, CpuTopology, CpuCache, CpuFeatures
    gpu.rs             -- GpuInfo, PcieLinkInfo, DisplayOutput
    memory.rs          -- MemoryInfo, DimmInfo, MemoryTimings
    motherboard.rs     -- MotherboardInfo, BiosInfo
    storage.rs         -- StorageDevice, NvmeDetails, SmartData
    network.rs         -- NetworkAdapter, IpAddress
    pci.rs             -- PciDevice
    audio.rs           -- AudioDevice
    usb.rs             -- UsbDevice
    battery.rs         -- BatteryInfo
    sensor.rs          -- SensorId, SensorReading, SensorUnit, SensorCategory

  config.rs            -- Config file loading (~/.config/siomon/config.toml)

  collectors/          -- One-shot hardware data collection
    cpu.rs             -- CPUID (x86) + ARM MIDR_EL1 + /proc/cpuinfo + sysfs
    gpu.rs             -- NVML + amdgpu + i915/xe sysfs + EDID
    memory.rs          -- Custom SMBIOS parser (fallback: dmidecode)
    motherboard.rs     -- DMI sysfs + SMBIOS supplement + chipset detection
    storage.rs         -- NVMe + SATA enumeration + SMART via ioctl
    network.rs         -- Interface enumeration + IP addresses
    audio.rs           -- /proc/asound + codec detection
    usb.rs             -- /sys/bus/usb device tree
    battery.rs         -- /sys/class/power_supply
    pci.rs             -- PCI bus scan + pci-ids name resolution
    me.rs              -- Intel ME/AMT version detection

  sensors/             -- Real-time sensor polling
    hwmon.rs           -- /sys/class/hwmon reader (with label overrides)
    cpu_freq.rs        -- Per-core frequency
    cpu_util.rs        -- Per-core utilization from /proc/stat deltas
    gpu_sensors.rs     -- NVML (persistent handle) + amdgpu hwmon polling
    rapl.rs            -- RAPL energy counter -> watts
    disk_activity.rs   -- /proc/diskstats -> MB/s
    network_stats.rs   -- Interface byte counters -> MB/s
    alerts.rs          -- Threshold-based sensor alerts with cooldown
    poller.rs          -- Threaded polling scheduler + shared state

  parsers/             -- Binary format parsers
    smbios.rs          -- Raw SMBIOS/DMI table parser (Types 0/1/2/17)
    edid.rs            -- EDID monitor info (manufacturer, resolution, name)

  platform/            -- Linux kernel interface abstraction
    sysfs.rs           -- Type-safe sysfs file readers
    procfs.rs          -- /proc/meminfo, /proc/cpuinfo parsers
    msr.rs             -- /dev/cpu/N/msr access
    nvml.rs            -- NVML dlopen wrapper (18 functions)
    nvme_ioctl.rs      -- NVMe SMART/Health via admin command ioctl

  output/              -- Output formatters
    text.rs            -- Pretty-printed terminal output
    json.rs            -- JSON via serde_json
    xml.rs             -- XML via quick-xml
    html.rs            -- Self-contained HTML report with CSS
    csv.rs             -- CSV sensor logging
    tui.rs             -- ratatui interactive sensor dashboard

  db/                  -- Embedded lookup databases
    cpu_codenames.rs   -- CPUID family/model -> codename (Intel/AMD/ARM)
    sensor_labels.rs   -- Board-specific sensor label overrides
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `raw-cpuid` | 11 | x86 CPUID instruction parsing |
| `serde` + `serde_json` | 1 | Serialization for JSON output |
| `toml` | 0.8 | Config file parsing |
| `clap` | 4 | CLI argument parsing |
| `ratatui` + `crossterm` | 0.29 / 0.28 | Terminal UI (optional: `tui` feature) |
| `quick-xml` | 0.37 | XML output (optional: `xml` feature) |
| `csv` | 1 | CSV sensor logging (optional: `csv` feature) |
| `pci-ids` | 0.2 | PCI vendor/device name database (compiled in) |
| `libloading` | 0.8 | dlopen for NVML (optional: `nvidia` feature) |
| `nix` | 0.29 | Unix syscall wrappers |
| `libc` | 0.2 | C FFI types (getifaddrs) |
| `chrono` | 0.4 | Timestamps |
| `thiserror` | 2 | Error derive macros |
| `glob` | 0.3 | Sysfs path enumeration |
| `log` + `env_logger` | 0.4 / 0.11 | Debug logging (`RUST_LOG=debug`) |

## Install

```bash
cargo install siomon
```

## License

MIT
