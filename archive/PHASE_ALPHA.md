# PHASE ALPHA: Windows Sensor & Hardware Parity Plan

**Repo:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Target:** `x86_64-pc-windows-msvc`
**Date:** 2026-03-16

---

## Current State

The Windows port compiles and runs. The following are functional:

| Area | Source | Working |
|------|--------|---------|
| CPU brand, cache, features | CPUID (native x86) | Yes |
| CPU topology (cores/threads/SMT) | sysinfo | Yes |
| CPU frequency (current) | sysinfo | Yes |
| Memory total/available/swap | sysinfo | Yes |
| Storage enumeration (drive letter, label, capacity, SSD/HDD) | sysinfo | Yes |
| Network adapter names | sysinfo | Yes |
| Motherboard vendor/product, BIOS version | wmic | Yes |
| Hostname, OS name, kernel version | env + registry + cmd | Yes |
| **Sensor:** per-core CPU utilization | sysinfo | Yes |
| **Sensor:** disk read/write throughput | sysinfo | Yes |
| **Sensor:** network RX/TX throughput | sysinfo | Yes |

Everything below is **not implemented on Windows** and requires work.

---

## Gap Analysis

Each section maps a Linux source to the data it provides, the Linux mechanism, and
the candidate Windows mechanisms. Effort estimates are relative: **Low** = days,
**Medium** = 1-2 weeks, **High** = 2-4 weeks, **Very High** = month+.

---

### 1. Hardware Monitor Temperatures, Fans, Voltages (hwmon)

**Linux source:** `src/sensors/hwmon.rs`
**Linux mechanism:** `/sys/class/hwmon/hwmon*/` sysfs interface

| Metric | Linux sysfs file | Units |
|--------|-----------------|-------|
| Temperature (CPU, chipset, VRM, ambient) | `temp*_input` | milli-C |
| Fan speed | `fan*_input` | RPM |
| Voltage rails | `in*_input` | mV |
| Power draw (package, rail) | `power*_input` or `power*_average` | uW |
| Current draw | `curr*_input` | mA |

**Windows options:**

| Option | Mechanism | Pros | Cons |
|--------|-----------|------|------|
| **WMI `MSAcpi_ThermalZoneTemperature`** | WMI query via COM/PowerShell | No driver needed, ships with Windows | Only ACPI thermal zones -- rarely exposes VRM, chipset, or per-core temps |
| **Open Hardware Monitor / LibreHardwareMonitor** | .NET library, reads via kernel driver `WinRing0` | Comprehensive (SuperIO, CPU, GPU, storage temps, fans, voltages) | Requires bundling or referencing an external .NET assembly; WinRing0 driver must be installed or embedded; GPLv2 license |
| **HWiNFO Shared Memory** | Read HWiNFO's shared-memory segment when HWiNFO is running | Very comprehensive, many sensors | Requires HWiNFO to be running; not standalone |
| **Direct port I/O via WinRing0/inpout32** | Port I/O driver for SuperIO chip access | Full SuperIO register access identical to Linux `/dev/port` path | Requires a signed kernel driver; Windows 11 HVCI may block; must ship or install the driver |
| **UEFI runtime variable reading** | Windows UEFI variable API | Some BIOS-exposed thermal data | Very limited scope |

**Recommended path:**
Integrate with LibreHardwareMonitor's C# library via a thin C bridge, or port the
SuperIO register-reading logic from `src/sensors/superio/` using a port I/O driver
(WinRing0 or a custom signed driver). The SuperIO chip detection code in
`chip_detect.rs`, `nct67xx.rs`, and `ite87xx.rs` is chip-level register logic that
is OS-agnostic once you have port I/O access.

**Effort:** High (driver packaging) or Medium (if depending on LibreHardwareMonitor)

---

### 2. Super I/O Direct Register Access (superio/)

