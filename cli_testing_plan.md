# CLI Testing Plan — sio.exe on Windows

**Binary:** `target/x86_64-pc-windows-msvc/release/sio.exe`
**Version:** 0.1.3
**Baseline system:** AMD Ryzen Threadripper PRO 3975WX, ASUS Pro WS WRX80E-SAGE SE WIFI, NVIDIA GTX 1650, 4x16 GiB Kingston DDR4, Samsung 980 PRO 1TB NVMe, 2x WD 20TB HDD (spanned), Windows 10 Pro 26200

---

## Test Matrix Overview

| Area | Non-Admin | Admin | JSON | Notes |
|------|-----------|-------|------|-------|
| T1. Default summary | X | X | X | Core output path |
| T2. Per-section subcommands (12) | X | X | X | Each subcommand |
| T3. Sensor snapshot | X | X | X | `sio sensors` |
| T4. TUI monitor | | X | | `sio -m` |
| T5. Output formats | X | | | text/json/xml/html |
| T6. CLI flags | X | | | --no-nvidia, --color, etc. |
| T7. Error handling | X | | | Invalid args, missing features |
| T8. CSV logging | | X | | `sio -m --log` |
| T9. Performance | X | X | | Execution time benchmarks |
| T10. Admin-gated features | | X | | SMART, SMBIOS serials |

---

## T1. Default Summary (`sio`)

### T1.1 Non-admin execution

```
sio
```

**Expected:** Output starts with header, shows admin hint:
```
  sio - System Information
  ========================
  (run as Administrator for SMART data)
```

**Verify each section present:**
- [ ] Hostname (not "unknown")
- [ ] Kernel version (format: `10.0.XXXXX.XXXX`)
- [ ] OS name (e.g., "Windows 10 Pro" or "Windows 11")
- [ ] CPU section with brand, vendor, codename, topology, cache, features
- [ ] Memory section with total/available/swap
- [ ] Motherboard with board name, BIOS, boot mode, chipset
- [ ] Storage with drive letters and interface type
- [ ] Network with MACs, IPs, speeds, interface types, drivers
- [ ] Audio with device names
- [ ] USB with VID:PID and speed classification
- [ ] PCI with device count and pci_ids names

### T1.2 Admin execution

```
# Run in elevated prompt
sio
```

**Expected:** No admin hint. Same sections plus:
- [ ] Storage shows SMART data lines (`SMART: XXC, XXXX hours, XX.X TiB written`)
- [ ] Memory shows DIMM details (manufacturer, part number, speed)
- [ ] CPU shows microcode version
- [ ] Motherboard shows serial number, UUID

### T1.3 JSON validation

```
sio -f json | python -c "import sys,json; d=json.load(sys.stdin); print(list(d.keys()))"
```

**Expected keys:** `timestamp`, `version`, `hostname`, `kernel_version`, `os_name`, `cpus`, `memory`, `motherboard`, `gpus`, `storage`, `network`, `audio`, `usb_devices`, `pci_devices`, `batteries`, `sensors`

**Verify non-null fields (admin):**
- [ ] `hostname` is string, not "unknown"
- [ ] `cpus[0].microcode` is not null
- [ ] `cpus[0].topology.physical_cores` > 0
- [ ] `memory.dimms` array is non-empty
- [ ] `memory.dimms[0].manufacturer` is not null
- [ ] `motherboard.chipset` is not null
- [ ] `motherboard.bios.date` matches `YYYY-MM-DD`
- [ ] `motherboard.bios.secure_boot` is boolean
- [ ] `storage[*].smart` is not null (for at least one drive)
- [ ] `gpus[0].display_outputs` is non-empty
- [ ] `network[*].mac_address` is not null (for physical adapters)
- [ ] `network[*].ip_addresses` is non-empty (for up adapters)

---

## T2. Per-Section Subcommands

Run each subcommand in both text and JSON, verify output is non-empty and well-formed.

