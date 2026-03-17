# PHASE BETA: Close the Windows Parity Gap

**Repo:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Predecessor:** PHASE_ALPHA (completed 2026-03-16)
**Goal:** Eliminate every user-visible difference between `sio` on Linux and `sio.exe` on Windows that does not require a kernel-mode driver.

---

## Context

PHASE ALPHA delivered a compilable, functional Windows build with CPUID, sysinfo,
NVML, NVMe/SATA SMART, WMI-based device enumeration, and 150 real-time sensors in
the TUI dashboard. The PHASE_ALPHA_EXIT_REPORT.md identified 14 gaps. This phase
addresses the gaps that can be closed with **user-mode Windows APIs only** — no
WinRing0, no custom driver, no third-party SDK.

Gaps that require WinRing0 (SuperIO, RAPL, HSMP, I2C/PMBus, PCIe config space) or
vendor SDKs (AMD ADL) are deferred to PHASE GAMMA.

---

## Work Items

### B1. Battery Information

**Exit Report Gap:** 4
**Severity:** Significant
**Effort:** Low (1 day)

The Windows stub in `src/collectors/battery.rs` returns `vec![]`. Implement using
WMI `Win32_Battery` via PowerShell (matching the pattern used in pci.rs, usb.rs,
audio.rs).

**Fields to populate:**

| BatteryInfo field | WMI source |
|-------------------|------------|
| name | `Win32_Battery.Name` or `DeviceID` |
| manufacturer | `Win32_Battery.Manufacturer` (may be null) |
| model | `Win32_Battery.Name` |
| chemistry | `Win32_Battery.Chemistry` (enum: 1=Other, 2=Unknown, 3=PbAcid, 4=NiCd, 5=NiMH, 6=LiIon, 7=ZnAir, 8=LiPoly) |
| status | `Win32_Battery.BatteryStatus` (1=Discharging, 2=AC, 3=Charged, 4-5=Low/Critical, 6-9=Charging, 10=Undefined, 11=Partial) |
| capacity_percent | `Win32_Battery.EstimatedChargeRemaining` |
| capacity_design_wh | `Win32_Battery.DesignCapacity` (mWh, divide by 1000) |
| capacity_full_wh | `Win32_Battery.FullChargeCapacity` (mWh) |
| voltage_v | `Win32_Battery.DesignVoltage` (mV, divide by 1000) |
| cycle_count | Not available from WMI — set to None |
| wear_percent | Compute from DesignCapacity vs FullChargeCapacity if both present |

**Acceptance criteria:**
- `sio battery` shows battery data on a laptop
- `sio --format json` includes populated battery fields
- Desktop systems (no battery) show empty section gracefully

---

### B2. Network Interface Details

**Exit Report Gap:** 5
**Severity:** Critical
**Effort:** Medium (2-3 days)

The Windows network collector only shows adapter names from sysinfo. Implement rich
network data using the Windows `GetAdaptersAddresses` API (IP Helper).

**Approach:** Add `iphlpapi` (IP Helper API) to the winapi features in Cargo.toml.
Call `GetAdaptersAddresses` with `GAA_FLAG_INCLUDE_PREFIX` to get full adapter info.

**New winapi features needed:** `"iphlpapi"`, `"iptypes"`, `"ifdef"`, `"winsock2"`, `"ws2def"`

**Fields to populate:**

| NetworkAdapter field | Windows source |
|----------------------|----------------|
| mac_address | `IP_ADAPTER_ADDRESSES.PhysicalAddress` (6 bytes, format as XX:XX:XX:XX:XX:XX) |
| ip_addresses | `IP_ADAPTER_ADDRESSES.FirstUnicastAddress` chain — iterate for AF_INET and AF_INET6 |
| speed_mbps | `IP_ADAPTER_ADDRESSES.TransmitLinkSpeed` (bits/sec, divide by 1_000_000) |
| mtu | `IP_ADAPTER_ADDRESSES.Mtu` |
| interface_type | `IP_ADAPTER_ADDRESSES.IfType` (6=Ethernet, 71=WiFi, 24=Loopback, 131=Tunnel, etc.) |
| is_physical | Derive from IfType (Ethernet/WiFi = physical) |
| operstate | `IP_ADAPTER_ADDRESSES.OperStatus` (1=Up, 2=Down, etc.) |
| driver | Not directly available — leave as None or query WMI `Win32_NetworkAdapter.ServiceName` |
| duplex | Not available from GetAdaptersAddresses — leave as None |

