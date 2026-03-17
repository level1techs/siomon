# PHASE ALPHA Exit Report

**Project:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Date:** 2026-03-16
**Target:** `x86_64-pc-windows-msvc`
**Test system:** AMD Ryzen Threadripper PRO 3975WX, ASUS Pro WS WRX80E-SAGE SE WIFI, NVIDIA GeForce GTX 1650, 64 GB RAM, Windows 10 Pro build 26200

---

## Deliverables

6 commits, 26 files changed, +3,084 / -152 lines from upstream `level1techs/siomon:main`.

| Commit | Description |
|--------|-------------|
| `55564ff` | Initial Windows port — platform gating, sysinfo-based collectors, basic sensor stubs |
| `7c11c14` | PHASE_ALPHA.md gap analysis document |
| `ac67937` | NVIDIA NVML GPU sensors + per-core CPU frequency via CallNtPowerInformation |
| `2837f42` | NVMe SMART (IOCTL_STORAGE_QUERY_PROPERTY) + SATA SMART (SMART_RCV_DRIVE_DATA) |
| `78d2aa6` | PCI, USB, and audio device enumeration via WMI/PowerShell |
| `683d35e` | PHASE_ALPHA.md status update |

New platform files:
- `src/platform/nvme_win.rs` — NVMe SMART via Windows storage IOCTL
- `src/platform/sata_win.rs` — SATA SMART via Windows SMART IOCTL

---

## What Works on Windows

### Static Hardware Info (`sio`)

| Feature | Source | Status |
|---------|--------|--------|
| Hostname | `COMPUTERNAME` env var | Working |
| Kernel version | `cmd /c ver` (parsed) | Working — "10.0.26200.7840" |
| OS name | Registry `ProductName` | Working — "Windows 10 Pro" |
| CPU brand, vendor, family/model/stepping | CPUID (native x86) | Working |
| CPU codename | CPUID + codename DB | Working — "Zen 2 (Rome)" |
| CPU cache (L1/L2/L3) | CPUID | Working |
| CPU features (SSE, AVX, etc.) | CPUID | Working |
| CPU topology (packages/cores/threads/SMT) | sysinfo | Working — 1 pkg, 32 cores, 64 threads |
| Memory total/available/swap | sysinfo | Working — 63.8 GiB |
| Motherboard vendor, product | wmic `baseboard` | Working — ASUSTeK Pro WS WRX80E-SAGE SE WIFI |
| BIOS version/date | wmic `bios` | Working |
| Boot mode (UEFI) | Hardcoded true | Working (approximate) |
| Storage drives (mount, label, capacity, type) | sysinfo Disks | Working — C: 930 GiB NVMe, D: 36.3 TiB |
| NVMe SMART health data | `IOCTL_STORAGE_QUERY_PROPERTY` | Working (requires admin) |
| SATA SMART health data | `SMART_RCV_DRIVE_DATA` | Working (requires admin) |
| Network adapter names | sysinfo Networks | Working — 5 adapters |
| GPU (NVIDIA) info via NVML | nvml.dll + libloading | Working — GTX 1650, VBIOS, driver, VRAM, clocks |
| PCI device enumeration | WMI `Win32_PnPEntity` + pci_ids | Working — 87 devices |
| USB device enumeration | WMI `Win32_PnPEntity` | Working — 10 devices |
| Audio device enumeration | WMI `Win32_SoundDevice` | Working — 2 devices |

### Real-Time Sensors (`sio sensors` / `--tui`)

| Sensor | Source | Status |
|--------|--------|--------|
| Per-core CPU utilization (64 threads) | sysinfo | Working |
| Total CPU utilization | sysinfo | Working |
| Per-core CPU frequency | `CallNtPowerInformation` / powrprof.dll | Working — 3501 MHz |
| Disk read/write throughput | sysinfo Disks | Working |
| Network RX/TX throughput | sysinfo Networks | Working |
| NVIDIA GPU temperature | NVML | Working — 37 C |
| NVIDIA GPU fan speed | NVML | Working — 24% |
| NVIDIA GPU power | NVML | Working |
| NVIDIA GPU core/mem clock | NVML | Working — 300/405 MHz |
| NVIDIA GPU utilization | NVML | Working — 6% GPU, 2% mem |
| NVIDIA GPU VRAM used | NVML | Working — 1335 MB |

