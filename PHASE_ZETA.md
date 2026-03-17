# PHASE ZETA: Distribution and Upstream Merge

**Repo:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port` (merged to `main` on fork)
**Predecessor:** PHASE_EPSILON (completed 2026-03-16)
**Goal:** Get sio installable via `winget install arndawg.sio`, submit upstream PR
to level1techs/siomon, and automate winget version bumps on future releases.

---

## Context

PHASE EPSILON delivered working CI (Linux + Windows builds pass), a tagged release
(`v0.1.3-win.3`) with artifacts for all three platforms, and a validated winget
manifest. The `windows-port` branch has been merged to `main` on the fork. CI is
active and producing releases.

The winget manifest from EPSILON used schema 1.6.0. The tmux-windows project
(`arndawg.tmux-windows`) uses schema **1.10.0** and is the reference pattern for
this phase. The key differences are the schema version, top-level
`NestedInstallerType`, and `ReleaseDate` field.

---

## Work Items

### Z1. Verify fork main branch is ready

**Effort:** Low (already done in EPSILON)

The merge from `windows-port` to `main` was completed during EPSILON via GitHub API.
CI workflows are active and the `v0.1.3-win.3` tag triggered a successful build.

**Verify:**
- [ ] `main` branch has all Windows port commits
- [ ] CI passed on tag push (Release workflow: 3 builds succeeded)
- [ ] GitHub Release exists with all 4 assets

**Status:** Already complete. Mark done.

---

### Z2. Submit winget PR to microsoft/winget-pkgs

**Effort:** Low (1-2 hours)

Submit a PR to `microsoft/winget-pkgs` with the manifest for `arndawg.sio` version
`0.1.3-win.3`. Follow the exact pattern of `arndawg.tmux-windows`.

**Manifest files** (schema 1.10.0, matching tmux-windows pattern):

#### `manifests/a/arndawg/sio/0.1.3-win.3/arndawg.sio.installer.yaml`

```yaml
# yaml-language-server: $schema=https://aka.ms/winget-manifest.installer.1.10.0.schema.json

PackageIdentifier: arndawg.sio
PackageVersion: 0.1.3-win.3
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: sio.exe
  PortableCommandAlias: sio
ReleaseDate: 2026-03-16
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/arndawg/siomon-win/releases/download/v0.1.3-win.3/sio-windows-x86_64-v0.1.3-win.3.zip
  InstallerSha256: 4DE854D315D2C7D15C1455B4E1EF7B7FD838372D9DC2DC67CC44B29B8653BBB1
ManifestType: installer
ManifestVersion: 1.10.0
```

#### `manifests/a/arndawg/sio/0.1.3-win.3/arndawg.sio.locale.en-US.yaml`

```yaml
# yaml-language-server: $schema=https://aka.ms/winget-manifest.defaultLocale.1.10.0.schema.json

PackageIdentifier: arndawg.sio
PackageVersion: 0.1.3-win.3
PackageLocale: en-US
Publisher: arndawg
PublisherUrl: https://github.com/arndawg
PublisherSupportUrl: https://github.com/arndawg/siomon-win/issues
Author: arndawg
PackageName: sio
PackageUrl: https://github.com/arndawg/siomon-win
License: MIT
LicenseUrl: https://github.com/arndawg/siomon-win/blob/main/LICENSE
ShortDescription: Hardware information and sensor monitoring tool
Description: |-
  A cross-platform hardware information and real-time sensor monitoring tool.
  Provides detailed CPU, memory, storage, GPU, network, and motherboard
  information with an interactive TUI dashboard for live sensor monitoring
  including temperatures, fan speeds, voltages, power, and utilization metrics.

  Windows port of level1techs/siomon with 154+ real-time sensors, NVMe/SATA
  SMART health data, NVIDIA GPU monitoring via NVML, and full system inventory.
  Standalone single-file binary with no runtime dependencies.
ReleaseNotes: |-
  NVMe SMART fixed — corrected struct size causing Samsung and other NVMe
  drives to fail SMART reads. Admin detection via TokenElevation API.
  SMBIOS DIMM details, CPU microcode, network driver names, ACPI thermal
  zones, WHEA error monitoring. 36/36 CLI smoke tests passing.
  CI builds for Linux x64, Linux aarch64, and Windows x64.
Tags:
- cli
- hardware
- sensors
- monitoring
- system-info
- tui
- cpu
- gpu
- temperature
- nvme
- smart
ReleaseNotesUrl: https://github.com/arndawg/siomon-win/releases/tag/v0.1.3-win.3
ManifestType: defaultLocale
ManifestVersion: 1.10.0
```

#### `manifests/a/arndawg/sio/0.1.3-win.3/arndawg.sio.yaml`

```yaml
# yaml-language-server: $schema=https://aka.ms/winget-manifest.version.1.10.0.schema.json

PackageIdentifier: arndawg.sio
PackageVersion: 0.1.3-win.3
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.10.0
```

**Submission steps:**

```bash
# 1. Fork microsoft/winget-pkgs (if not already)
gh repo fork microsoft/winget-pkgs --clone --remote

# 2. Create branch
cd winget-pkgs
git checkout -b arndawg-sio-0.1.3-win.3

# 3. Create manifest directory
mkdir -p manifests/a/arndawg/sio/0.1.3-win.3

# 4. Write the three manifest files (from above)