**Linux source:** `src/sensors/superio/chip_detect.rs`, `nct67xx.rs`, `ite87xx.rs`
**Linux mechanism:** `/dev/sinfo_io` kernel module (atomic) or `/dev/port` (fallback)

| Chip Family | Chips Supported | Data Collected |
|-------------|-----------------|----------------|
| Nuvoton NCT67xx | NCT6775, 6776, 6779, 6791-6799 | 18 voltage channels, 7+ temps (SYSTIN, CPUTIN, AUXTIN0-3, PECI), 7 fan tachs |
| ITE IT87xx | IT8686E, IT8688E, IT8689E | 13 voltage channels, 3 temps, 6 fan tachs |

The register-level code in `nct67xx.rs` and `ite87xx.rs` is **platform-agnostic** --
it only needs `read_byte(port)` and `write_byte(port, value)` on x86 I/O ports.

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **WinRing0 (`WinRing0x64.sys`)** | Kernel driver providing `inb()`/`outb()` from userspace | Used by HWiNFO, HWMonitor, AIDA64, LibreHardwareMonitor. Must be installed or embedded. Signed driver available. |
| **inpout32 / inpoutx64** | Alternative port I/O driver | Smaller footprint, but older and less maintained |
| **Custom signed driver** | Write a minimal driver exposing port I/O via DeviceIoControl | Full control, but requires EV code signing or WHQL |

**Implementation plan:**
1. Create `src/platform/port_io_win.rs` wrapping WinRing0 (`Ols_ReadIoPortByte`, `Ols_WriteIoPortByte`)
2. The existing `HwmAccess` enum in `sinfo_io.rs` already abstracts this -- add a `WinRing0` variant
3. `nct67xx.rs` and `ite87xx.rs` use `HwmAccess` -- they work unmodified once port I/O is available

**Effort:** Medium (driver integration + testing across boards)

---

### 3. Intel RAPL Energy/Power Monitoring (rapl)

**Linux source:** `src/sensors/rapl.rs`
**Linux mechanism:** `/sys/class/powercap/intel-rapl:*` (energy_uj counters)

| Metric | Domain | Units |
|--------|--------|-------|
| Package power | `package-0`, `package-1` | Watts (derived from energy delta / time) |
| DRAM power | `dram` | Watts |
| Core power | `core` (sub-domain) | Watts |
| Uncore power | `uncore` (sub-domain) | Watts |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **MSR 0x611 (PKG_ENERGY_STATUS)** | Read MSR via kernel driver | Exact same data as Linux RAPL sysfs; needs ring-0 access |
| **Intel Power Gadget API** | Intel's official Windows library | Provides package/core/DRAM power. Deprecated but still functional on many systems |
| **WinRing0 MSR access** | `Ols_Rdmsr()` function | Read MSR_PKG_ENERGY_STATUS (0x611), MSR_DRAM_ENERGY_STATUS (0x619), MSR_PP0_ENERGY_STATUS (0x639) directly |
| **AMD equivalent: HSMP / MSR** | See section 5 below | AMD uses HSMP or RAPL-equivalent MSRs |

**MSR registers needed:**

| MSR | Address | Content |
|-----|---------|---------|
| MSR_RAPL_POWER_UNIT | 0x606 | Energy units (bits 12:8), power units (bits 3:0), time units (bits 19:16) |
| MSR_PKG_ENERGY_STATUS | 0x611 | Package cumulative energy counter |
| MSR_DRAM_ENERGY_STATUS | 0x619 | DRAM cumulative energy counter |
| MSR_PP0_ENERGY_STATUS | 0x639 | Core domain energy counter |
| MSR_PP1_ENERGY_STATUS | 0x641 | Uncore/GPU domain energy counter (Intel) |

**Implementation plan:**
1. Add MSR reading via WinRing0 `Ols_Rdmsr()`
2. Port the delta-based power calculation from `rapl.rs` (trivial once MSR reads work)

**Effort:** Low-Medium (if WinRing0 already integrated for SuperIO)

---

### 4. Per-Core CPU Frequency (cpu_freq)