---

## Known Gaps: Linux vs Windows

### Gap 1: Hardware Monitor Sensors (hwmon)

**What Linux shows:** Temperatures (CPU, chipset, VRM, ambient), fan RPMs, voltage rails, power draw, and current from all hwmon-registered devices. This is the primary source of environmental sensor data.

**Windows status:** Not implemented.

**Blocking on:** WinRing0 kernel driver for port I/O (SuperIO access) or LibreHardwareMonitor integration. See PHASE_ALPHA.md Section 1-2.

**Impact:** No motherboard sensor data — no CPU temperature, no VRM temperature, no fan speeds, no voltage monitoring outside of NVML GPU data.

---

### Gap 2: RAPL Power Metering

**What Linux shows:** CPU package power (Watts), DRAM power, core power, uncore power — derived from Intel RAPL energy counters.

**Windows status:** Not implemented.

**Blocking on:** WinRing0 MSR access (`Ols_Rdmsr` for MSR 0x606/0x611/0x619/0x639). See PHASE_ALPHA.md Section 3.

**Impact:** No CPU or DRAM power consumption shown in dashboard.

---

### Gap 3: AMD HSMP Telemetry

**What Linux shows:** Socket power, SVI rail power, FCLK/MCLK frequencies, core clock throttle limit, C0 residency, DDR bandwidth utilization, Fmax/Fmin.

**Windows status:** Not implemented.

**Blocking on:** WinRing0 PCI config space access for SMN mailbox writes. See PHASE_ALPHA.md Section 5.

**Impact:** No AMD platform-level telemetry. This system (Threadripper PRO 3975WX / WRX80E) would particularly benefit from HSMP data.

---

### Gap 4: Battery Information

**What Linux shows:** Battery name, manufacturer, model, chemistry, status (charging/discharging), design/full/remaining capacity, voltage, power draw, wear percentage, cycle count.

**Windows status:** Returns empty. The `#[cfg(not(unix))]` stub returns `vec![]`.

**Blocking on:** Nothing — `GetSystemPowerStatus` from winapi or WMI `Win32_Battery` would provide this. Straightforward to implement.

**Impact:** Laptop users see no battery data.

---

### Gap 5: Network Interface Details

**What Linux shows:** MAC address, IP addresses (v4/v6 with prefix length and scope), link speed (Mbps), duplex, MTU, driver name, PCI bus address, interface type (Ethernet/WiFi/Bridge/etc.), NUMA node.

**Windows status:** Only adapter name and "up" state. All other fields are None/empty.

**Blocking on:** Nothing critical — Windows `GetAdaptersAddresses` API provides MAC, IPs, speed, MTU. Could use the `if-addrs` crate or raw Windows API.

**Impact:** Network section shows minimal information — no MACs, no IPs, no speeds.

---

### Gap 6: GPU Display Outputs and AMD GPU Sensors

**What Linux shows:**
- Display outputs per GPU (connector type, status, monitor name via EDID, resolution)
- AMD GPU: temperature, fan speed, power, GPU busy %, VRAM used (from sysfs hwmon)

**Windows status:**
- Display outputs: Not implemented (no DRM equivalent)
- AMD GPU sensors: Not implemented (requires AMD ADL/ADLX library)
- NVIDIA GPU: Fully working via NVML

**Blocking on:** AMD ADL SDK for GPU sensors. Display outputs could use DXGI or EnumDisplayDevices. See PHASE_ALPHA.md Section 10.

**Impact:** No AMD GPU monitoring. No connected monitor information for any GPU.

---

### Gap 7: PCIe Link Information

**What Linux shows:** Per-device PCIe negotiated and maximum link speed (GT/s), negotiated and maximum link width (x1-x16), computed bandwidth.