**Implementation plan:**
1. Create a helper `fn collect_adapters_win() -> Vec<NetworkAdapter>` using raw `GetAdaptersAddresses` FFI
2. Replace the sysinfo-based `#[cfg(not(unix))] collect()` with this richer version
3. Implement `#[cfg(not(unix))] collect_ip_addresses()` to return IPs from the same data

**Acceptance criteria:**
- `sio network` shows MAC, IPs, speed, MTU, interface type for each adapter
- WiFi and Ethernet correctly classified
- `sio --format json` includes populated network fields
- Virtual adapters (VMware, Hyper-V, ZeroTier) detected and displayed

---

### B3. Motherboard Detail Fields

**Exit Report Gap:** 11
**Severity:** Minor
**Effort:** Low (1 day)

The Windows motherboard collector uses 3 wmic queries. Expand it to populate all
available fields.

**Additional wmic queries:**

| Field | wmic command |
|-------|-------------|
| serial | `wmic baseboard get SerialNumber /value` |
| version | `wmic baseboard get Version /value` |
| system_vendor | Already populated (`computersystem Manufacturer`) |
| system_product | Already populated (`computersystem Model`) |
| system_family | `wmic computersystem get SystemFamily /value` |
| system_sku | `wmic computersystem get SystemSKUNumber /value` |
| system_uuid | `wmic csproduct get UUID /value` |
| chassis_type | `wmic systemenclosure get ChassisTypes /value` (parse array) |
| bios.vendor | `wmic bios get Manufacturer /value` |
| bios.release | `wmic bios get SMBIOSMajorVersion,SMBIOSMinorVersion /value` |
| bios.secure_boot | Registry: `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State\UEFISecureBootEnabled` (REG_DWORD, 1=enabled) |
| bios.uefi_boot | Registry: check `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State` key exists — if present, UEFI boot |
| chipset | Look up PCI 00:00.0 device name from the PCI collector results (already available) |

**Acceptance criteria:**
- `sio board` shows serial, UUID, chassis type, BIOS vendor, Secure Boot status
- `sio --format json` has populated motherboard fields

---

### B4. USB Device Details (Speed, Power, Class)

**Exit Report Gap:** 12
**Severity:** Minor
**Effort:** Medium (2 days)

The Windows USB collector gets VID/PID and product name but not speed, power, or
device class.

**Approach:** Use SetupAPI via `SetupDiGetClassDevs` + `SetupDiGetDeviceProperty`
with USB-specific DEVPKEYs, or enhance the existing WMI query with
`Win32_USBControllerDevice` + `Win32_USBHub` properties.

**Simpler WMI approach:** Query `Win32_PnPEntity` with additional properties:

| UsbDevice field | Source |
|-----------------|--------|
| speed | Parse from WMI `Win32_PnPEntity.CompatibleID` — look for `USB\Class_*` patterns and `USBSTOR` vs `USBSTOR\Disk` for speed inference. Or query `Win32_USBHub.USBVersion`. |
| device_class | Extract from DeviceID or CompatibleID (e.g., `USB\Class_08` = Mass Storage) |
| max_power_ma | Not available from WMI — set None |
| usb_version | `Win32_USBHub.USBVersion` if available |

**Acceptance criteria:**
- `sio usb` shows speed classification for at least USB 2.0 vs 3.0 devices
- Device class shown where extractable
- No regressions on existing VID/PID/name display

---

### B5. Audio Codec Name

**Exit Report Gap:** 13
**Severity:** Minor
**Effort:** Low (1 day)

The Windows audio collector shows device name and manufacturer but no codec name.

**Approach:** Query the Windows registry for codec information:
- `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e96c-e325-11ce-bfc1-08002be10318}\*`
- Each subkey has `DriverDesc` (device name) and sometimes `HardwareID` which contains
  the codec vendor/device (e.g., `HDAUDIO\FUNC_01&VEN_10EC&DEV_0887` = Realtek ALC887)

Alternatively, parse the `HardwareID` from the existing WMI `Win32_SoundDevice`
output — the DeviceID field already contains the HDAUDIO codec identifiers.