**Linux source:** `src/sensors/cpu_freq.rs`
**Linux mechanism:** `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq`

| Metric | Units |
|--------|-------|
| Per-logical-core current frequency | MHz |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **`CallNtPowerInformation(ProcessorInformation)`** | `ntdll.dll` / `powrprof.dll` | Returns `PROCESSOR_POWER_INFORMATION` per core with `CurrentMhz` and `MaxMhz`. No driver needed. |
| **MSR 0x198 (IA32_PERF_STATUS)** | WinRing0 MSR read | Raw P-state ratio; multiply by bus clock for actual MHz |
| **sysinfo crate `cpu.frequency()`** | Already available | Reports a single frequency per core; may not update in real-time on all systems |

**Recommended path:**
Use `CallNtPowerInformation` from `powrprof.dll` -- it's a documented Windows API
that returns per-core MHz without any driver. Fall back to sysinfo if unavailable.

**Effort:** Low

---

### 5. AMD HSMP Telemetry (hsmp)

**Linux source:** `src/sensors/hsmp.rs`
**Linux mechanism:** `/dev/hsmp` ioctl (AMD kernel module)

| Metric | HSMP Command | Units |
|--------|-------------|-------|
| Socket power | GET_SOCKET_POWER (0x04) | Watts |
| Socket power limit | GET_SOCKET_POWER_LIMIT (0x06) | Watts |
| SVI rail power | GET_RAILS_SVI (0x1B) | Watts |
| Fabric clock (FCLK) | GET_FCLK_MCLK (0x0F) | MHz |
| Memory clock (MCLK) | GET_FCLK_MCLK (0x0F) | MHz |
| Core clock throttle limit | GET_CCLK_THROTTLE_LIMIT (0x10) | MHz |
| C0 residency | GET_C0_PERCENT (0x11) | Percent |
| DDR bandwidth (max/used/util) | GET_DDR_BANDWIDTH (0x14) | GB/s / Percent |
| Fmax / Fmin | GET_SOCKET_FMAX_FMIN (0x1C) | MHz |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **AMD uProf** | AMD's official profiling tool | Exposes some SMU telemetry; SDK is not public |
| **SMN register access** | PCI config space (bus 0, dev 0, fn 0, reg B8/BC) + WinRing0 | HSMP messages go via SMN (System Management Network). On Linux, `/dev/hsmp` is a thin wrapper around SMN mailbox writes. Direct SMN access is possible via PCI config space if you know the mailbox addresses. |
| **Ryzen Master SDK** | AMD's overclock utility | Undocumented internal APIs; reverse-engineering required |
| **ZenStates-Core** | Open-source C# library (used by ZenTimings, CTR) | Reads SMU mailboxes via WinRing0 PCI config writes; community-maintained; covers Zen 2-5 |

**Recommended path:**
Study ZenStates-Core for the SMU mailbox protocol. The HSMP message IDs used in
`hsmp.rs` are documented in AMD's PPR (Processor Programming Reference). The
transport is SMN PCI config space writes which WinRing0 can do via
`Ols_WritePciConfigDword` / `Ols_ReadPciConfigDword`.

**Effort:** High (AMD platform-specific, requires per-generation testing)

---

### 6. IPMI/BMC Sensor Access (ipmi)

**Linux source:** `src/sensors/ipmi.rs`
**Linux mechanism:** `/dev/ipmiN` ioctl via `ipmi-rs` crate

| Metric | Source | Units |
|--------|--------|-------|
| DIMM temperatures | BMC SDR | Celsius |
| PSU power/current/voltage | BMC SDR | Watts/Amps/Volts |
| Ambient temperature probes | BMC SDR | Celsius |
| Fan RPMs (labeled) | BMC SDR | RPM |
| Per-rail VRM voltages | BMC SDR | Volts |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **Windows IPMI driver** | `\\.\IPMI` device via DeviceIoControl | Windows ships an IPMI driver (`ipmidrv.sys`) on server SKUs. Works with `IOCTL_IPMI_SUBMIT_RAW_REQUEST`. Same raw IPMI commands as Linux. |
| **ipmitool for Windows** | CLI tool, parse output | Functional but slow; not ideal for polling |
| **Direct KCS/SMIC interface** | Port I/O to BMC interface | Very low-level; not recommended |