**Windows status:** Not available. WMI `Win32_PnPEntity` does not expose PCIe link parameters.

**Blocking on:** WinRing0 PCI config space reads to access PCIe Capability Structure, or use of SetupAPI with `DEVPKEY_PciDevice_CurrentLinkSpeed`.

**Impact:** `sio pcie` shows devices without link speed/width information. Storage NVMe details lack transport info.

---

### Gap 8: Hardware Error Monitoring (EDAC, AER, MCE)

**What Linux shows:**
- EDAC: Correctable/uncorrectable memory errors per memory controller and rank
- AER: PCIe correctable/nonfatal/fatal error counters per device
- MCE: Machine check exception counts per MCA bank

**Windows status:** Not implemented.

**Blocking on:** Windows Event Log / WHEA API for EDAC and MCE equivalents. WinRing0 PCI config space for AER registers. See PHASE_ALPHA.md Sections 11-13.

**Impact:** No hardware error tracking in the monitoring dashboard.

---

### Gap 9: IPMI/BMC Sensors

**What Linux shows:** DIMM temperatures, PSU power/current/voltage, ambient probes, labeled fan RPMs — all from the BMC via IPMI SDR.

**Windows status:** Not implemented.

**Blocking on:** Windows IPMI driver (`\\.\IPMI` + DeviceIoControl). Only available on server SKUs with BMC. See PHASE_ALPHA.md Section 6.

**Impact:** No BMC sensor data. Significant for this WRX80E workstation which has a BMC.

---

### Gap 10: I2C/SMBus Sensors (PMBus VRM, DDR5 DIMM temps)

**What Linux shows:** Per-VRM voltage, current, temperature, power via PMBus protocol. DDR5 DIMM temperatures via SPD5118 temperature sensor.

**Windows status:** Not implemented.

**Blocking on:** WinRing0 for SMBus host controller access. See PHASE_ALPHA.md Section 7.

**Impact:** No per-rail VRM telemetry, no per-DIMM temperature monitoring.

---

### Gap 11: Motherboard Detail Fields

**What Linux shows:** Serial number, version, system family, system SKU, system UUID, chassis type, chipset name (from PCI 00:00.0), ME firmware version, BIOS vendor, BIOS release version, Secure Boot status.

**Windows status:** Board manufacturer + product, BIOS version + date, UEFI boot (hardcoded true). All other fields are None.

**Blocking on:** Nothing — `wmic` or WMI can provide most of these (system UUID, serial, chassis type). Secure Boot via `Confirm-SecureBootUEFI` PowerShell cmdlet.

**Impact:** Motherboard section is sparse compared to Linux.

---

### Gap 12: USB Device Details

**What Linux shows:** USB spec version, device class, speed classification (Low/Full/High/Super/SuperPlus/SuperPlus2x2), bus/port path, max power draw (mA).

**Windows status:** VID/PID, product name only. Speed always "Unknown", class/version/power not populated.

**Blocking on:** SetupAPI `GUID_DEVINTERFACE_USB_DEVICE` with `DEVPKEY_Device_*` properties could provide speed and power.

**Impact:** USB device listing lacks speed and power info.

---

### Gap 13: Audio Codec Details

**What Linux shows:** ALSA codec name (e.g., "Realtek ALC1220"), PCI bus address from sysfs symlink.

**Windows status:** Device name and manufacturer from WMI. No codec name.

**Blocking on:** Nothing major — codec info could come from registry `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices` or Core Audio APIs.

**Impact:** Minor — audio codec name missing.

---

### Gap 14: Super I/O Chip Detection

**What Linux shows:** Detected Super I/O chip model (e.g., "NCT6798D"), chip ID, HWM base address, kernel driver loaded status. Displayed in the Motherboard section.

**Windows status:** Not available.

**Blocking on:** WinRing0 port I/O for LPC config port probing (ports 0x2E/0x4E). See PHASE_ALPHA.md Section 2.

**Impact:** No Super I/O identification in motherboard output.

---

## Gap Severity Summary

