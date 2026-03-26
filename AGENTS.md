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
  stdout is a terminal, and format is not explicitly set.
- **One-shot info** -- hardware details via subcommands (`sio cpu`, `sio gpu`,
  etc.) or a sensor snapshot (`sio sensors`).

## Source Structure

The `src/` directory is organized by concern:

- `collectors/` -- one-shot hardware data collection (one module per subsystem).
  Each exposes a `collect()` function.
- `sensors/` -- real-time polling sources implementing the `SensorSource` trait.
  Includes hwmon, CPU, GPU, RAPL, disk, network, IPMI, HSMP, EDAC, MCE, AER,
  I2C/PMBus, and Super I/O chip drivers.
- `model/` -- serde data structures shared between collectors and output.
- `output/` -- formatters (text, json, xml, html, csv) and the TUI dashboard.
- `platform/` -- OS interface abstraction (sysfs, procfs, MSR, NVML, NVMe/SATA
  ioctl, port I/O, kernel module, Tegra).
- `parsers/` -- binary format parsers (SMBIOS, EDID).
- `db/` -- embedded lookup databases (CPU codenames, sensor labels, board
  templates organized by vendor).
- `cli` -- clap argument definitions.
- `config` -- config file loading (`$XDG_CONFIG_HOME/siomon/config.toml`).
- `error` -- error types (thiserror).

## Key Patterns

**Parallel collection** -- all hardware collectors run in parallel via
`std::thread::scope()`. Panics are caught and default values returned.

**SensorSource trait** -- each polling source implements `name()` and `poll()`.
Discovery happens during construction. A poller orchestrates the loop in a
background thread with shared state.

**Board database** -- per-board templates matched by DMI board name substring,
organized by vendor under `db/boards/`. First match wins — more specific boards
must come before generic ones. Adding a board: create the `.rs` file, add it to
the vendor's `mod.rs`, and register it in the `BOARDS` array.

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

**CI** (`.github/workflows/ci.yml`) -- triggered on push/PR to main. Jobs:
check, clippy (with `-A dead_code`), fmt, test, build, build-minimal.

**Release** (`.github/workflows/release.yml`) -- triggered on `v*` tags.
Build matrix: x86_64 + aarch64 (cross-compiled). Pipeline: build → crates.io
publish (OIDC) → GitHub Release → AUR + PPA in parallel.

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