**Implementation plan:**
1. Create `src/platform/ipmi_win.rs` that opens `\\.\IPMI` and sends raw IPMI requests
2. Reuse the SDR parsing and sensor linearization logic from `ipmi.rs` (it's protocol-level, not OS-specific)
3. The `ipmi-rs` crate's `File` transport is Linux-only, but the `ipmi-rs-core` crate's protocol types compile everywhere

**Effort:** Medium (server boards only; IPMI driver availability varies)

---

### 7. I2C/SMBus Sensors (i2c/)

**Linux source:** `src/sensors/i2c/smbus_io.rs`, `bus_scan.rs`, `pmbus.rs`, `spd5118.rs`
**Linux mechanism:** `/dev/i2c-N` ioctl

#### 7a. PMBus VRM Controllers

| Metric | PMBus Register | Units |
|--------|---------------|-------|
| VRM input voltage | 0x88 (VIN) | Volts (LINEAR11) |
| VRM input current | 0x89 (IIN) | Amps (LINEAR11) |
| VRM output voltage | 0x8B (VOUT) | Volts (LINEAR16) |
| VRM output current | 0x8C (IOUT) | Amps (LINEAR11) |
| VRM temperature 1 | 0x8D (TEMP1) | Celsius (LINEAR11) |
| VRM temperature 2 | 0x8E (TEMP2) | Celsius (LINEAR11) |
| VRM output power | 0x96 (POUT) | Watts (LINEAR11) |
| VRM input power | 0x97 (PIN) | Watts (LINEAR11) |

#### 7b. DDR5 DIMM Temperature (SPD5118)

| Metric | Register | Units |
|--------|----------|-------|
| DIMM temperature | MR31 (0x31) | Celsius (13-bit signed, 0.0625 C/LSB) |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **WinRing0 SMBus access** | Direct SMBus controller register access via PCI config + port I/O | Must implement SMBus transaction protocol for the specific host controller (AMD FCH / Intel PCH). Complex but proven by HWiNFO. |
| **Windows I2C/SPB driver** | `SPBCx` framework | Only for hardware with a Microsoft-provided SPB driver; rare for desktop SMBus |
| **LibreHardwareMonitor** | .NET library | Already implements AMD FCH and Intel PCH SMBus access |

**Implementation plan:**
The PMBus register format decoders (`LINEAR11`, `LINEAR16`) in `pmbus.rs` are pure
math -- fully portable. The bottleneck is SMBus I/O. Options:
1. Implement AMD FCH SMBus host controller access (PCI BAR at bus 0, dev 20, fn 0, offset 10h) using WinRing0 PCI reads
2. Implement Intel PCH I801 SMBus similarly
3. The bus scan logic in `bus_scan.rs` and address probing in `pmbus.rs` port directly

**Effort:** High (host-controller-specific SMBus protocol implementation)

---

### 8. NVMe SMART/Health Data (nvme_ioctl)

**Linux source:** `src/platform/nvme_ioctl.rs`
**Linux mechanism:** `/dev/nvmeN` ioctl `NVME_IOCTL_ADMIN_CMD`

| Metric | Log Offset | Units |
|--------|-----------|-------|
| Temperature | 1-2 | Celsius (Kelvin - 273) |
| Available spare | 3 | Percent |
| Percentage used | 5 | Percent |
| Data units read | 8-23 | 512KB units (u128) |
| Data units written | 24-39 | 512KB units (u128) |
| Power cycles | 88-103 | Count (u128) |
| Power-on hours | 104-119 | Hours (u128) |
| Unsafe shutdowns | 120-135 | Count (u128) |
| Media errors | 136-151 | Count (u128) |
| Warning temp time | 168-171 | Minutes |
| Critical temp time | 172-175 | Minutes |
| Temp sensors 1-8 | 176-191 | Kelvin |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **`IOCTL_STORAGE_QUERY_PROPERTY` with `StorageAdapterProtocolSpecificProperty`** | `DeviceIoControl` on `\\.\PhysicalDriveN` | Windows 10+ native API for NVMe SMART. Sends NVMe Admin Get Log Page internally. Well-documented by Microsoft. |
| **`nvme-cli` for Windows** | CLI tool | Functional but not library-friendly |
| **Direct NVMe via `CreateFile` + `DeviceIoControl`** | SCSI pass-through to NVMe | More complex but gives full NVMe admin command access |

**Implementation plan:**
1. Create `src/platform/nvme_win.rs`
2. Open `\\.\PhysicalDriveN` with `CreateFile`
3. Use `IOCTL_STORAGE_QUERY_PROPERTY` + `PropertyId = StorageDeviceProtocolSpecificProperty`
4. Protocol: `ProtocolTypeNvme`, DataType: `NVMeDataTypeLogPage`, LogID: 2 (SMART)
5. Parse the returned 512-byte SMART log using the same `NvmeSmartLog` struct (it's
   the same NVMe spec data on both OSes)

**Effort:** Medium (well-documented Windows API; main work is enumerate + open drives)

---

### 9. SATA SMART Data (sata_ioctl)

**Linux source:** `src/platform/sata_ioctl.rs`
**Linux mechanism:** `/dev/sdX` ioctl `SG_IO` with ATA PASS-THROUGH(12)

| Metric | SMART Attr ID | Units |
|--------|--------------|-------|
| Temperature | 190, 194 | Celsius |
| Reallocated sectors | 5 | Count |
| Power-on hours | 9 | Hours |
| Power cycle count | 12 | Count |
| Pending sectors | 197 | Count |
| Uncorrectable sectors | 198 | Count |
| Total LBAs written | 241 | Sectors x 512 |
| Total LBAs read | 242 | Sectors x 512 |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **`SMART_RCV_DRIVE_DATA` (`IOCTL_SMART_RCV_DRIVE_DATA`)** | `DeviceIoControl` on `\\.\PhysicalDriveN` | Legacy but universally supported. Returns the 512-byte SMART data page. Requires `SENDCMDINPARAMS` struct. |
| **ATA PASS-THROUGH via `IOCTL_ATA_PASS_THROUGH`** | `DeviceIoControl` | More flexible; sends arbitrary ATA commands. Same approach as Linux SG_IO. |
| **WMI `MSStorageDriver_ATAPISmartData`** | WMI query | Returns raw SMART data blob; parsing is the same |

**Implementation plan:**
1. Create `src/platform/sata_win.rs`
2. Open `\\.\PhysicalDriveN` with `CreateFile`
3. Issue `IOCTL_SMART_RCV_DRIVE_DATA` with feature 0xD0 (SMART READ DATA)
4. Parse the returned 512-byte SMART page using the existing `AtaSmartAttribute`
   struct and `sata_smart_to_smart_data()` converter -- they're pure data parsing

**Effort:** Medium (well-documented; attribute parsing code is fully reusable)

---

### 10. GPU Sensors -- AMD sysfs + NVIDIA NVML (gpu_sensors)

**Linux source:** `src/sensors/gpu_sensors.rs`
**Linux mechanism:** NVML (dynamic load) + AMD sysfs hwmon

#### NVIDIA (NVML)

| Metric | NVML Function | Units |
|--------|--------------|-------|
| Temperature | `nvmlDeviceGetTemperature` | Celsius |
| Fan speed | `nvmlDeviceGetFanSpeed` | Percent |
| Power | `nvmlDeviceGetPowerUsage` | Watts |
| Core clock | `nvmlDeviceGetClockInfo(GRAPHICS)` | MHz |
| Memory clock | `nvmlDeviceGetClockInfo(MEM)` | MHz |
| GPU utilization | `nvmlDeviceGetUtilizationRates` | Percent |
| Memory utilization | `nvmlDeviceGetUtilizationRates` | Percent |
| VRAM used | `nvmlDeviceGetMemoryInfo` | MB |

**Windows status:** NVML ships with the NVIDIA driver on Windows (`nvml.dll`).
The existing `src/platform/nvml.rs` uses `libloading` which is cross-platform.
The NVML wrapper should work on Windows with minimal changes: change the library
name from `libnvidia-ml.so.1` to `nvml.dll`.

**Effort:** Low (library name change + test)

#### AMD (sysfs hwmon)

On Linux, AMD GPU sensors come from `/sys/class/drm/cardN/device/hwmon/`.
On Windows, options are:

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **AMD ADL (AMD Display Library)** | `atiadlxx.dll` (ships with AMD driver) | Temperature, fan speed, clocks, VRAM usage, power. Official AMD SDK. |
| **AMD ADLX (newer API)** | `amdadlx64.dll` | Successor to ADL; better GPU metrics API |
| **WMI** | `Win32_VideoController` | Very basic -- no temps or clocks |

**Implementation plan:**
1. Create `src/platform/adl.rs` wrapping AMD ADL/ADLX via `libloading`
2. Map ADL sensor types to the existing `SensorReading` model

**Effort:** Medium (ADL SDK is documented but large)

---

### 11. Machine Check Exceptions (mce)

**Linux source:** `src/sensors/mce.rs`
**Linux mechanism:** `/sys/devices/system/machinecheck/machinecheck0/bank*`

| Metric | Units |
|--------|-------|
| Per-bank correctable error count | Count |
| Per-bank uncorrectable error count | Count |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **Windows Event Log (WHEA)** | `wevtutil` or ETW tracing for `Microsoft-Windows-WHEA-Logger` | Windows logs MCEs as WHEA events. Query via Event Log API or PowerShell `Get-WinEvent`. |
| **MSR 0x179 (IA32_MCG_STATUS)** + **MSR 0x401+ (IA32_MCi_STATUS)** | WinRing0 MSR read | Direct MCA bank status register reading; same data as Linux sysfs exposes |

**Recommended path:**
Read WHEA events from the Windows Event Log -- it's the official mechanism and
doesn't require a driver. For real-time monitoring, use ETW (Event Tracing for
Windows) to subscribe to WHEA events.

