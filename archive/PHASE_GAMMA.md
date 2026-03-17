# PHASE GAMMA: Driver-Gated Hardware Sensors

**Repo:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Predecessor:** PHASE_BETA (completed 2026-03-16)
**Goal:** Close the final 7 parity gaps by integrating kernel-mode I/O access (WinRing0) and vendor GPU libraries (AMD ADL), enabling full motherboard sensor monitoring, CPU power metering, and PCIe link interrogation on Windows.

---

## Context

PHASE ALPHA delivered the Windows port foundation (CPUID, sysinfo, NVML, SMART,
device enumeration — 150 sensors). PHASE BETA closed all user-mode gaps (battery,
network details, motherboard fields, USB details, audio codec, display outputs, WHEA
errors, IPMI — 154 sensors). Every remaining gap requires either:

1. **Direct hardware register access** (I/O ports, MSRs, PCI config space) — needs a
   kernel-mode driver like WinRing0
2. **Vendor GPU library** (AMD ADL/ADLX) — needs the AMD display driver

The key insight from the BETA exit report: **WinRing0 is the single dependency that
unlocks 6 of 7 remaining gaps.** It provides port I/O, MSR reads, and PCI config
space access — the three primitives that all remaining sensor sources need.

---

## Prerequisite: WinRing0 Integration

### What is WinRing0

WinRing0 is a BSD-licensed kernel driver (`WinRing0x64.sys`) that exposes direct
hardware access from userspace via a DeviceIoControl interface. It is used by
HWiNFO, HWMonitor, AIDA64, LibreHardwareMonitor, and many other tools.

**Capabilities:**
- `Ols_ReadIoPortByte(port)` / `Ols_WriteIoPortByte(port, val)` — x86 IN/OUT
- `Ols_Rdmsr(index, &eax, &edx)` — read Model-Specific Registers
- `Ols_ReadPciConfigDword(pci_addr, reg)` / `Ols_WritePciConfigDword(...)` — PCI config

**Distribution options:**
1. Bundle `WinRing0x64.sys` + `WinRing0x64.dll` with `sio.exe` (simplest)
2. Use a Rust FFI wrapper loading `WinRing0x64.dll` at runtime via `libloading`
3. Implement the ioctl protocol directly against the driver without the DLL