**Acceptance criteria:**
- `sio audio` shows codec identification where available (at least for HD Audio devices)
- Non-HD-Audio devices (USB, virtual) gracefully show no codec

---

### B6. Display Output Enumeration

**Exit Report Gap:** 6 (partial — display outputs only, not AMD GPU sensors)
**Severity:** Minor
**Effort:** Medium (2 days)

Linux enumerates display outputs per GPU via DRM sysfs with EDID parsing for monitor
names and resolutions. Windows has equivalent APIs.

**Approach:** Use DXGI (`IDXGIFactory::EnumAdapters` + `IDXGIAdapter::EnumOutputs`)
or the simpler `EnumDisplayDevices` + `EnumDisplaySettings` Win32 API.

**Simpler Win32 approach:**

```
EnumDisplayDevices(NULL, i, &dd, 0)        → adapter name
EnumDisplayDevices(adapter, j, &dd, 0)     → monitor name
EnumDisplaySettings(adapter, ENUM_CURRENT) → resolution, refresh rate
```

**Fields to populate:**

| DisplayOutput field | Source |
|--------------------|--------|
| connector_type | "HDMI", "DP", etc. — parse from DeviceString or MonitorName |
| index | Output enumeration index |
| status | "connected" if DISPLAY_DEVICE_ACTIVE flag set |
| monitor_name | From EnumDisplayDevices monitor-level call |
| resolution | From EnumDisplaySettings (dmPelsWidth x dmPelsHeight @ dmDisplayFrequency Hz) |

**Acceptance criteria:**
- `sio gpu` shows connected monitors with names and resolutions
- Multi-monitor setups correctly enumerated
- Disconnected outputs not shown (or shown as disconnected)

---

### B7. WHEA Error Monitoring (MCE + EDAC + AER equivalent)

**Exit Report Gap:** 8
**Severity:** Minor
**Effort:** Medium (2-3 days)

Linux has three separate error tracking systems (MCE, EDAC, AER). Windows unifies
hardware error reporting through WHEA (Windows Hardware Error Architecture). WHEA
events are logged to the Windows Event Log under `Microsoft-Windows-WHEA-Logger`.

**Approach:** Query WHEA events via PowerShell `Get-WinEvent`:
```powershell
Get-WinEvent -LogName 'System' -FilterXPath "*[System[Provider[@Name='Microsoft-Windows-WHEA-Logger']]]" -MaxEvents 100
```

Or use `wevtutil`:
```
wevtutil qe System /q:"*[System[Provider[@Name='Microsoft-Windows-WHEA-Logger']]]" /c:100 /f:xml
```

**WHEA event IDs of interest:**

| Event ID | Meaning | Linux Equivalent |
|----------|---------|-----------------|
| 17 | Corrected hardware error | EDAC CE / MCE correctable |
| 18 | Fatal hardware error | EDAC UE / MCE uncorrectable |
| 19 | Corrected machine check | MCE correctable |
| 47 | Corrected PCIe error | AER correctable |

**Sensor output:** Map to the same `SensorSource` pattern — poll on interval, count
new events since last poll, emit as sensor readings with category Other.

**Acceptance criteria:**
- `sio sensors` shows WHEA error counts when events exist
- Zero-error case shows nothing (same as Linux behavior)
- TUI dashboard "Errors" panel populated when errors present

---

### B8. IPMI BMC Sensors (Server Boards)

**Exit Report Gap:** 9
**Severity:** Significant (server/workstation only)
**Effort:** Medium (3-4 days)

Windows ships an IPMI driver (`ipmidrv.sys`) on server SKUs and some workstation
boards. The protocol is the same — raw IPMI request/response via DeviceIoControl.

**Approach:**
1. Create `src/platform/ipmi_win.rs` that opens `\\.\IPMI` via CreateFileW
2. Define `IOCTL_IPMI_SUBMIT_RAW_REQUEST` (0x220804) for DeviceIoControl
3. Port the SDR enumeration and sensor reading logic from `src/sensors/ipmi.rs`
4. The `ipmi-rs-core` crate (protocol types, SDR parsing, linearization) compiles on
   Windows — only the `ipmi-rs` transport (`File`) is unix-gated

**Structure:**

```rust
// src/platform/ipmi_win.rs
pub fn open_ipmi() -> Option<HANDLE> { ... }
pub fn send_raw(handle: HANDLE, req: &[u8]) -> Option<Vec<u8>> { ... }

// src/sensors/ipmi_win.rs (or extend ipmi.rs with #[cfg(windows)])
// Reuse ipmi-rs-core SDR parsing + sensor linearization
```