**Effort:** Medium

---

### 12. EDAC Memory Error Counters (edac)

**Linux source:** `src/sensors/edac.rs`
**Linux mechanism:** `/sys/devices/system/edac/mc/*/`

| Metric | Units |
|--------|-------|
| Correctable errors (CE) per MC, per rank | Count |
| Uncorrectable errors (UE) per MC, per rank | Count |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **WHEA Event Log** | Same as MCE above | Memory CE/UE events are logged as WHEA corrected/uncorrected events |
| **IPMI (if BMC present)** | See section 6 | BMC often tracks DIMM error counts in SDR |
| **WMI `Win32_PhysicalMemoryArray`** | WMI query | Has `MemoryErrorCorrection` field but no runtime counters |

**Effort:** Low-Medium (WHEA events cover this)

---

### 13. PCIe AER Error Counters (aer)

**Linux source:** `src/sensors/aer.rs`
**Linux mechanism:** `/sys/bus/pci/devices/*/aer_dev_*`

| Metric | Units |
|--------|-------|
| Correctable AER errors per PCI device | Count |
| Non-fatal AER errors per PCI device | Count |
| Fatal AER errors per PCI device | Count |

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **WHEA Event Log** | ETW / Event Log | PCIe AER errors are logged as WHEA events with PCI source |
| **PCI config space read (AER capability)** | WinRing0 PCI config access | Read AER Extended Capability (offset 0x100+) directly |