| # | Command | Text Check | JSON Check |
|---|---------|-----------|------------|
| T2.1 | `sio cpu` | Brand, topology, cache, features present | `cpus` array non-empty |
| T2.2 | `sio gpu` | GPU name, vendor, VRAM, driver, display output | `gpus` array non-empty |
| T2.3 | `sio memory` | Total/available, DIMM details (admin) | `memory.dimms` populated (admin) |
| T2.4 | `sio storage` | Drive letters, capacity, SMART (admin) | `storage[*].smart` non-null (admin) |
| T2.5 | `sio network` | Adapter names, MACs, IPs, speeds | `network` array has entries with `mac_address` |
| T2.6 | `sio pci` | Device count header, pci_ids names | `pci_devices` array with `vendor_name` |
| T2.7 | `sio usb` | VID:PID, product name, speed class | `usb_devices` with `speed` not "Unknown" |
| T2.8 | `sio audio` | Device names, codec for HD Audio | `audio` with `codec` field |
| T2.9 | `sio battery` | "No batteries detected" on desktop | `batteries` empty array on desktop |
| T2.10 | `sio board` | Board name, BIOS, Secure Boot, chipset | `motherboard.chipset` non-null |
| T2.11 | `sio pcie` | "No PCIe devices detected" without WinRing0 | `pci_devices` with `pcie_link` null |
| T2.12 | `sio sensors` | Sensor categories: cpu/cpufreq, cpu/utilization, disk, net, nvml, whea | 150+ sensor entries in JSON |

---

## T3. Sensor Snapshot (`sio sensors`)

### T3.1 Text output

```
sio sensors
```

**Verify categories present:**
- [ ] `cpu/cpufreq` — 64 lines, each showing `Core N Frequency XXXX.X MHz`
- [ ] `cpu/utilization` — 65 lines (64 cores + total), values 0-100%
- [ ] `disk/*` — read/write MB/s per volume
- [ ] `net/*` — RX/TX MB/s per adapter
- [ ] `nvml/gpu0` — 7 readings: temperature, fan, power, clocks, util, VRAM
- [ ] `whea/system` — 4 error counters (usually 0.0)

### T3.2 JSON output

```
sio sensors -f json | python -c "import sys,json; d=json.load(sys.stdin); print(len(d), 'sensors')"
```

**Expected:** 150+ sensors. Each entry has: `label`, `current`, `unit`, `category`, `min`, `max`, `avg`, `sample_count`.

### T3.3 Sensor value sanity

- [ ] CPU frequencies are in 1000-6000 MHz range
- [ ] CPU utilization values are 0.0-100.0
- [ ] GPU temperature is 20-100 C
- [ ] WHEA error counts are >= 0
- [ ] Disk throughput is >= 0

---

## T4. TUI Monitor (`sio -m`)

### T4.1 Basic launch and exit

```
sio -m
# Press 'q' to exit
```

**Verify:**
- [ ] TUI renders with header bar showing sensor count and group count
- [ ] Header does NOT show admin warning when elevated
- [ ] Header DOES show admin warning when not elevated
- [ ] Sensor groups appear (cpu, disk, net, nvml, whea)
- [ ] Values update after ~1 second
- [ ] 'q' exits cleanly to terminal

### T4.2 Navigation

- [ ] Up/Down arrows move selection highlight
- [ ] Enter/Space toggles group collapse/expand
- [ ] 'c' collapses all, 'e' expands all
- [ ] PageUp/PageDown scrolls
- [ ] Mouse scroll works

### T4.3 Filter mode

```
sio -m
# Press '/', type "temp", press Enter
```

- [ ] Only temperature sensors visible after filter
- [ ] Esc clears filter

### T4.4 Custom interval

```
sio -m --interval 500
```

- [ ] Updates are visibly faster than default 1000ms

---

## T5. Output Formats

