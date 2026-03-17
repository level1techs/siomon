#!/bin/bash
# cli_smoke_test.sh — Windows CLI smoke tests for sio.exe
# Run from the repo root in an elevated prompt.
# Usage: bash tests/cli_smoke_test.sh [path-to-sio.exe]

SIO="${1:-./target/x86_64-pc-windows-msvc/release/sio.exe}"
FAIL=0
PASS=0
TOTAL=0

check() {
    TOTAL=$((TOTAL + 1))
    if [ "$1" -eq 0 ]; then
        PASS=$((PASS + 1))
        echo "  PASS: $2"
    else
        FAIL=$((FAIL + 1))
        echo "  FAIL: $2"
    fi
}

echo "=== sio CLI Smoke Tests ==="
echo "Binary: $SIO"
echo ""

# --- Version and help ---
echo "-- Version / Help --"
$SIO --version 2>&1 | grep -q "sio 0\." ; check $? "T6.1 --version"
$SIO --help 2>&1 | grep -q "Usage:" ; check $? "T6.2 --help"

# --- All 12 subcommands produce output ---
echo ""
echo "-- Subcommand text output --"
for cmd in cpu gpu memory storage network pci usb audio battery board pcie sensors; do
    lines=$($SIO $cmd 2>&1 | wc -l)
    [ "$lines" -gt 0 ] ; check $? "T2 $cmd text ($lines lines)"
done

# --- All 12 subcommands produce valid JSON ---
echo ""
echo "-- Subcommand JSON output --"
for cmd in cpu gpu memory storage network pci usb audio battery board pcie sensors; do
    $SIO $cmd -f json 2>&1 | python -c "import sys,json; json.load(sys.stdin)" 2>/dev/null
    check $? "T2 $cmd JSON valid"
done

# --- JSON top-level field checks ---
echo ""
echo "-- JSON field validation --"
$SIO -f json 2>&1 | python -c "
import sys,json
d=json.load(sys.stdin)
checks = [
    ('hostname populated', d['hostname'] not in ['unknown','']),
    ('cpus non-empty', len(d['cpus']) > 0),
    ('cpu cores > 0', d['cpus'][0]['topology']['physical_cores'] > 0),
    ('memory total > 0', d['memory']['total_bytes'] > 0),
    ('network non-empty', len(d['network']) > 0),
    ('pci non-empty', len(d['pci_devices']) > 0),
    ('usb non-empty', len(d['usb_devices']) > 0),
    ('audio non-empty', len(d['audio']) > 0),
    ('motherboard chipset', d['motherboard'].get('chipset') is not None),
    ('bios date format', d['motherboard']['bios'].get('date','').count('-') == 2),
]
for name, ok in checks:
    print(f'{1 if ok else 0}|{name}')
" 2>&1 | while IFS='|' read -r code name; do
    [ "$code" = "1" ] ; check $? "T1.3 $name"
done

# --- Sensor count ---
echo ""
echo "-- Sensor snapshot --"
count=$($SIO sensors -f json 2>&1 | python -c "import sys,json; print(len(json.load(sys.stdin)))" 2>&1)
[ "$count" -ge 100 ] ; check $? "T3.2 sensor count >= 100 (got: $count)"

# --- Output format gating ---
echo ""
echo "-- Output formats --"
$SIO -f text 2>&1 | grep -q "sio" ; check $? "T5.2 text format"
$SIO -f json 2>&1 | python -c "import sys,json; json.load(sys.stdin)" 2>/dev/null ; check $? "T5.3 JSON valid"
$SIO -f xml 2>&1 | grep -q "not available" ; check $? "T5.4 xml feature-gated"
$SIO -f html 2>&1 | grep -q "not available" ; check $? "T5.5 html feature-gated"

# --- Error handling ---
echo ""
echo "-- Error handling --"
$SIO invalidcmd 2>&1 | grep -q "unrecognized subcommand" ; check $? "T7.1 invalid subcommand"
$SIO -f yaml 2>&1 | grep -q "invalid value" ; check $? "T7.2 invalid format"

# --- CLI flags ---
echo ""
echo "-- CLI flags --"
$SIO --no-nvidia gpu 2>&1 | wc -l | grep -q "^0$" ; check $? "T6.3 --no-nvidia (empty GPU)"
$SIO --color never 2>&1 | grep -q "sio" ; check $? "T6.4 --color never"
$SIO --color always 2>&1 | grep -q "sio" ; check $? "T6.5 --color always"

# --- Summary ---
echo ""
echo "==============================="
echo "Results: $PASS passed, $FAIL failed, $TOTAL total"
[ "$FAIL" -eq 0 ] && echo "ALL TESTS PASSED" || echo "SOME TESTS FAILED"
exit $FAIL
