# CLI Testing Results — sio.exe on Windows

**Date:** 2026-03-16
**Binary:** `target/x86_64-pc-windows-msvc/release/sio.exe` v0.1.3
**Platform:** AMD Threadripper PRO 3975WX, ASUS WRX80E, NVIDIA GTX 1650, Windows 10 Pro 26200
**Elevation:** Administrator (elevated prompt)

---

## Automated Smoke Test: 36/36 PASSED

```
tests/cli_smoke_test.sh — ALL TESTS PASSED
```

| Category | Tests | Passed | Failed |
|----------|-------|--------|--------|
| Version / Help | 2 | 2 | 0 |
| Subcommand text output (12) | 12 | 12 | 0 |
| Subcommand JSON output (12) | 12 | 12 | 0 |
| JSON field validation | 10 | 10 | 0 |
| Sensor snapshot | 1 | 1 | 0 |
| Output formats | 4 | 4 | 0 |
| Error handling | 2 | 2 | 0 |
| CLI flags | 3 | 3 | 0 |
| **Total** | **36** | **36** | **0** |

---

## Manual Test Results

### T1: Default Summary (Admin)

| Check | Result | Details |
|-------|--------|---------|
| No admin hint | PASS | TokenElevation correctly detects elevation |
| Hostname | PASS | `DATAPROCESSOR2` |
| Kernel | PASS | `10.0.26200.7840` |
| OS | PASS | `Windows 10 Pro` |
| CPU section | PASS | Brand, Zen 2 codename, 32c/64t, microcode |
| Memory DIMMs | PASS | 4x Kingston KHX3600C18D4/16GX DDR4 3200 MT/s |
| Motherboard | PASS | ASUSTeK WRX80E, AMI BIOS 2025-10-16, chipset |
| GPU | PASS | GTX 1650, NVML, display output |
| Storage SMART | PASS | C: 47C/22216h NVMe, D: 38C/27293h SATA |
| Network | PASS | 11 adapters, MACs, IPs, speeds, drivers |
| Audio | PASS | 2 devices, NVIDIA HD Audio codec |
| USB | PASS | 10 devices, Full/High/Super speed |
| PCI | PASS | 87 devices, pci_ids names |

### T2: Subcommand Line Counts

| Subcommand | Lines | JSON Valid |
|------------|-------|-----------|
| cpu | 148 | Yes |
| gpu | 69 | Yes |
| memory | 177 | Yes |
| storage | 76 | Yes |
| network | 480 | Yes |
| pci | 87 | Yes |
| usb | 11 | Yes |
| audio | 7 | Yes |
| battery | 1 | Yes |
| board | 54 | Yes |
| pcie | 1 | Yes |
| sensors | 175 | Yes |

### T3: Sensor Snapshot

| Check | Result |
|-------|--------|
| Sensor count | 154 |
| Categories | cpu, disk, net, nvml, whea |
| CPU frequency range | 3501 MHz (all cores) |
| CPU utilization range | 0-100% |
| GPU temperature | 47C |
| WHEA errors | 0 (all categories) |
| Value sanity (all) | PASS |

### T4: TUI Monitor

| Check | Result |
|-------|--------|
| Launch | PASS (154 sensors, 31 groups) |
| Header no admin warning | PASS |
| Sensor values update | PASS (visible after 1s) |
| Exit on timeout | PASS (clean exit code 124) |

### T5: Output Formats

| Format | Result |
|--------|--------|
| text (default) | PASS |
| text (-f text) | PASS |
| json (-f json) | PASS (valid, parseable) |
| xml (-f xml) | PASS ("not available" — feature-gated) |
| html (-f html) | PASS ("not available" — feature-gated) |

### T6: CLI Flags

| Flag | Result |
|------|--------|
| --version | PASS (`sio 0.1.3`) |
| --help | PASS (usage text) |
| --no-nvidia | PASS (empty GPU output) |
| --color never | PASS |
| --color always | PASS |
| --show-empty | PASS |
| --direct-io | PASS (no WinRing0, graceful) |

### T7: Error Handling

| Input | Result |
|-------|--------|
| `sio invalidcmd` | PASS (exit 2, "unrecognized subcommand") |
| `sio -f yaml` | PASS (exit 2, "invalid value", suggests xml) |

### T8: CSV Logging

| Check | Result |
|-------|--------|
| File created | PASS |
| Row count | 8 (header + 7 data rows in 4s) |
| Format | Valid CSV |

### T9: Performance

| Command | Time |
|---------|------|
| `sio` (text) | ~2.1s |
| `sio -f json` | ~2.0s |
| `sio sensors` | ~1.1s |
| `sio cpu` | ~0.1s |
| `sio network` | ~0.3s |
| `sio pci` | ~1.5s |

### T10: Admin-Gated Features

| Feature | Result |
|---------|--------|
| SMART C: (NVMe) | PASS (47C, 22216h, 97% spare) |
| SMART D: (SATA) | PASS (38C, 27293h) |
| Admin hint hidden | PASS (elevated correctly detected) |
| DIMM details | PASS (4x Kingston DDR4) |
| CPU microcode | PASS (0x7D103008) |
| Motherboard serial | PASS (211092904301164) |

---

## Issues Found

None. All 36 automated tests and all manual tests passed.

## Known Limitations (documented, not bugs)

- `sio pcie` shows "No PCIe devices detected" — requires WinRing0 for PCI config space
- `sio battery` shows "No batteries detected" — expected on desktop
- `sio -f xml` / `sio -f html` — requires compile-time features
- `sio board` uses Rust Debug format, not pretty text — upstream display choice
- Network `if-type:53` for TAP adapters — Windows IfType code
- Execution ~2s — WMI/PowerShell query overhead