**Effort:** Medium

---

### 14. Collectors: PCI, USB, Audio Device Enumeration

#### PCI Devices

**Linux:** `/sys/bus/pci/devices/*` + pci_ids crate

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **SetupAPI `SetupDiGetClassDevs`** | Windows DDK API | Full PCI device enumeration with vendor/device IDs |
| **WMI `Win32_PnPEntity`** | WMI query | Simpler but less detailed |
| **`pci_ids` crate** | Already a dependency | Name resolution works cross-platform once you have vendor/device IDs |

**Effort:** Medium

#### USB Devices

**Linux:** `/sys/bus/usb/devices/*`

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **SetupAPI `GUID_DEVINTERFACE_USB_DEVICE`** | Windows DDK API | Full USB enumeration with VID/PID, speed, power |
| **WMI `Win32_USBHub` + `Win32_USBControllerDevice`** | WMI query | Simpler |

**Effort:** Medium

#### Audio Devices

**Linux:** `/proc/asound/cards`

**Windows options:**

| Option | Mechanism | Notes |
|--------|-----------|-------|
| **MMDevice API (`IMMDeviceEnumerator`)** | Windows Core Audio | Standard Windows audio enumeration |
| **WMI `Win32_SoundDevice`** | WMI query | Simpler, less detail |