| # | Command | Expected |
|---|---------|----------|
| T5.1 | `sio` (default) | Pretty-printed text with section headers |
| T5.2 | `sio -f text` | Same as default |
| T5.3 | `sio -f json` | Valid JSON, parseable by `python -c "import json"` |
| T5.4 | `sio -f xml` | Error: "XML output not available — compile with 'xml' feature" |
| T5.5 | `sio -f html` | Error: "HTML output not available — compile with 'html' feature" |
| T5.6 | `sio sensors -f json` | Valid JSON with 150+ sensor entries |

**Note:** XML and HTML require `--features xml` and `--features html` at compile time. Default build only includes text, json, csv.

---

## T6. CLI Flags

| # | Flag | Command | Expected |
|---|------|---------|----------|
| T6.1 | `--version` | `sio --version` | `sio 0.1.3` |
| T6.2 | `--help` | `sio --help` | Usage text with all subcommands and options |
| T6.3 | `--no-nvidia` | `sio --no-nvidia gpu` | Empty GPU output (no NVML) |
| T6.4 | `--color never` | `sio --color never` | Output without ANSI escape codes |
| T6.5 | `--color always` | `sio --color always` | Output with ANSI codes even when piped |
| T6.6 | `--show-empty` | `sio --show-empty` | Additional fields shown (may be same on this system) |
| T6.7 | `--direct-io` | `sio --direct-io` | Attempts WinRing0 (no effect without driver) |
| T6.8 | `--interval` | `sio -m --interval 2000` | TUI updates every 2 seconds |

---

## T7. Error Handling

| # | Input | Expected |
|---|-------|----------|
| T7.1 | `sio invalidcmd` | Error: "unrecognized subcommand", exit code 2 |
| T7.2 | `sio -f yaml` | Error: "invalid value 'yaml'", suggests 'xml', exit code 2 |
| T7.3 | `sio --log /nonexistent/path.csv` | Error about log file (TUI context only) |
| T7.4 | `sio -m --alert "bad syntax"` | Error: "Invalid alert rule" on stderr |
| T7.5 | `sio -f json \| <broken pipe>` | Clean exit (no panic on broken pipe) |

---

## T8. CSV Logging

```
sio -m --log test_output.csv --interval 500
# Let run for 3-5 seconds, then press 'q'
```

**Verify:**
- [ ] `test_output.csv` file created
- [ ] First line is header row with sensor names
- [ ] Subsequent lines have numeric values
- [ ] Timestamps are in ISO 8601 format
- [ ] File has 3-10 data rows (depending on runtime)
- [ ] File is valid CSV (parseable by `python csv.reader`)

---

## T9. Performance Benchmarks

Measure execution time of key commands. Record as baseline for regression detection.

| # | Command | Baseline (this system) | Acceptable |
|---|---------|----------------------|------------|
| T9.1 | `sio` (text, admin) | ~2.1s | < 5s |
| T9.2 | `sio -f json` (admin) | ~2.0s | < 5s |
| T9.3 | `sio sensors` | ~1.1s | < 3s |
| T9.4 | `sio cpu` | ~0.1s | < 1s |
| T9.5 | `sio network` | ~0.3s | < 2s |
| T9.6 | `sio pci` | ~1.5s | < 3s (WMI/PowerShell overhead) |

**Note:** The ~2s default runtime is dominated by parallel WMI/PowerShell queries for PCI, USB, audio, motherboard, and network driver lookups. This is expected and matches similar Windows system tools.

---

## T10. Admin-Gated Features

Run both non-admin and admin, compare output for these fields:

| Feature | Non-Admin | Admin |
|---------|-----------|-------|
| Admin hint in header | Shown | Not shown |
| Storage SMART data | `smart: None` | `smart: Some(...)` |
| NVMe SMART temperature | Not shown | Shown (e.g., 47C) |
| SATA SMART hours | Not shown | Shown (e.g., 27293h) |
| Memory DIMM details | Populated (SMBIOS accessible) | Populated |
| CPU microcode | Populated (registry accessible) | Populated |
| Motherboard serial | Populated (wmic accessible) | Populated |
| WinRing0 sensors | Not loaded | Loaded if driver present |

