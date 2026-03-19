# siomon - Agent Context

## Project Overview

siomon is a Linux hardware information and real-time sensor monitoring tool
written in Rust (edition 2024, rust-version 1.85+). The binary is called `sio`,
the package is called `siomon`. Licensed under MIT.

Repository: https://github.com/level1techs/siomon

Architectures: x86_64 and aarch64 (including NVIDIA Tegra/Jetson platforms).

Two operating modes:
- **TUI dashboard** -- real-time sensor monitoring with min/max/avg tracking,
  alerts, and CSV logging. This is the default when no subcommand is given,
  stdout is a terminal, and format is not explicitly set (`main.rs` lines 25-32).
- **One-shot info** -- hardware details via subcommands (`sio cpu`, `sio gpu`,
  etc.) or a sensor snapshot (`sio sensors`).

## Source Structure

Top-level modules (from `src/lib.rs`):

- `cli` -- clap argument definitions (`Cli`, `Commands` enum, `OutputFormat`)
- `collectors` -- one-shot hardware data collection (cpu, gpu, memory, storage,
  network, pci, audio, usb, battery, motherboard, me). Each exposes a free
  `collect()` function.
- `config` -- config file loading from `$XDG_CONFIG_HOME/siomon/config.toml`
  (falls back to `~/.config/siomon/config.toml`). Supports format, interval,
  color, theme, no_nvidia, physical_net_only, storage_exclude, sensor_labels.
- `db` -- embedded lookup databases: `cpu_codenames`, `sensor_labels`, `mce`,
  `voltage_scaling`, and `boards/` (per-board hardware templates by vendor).
- `error` -- `SiomonError` and `NvmlError` types (thiserror).
- `model` -- serde Serialize/Deserialize data structures: system, cpu, gpu,
  memory, motherboard, storage, network, audio, usb, battery, pci, sensor.
- `output` -- formatters: text, json (feature-gated), xml (feature-gated), html
  (feature-gated), csv (feature-gated, sensor logging), tui/ (dashboard, theme).
- `parsers` -- binary format parsers: smbios (custom, no dmidecode dependency),
  edid (monitor info).
- `platform` -- OS interface abstraction: sysfs, procfs, msr, nvml (dlopen via
  libloading), nvme_ioctl, sata_ioctl, port_io, sinfo_io (kernel module), tegra.
- `sensors` -- real-time polling sources: poller, hwmon, cpu_freq, cpu_util,
  gpu_sensors, rapl, disk_activity, network_stats, alerts, ipmi, hsmp, edac,
  mce, aer, i2c/ (bus_scan, smbus_io, pmbus, spd5118), superio/ (chip_detect,
  nct67xx, ite87xx).

## Key Patterns

**Parallel collection** -- `collect_all()` in `main.rs` runs all collectors in
parallel via `std::thread::scope()`. Each collector is a free `collect()`
function (the `Collector` trait is defined in `collectors/mod.rs` but not used
for dispatch). Panics are caught by `join_or_default()` which logs and returns
`T::default()`.

**`SensorSource` trait** (`sensors/mod.rs`) -- `fn name(&self) -> &str` and
`fn poll(&mut self) -> Vec<(SensorId, SensorReading)>`. Discovery happens
per-source during construction (not part of the trait). The `Poller` struct
orchestrates the polling loop in a background thread with state shared via
`Arc<Mutex<>>`.

**Board database** (`db/boards/`) -- per-board `BoardTemplate` structs matched
by DMI board name substring. Organized by vendor: `asus/` (6 boards), `asrock/`
(1), `nvidia/` (2: DGX Spark, Jetson Thor). First match wins -- more specific
boards must come before generic ones. Adding a board: create the `.rs` file, add
`pub mod` to vendor's `mod.rs`, add reference to the `BOARDS` array.

**Direct I/O** -- Super I/O (port I/O) and I2C/PMBus/SPD5118 sensors are only
enabled with the `--direct-io` flag (requires root). Standard hwmon sensors work
without root.

## Feature Flags

From `Cargo.toml`: `default = ["tui", "nvidia", "json", "csv"]`

| Feature | Purpose | Optional dep |
|---------|---------|-------------|
| `tui` | Interactive TUI dashboard | ratatui, crossterm |
| `nvidia` | GPU support via NVML (dlopen) | libloading |
| `json` | JSON output | serde_json |
| `csv` | CSV sensor logging | csv |
| `html` | HTML report output | (none) |
| `xml` | XML output | quick-xml |

Build with `--no-default-features` for a text-only minimal binary.

## CLI

Subcommands: `cpu`, `gpu`, `memory`, `storage`, `network`, `pci`, `usb`,
`audio`, `battery`, `board`, `pcie`, `sensors`.

TUI auto-activates when: no subcommand + stdout is terminal + format not
explicitly set + `tui` feature enabled. Config file values apply as defaults
for any CLI argument not explicitly set on the command line.

## Kernel Module

`kmod/sinfo_io/` contains a DKMS-based Linux kernel module (`sinfo_io.c`,
`sinfo_io.h`, `Makefile`, `dkms.conf`). Accessed from Rust via
`src/platform/sinfo_io.rs`.

## CI and Release

**CI** (`.github/workflows/ci.yml`) -- triggered on push/PR to main. Uses
`dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache@v2`. Jobs: check,
clippy (with `-A dead_code`), fmt, test, build (artifact: `sio-linux-x86_64`),
build-minimal (`--no-default-features`).

**Release** (`.github/workflows/release.yml`) -- triggered on `v*` tags.
Build matrix: x86_64 + aarch64 (cross-compiled with `gcc-aarch64-linux-gnu`).
Pipeline: build -> publish to crates.io (OIDC via `rust-lang/crates-io-auth-action`)
-> GitHub Release (`softprops/action-gh-release`) -> AUR + PPA in parallel.

**Packaging** -- AUR and PPA workflows auto-increment version numbers. See
`packaging/aur/AGENTS.md` and `packaging/launchpad/AGENTS.md` for details.

## Build and Test

Release profile: `opt-level = "z"`, LTO, `codegen-units = 1`, `panic = "abort"`,
strip.

```bash
cargo test --all-features
cargo clippy --all-features -- -D warnings -A dead_code
cargo build --release --all-features
```