**Effort:** Low-Medium

---

## Implementation Priority

### Tier 1 -- High impact, unblocks the TUI dashboard

These give the most visible improvement to the monitoring dashboard:

| Item | Section | Effort | Impact |
|------|---------|--------|--------|
| NVIDIA NVML on Windows | 10 | Low | GPU temps, clocks, power, utilization in dashboard |
| NVMe SMART | 8 | Medium | Drive health, temperature in dashboard |
| Per-core CPU frequency | 4 | Low | Frequency column in dashboard |
| SATA SMART | 9 | Medium | HDD health, temperature |

### Tier 2 -- Requires driver, unlocks hardware sensors

| Item | Section | Effort | Impact |
|------|---------|--------|--------|
| WinRing0 port I/O integration | 2 | Medium | Prerequisite for SuperIO + RAPL |
| SuperIO temps/fans/voltages | 1, 2 | Medium | CPU, system, VRM temps and fan speeds |
| RAPL power metering | 3 | Low-Med | CPU/DRAM power in dashboard |
| AMD ADL GPU sensors | 10 | Medium | AMD GPU temps, clocks, power |

### Tier 3 -- Advanced / server features

| Item | Section | Effort | Impact |
|------|---------|--------|--------|
| I2C/PMBus VRM monitoring | 7 | High | Per-rail VRM telemetry |
| IPMI BMC sensors | 6 | Medium | Server board telemetry |
| AMD HSMP telemetry | 5 | High | AMD-specific power/freq data |
| WHEA/MCE error tracking | 11, 12, 13 | Medium | Error monitoring |

### Tier 4 -- Device enumeration polish

| Item | Section | Effort | Impact |
|------|---------|--------|--------|
| PCI device enumeration | 14 | Medium | PCIe link info, device listing |
| USB device enumeration | 14 | Medium | USB device listing |
| Audio device enumeration | 14 | Low-Med | Audio device listing |
| DDR5 DIMM temps (SPD5118) | 7b | High | Per-DIMM temperature |

---

## Key Dependencies

| Dependency | Used By | License | Notes |
|------------|---------|---------|-------|
| **WinRing0** (kernel driver) | SuperIO, RAPL, HSMP, I2C | BSD | Required for any direct hardware register access. Must be distributed with the binary or installed separately. |
| **nvml.dll** | NVIDIA GPU sensors | Proprietary (ships with driver) | Already installed on systems with NVIDIA GPUs |
| **atiadlxx.dll** / **amdadlx64.dll** | AMD GPU sensors | Proprietary (ships with driver) | Already installed on systems with AMD GPUs |
| **powrprof.dll** | Per-core frequency | Windows system DLL | Always available |
| **ipmidrv.sys** | IPMI BMC | Windows inbox driver | Available on server SKUs |