**License:** BSD — compatible with MIT (siomon's license)

### Driver signing concerns

- WinRing0 has a valid Authenticode signature on most distributions
- Windows 11 with HVCI (Hypervisor-enforced Code Integrity) may block unsigned
  kernel drivers; the user may need to disable Memory Integrity temporarily
- Alternative: use a WHQL-signed equivalent (e.g., from LibreHardwareMonitor)

---

## Work Items

### G1. WinRing0 Port I/O Integration

**Exit Report Gaps:** 1, 14
**Severity:** Critical prerequisite — blocks G2, G3, G4, G5, G6
**Effort:** Medium (3-4 days)

Create `src/platform/port_io_win.rs` that wraps WinRing0's I/O port access.

**API to implement (matching Linux `PortIo` interface):**

```rust
pub struct PortIo { /* WinRing0 handle */ }

impl PortIo {
    pub fn open() -> Option<Self>;
    pub fn read_byte(&self, port: u16) -> io::Result<u8>;
    pub fn write_byte(&self, port: u16, val: u8) -> io::Result<()>;
    pub fn write_read(&self, write_port: u16, write_val: u8, read_port: u16) -> io::Result<u8>;
    pub fn is_available() -> bool;
}
```

**Implementation approach:**
1. Load `WinRing0x64.dll` via `libloading` at runtime
2. Call `InitializeOls()` on first use
3. Wrap `ReadIoPortByte` / `WriteIoPortByte` functions
4. Return `None` from `open()` if DLL not found or initialization fails

**Also create `src/platform/sinfo_io_win.rs`** with a Windows `HwmAccess` variant:

```rust
pub enum HwmAccess {
    #[cfg(unix)]  KernelModule(SinfoIo),
    #[cfg(unix)]  DevPort(PortIo),
    #[cfg(windows)] WinRing0(WinRing0PortIo),
}
```

Or simpler: just add `#[cfg(windows)]` to the existing `HwmAccess::open()` to
try `PortIo` (WinRing0-backed) on Windows.

**Acceptance criteria:**
- `PortIo::open()` succeeds when WinRing0 DLL + driver are present
- `PortIo::is_available()` returns false when WinRing0 not installed
- `chip_detect::detect_all()` works on Windows (probes 0x2E/0x4E)
- `sio board` shows detected Super I/O chip on Windows

---

### G2. SuperIO Temperature/Fan/Voltage Monitoring (NCT67xx + IT87xx)

**Exit Report Gap:** 1
**Severity:** Critical — this is the primary environmental sensor source
**Effort:** Medium (2-3 days, after G1)
**Depends on:** G1

The chip-level register code in `nct67xx.rs` and `ite87xx.rs` is **already
platform-agnostic**. It only needs the `HwmAccess` / `PortIo` abstraction to work.

**What is already portable (no changes needed):**

| File | What it does | Portable? |
|------|-------------|-----------|
| `superio/chip_detect.rs` | Probe 0x2E/0x4E, read chip ID | Yes — uses only PortIo |
| `superio/nct67xx.rs` | Read 18 voltages, 7+ temps, 7 fans via banked registers | Yes — uses HwmAccess |
| `superio/ite87xx.rs` | Read 13 voltages, 3 temps, 6 fans via direct registers | Yes — uses PortIo |
| `db/voltage_scaling.rs` | Board-specific voltage multiplier lookup | Yes — pure data |
| `db/boards/*.rs` | Per-board sensor label templates | Yes — pure data |

**What needs to change:**
1. Un-gate `superio` module in `src/sensors/mod.rs` (remove `#[cfg(unix)]`)
2. Add `#[cfg(windows)]` path in `HwmAccess::open()` using WinRing0 `PortIo`
3. Register `Nct67xxSource` and `Ite87xxSource` in `poller.rs` `discover_all_sources` (Windows)
4. Un-gate `chip_detect` display in `output/text.rs` Motherboard section

**Acceptance criteria:**
- `sio sensors` shows hwmon-equivalent sensor data (temps, fans, voltages)
- `sio board` shows detected Super I/O chip
- TUI dashboard shows temperature/fan/voltage panels
- Voltage scaling applied correctly per board template

**Expected sensors (NCT6798 on WRX80E):**
- ~18 voltage readings (3.3V, 5V, 12V, VCORE, VDIMM, etc.)
- ~7 temperature readings (CPU, System, VRM, Chipset, etc.)
- ~7 fan tachometer readings (CPU fan, chassis fans, etc.)

---

### G3. Intel RAPL Power Metering via MSR

**Exit Report Gap:** 2
**Severity:** Significant
**Effort:** Low-Medium (1-2 days, after G1)
**Depends on:** G1

The Linux `rapl.rs` reads from sysfs (`/sys/class/powercap/intel-rapl:*/energy_uj`).
On Windows, the same data is available directly from MSRs.

**MSRs needed:**

| MSR | Address | Content |
|-----|---------|---------|
| MSR_RAPL_POWER_UNIT | 0x606 | Energy units (bits 12:8) |
| MSR_PKG_ENERGY_STATUS | 0x611 | Package energy counter |
| MSR_DRAM_ENERGY_STATUS | 0x619 | DRAM energy counter |
| MSR_PP0_ENERGY_STATUS | 0x639 | Core domain energy |
| MSR_PP1_ENERGY_STATUS | 0x641 | Uncore/GPU domain energy |

**Implementation plan:**
1. Create `src/platform/msr_win.rs` wrapping WinRing0's `Ols_Rdmsr()`
2. Create `src/sensors/rapl_win.rs` (or add `#[cfg(windows)]` to `rapl.rs`):
   - `discover()`: Read MSR_RAPL_POWER_UNIT to get energy unit scaling factor
   - `poll()`: Read energy MSRs, compute delta watts same as Linux
3. Register in poller

**Note:** RAPL MSRs exist on AMD Zen processors too (Family 17h+). The MSR
addresses are identical. This will work on both Intel and AMD.

**Acceptance criteria:**
- `sio sensors` shows package/DRAM/core power in watts
- Power readings update in TUI dashboard
- Correct on both Intel and AMD x86_64 platforms

---

### G4. AMD HSMP Telemetry via SMN Mailbox

**Exit Report Gap:** 3
**Severity:** Significant (AMD workstation/server only)
**Effort:** High (4-5 days, after G1)
**Depends on:** G1

On Linux, `/dev/hsmp` is a kernel driver wrapping SMN (System Management Network)
mailbox writes via PCI config space. On Windows, the same mailbox is accessible via
direct PCI config writes using WinRing0.

**SMN Mailbox Protocol (from AMD PPR):**

```
PCI Bus 0, Device 0, Function 0:
  Register 0x60 (SMN_INDEX) — write SMN address
  Register 0x64 (SMN_DATA)  — read/write SMN data

HSMP Mailbox registers (via SMN):
  0x3B10534 — HSMP_MSG_ID_REG    (write command)
  0x3B10538 — HSMP_MSG_RESP_REG  (read response)
  0x3B10998+ — HSMP_MSG_ARG_REGs (read/write args)
```

**Implementation plan:**
1. Add WinRing0 PCI config access to `port_io_win.rs`:
   `read_pci_config(bus, dev, fn, reg) -> u32`
   `write_pci_config(bus, dev, fn, reg, val)`
2. Create `src/sensors/hsmp_win.rs` implementing the SMN mailbox protocol
3. Reuse the HSMP message definitions from `hsmp.rs` (msg IDs, argument layouts)
4. Register in poller for Windows

**Sensor output (same as Linux):**
- Socket power (watts), power limit, SVI rail power
- FCLK, MCLK frequencies (MHz)
- Core clock throttle limit, C0 residency
- DDR bandwidth utilization
- Fmax/Fmin

**Acceptance criteria:**
- `sio sensors` shows HSMP telemetry on AMD Zen 3+ systems
- Socket power matches expectations (cross-reference with HWiNFO)
- Graceful failure on non-AMD or pre-Zen 3 systems

---

### G5. I2C/SMBus VRM and DIMM Temperature Monitoring

**Exit Report Gap:** 10
**Severity:** Significant (enthusiast/workstation)
**Effort:** High (5-7 days, after G1)
**Depends on:** G1

PMBus VRM controllers and DDR5 DIMM temperature sensors communicate over the SMBus.
On Linux, `/dev/i2c-N` provides access. On Windows, the SMBus host controller must
be driven directly via port I/O or PCI BAR access.

**SMBus Host Controllers:**

| Controller | Platform | PCI Location | BAR |
|------------|----------|-------------|-----|
| AMD FCH (piix4) | AMD AM4/AM5/TR | Bus 0, Dev 20, Fn 0 | BAR at offset 0x10 |
| Intel PCH (i801) | Intel LGA1200+ | Bus 0, Dev 31, Fn 4 | BAR at offset 0x20 |

**Implementation plan:**
1. Create `src/platform/smbus_win.rs` implementing SMBus transaction protocol:
   - Detect host controller via PCI vendor/device ID
   - Read BAR address via WinRing0 PCI config
   - Implement SMBus byte read/write using host controller registers
2. Port `bus_scan.rs` logic to use Windows SMBus access
3. The PMBus protocol decoders (`LINEAR11`, `LINEAR16`) in `pmbus.rs` are already
   pure math — fully portable
4. The SPD5118 temperature register reading in `spd5118.rs` is portable once SMBus
   I/O works

**Portable code (no changes needed):**
- `i2c/pmbus.rs` — PMBus register definitions, LINEAR11/LINEAR16 decoders
- `i2c/spd5118.rs` — SPD5118 MR31 temperature parsing
- `i2c/bus_scan.rs` — address probing logic (needs SMBus transport)

**Acceptance criteria:**
- `sio sensors` shows VRM input/output voltage, current, power, temperature
- DDR5 DIMM temperatures appear on DDR5 systems
- Correct readings on AMD FCH and Intel PCH platforms

---

### G6. PCIe Link Speed and Width

**Exit Report Gap:** 7
**Severity:** Minor
**Effort:** Medium (2-3 days, after G1)
**Depends on:** G1

On Linux, PCIe link info comes from sysfs (`current_link_speed`, `current_link_width`).
On Windows, this data lives in the PCI Express Capability Structure in config space.

**PCI Express Capability (offset varies per device):**

| Register | Offset from cap | Content |
|----------|----------------|---------|
| Link Capabilities | +0x0C | Max speed (bits 3:0), max width (bits 9:4) |
| Link Status | +0x12 | Current speed (bits 3:0), current width (bits 9:4) |

**Speed encoding:** 1=2.5GT/s (Gen1), 2=5GT/s (Gen2), 3=8GT/s (Gen3), 4=16GT/s (Gen4), 5=32GT/s (Gen5)

**Implementation plan:**
1. Add PCI config space extended read to `port_io_win.rs` (WinRing0 PCI access)
2. Find PCIe Capability by walking PCI Capabilities List (start at offset 0x34)
3. Read Link Capabilities and Link Status registers
4. Populate `PcieLinkInfo` struct in PCI collector

**Alternative approach (no driver needed):**
SetupAPI `DEVPKEY_PciDevice_CurrentLinkSpeed` and `DEVPKEY_PciDevice_CurrentLinkWidth`
may provide this data without WinRing0. Evaluate this first as it's simpler.

**Acceptance criteria:**
- `sio pcie` shows negotiated and max link speed/width for each PCI device
- NVMe controller shows correct Gen3/Gen4 x4 link
- GPU shows correct Gen3/Gen4 x16 link

---

### G7. AMD ADL GPU Sensors

**Exit Report Gap:** 6 (partial — AMD GPU sensors)
**Severity:** Significant (AMD GPU users)
**Effort:** Medium (3-4 days, independent of G1)
**No dependency on WinRing0**

AMD's ADL (AMD Display Library) ships as `atiadlxx.dll` with the AMD display driver.
It provides temperature, fan speed, clock frequencies, VRAM usage, and power draw —
the same data that Linux gets from sysfs hwmon.

**Implementation plan:**
1. Create `src/platform/adl.rs` using `libloading` (same pattern as `nvml.rs`)
2. Load `atiadlxx.dll` (64-bit) at runtime
3. Key ADL functions to wrap:

| ADL Function | Returns |
|-------------|---------|
| `ADL2_Main_Control_Create` | Initialize ADL context |
| `ADL2_Adapter_NumberOfAdapters_Get` | GPU count |
| `ADL2_Adapter_AdapterInfo_Get` | Adapter names, PCI info |
| `ADL2_Overdrive_Caps` | OD version (5, 6, 7, 8, N) |
| `ADL2_OverdriveN_Temperature_Get` | Temperature (Celsius) |
| `ADL2_OverdriveN_FanControl_Get` | Fan RPM/percentage |
| `ADL2_OverdriveN_PerformanceStatus_Get` | Core/mem clock, VRAM used |
| `ADL2_Overdrive6_CurrentPower_Get` | Power draw (watts) |

4. Create `src/sensors/gpu_sensors_adl.rs` implementing `SensorSource` for AMD
5. Register in poller alongside NVML source

**Sensor output:**
- AMD GPU temperature (Celsius)
- AMD GPU fan speed (RPM or %)
- AMD GPU core clock (MHz)
- AMD GPU memory clock (MHz)
- AMD GPU utilization (%)
- AMD GPU VRAM used (MB)
- AMD GPU power draw (watts)

**Acceptance criteria:**
- `sio sensors` shows AMD GPU metrics on systems with AMD GPUs
- Readings match AMD Adrenalin software values
- Systems without AMD GPUs skip gracefully (DLL not found)
- Both NVML (NVIDIA) and ADL (AMD) can coexist

---

### G8. DDR5 DIMM Temperatures via SPD5118

**Exit Report Gap:** 10
**Severity:** Minor (DDR5 enthusiast)
**Effort:** High (included in G5 — SMBus is the prerequisite)
**Depends on:** G5

Once SMBus access works (G5), the SPD5118 temperature reading code is already
portable. SPD5118 temperature sensors sit at I2C addresses 0x50-0x57 on the SMBus.

**Register:** MR31 (0x31) — 13-bit signed temperature, 0.0625 C/LSB

**Implementation:** Included in G5. If G5 delivers PMBus VRM monitoring, DDR5 DIMM
temps come essentially for free since they share the same SMBus transport.

---

## Work Item Dependencies

```
G1 (WinRing0 port I/O)
 ├── G2 (SuperIO sensors)          — needs port I/O
 ├── G3 (RAPL via MSR)             — needs MSR read
 ├── G4 (HSMP via PCI config)      — needs PCI config write
 ├── G5 (I2C/SMBus sensors)        — needs port I/O + PCI config
 │    └── G8 (DDR5 DIMM temps)     — needs SMBus
 └── G6 (PCIe link info)           — needs PCI config read

G7 (AMD ADL GPU sensors)           — INDEPENDENT (no WinRing0 needed)
```

**Critical path:** G1 → G2 (SuperIO sensors = highest user impact)

**Parallel track:** G7 (AMD ADL) can start immediately, no blockers.

---

## Implementation Order

| Priority | Item | Effort | Impact | Notes |
|----------|------|--------|--------|-------|
| 1 | G1: WinRing0 integration | Medium | Critical | Unlocks everything else |
| 2 | G2: SuperIO sensors | Medium | Critical | Temps, fans, voltages — the core sensor gap |
| 3 | G7: AMD ADL GPU sensors | Medium | Significant | Can run in parallel with G1-G2 |
| 4 | G3: RAPL power metering | Low-Med | Significant | Quick once G1 done |
| 5 | G6: PCIe link info | Medium | Minor | Useful for `sio pcie` |
| 6 | G4: AMD HSMP | High | Significant | AMD workstation/server only |
| 7 | G5+G8: I2C/SMBus + DIMM temps | High | Significant | Enthusiast feature, complex |

---

## WinRing0 Distribution Strategy

The team must decide how to package WinRing0 with `sio.exe`:

### Option A: Bundle DLL + SYS (recommended for development)

Ship `WinRing0x64.dll` and `WinRing0x64.sys` alongside `sio.exe`. The DLL handles
driver installation/loading automatically. Simple but requires files next to exe.

### Option B: Embed driver in binary

Embed the .sys file as a Rust `include_bytes!()` resource. Extract to temp directory
at runtime, load, clean up on exit. Self-contained single-exe distribution.

### Option C: Optional dependency

`sio.exe` works without WinRing0 (as it does today). If WinRing0 is detected,
additional sensors become available. Print a hint: "Install WinRing0 for SuperIO
sensor monitoring" when not found.

**Recommendation:** Start with Option C during development (graceful fallback),
move to Option A or B for release builds.

---

## Testing Matrix

| Platform | G1 | G2 | G3 | G4 | G5 | G6 | G7 |
|----------|----|----|----|----|----|----|----|
| AMD AM5 (Zen 4, B650/X670E) | Port I/O | NCT6799 or IT8689 | RAPL (AMD) | HSMP (Zen 4) | AMD FCH SMBus | PCIe Gen4/5 | AMD GPU |
| AMD WRX80 (Zen 2 TR PRO) | Port I/O | NCT6798 | RAPL (AMD) | HSMP (Zen 2) | AMD FCH SMBus | PCIe Gen4 | — |
| Intel Z690/Z790 (12-14th Gen) | Port I/O | NCT6799 | RAPL (Intel) | — | Intel I801 SMBus | PCIe Gen4/5 | — |
| Intel Z890 (Core Ultra 200S) | Port I/O | NCT67xx | RAPL (Intel) | — | Intel I801 SMBus | PCIe Gen5 | Intel Arc |
| Laptop (Intel/AMD) | May fail (HVCI) | N/A (no SuperIO) | RAPL | — | — | PCIe | Optimus/hybrid |

---

## Exit Criteria

PHASE GAMMA is complete when:

1. WinRing0 integration (G1) loads and initializes successfully
2. SuperIO sensors (G2) show temperatures, fans, and voltages matching HWiNFO
3. RAPL power (G3) shows CPU package/DRAM watts on Intel and AMD
4. AMD ADL (G7) shows GPU sensors on AMD GPU systems
5. At least one of G4/G5/G6 is implemented (HSMP, SMBus, or PCIe link)
6. `sio -m` TUI dashboard displays environmental sensors (temperature panel populated)
7. `sio.exe` still functions correctly when WinRing0 is absent (graceful fallback)
8. PHASE_GAMMA section of `phase_status.json` updated to `"completed"`

---

## Future: PHASE DELTA (potential)

If GAMMA completes all items, the remaining work would be:
- **SMBIOS table enrichment** — `GetSystemFirmwareTable` for DIMM info, chipset from PCI 00:00.0
- **Memory DIMM details** — DDR4/DDR5 module info (manufacturer, part, speed) from SMBIOS Type 17
- **CPU microcode version** — from CPUID or registry
- **Network driver name** — from WMI `Win32_NetworkAdapter.ServiceName`
- **ACPI thermal zones** — WMI `MSAcpi_ThermalZoneTemperature` as supplementary data
- **Upstream PR preparation** — rebase, squash, CI integration, cross-compile testing