**Note:** SMBIOS, registry, and WMI queries work without admin on most Windows systems. Only SMART ioctl requires elevation. The admin hint message is specifically about SMART data.

---

## Test Assignment Guide

For a team of 3-4 testers:

| Tester | Sections | Focus |
|--------|----------|-------|
| Tester A | T1, T5, T7 | Core output, formats, error handling |
| Tester B | T2 (all 12 subcommands) | Subcommand coverage, text + JSON |
| Tester C | T3, T4, T8 | Sensors, TUI interaction, CSV logging |
| Tester D | T6, T9, T10 | CLI flags, performance, admin gating |

Each tester should run their tests on:
1. **Non-elevated** command prompt or terminal
2. **Elevated** (Run as Administrator) command prompt or terminal
3. Compare outputs and verify admin-dependent differences

---

## Automated Test Script

For CI or regression testing, the following can be automated:

```bash
#!/bin/bash
# cli_smoke_test.sh — run from elevated prompt
SIO=./target/x86_64-pc-windows-msvc/release/sio.exe
FAIL=0

# Version check
$SIO --version | grep -q "sio 0.1.3" || { echo "FAIL: version"; FAIL=1; }

# All subcommands produce output
for cmd in cpu gpu memory storage network pci usb audio battery board pcie sensors; do
  lines=$($SIO $cmd 2>&1 | wc -l)
  [ "$lines" -gt 0 ] || { echo "FAIL: $cmd produced no output"; FAIL=1; }
done

# All subcommands produce valid JSON
for cmd in cpu gpu memory storage network pci usb audio battery board pcie sensors; do
  $SIO $cmd -f json 2>&1 | python -c "import sys,json; json.load(sys.stdin)" 2>/dev/null \
    || { echo "FAIL: $cmd -f json invalid"; FAIL=1; }
done

# JSON field checks
$SIO -f json 2>&1 | python -c "
import sys,json
d=json.load(sys.stdin)
assert d['hostname'] != 'unknown', 'hostname is unknown'
assert len(d['cpus']) > 0, 'no cpus'
assert d['cpus'][0]['topology']['physical_cores'] > 0, 'no cores'
assert d['memory']['total_bytes'] > 0, 'no memory'
assert len(d['network']) > 0, 'no network'
assert len(d['pci_devices']) > 0, 'no pci'
print('JSON field checks: PASS')
" 2>&1 || { echo "FAIL: JSON field checks"; FAIL=1; }

# Sensor count
count=$($SIO sensors -f json 2>&1 | python -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$count" -ge 100 ] || { echo "FAIL: only $count sensors (expected 100+)"; FAIL=1; }

# Error handling
$SIO invalidcmd 2>&1 | grep -q "unrecognized subcommand" || { echo "FAIL: invalid cmd error"; FAIL=1; }
$SIO -f yaml 2>&1 | grep -q "invalid value" || { echo "FAIL: invalid format error"; FAIL=1; }

[ "$FAIL" -eq 0 ] && echo "ALL TESTS PASSED" || echo "SOME TESTS FAILED"
exit $FAIL
```

---

## Known Limitations (not bugs)

| Item | Behavior | Reason |
|------|----------|--------|
| `sio pcie` shows "No PCIe devices detected" | WinRing0 not installed | PCIe link info requires PCI config space access |
| `sio battery` shows "No batteries detected" | Desktop system | Expected on non-laptop |
| `sio -f xml` / `sio -f html` | "not available" error | Requires compile-time features |
| Network shows "if-type:53" | TAP/TUN adapter | IfType 53 is proprietary virtual adapter |
| DIMM locators all show "DIMM 0" | SMBIOS device_locator field | Board-specific; some boards use "DIMM_A1" etc. |
| NVMe SMART reads fail on some drives | Driver-specific | Samsung older firmware, some Intel NVMe |
| GPU display shows "Display-0" connector | Windows limitation | Win32 API doesn't expose HDMI/DP type |
| Execution takes ~2s | WMI/PowerShell queries | PCI, USB, audio, network driver lookups |
