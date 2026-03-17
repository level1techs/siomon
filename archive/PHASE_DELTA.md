# PHASE DELTA: Polish and Upstream Preparation

**Repo:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Predecessor:** PHASE_GAMMA (completed 2026-03-16)
**Goal:** Fill the remaining data-quality gaps visible in `sio` output, enrich
hardware details via SMBIOS, clean up code for upstream submission, and validate
WinRing0-dependent features on live hardware.

---

## Context

Phases Alpha through Gamma addressed all 14 original functional gaps. The Windows
build now has 154 active sensors (200+ with WinRing0) and covers CPU, memory,
storage, network, GPU, PCI, USB, audio, battery, motherboard, WHEA errors, and IPMI.

What remains is **data quality polish** — fields that show "(unknown)" or are
absent on Windows but populated on Linux — and **upstream preparation** to merge
the port back into `level1techs/siomon`.

---

## Work Items

### D1. SMBIOS Table Parsing via GetSystemFirmwareTable

**Effort:** Medium (2-3 days)

The existing `src/parsers/smbios.rs` reads raw SMBIOS data from
`/sys/firmware/dmi/tables/DMI` on Linux. The parser itself is pure binary parsing
with no OS-specific code. On Windows, the same raw data is available via
`GetSystemFirmwareTable('RSMB', 0, ...)`.

**Implementation plan:**
1. Add `#[cfg(windows)]` path to `smbios::parse()` that calls `GetSystemFirmwareTable`
2. The returned buffer starts with a `RawSMBIOSData` header (8 bytes):
   ```c
   struct RawSMBIOSData {
       BYTE  Used20CallingMethod;
       BYTE  SMBIOSMajorVersion;
       BYTE  SMBIOSMinorVersion;
       BYTE  DmiRevision;
       DWORD Length;
       BYTE  SMBIOSTableData[];  // <- this is what the parser expects
   }
   ```
3. Skip the 8-byte header and pass the table data to `parse_from_bytes()`
4. Add `parse_from_bytes(&[u8]) -> Option<SmbiosData>` (extract from `parse_from_path`)

**winapi feature needed:** `"sysinfoapi"` (already present — `GetSystemFirmwareTable` is in `sysinfoapi`)

**Acceptance criteria:**
- `smbios::parse()` returns populated `SmbiosData` on Windows
- BIOS, System, Baseboard, and MemoryDevice entries all parsed

---

### D2. Memory DIMM Details from SMBIOS Type 17

**Effort:** Medium (1-2 days, after D1)
**Depends on:** D1

Once SMBIOS parsing works on Windows, the memory collector can populate DIMM details
that are currently empty.

**Fields to populate from MemoryDeviceEntry:**

| DimmInfo field | SMBIOS source |
|---------------|---------------|
| locator | device_locator (e.g., "DIMM_A1") |
| manufacturer | manufacturer (e.g., "Samsung") |
| part_number | part_number (e.g., "M393A2K43BB1-CTD") |
| serial | serial_number |
| size_bytes | size_bytes |
| memory_type | memory_type (map to DDR4/DDR5/etc.) |
| speed_mts | configured_speed_mts or speed_mts |
| voltage_mv | configured_voltage_mv |
| ecc | Derive from total_width_bits > data_width_bits |
| rank | rank |

**Implementation:**
1. In `src/collectors/memory.rs`, call `smbios::parse()` in the Windows path
2. Map `MemoryDeviceEntry` to `DimmInfo` (same logic as Linux `collect_dimms()`)

**Acceptance criteria:**
- `sio memory` shows DIMM details (locator, manufacturer, part, speed, size)
- `sio --format json` has populated dimms array

---

### D3. CPU Microcode Version

**Effort:** Low (0.5 day)

Linux reads microcode from `/proc/cpuinfo`. On Windows, it's in the registry:
`HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\Update Revision`

**Implementation:**
1. In `src/collectors/cpu.rs`, add `#[cfg(not(unix))]` microcode reading
2. Query registry: `reg query "HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0" /v "Update Revision"`
3. Parse the REG_BINARY value (8 bytes, microcode revision in upper 4 bytes)

**Acceptance criteria:**
- `sio cpu` shows microcode version

---

### D4. Network Interface Type and Driver Name

**Effort:** Low (1 day)

The network collector shows "(unknown)" for interface type classification. The data
is actually available from `GetAdaptersAddresses` (IfType is already read) but the
mapping needs improvement.

**Issues to fix:**
1. Interface type shows "(unknown)" even though IfType is read — check if the
   mapping in `collect()` is being applied to the text display correctly
2. Driver name: query `Win32_NetworkAdapter.ServiceName` via WMI and match by
   adapter name to populate the `driver` field

**Acceptance criteria:**
- `sio network` shows "Ethernet", "WiFi", etc. instead of "(unknown)"
- Driver name shown where available

---

### D5. ACPI Thermal Zones as Supplementary Sensor Source

**Effort:** Medium (2 days)

WMI `MSAcpi_ThermalZoneTemperature` provides ACPI thermal zone readings without
any driver. While limited (only ACPI-reported zones, not per-VRM or per-core),
it provides some temperature data on systems without WinRing0.

**Implementation:**
1. Create `src/sensors/acpi_thermal_win.rs`
2. Query: `Get-CimInstance -Namespace root\WMI -ClassName MSAcpi_ThermalZoneTemperature`
3. Returns `CurrentTemperature` in tenths of Kelvin: `(val / 10) - 273.15 = Celsius`
4. Typically returns 1-3 zones (CPU, system, chipset)

**Acceptance criteria:**
- `sio sensors` shows ACPI thermal zone temperatures (if available)
- Works without WinRing0 or admin privileges

---

### D6. BIOS Date Formatting

**Effort:** Low (0.5 day)