### Critical (affects core monitoring use case)

| Gap | Description | Path to Fix |
|-----|-------------|-------------|
| 1 | No hwmon temperatures/fans/voltages | WinRing0 driver integration |
| 2 | No RAPL power metering | WinRing0 MSR access |
| 5 | No network IPs/MAC/speed | GetAdaptersAddresses API (no driver needed) |

### Significant (affects specific hardware or users)

| Gap | Description | Path to Fix |
|-----|-------------|-------------|
| 3 | No AMD HSMP telemetry | WinRing0 + SMN mailbox |
| 4 | No battery data | GetSystemPowerStatus (trivial) |
| 6 | No AMD GPU sensors or display outputs | AMD ADL SDK + DXGI |
| 9 | No IPMI/BMC data | Windows IPMI driver |
| 10 | No I2C/PMBus/DIMM temps | WinRing0 SMBus |

### Minor (polish / completeness)

| Gap | Description | Path to Fix |
|-----|-------------|-------------|
| 7 | No PCIe link info | SetupAPI or WinRing0 PCI |
| 8 | No EDAC/AER/MCE errors | WHEA Event Log API |
| 11 | Sparse motherboard fields | Expanded wmic/WMI queries |
| 12 | Sparse USB device details | SetupAPI USB properties |
| 13 | No audio codec name | Core Audio API or registry |
| 14 | No Super I/O detection | WinRing0 port I/O |

---

## Quick Wins (no driver required)

These gaps can be closed without WinRing0 or any kernel driver:

1. **Battery data** — `GetSystemPowerStatus` or WMI `Win32_Battery` (Gap 4)
2. **Network IPs, MAC, speed** — `GetAdaptersAddresses` or `if-addrs` crate (Gap 5)
3. **Motherboard serial/UUID/chassis** — expanded wmic queries (Gap 11)
4. **Secure Boot detection** — `Confirm-SecureBootUEFI` or registry (Gap 11)

---

## WinRing0-Gated Features

These all require the WinRing0 kernel driver (or equivalent port I/O / MSR / PCI config access):

- SuperIO temp/fan/voltage monitoring (Gaps 1, 14)
- RAPL power metering via MSR (Gap 2)
- AMD HSMP via SMN PCI config writes (Gap 3)
- I2C/PMBus VRM and DDR5 DIMM temps (Gap 10)
- PCIe link info via PCI config space (Gap 7)
- AER error counters via PCI extended config (Gap 8)

WinRing0 is a single dependency that unlocks the majority of remaining sensor gaps. It is BSD-licensed and used by HWiNFO, HWMonitor, AIDA64, and LibreHardwareMonitor.

---

## Output Format Parity

| Format | Linux | Windows | Notes |
|--------|-------|---------|-------|
| Text (`sio`) | Full | Working, with gaps above | Admin hint instead of root hint |
| JSON (`sio --format json`) | Full | Working | Empty fields serialize as null |
| XML (`sio --format xml`) | Full | Working | Same empty-field behavior |
| CSV logging (`sio --tui --log`) | Full | Working | Logs only available sensors |
| TUI dashboard (`sio --tui`) | Full | Working | Fewer sensor panels populated |

---

## Build and Release Artifact

```
Target:    x86_64-pc-windows-msvc
Binary:    target/x86_64-pc-windows-msvc/release/sio.exe
Toolchain: rustc 1.94.0, cargo 1.94.0
Profile:   release (opt-level=z, LTO, strip, panic=abort)
```

The binary runs standalone with no runtime dependencies beyond system DLLs. NVML requires `nvml.dll` (ships with NVIDIA driver). SMART access requires administrator privileges.

---

## Recommendation

The immediate next phase should:

1. Close the **quick wins** (battery, network details, motherboard fields) — no blockers, ~2-3 days
2. Evaluate **WinRing0 integration** as a single effort that unlocks ~60% of remaining sensor gaps
3. Defer **IPMI**, **AMD HSMP**, and **I2C/PMBus** to a later phase as they require specific hardware for testing
