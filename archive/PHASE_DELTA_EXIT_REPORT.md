# PHASE DELTA Exit Report

**Project:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Date:** 2026-03-16
**Target:** `x86_64-pc-windows-msvc`
**Test system:** AMD Ryzen Threadripper PRO 3975WX, ASUS Pro WS WRX80E-SAGE SE WIFI, NVIDIA GeForce GTX 1650, 64 GB RAM, 4x16 GiB Kingston DDR4, Windows 10 Pro build 26200 (elevated)

---

## Deliverables

3 commits, 11 files changed, +655 / -26 lines.

| Commit | Work Items | Description |
|--------|-----------|-------------|
| `63c803a` | D1 | SMBIOS parsing via GetSystemFirmwareTable |
| `ccbb408` | D2-D7 | DIMM details, CPU microcode, BIOS date, chipset, network fix, ACPI thermal |
| `9d74ef4` | D8 | Zero-warning Windows build (gate unused unix imports) |

New files:
- `src/sensors/acpi_thermal_win.rs` — ACPI thermal zone temperature sensor

---

## What Was Implemented

### D1: SMBIOS Table Parsing on Windows
- Added `#[cfg(windows)]` path to `smbios::parse()` using `GetSystemFirmwareTable('RSMB', 0)`
- Extracted `parse_from_bytes()` for direct byte-slice input
- Skips 8-byte `RawSMBIOSData` header, passes table data to existing binary parser
- Added SMBIOS Type 16 (Physical Memory Array) parsing for `max_capacity_bytes` and slot count
- All 15 existing SMBIOS unit tests pass

### D2: Memory DIMM Details
- Wired `smbios::parse()` into Windows memory collector
- Populates: locator, manufacturer, part number, serial, size, type, speed, voltage, ECC, rank
- Verified: 4x Kingston KHX3600C18D4/16GX DDR4, 3200 MT/s, 1200 mV
- Max capacity 512 GiB, 8 slots, 4 populated

### D3: CPU Microcode Version
- Reads from registry `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\Update Revision`
- Parses REG_BINARY hex, extracts upper 4 bytes as revision
- Verified: `0x7D103008`

### D4: Network Interface Type + Driver Name
- Added `Display` impl for `NetworkInterfaceType` (ethernet/wifi/loopback/tunnel)
- Added WMI `Win32_NetworkAdapter.ServiceName` lookup for driver names
- Eliminated all "(unknown)" from network output
- Verified: "Wi-Fi: 155 Mbps (wifi, Netwtw10) [up]"

### D5: ACPI Thermal Zones
- New `AcpiThermalSource` querying `MSAcpi_ThermalZoneTemperature` via PowerShell
- Converts tenths-of-Kelvin to Celsius
- Graceful fallback when no zones available (this test system returned none)
- 12 unit tests

### D6: BIOS Date Formatting
- Parses WMI datetime `20251016000000.000000+000` → `2025-10-16`
- Verified in motherboard output

### D7: Chipset from PCI 00:00.0
- Populates `motherboard.chipset` from PCI device at 00:00.0 after collection
- Prefers devices with "root complex" or "host bridge" in name
- Verified: `Starship/Matisse Root Complex`

### D8: Zero Warnings
- Gated `BTreeMap`, `BTreeSet`, `NumaNode` imports with `#[cfg(unix)]`
- Gated `count_cpulist_entries()` with `#[cfg(unix)]`
- Windows release build produces zero warnings

---

## Final Output Verification

Every section of `sio` now shows populated data on Windows:

| Section | Before Delta | After Delta |
|---------|-------------|-------------|
| CPU Microcode | Missing | `0x7D103008` |
| Memory DIMMs | Empty | 4x Kingston DDR4 16 GiB @ 3200 MT/s |
| Memory Slots | Unknown | 4/8 populated, 512 GiB max |
| BIOS Date | `20251016000000.000000+000` | `2025-10-16` |
| Chipset | Missing | `Starship/Matisse Root Complex` |
| Network Type | "(unknown)" | "ethernet", "wifi", "if-type:53" |
| Network Driver | Missing | Netwtw10, BthPan, VMnetAdapter, etc. |
| Build Warnings | 3 | 0 |

---

## Cumulative Project Stats (All Phases)

| Phase | Files | Insertions | Deletions | Key Deliverable |
|-------|-------|------------|-----------|-----------------|
| ALPHA | 29 | +3,436 | -158 | Windows port foundation, 150 sensors |
| BETA | 11 | +1,706 | -63 | User-mode parity, 154 sensors |
| GAMMA | 19 | +2,397 | -25 | WinRing0 integration, SuperIO/RAPL/HSMP/ADL/SMBus |
| DELTA | 11 | +655 | -26 | SMBIOS, DIMM details, polish, zero warnings |
| **Total** | **~60** | **+8,194** | **-272** | **Complete Windows hardware monitoring tool** |

---

## Deferred Items

### D9: WinRing0 Live Hardware Validation
Requires installing WinRing0 on test systems with various hardware configurations. Documented in PHASE_DELTA.md with a full validation matrix. Deferred to PHASE EPSILON or as part of upstream PR review.

### D8 Upstream PR (partial)
The codebase is ready for PR preparation:
- Zero warnings on Windows
- All collectors populated
- Graceful fallback when optional drivers absent
- `[target.'cfg(windows)'.dependencies]` keeps Windows deps out of Linux builds

Remaining for PR: rebase onto upstream main, squash commits, add CI, update README.

---

## Sensor Count Summary

| Source | Count | Notes |
|--------|-------|-------|
| CPU frequency | 64 | Per-logical-core MHz |
| CPU utilization | 65 | Per-core + total |
| Disk activity | 4 | Read/write per volume |
| Network stats | 10 | RX/TX per adapter |
| NVIDIA GPU (NVML) | 7 | Temp, fan, power, clocks, util, VRAM |
| WHEA errors | 4 | Corrected/fatal/MCE/PCIe |
| ACPI thermal | 0-3 | System-dependent |
| **Always active** | **154+** | No driver needed |
| SuperIO (WinRing0) | ~32 | Temps, fans, voltages |
| RAPL (WinRing0) | ~3 | Package/DRAM/core power |
| HSMP (WinRing0) | ~14 | AMD SMU telemetry |
| PCIe link (WinRing0) | per-device | Speed/width info |
| SMBus (WinRing0) | varies | VRM + DIMM temps |
| AMD ADL | ~3-7 | AMD GPU sensors |
| IPMI (ipmitool) | varies | BMC sensors |
| **With all drivers** | **~210+** | Full hardware monitoring |

---

## Recommendations for PHASE EPSILON

1. **Install WinRing0 and validate** — the highest-impact next step. SuperIO sensors, RAPL power, and PCIe link info are all coded and ready but untested on live hardware.

2. **Upstream PR** — rebase the 20+ commits into ~4 logical commits (one per phase theme), add Windows CI to GitHub Actions, update README with build/install instructions.

3. **Cross-compile testing** — verify `cargo check --target x86_64-pc-windows-msvc` works from a Linux CI runner (cross-rs or native MSVC cross-tools).

4. **ARM64 port** — the codebase is clean for expansion to `aarch64-pc-windows-msvc` (Surface Pro, Snapdragon laptops). CPUID code is already x86-gated.
