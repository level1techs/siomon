# PHASE BETA Exit Report

**Project:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Date:** 2026-03-16
**Target:** `x86_64-pc-windows-msvc`
**Test system:** AMD Ryzen Threadripper PRO 3975WX, ASUS Pro WS WRX80E-SAGE SE WIFI, NVIDIA GeForce GTX 1650, 64 GB RAM, Windows 10 Pro build 26200 (elevated)

---

## Deliverables

3 commits, 11 files changed, +1,706 / -63 lines.

| Commit | Work Items | Description |
|--------|-----------|-------------|
| `bfcad5a` | B2 | Network details via GetAdaptersAddresses (MAC, IPs, speed, MTU) |
| `f89cf60` | B1, B3, B5 | Battery via WMI, motherboard fields expansion, audio codec detection |
| `4830f21` | B4, B6, B7, B8 | USB speed/class, display outputs, WHEA errors, IPMI via ipmitool |

New platform files:
- `src/sensors/whea.rs` — WHEA error monitoring via wevtutil
- `src/sensors/ipmi_win.rs` — IPMI BMC sensors via ipmitool CLI

---

## What Was Implemented

### B1: Battery Information
- WMI `Win32_Battery` query via PowerShell JSON
- Chemistry mapping (LiIon, LiPoly, NiMH, NiCd, PbAcid, ZnAir)
- Status mapping (Charging, Discharging, Full, NotCharging)
- Capacity (design/full in mWh), voltage (mV), wear percent
- Desktop: graceful "No batteries detected"

### B2: Network Interface Details
- Replaced sysinfo stub with full `GetAdaptersAddresses` implementation
- MAC addresses (XX:XX:XX:XX:XX:XX)
- IPv4 and IPv6 with prefix length and scope (global/link)
- Link speed in Mbps (Wi-Fi 163 Mbps, Hyper-V vEthernet 10 Gbps)
- MTU, interface type (Ethernet/WiFi/Loopback/Tunnel), OperStatus
- 11 adapters enumerated on test system

### B3: Motherboard Detail Fields
- Serial number: `211092904301164`
- Board version: `Rev 1.xx`
- System UUID: `5406B63E-4204-EF1A-3445-04421AEF3446`
- Chassis type: `Desktop` (parsed from WMI `{3}`)
- BIOS vendor: `American Megatrends Inc.`, release: `3.3`
- Secure Boot: detected as `false` from registry
- UEFI boot: detected from registry key existence (replaces hardcoded `true`)
- System family, SKU populated

### B4: USB Device Details
- Speed classification: Full/High/Super from CompatibleID + DeviceID heuristics
- Device class extraction from USB Class codes in CompatibleID
- USB version propagation from Win32_USBHub when available
- 10 devices with proper speed classification

### B5: Audio Codec Name
- Parse HDAUDIO VEN_/DEV_ from WMI DeviceID
- Vendor mapping: Realtek ALC###, NVIDIA HD Audio, Intel, AMD, Conexant
- NVIDIA device shows `Codec: NVIDIA HD Audio`
- USB audio devices correctly show no codec

### B6: Display Output Enumeration
- `EnumDisplayDevicesW` + `EnumDisplaySettingsW` for monitor discovery
- Monitor name, resolution, refresh rate, connected status
- Attached to GPU via NVML adapter matching
- Test: "Generic PnP Monitor" at 1280x1280@32Hz on GTX 1650

### B7: WHEA Error Monitoring
- New `WheaSource` sensor querying `Microsoft-Windows-WHEA-Logger` via wevtutil
- Tracks 4 event categories: corrected HW (17), fatal HW (18), corrected MCE (19), corrected PCIe (47)
- Baseline-delta reporting (0 errors on healthy system)
- Appears in `sio sensors` and TUI dashboard

### B8: IPMI BMC Sensors
- New `IpmiWinSource` using ipmitool CLI backend
- Background 5-second polling thread with `Arc<RwLock>` shared state
- Parses pipe-delimited sensor list (temperature, RPM, voltage, watts, amps, percent)
- Graceful fallback when ipmitool not installed (no sensors shown)
- On this system: ipmitool not installed, IPMI source inactive but ready

---

## Sensor Count Summary