The BIOS date shows raw WMI format: `20251016000000.000000+000`. This should be
formatted as `2025-10-16` or `10/16/2025` to match Linux display.

**Implementation:**
1. In `src/collectors/motherboard.rs`, parse the WMI datetime format and reformat
2. Pattern: `YYYYMMDD...` → `YYYY-MM-DD`

**Acceptance criteria:**
- `sio board` shows human-readable BIOS date

---

### D7. Motherboard Chipset Name from PCI 00:00.0

**Effort:** Low (0.5 day)

Linux reads the chipset from PCI device 00:00.0 name. On Windows, the PCI collector
already enumerates all devices including 00:00.0. Wire the motherboard collector to
look up the host bridge device name from PCI results.

**Implementation:**
1. In `src/main.rs` `collect_all()`, after both motherboard and PCI collection,
   populate `motherboard.chipset` from the PCI device at 00:00.0 if present

**Acceptance criteria:**
- `sio board` shows chipset name (e.g., "Starship/Matisse Root Complex")

---

### D8. Upstream PR Preparation

**Effort:** Medium (2-3 days)

Prepare the branch for submission as a pull request to `level1techs/siomon`.

**Tasks:**
1. **Rebase** onto current upstream main (resolve any conflicts)
2. **Squash** into logical commits (one per phase or per feature area)
3. **CI integration**: ensure `cargo check --target x86_64-pc-windows-msvc` passes
   in GitHub Actions (add Windows to the CI matrix if not present)
4. **Documentation**: update README.md with Windows build instructions
5. **Feature gating**: consider a `windows` feature flag to keep Windows deps
   out of Linux builds (already achieved via `[target.'cfg(windows)'.dependencies]`)
6. **Code review cleanup**: fix remaining warnings (unused imports in cpu.rs),
   remove any debug println/eprintln

**Acceptance criteria:**
- Clean rebase onto upstream main
- CI passes for both Linux and Windows targets
- README documents Windows build process

---

### D9. WinRing0 Live Hardware Validation

**Effort:** High (3-5 days, requires multiple test systems)

All GAMMA work items (SuperIO, RAPL, HSMP, PCIe link, SMBus) were implemented
without WinRing0 installed. They compile and gracefully fall back, but have not
been validated against real hardware readings.

**Validation plan:**

| Test | Platform | What to verify |
|------|----------|----------------|
| SuperIO sensors | AMD desktop with NCT6799 | Temps, fans, voltages match HWiNFO |
| SuperIO sensors | Intel desktop with NCT6799 | Same verification |
| RAPL power | Intel desktop (any recent) | Package power matches HWiNFO |
| RAPL power | AMD Zen 3+ | Package power matches |
| HSMP telemetry | AMD Zen 3+ desktop/workstation | Socket power, FCLK match HWiNFO |
| PCIe link | Any system with WinRing0 | GPU shows Gen3/4 x16, NVMe shows Gen3/4 x4 |
| SMBus PMBus | Board with IR/ISL VRM controllers | VRM voltage/current/temp readable |
| SMBus SPD5118 | DDR5 system | Per-DIMM temperature readable |
| AMD ADL | System with AMD GPU + driver | Temp, fan, clocks match Adrenalin |
| HVCI compatibility | Windows 11 with HVCI | Document any driver-loading failures |

**Acceptance criteria:**
- At least 3 of the above scenarios verified on real hardware
- Known issues documented

---

## Work Item Dependencies

```
D1 (SMBIOS parsing)
 └── D2 (DIMM details)

D3 (CPU microcode)        → independent
D4 (Network type/driver)  → independent
D5 (ACPI thermal)         → independent
D6 (BIOS date format)     → independent
D7 (Chipset name)         → independent (uses existing PCI data)
D8 (Upstream PR)          → after D1-D7 complete
D9 (WinRing0 validation)  → independent (hardware-dependent)
```

---

## Implementation Order

| Priority | Item | Effort | Impact | Notes |
|----------|------|--------|--------|-------|
| 1 | D1: SMBIOS on Windows | Medium | High | Unlocks D2, enriches motherboard |
| 2 | D2: DIMM details | Medium | High | Memory section currently empty of DIMM info |
| 3 | D6: BIOS date format | Low | Minor | Quick cosmetic fix |
| 4 | D7: Chipset name | Low | Minor | Wire existing PCI data |
| 5 | D3: CPU microcode | Low | Minor | Registry query |
| 6 | D4: Network type/driver | Low | Minor | Fix "(unknown)" display |
| 7 | D5: ACPI thermal zones | Medium | Minor | Supplementary temps without WinRing0 |
| 8 | D8: Upstream PR prep | Medium | High | Required for merge |
| 9 | D9: WinRing0 validation | High | High | Requires multiple test machines |

---

## Exit Criteria

PHASE DELTA is complete when:

1. `sio` default output shows DIMM details, CPU microcode, proper BIOS date,
   chipset name, and correct network interface types
2. SMBIOS parsing works on Windows via GetSystemFirmwareTable
3. No "(unknown)" appears where data is available from Windows APIs
4. Branch is rebased and ready for upstream PR review
5. At least basic WinRing0 validation performed on one test system
6. PHASE_DELTA section of `phase_status.json` updated to `"completed"`

---

## Future: PHASE EPSILON (potential)

If Delta completes and upstream accepts the PR:
- **Cross-compile CI** — build Windows from Linux CI runner via cross-rs
- **ARM64 Windows** — `aarch64-pc-windows-msvc` port (Surface, Snapdragon)
- **Installer/packaging** — MSI installer, winget manifest, portable zip
- **hwmon abstraction layer** — unified sensor API for both Linux and Windows
- **Configuration file** — Windows-specific config paths (`%APPDATA%`)
