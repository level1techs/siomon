# Contributing to siomon

## Getting Started

### Prerequisites

- Rust 1.85+ (edition 2024)
- Linux (kernel 4.x+ for full sysfs support; 5.x+ recommended)
- Standard build tools (`gcc` or `cc` for libc linking)

### Building

```bash
cargo build --release
```

The binary is at `./target/release/sio`.

### Running Tests

```bash
cargo test
```

### Submitting Changes

1. Fork the repository
2. Create a feature branch from `main`
3. Keep PRs small and focused — one logical change per PR
4. Run `cargo test` and `cargo clippy` before submitting
5. Open a pull request against `main`

### Reporting Issues

File issues at [github.com/level1techs/siomon/issues](https://github.com/level1techs/siomon/issues). Include your kernel version (`uname -r`) and hardware details when reporting hardware detection issues.

## Data Sources

sio reads directly from Linux kernel interfaces -- no lm-sensors or other userspace daemons required.

| Data                | Source                                         |
| ------------------- | ---------------------------------------------- |
| CPU identification  | CPUID instruction (`raw-cpuid` crate)          |
| CPU topology        | `/sys/devices/system/cpu/cpu*/topology/`       |
| CPU frequency       | `/sys/devices/system/cpu/cpu*/cpufreq/`        |
| CPU utilization     | `/proc/stat`                                   |
| CPU vulnerabilities | `/sys/devices/system/cpu/vulnerabilities/`     |
| Memory              | `/proc/meminfo` + SMBIOS Type 17               |
| Motherboard/BIOS    | `/sys/class/dmi/id/`                           |
| Chipset             | PCI host bridge at `0000:00:00.0`              |
| UEFI/Secure Boot    | `/sys/firmware/efi/`                           |
| GPU (NVIDIA)        | NVML via `dlopen("libnvidia-ml.so.1")`         |
| GPU (AMD)           | `/sys/class/drm/card*/device/` + hwmon         |
| GPU (Intel)         | `/sys/class/drm/card*/` + hwmon                |
| Storage             | `/sys/class/block/` + `/sys/class/nvme/`       |
| Network             | `/sys/class/net/` + `getifaddrs()`             |
| PCI devices         | `/sys/bus/pci/devices/` + `pci.ids` (embedded) |
| Sensors (hwmon)     | `/sys/class/hwmon/hwmon*/`                     |
| Power (RAPL)        | `/sys/class/powercap/intel-rapl:*/`            |
| Disk throughput     | `/proc/diskstats`                              |
| Network throughput  | `/sys/class/net/*/statistics/`                 |

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

| Crate                   | Version     | Purpose                                       |
| ----------------------- | ----------- | --------------------------------------------- |
| `raw-cpuid`             | 11          | x86 CPUID instruction parsing                 |
| `serde` + `serde_json`  | 1           | Serialization for JSON output                 |
| `toml`                  | 0.8         | Config file parsing                           |
| `clap`                  | 4           | CLI argument parsing                          |
| `clap_complete`         | 4           | Shell completions                             |
| `ratatui` + `crossterm` | 0.29 / 0.28 | Terminal UI (optional: `tui` feature)         |
| `quick-xml`             | 0.37        | XML output (optional: `xml` feature)          |
| `csv`                   | 1           | CSV sensor logging (optional: `csv` feature)  |
| `pci-ids`               | 0.2         | PCI vendor/device name database (compiled in) |
| `libloading`            | 0.8         | dlopen for NVML (optional: `nvidia` feature)  |
| `nix`                   | 0.29        | Unix syscall wrappers                         |
| `libc`                  | 0.2         | C FFI types (getifaddrs)                      |
| `chrono`                | 0.4         | Timestamps                                    |
| `thiserror`             | 2           | Error derive macros                           |
| `glob`                  | 0.3         | Sysfs path enumeration                        |
| `log` + `env_logger`    | 0.4 / 0.11  | Debug logging (`RUST_LOG=debug`)              |