# 5. Validate
winget validate manifests/a/arndawg/sio/0.1.3-win.3/

# 6. Commit and push
git add manifests/a/arndawg/sio/
git commit -m "New package: arndawg.sio version 0.1.3-win.3"
git push origin arndawg-sio-0.1.3-win.3

# 7. Create PR
gh pr create --repo microsoft/winget-pkgs \
  --title "New package: arndawg.sio version 0.1.3-win.3" \
  --body "## arndawg.sio 0.1.3-win.3

Hardware information and sensor monitoring tool for Windows.
Port of [level1techs/siomon](https://github.com/level1techs/siomon).

- Single-file portable binary (sio.exe)
- 154+ real-time sensors
- NVMe/SATA SMART, NVIDIA GPU via NVML
- TUI dashboard, JSON output, CSV logging
- MIT license"
```

**After submission:** Microsoft bots will validate the manifest, test installation,
and merge within 1-3 days if everything passes.

**Acceptance criteria:**
- `winget install arndawg.sio` installs sio.exe
- `sio --version` works immediately (in PATH via PortableCommandAlias)
- `winget upgrade arndawg.sio` works for future versions

---

### Z3. Upstream PR to level1techs/siomon

**Effort:** Medium (2-3 days)

Prepare and submit a PR adding Windows support to the upstream repository.

**Preparation steps:**

1. **Sync with upstream main:**
   ```bash
   git fetch upstream
   git rebase upstream/main
   # Resolve any conflicts from upstream changes
   ```

2. **Squash into logical commits** (suggested 4-5):
   - `Add Windows platform support (cfg gating + sysinfo collectors)`
   - `Add Windows hardware sensors (NVML, SMART, CPU freq, network, PCI/USB/audio)`
   - `Add WinRing0 integration for SuperIO/RAPL/HSMP/PCIe/SMBus`
   - `Add Windows CI and release workflow`
   - `Polish: SMBIOS, DIMM details, admin detection, README`

3. **PR description** should include:
   - Summary of what the Windows port provides
   - How cfg gating works (no impact on Linux builds)
   - How to build and test on Windows
   - List of Windows-specific dependencies
   - Known limitations and what requires WinRing0

4. **Ensure Linux CI still passes** after rebase

**Acceptance criteria:**
- PR passes upstream CI (Linux jobs)
- Windows CI jobs added and passing
- No regressions in Linux functionality
- Clean commit history

---

### Z6. Automated winget Version Bump on Release

**Effort:** Medium (1-2 days)

Add a GitHub Actions workflow that automatically submits a winget manifest update
PR when a new release is created. This mirrors the pattern used by many winget
packages for automatic updates.

**Implementation:** Add `.github/workflows/winget-update.yml`:

```yaml
name: Update winget manifest

on:
  release:
    types: [published]

jobs:
  winget:
    if: "!github.event.release.prerelease"
    runs-on: windows-latest
    steps:
      - uses: vedantmgoyal9/winget-releaser@v2
        with:
          identifier: arndawg.sio
          installers-regex: 'sio-windows-x86_64-.*\.zip$'
          token: ${{ secrets.WINGET_TOKEN }}
```

**Prerequisites:**
- Create a GitHub Personal Access Token with `public_repo` scope
- Add it as a repository secret named `WINGET_TOKEN`
- The `vedantmgoyal9/winget-releaser` action handles manifest creation, validation,
  and PR submission automatically

**Alternative (manual script):**
If the action doesn't fit, add a step to the Release workflow that:
1. Downloads the Windows zip from the release
2. Computes SHA256
3. Generates the three manifest YAML files
4. Forks winget-pkgs, creates branch, commits, submits PR via `gh`

**Acceptance criteria:**
- New release tag → winget PR created automatically
- Manifest passes `winget validate`
- No manual steps required for winget updates

---

## Work Item Dependencies

```
Z1 (Verify main)      → already done
Z2 (winget PR)         → needs Z1 confirmed
Z3 (Upstream PR)       → independent, can start anytime
Z6 (Automated winget)  → after Z2 is accepted (proves the manifest works)
```

---

## Implementation Order

| Priority | Item | Effort | Notes |
|----------|------|--------|-------|
| 1 | Z1: Verify main | Done | Already merged in EPSILON |
| 2 | Z2: winget PR | Low | Submit to microsoft/winget-pkgs |
| 3 | Z3: Upstream PR | Medium | Rebase, squash, submit to level1techs |
| 4 | Z6: Automated winget | Medium | After Z2 accepted |

---

## Out of Scope (deferred to PHASE ETA)

| Item | Reason |
|------|--------|
| Z4: ARM64 Windows | Requires aarch64-pc-windows-msvc cross-compile setup |
| Z5: WinRing0 live validation | Requires WinRing0 installation on multiple test systems |

---

## Exit Criteria

PHASE ZETA is complete when:

1. `winget install arndawg.sio` works (or PR submitted and pending review)
2. Upstream PR submitted to `level1techs/siomon` (or ready for submission)
3. Automated winget workflow configured (or documented for future setup)
4. `phase_status.json` updated

---

## Future: PHASE ETA (potential)

- ARM64 Windows (`aarch64-pc-windows-msvc`)
- WinRing0 live hardware validation on multiple platforms
- macOS port investigation
- Plugin/extension system for vendor-specific sensors
