# siomon

A comprehensive Linux hardware information and real-time sensor monitoring tool. Single static binary, no runtime dependencies.

Run `sio` to launch an interactive TUI dashboard that polls all kernel-exported sensors in real time with min/max/avg tracking, configurable alerts, and CSV logging. Use subcommands like `sio cpu`, `sio gpu`, or `sio storage` for one-shot hardware information.

![siomon TUI dashboard](assets/screenshot.png)

## Install

### Ubuntu 24.04 / 25.10 (PPA)

```bash
sudo apt update
sudo apt install software-properties-common
sudo add-apt-repository ppa:level1techs/siomon
sudo apt update
sudo apt install siomon
```

### Arch Linux (AUR)

```bash
# Using an AUR helper (e.g., paru, yay)
paru -S siomon
```

### Nix

```bash
# NixOS (add to configuration.nix)
environment.systemPackages = [ pkgs.siomon ];

# Non-NixOS / nix profile
nix profile install nixpkgs#siomon

# Temporary shell
nix-shell -p siomon
```

More installation options are planned. If you'd like to help package siomon
for other distributions, see [issue #4](https://github.com/level1techs/siomon/issues/4).

## Quick Start

```bash
# Interactive TUI sensor monitor (default)
sio

# TUI with custom polling interval (ms)
sio --interval 500

# TUI with CSV logging
sio --log sensors.csv

# Sensor alerts
sio --alert "hwmon/nct6798/temp1 > 80" --alert "hwmon/nct6798/fan1 < 100 @60s"

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

# Full access (SMART, DMI serials, MSR)
sudo sio
```

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
- **Intel MCHBAR** (`--direct-io`, Arrow Lake-S) -- CPU/SoC BCLK, CPU-cores and iGPU temperatures, requested SA voltage; trained DDR5 rate, gear, command rate and timings in `sio memory` and the DIMM view
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
- Pretty-printed text summary (default for subcommands)
- JSON (`-f json`)
- XML (`-f xml`)
- HTML report (`-f html`) -- self-contained dark-themed report with color-coded vulnerability status
- Per-section views (`sio cpu`, `sio gpu`, `sio storage`, `sio pcie`, etc.)
- Sensor snapshot (`sio sensors`)

### Configuration
- Config file at `~/.config/siomon/config.toml` for persistent preferences
- Sensor label overrides (built-in board mappings + user custom labels)

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
| **`--direct-io`** | + Super I/O registers, I2C SPD/PMBus, Intel MCHBAR (BCLK, CPU-cores/iGPU temps, trained memory timings) |

Fields requiring elevation show `[requires root]` or are omitted.

### Running `--direct-io` Without Root

`--direct-io` needs raw hardware access. The recommended path is the `sinfo_io`
kernel module (`/dev/sinfo_io`), which avoids direct `/dev/port` access.

1. Create a dedicated group:
```bash
sudo groupadd -f siomon
```

2. Add your user to that group:
```bash
sudo usermod -aG siomon "$USER"
```

3. Install a udev rule:
```bash
echo 'KERNEL=="sinfo_io", MODE="0660", GROUP="siomon"' | sudo tee /etc/udev/rules.d/99-siomon-sinfo-io.rules
```

4. Reload rules and trigger:
```bash
sudo udevadm control --reload-rules
sudo udevadm trigger --name-match=sinfo_io
```

5. Re-login (or reboot) so new group membership applies.

Then verify:
```bash
ls -l /dev/sinfo_io
sio sensors --direct-io
```

If `/dev/sinfo_io` is missing, the module is not loaded for your current
kernel. Load/install `sinfo_io` first (DKMS or manual module install).

Troubleshooting:

- `sio` says `Cannot open /dev/port for Super I/O detection` and no `superio/*` sensors:
  `--direct-io` could not access either `/dev/sinfo_io` or `/dev/port`.
- `/dev/sinfo_io` is missing:
  load the module for the running kernel (`sudo modprobe sinfo_io`), or install/rebuild it (DKMS/manual) for the current `uname -r`.
- `modprobe: FATAL: Module sinfo_io not found`:
  the module is not installed for this kernel version. Rebuild/install `sinfo_io` against the currently running kernel, then run `sudo depmod -a`.
- `/dev/sinfo_io` exists but is `root:root` or mode `0600`:
  udev rule did not apply. Re-run `udevadm control --reload-rules` and `udevadm trigger --name-match=sinfo_io`, or reload the module.
- User is in `siomon` group but access still denied:
  session has stale group memberships. Log out/in (or reboot), then verify with `id`.
- Board hwmon driver exists but sensors still do not appear:
  some drivers only bind with a force flag (example: `sudo modprobe nct6683 force=1` on ASRock Z890 Nova WiFi). Use only when you have confirmed it matches your board/chip.
- Very new board, older kernel:
  support for newer Super I/O / hwmon chips usually lands in newer kernels first; older LTS kernels often lack those IDs unless your distro backports them.

### Board Notes

- **ASRock Z890 Nova WiFi** -- board fan telemetry is exposed by an NCT6686D-class companion controller. On current kernels this requires loading the in-tree `nct6683` hwmon driver with `force=1`:

```bash
sudo modprobe nct6683 force=1
```

  Once loaded, the board fans appear under `hwmon/nct6686`.

## Building

### Prerequisites

- Rust 1.88+ (edition 2024)
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

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for project-specific details and how to contribute.

## Packaging

Distribution packages are built and published automatically when a version
tag is pushed. See [PACKAGING.md](PACKAGING.md) for setup and configuration.

- **AUR**: Workflow queries the AUR API and auto-increments `pkgrel` when
  re-publishing the same version. See [`packaging/aur/`](packaging/aur/).
- **PPA**: Workflow queries the Launchpad API and auto-increments the
  packaging revision to avoid upload conflicts. See
  [`packaging/launchpad/`](packaging/launchpad/).

## License

MIT