| Source | Sensors | Notes |
|--------|---------|-------|
| CPU frequency | 64 | Per-logical-core MHz |
| CPU utilization | 65 | Per-core + total |
| Disk activity | 4 | Read/write per volume |
| Network stats | 10 | RX/TX per adapter |
| NVIDIA GPU | 7 | Temp, fan, power, clocks, util, VRAM |
| WHEA errors | 4 | Corrected/fatal/MCE/PCIe |
| **Total** | **154** | Up from 150 in Alpha |

IPMI sensors will appear additionally on systems with ipmitool installed and a BMC.

---

## Gaps Closed by Beta

| Gap # | Description | Status |
|-------|-------------|--------|
| 4 | Battery information | Closed (B1) |
| 5 | Network MAC/IP/speed/MTU | Closed (B2) |
| 6 | Display outputs | Partially closed (B6 — monitors enumerated, AMD GPU sensors deferred) |
| 8 | Hardware error monitoring | Closed (B7 — WHEA replaces MCE+EDAC+AER) |
| 9 | IPMI BMC sensors | Closed (B8 — via ipmitool backend) |
| 11 | Motherboard detail fields | Closed (B3) |
| 12 | USB speed/class | Closed (B4) |
| 13 | Audio codec name | Closed (B5) |

---

## Remaining Gaps (for PHASE GAMMA)

These gaps require WinRing0 kernel driver or vendor SDKs:

| Gap # | Description | Blocker |
|-------|-------------|---------|
| 1 | hwmon temperatures/fans/voltages | WinRing0 port I/O for SuperIO |
| 2 | RAPL power metering | WinRing0 MSR access |
| 3 | AMD HSMP telemetry | WinRing0 PCI config + AMD SMN mailbox |
| 6 | AMD GPU sensors (partial) | AMD ADL/ADLX SDK |
| 7 | PCIe link speed/width | WinRing0 PCI config or SetupAPI DEVPKEY |
| 10 | I2C/PMBus VRM + DDR5 DIMM temps | WinRing0 SMBus host controller |
| 14 | Super I/O chip detection | WinRing0 port I/O |

All remaining gaps share a common prerequisite: **WinRing0 integration**. This single dependency would unlock gaps 1, 2, 3, 7, 10, and 14. Gap 6 (AMD GPU) requires the separate AMD ADL SDK.

---

## Files Changed in Beta

| File | Lines Added | Description |
|------|-------------|-------------|
| `src/collectors/network.rs` | +214 | GetAdaptersAddresses implementation |
| `src/collectors/gpu.rs` | +340 | Display output enumeration, Windows cfg restructuring |
| `src/sensors/ipmi_win.rs` | +342 | New: IPMI via ipmitool |
| `src/collectors/usb.rs` | +229 | Speed/class/version from CompatibleID |
| `src/sensors/whea.rs` | +228 | New: WHEA error sensor |
| `src/collectors/battery.rs` | +167 | WMI Win32_Battery |
| `src/collectors/audio.rs` | +128 | HDAUDIO codec resolution |
| `src/collectors/motherboard.rs` | +86 | 10 additional fields |
| `src/sensors/mod.rs` | +4 | whea + ipmi_win modules |
| `src/sensors/poller.rs` | +4 | Register new sensors |
| `Cargo.toml` | +2 | wingdi, winuser features |

---

## Recommendations for PHASE GAMMA

1. **WinRing0 is the critical path.** Integrating WinRing0 (`WinRing0x64.sys`, BSD license) as a build dependency unlocks SuperIO monitoring, RAPL power, PCIe link info, and potentially HSMP. This is the single highest-impact piece of work remaining.

2. **SuperIO register code is portable.** The existing `nct67xx.rs` and `ite87xx.rs` chip-level code only needs `read_byte(port)` / `write_byte(port, value)` — the register logic is platform-agnostic. Only the port I/O transport needs a Windows implementation.

3. **RAPL is low effort once MSR access exists.** The delta-based power calculation in `rapl.rs` is pure math. Only the MSR read needs porting.

4. **AMD ADL is independent of WinRing0.** It loads `atiadlxx.dll` (ships with AMD driver) via libloading. Can be developed in parallel with WinRing0 work.

5. **Test on multiple platforms.** Beta was tested only on AMD TRX/WRX80E. GAMMA should include Intel desktop (for RAPL), AMD AM5 desktop (for Zen 4+ HSMP), and laptop (for battery + thermal zone coverage).