**Acceptance criteria:**
- `sio sensors` shows IPMI sensors on boards with BMC (DIMM temps, fan RPMs, PSU data)
- Systems without BMC gracefully skip (device open fails → empty)
- Background polling thread (same 5s interval as Linux)

---

## Work Item Dependencies

```
B1 (Battery)           → independent
B2 (Network)           → independent
B3 (Motherboard)       → independent
B4 (USB details)       → independent
B5 (Audio codec)       → independent
B6 (Display outputs)   → independent
B7 (WHEA errors)       → independent
B8 (IPMI)              → independent

All of B1-B8 are independent and can be developed in parallel.
```

---

## Implementation Order

Items are ordered by impact-to-effort ratio:

| Priority | Item | Effort | Impact | Rationale |
|----------|------|--------|--------|-----------|
| 1 | B2: Network details | Medium | Critical | Most visible gap in default `sio` output |
| 2 | B1: Battery | Low | Significant | Trivial WMI query, enables laptop use case |
| 3 | B3: Motherboard fields | Low | Minor | Trivial wmic expansion, visible in default output |
| 4 | B6: Display outputs | Medium | Minor | Visible in `sio gpu`, useful for multi-monitor users |
| 5 | B5: Audio codec | Low | Minor | Small registry/WMI addition |
| 6 | B4: USB details | Medium | Minor | Speed/class polish |
| 7 | B7: WHEA errors | Medium | Minor | Error monitoring for reliability-conscious users |
| 8 | B8: IPMI | Medium | Significant | Server/workstation only, requires specific hardware |

---

## Out of Scope (deferred to PHASE GAMMA)

These items require WinRing0 kernel driver or vendor-specific SDKs and are **not**
part of PHASE BETA:

| Item | Exit Report Gap | Blocker |
|------|----------------|---------|
| SuperIO temps/fans/voltages | 1, 14 | WinRing0 port I/O |
| RAPL power metering | 2 | WinRing0 MSR access |
| AMD HSMP telemetry | 3 | WinRing0 PCI config + AMD-specific |
| I2C/PMBus VRM monitoring | 10 | WinRing0 SMBus host controller |
| PCIe link speed/width | 7 | WinRing0 PCI config or SetupAPI DEVPKEY |
| AMD ADL GPU sensors | 6 (partial) | AMD ADL SDK |
| DDR5 DIMM temperatures | 10 | WinRing0 SMBus |

---

## Testing Requirements

### Per-item validation

Each work item must pass:
1. `cargo check --target x86_64-pc-windows-msvc` — zero errors
2. `cargo build --release --target x86_64-pc-windows-msvc` — clean build
3. Relevant `sio` subcommand produces correct output
4. `sio --format json` serializes new fields correctly
5. `sio -m` (TUI) displays new sensors if applicable

### Platform coverage

| Item | Desktop (no battery) | Laptop | Server/Workstation |
|------|---------------------|--------|--------------------|
| B1: Battery | Empty (graceful) | Must show data | Empty (graceful) |
| B2: Network | Ethernet + virtual | WiFi + Ethernet | Ethernet + IPMI NIC |
| B3: Motherboard | Full | Full | Full |
| B4: USB | Full | Full | Full |
| B5: Audio | HD Audio | HD Audio + USB | HD Audio |
| B6: Display | NVIDIA/AMD | iGPU + eGPU | ASPEED BMC VGA |
| B7: WHEA | If errors present | If errors present | If errors present |
| B8: IPMI | N/A (no BMC) | N/A (no BMC) | Must show sensors |

---

## Exit Criteria

PHASE BETA is complete when:

1. All 8 work items (B1-B8) are implemented, tested, and committed
2. `sio` default output on Windows shows: battery (laptop), network IPs/MAC/speed,
   full motherboard details, display outputs
3. `sio sensors` includes WHEA error counts (when errors exist) and IPMI sensors
   (on server/workstation boards)
4. No Linux-specific strings remain in user-visible output
5. `sio --format json` on Windows produces populated fields for all implemented
   collectors (null only for genuinely unavailable data)
6. PHASE_BETA section of `phase_status.json` updated to `"completed"`