---

## Architecture Recommendation

The existing codebase gates Linux code with `#[cfg(unix)]`. The recommended pattern
for Windows implementations:

```
src/platform/
    port_io.rs          # existing Linux /dev/port
    port_io_win.rs      # NEW: WinRing0 wrapper (cfg(windows))
    nvme_ioctl.rs       # existing Linux ioctl
    nvme_win.rs         # NEW: Windows DeviceIoControl (cfg(windows))
    sata_ioctl.rs       # existing Linux SG_IO
    sata_win.rs         # NEW: Windows SMART ioctl (cfg(windows))
    nvml.rs             # existing (needs library name fix for Windows)
    adl.rs              # NEW: AMD ADL wrapper (cfg(windows))
```

Each `*_win.rs` module should expose the same public types and functions as its
Linux counterpart so that the sensor and collector code can use them uniformly
via `#[cfg]` imports.

---

## Testing Matrix

Every Windows sensor implementation must be tested on:

| Platform | CPU | Chipset | SuperIO | GPU | Notes |
|----------|-----|---------|---------|-----|-------|
| AMD AM5 desktop | Zen 4/5 | X670E/B650 | NCT6799/IT8689 | NVIDIA + AMD | Most common enthusiast platform |
| AMD TRX50/WRX90 workstation | Zen 4 TR | WRX90 | NCT6798 | NVIDIA | Server-class with IPMI |
| Intel LGA1700 desktop | 12th-14th Gen | Z690/Z790 | NCT6799 | NVIDIA + Intel Arc | Intel RAPL + SuperIO |
| Intel LGA1851 desktop | Core Ultra 200S | Z890 | NCT67xx | NVIDIA | Latest Intel platform |
| Laptop (Intel) | Any | PCH | N/A | iGPU + dGPU | Battery, thermal zones, no SuperIO |
| Laptop (AMD) | Zen 3/4 | FCH | N/A | iGPU + dGPU | HSMP unavailable on mobile |

---

## Implementation Status (2026-03-16)

### Completed

| Item | Section | Commit | Verified |
|------|---------|--------|----------|
| NVIDIA NVML on Windows | 10 | `ac67937` | GPU temp 37C, fan 24%, clocks, utilization, VRAM on GTX 1650 |
| Per-core CPU frequency | 4 | `ac67937` | 64 cores at 3501 MHz via CallNtPowerInformation |
| NVMe SMART data | 8 | `2837f42` | nvme_win.rs via IOCTL_STORAGE_QUERY_PROPERTY (requires admin) |
| SATA SMART data | 9 | `2837f42` | sata_win.rs via SMART_RCV_DRIVE_DATA (requires admin) |
| PCI device enumeration | 14 | `78d2aa6` | 87 PCI devices with pci_ids name resolution |
| USB device enumeration | 14 | `78d2aa6` | 10 USB devices with VID/PID |
| Audio device enumeration | 14 | `78d2aa6` | 2 audio devices (Realtek USB, NVIDIA HD Audio) |

### Remaining (Tier 2-3, requires WinRing0 driver or external dependencies)

| Item | Section | Blocker |
|------|---------|---------|
| SuperIO temps/fans/voltages | 1, 2 | Requires WinRing0 kernel driver for port I/O |
| RAPL power metering | 3 | Requires WinRing0 for MSR access |
| AMD HSMP telemetry | 5 | Requires WinRing0 + AMD platform-specific testing |
| IPMI BMC sensors | 6 | Requires server board with ipmidrv.sys |
| I2C/PMBus VRM monitoring | 7 | Requires SMBus host controller implementation |
| AMD ADL GPU sensors | 10 | Requires AMD GPU + ADL SDK integration |
| WHEA/MCE error tracking | 11, 12, 13 | Requires Windows Event Log API integration |
