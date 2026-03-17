# PHASE EPSILON: CI, Release, and winget Distribution

**Repo:** `arndawg/siomon-win` (fork of `level1techs/siomon`)
**Branch:** `windows-port`
**Predecessor:** PHASE_DELTA (completed 2026-03-16)
**Goal:** Add Windows to CI, produce tagged releases with GitHub Actions, and submit
a winget package manifest following the same pattern as `arndawg/tmux-windows`.

---

## Context

Phases Alpha through Delta built a complete Windows port with 154+ active sensors,
zero build warnings, and full data-quality parity. The upstream CI
(`.github/workflows/ci.yml`) runs Linux-only jobs (check, clippy, fmt, test, build).
The release workflow builds Linux x86_64 and aarch64 only.

This phase adds Windows to both pipelines, creates a GitHub Release with a Windows
zip artifact, and submits a winget manifest to `microsoft/winget-pkgs` — the same
distribution path used by `arndawg/tmux-windows`.

---

## Work Items

### E1. Add Windows Jobs to CI Workflow

**Effort:** Low (1 day)

Add a `windows-check` and `windows-build` job to `.github/workflows/ci.yml` that
run on `windows-latest` with the MSVC toolchain.

**Implementation:**

Add after the existing `build-minimal` job:

```yaml
  windows-check:
    name: Check (Windows)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc
      - uses: Swatinem/rust-cache@v2
        with:
          key: windows
      - run: cargo check --target x86_64-pc-windows-msvc

  windows-build:
    name: Build Release (Windows)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc
      - uses: Swatinem/rust-cache@v2
        with:
          key: windows
      - run: cargo build --release --target x86_64-pc-windows-msvc
      - uses: actions/upload-artifact@v4
        with:
          name: sio-windows-x86_64
          path: target/x86_64-pc-windows-msvc/release/sio.exe

  windows-test:
    name: Test (Windows)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc
      - uses: Swatinem/rust-cache@v2
        with:
          key: windows
      - run: cargo test --target x86_64-pc-windows-msvc
```

**Note:** `--all-features` cannot be used on Windows because the `nvidia` feature
pulls in `libloading` which is fine, but clippy/test may have platform-specific
issues. Use default features on Windows.

**Acceptance criteria:**
- CI runs on every push/PR
- Windows check, build, and test all pass
- Linux jobs remain unchanged

---

### E2. Add Windows to Release Workflow

**Effort:** Medium (1-2 days)

Extend `.github/workflows/release.yml` to build Windows x86_64 alongside the
existing Linux targets, and include `sio.exe` as a zip in the GitHub Release.

**Implementation:**

Add to the release matrix:

```yaml
matrix:
  include:
    - target: x86_64-unknown-linux-gnu
      artifact: sio-linux-x86_64
      os: ubuntu-latest
    - target: aarch64-unknown-linux-gnu
      artifact: sio-linux-aarch64
      os: ubuntu-latest
    - target: x86_64-pc-windows-msvc
      artifact: sio-windows-x86_64
      os: windows-latest
```

Update the `runs-on` to use `${{ matrix.os }}`.

For the Windows package step, produce a zip instead of tar.gz:

```yaml
- name: Package (Windows)
  if: contains(matrix.target, 'windows')
  shell: pwsh
  run: |
    $version = "${{ github.ref_name }}"
    $zipName = "sio-windows-x86_64-$version.zip"
    Compress-Archive -Path "target/${{ matrix.target }}/release/sio.exe" -DestinationPath $zipName
    $hash = (Get-FileHash $zipName -Algorithm SHA256).Hash
    "$hash  $zipName" | Out-File -Encoding UTF8 "$zipName.sha256"

- name: Package (Linux)
  if: "!contains(matrix.target, 'windows')"
  run: |
    cd target/${{ matrix.target }}/release
    tar czf ../../../${{ matrix.artifact }}.tar.gz sio
```

**Acceptance criteria:**
- `v*` tag push produces GitHub Release with:
  - `sio-linux-x86_64.tar.gz`
  - `sio-linux-aarch64.tar.gz`
  - `sio-windows-x86_64-vX.Y.Z.zip` + `.sha256`
- Release notes auto-generated

---

### E3. Create Initial Tagged Release

**Effort:** Low (0.5 day)

Create the first Windows-capable release tag on `arndawg/siomon-win` to generate
the GitHub Release artifact needed for the winget manifest.

**Steps:**
1. Ensure CI passes on both Linux and Windows
2. Tag: `git tag v0.1.3-win.1`
3. Push tag: `git push origin v0.1.3-win.1`
4. Verify GitHub Release appears with `sio-windows-x86_64-v0.1.3-win.1.zip`
5. Note the SHA256 from the `.sha256` file

**Versioning scheme:** `v0.1.3-win.N` — matches upstream version with a `-win.N`
suffix for Windows-specific releases. When upstream accepts the PR, releases will
use upstream's version tags.

---

### E4. Submit winget Package Manifest

**Effort:** Medium (1-2 days)

Submit a PR to `microsoft/winget-pkgs` following the same pattern as other
community packages.

**Manifest structure** (following
[winget manifest schema v1.6](https://github.com/microsoft/winget-pkgs/tree/master/doc/manifest)):

```
manifests/a/arndawg/sio/0.1.3-win.1/
  arndawg.sio.yaml                    # singleton manifest
```

Or the multi-file format:

```
manifests/a/arndawg/sio/0.1.3-win.1/
  arndawg.sio.installer.yaml
  arndawg.sio.locale.en-US.yaml
  arndawg.sio.yaml
```

**Installer manifest** (`arndawg.sio.installer.yaml`):

```yaml
PackageIdentifier: arndawg.sio
PackageVersion: 0.1.3-win.1
InstallerType: zip
Installers:
  - Architecture: x64
    InstallerUrl: https://github.com/arndawg/siomon-win/releases/download/v0.1.3-win.1/sio-windows-x86_64-v0.1.3-win.1.zip
    InstallerSha256: <sha256-from-release>
    NestedInstallerType: portable
    NestedInstallerFiles:
      - RelativeFilePath: sio.exe
        PortableCommandAlias: sio
ManifestType: installer
ManifestVersion: 1.6.0
```

**Locale manifest** (`arndawg.sio.locale.en-US.yaml`):

```yaml
PackageIdentifier: arndawg.sio
PackageVersion: 0.1.3-win.1
PackageLocale: en-US
Publisher: arndawg
PublisherUrl: https://github.com/arndawg
PackageName: sio
PackageUrl: https://github.com/arndawg/siomon-win
License: MIT
ShortDescription: Hardware information and sensor monitoring tool
Description: >-
  sio is a cross-platform hardware information and real-time sensor monitoring
  tool. It provides CPU, memory, storage, GPU, network, and motherboard
  details with a TUI dashboard for live sensor monitoring.
Tags:
  - hardware
  - sensors
  - monitoring
  - system-info
  - tui
ManifestType: defaultLocale
ManifestVersion: 1.6.0
```

**Version manifest** (`arndawg.sio.yaml`):

```yaml
PackageIdentifier: arndawg.sio
PackageVersion: 0.1.3-win.1
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
```

**Submission steps:**
1. Fork `microsoft/winget-pkgs`
2. Create branch with manifest files
3. Run `winget validate` locally
4. Submit PR (bots will auto-test)

**Acceptance criteria:**
- `winget install arndawg.sio` downloads and installs `sio.exe`
- `sio --version` works after install
- `winget upgrade arndawg.sio` works for future versions

---

### E5. Update README with Windows Instructions

**Effort:** Low (0.5 day)

Add a Windows section to the project README.md covering:

1. **Installation via winget:** `winget install arndawg.sio`
2. **Manual installation:** download zip from GitHub Releases
3. **Building from source:** `cargo build --release --target x86_64-pc-windows-msvc`
4. **Admin privileges:** note about running elevated for SMART data
5. **WinRing0:** optional driver for SuperIO/RAPL/PCIe sensors

---

### E6. Fix Linux CI Compatibility

**Effort:** Low-Medium (1 day)

The Windows port added new files and modified existing ones. Ensure all Linux CI
jobs still pass:

1. `cargo check --all-features` on Linux
2. `cargo clippy --all-features -- -D warnings -A dead_code` on Linux
3. `cargo fmt --check`
4. `cargo test --all-features`

Fix any issues found (e.g., `#[cfg(windows)]` modules that clippy flags, format
differences, test failures).

**Acceptance criteria:**
- All 6 existing Linux CI jobs pass
- All 3 new Windows CI jobs pass
- `cargo fmt --check` passes (no formatting drift)

---

## Work Item Dependencies

```
E6 (Fix Linux CI)    → should be first (ensures nothing is broken)
E1 (Windows CI)      → independent, but run after E6 to verify
E2 (Release workflow) → after E1 (CI must pass before release)
E3 (Tagged release)   → after E2 (workflow must exist)
E4 (winget manifest)  → after E3 (needs release URL + SHA256)
E5 (README)           → after E4 (reference winget install command)
```

---

## Implementation Order

| Priority | Item | Effort | Notes |
|----------|------|--------|-------|
| 1 | E6: Fix Linux CI compatibility | Low-Med | Prerequisite — don't break existing CI |
| 2 | E1: Windows CI jobs | Low | Add check + build + test for Windows |
| 3 | E2: Windows release workflow | Medium | Zip + SHA256 artifact alongside Linux tar.gz |
| 4 | E3: Create tagged release | Low | `v0.1.3-win.1` tag triggers release |
| 5 | E4: winget manifest PR | Medium | Submit to microsoft/winget-pkgs |
| 6 | E5: README update | Low | Installation docs |

---

## Exit Criteria

PHASE EPSILON is complete when:

1. GitHub Actions CI passes for both Linux and Windows on every push
2. Tagged release produces downloadable `sio.exe` zip with SHA256
3. `winget install arndawg.sio` works (or PR submitted, pending review)
4. README documents Windows installation and usage
5. PHASE_EPSILON section of `phase_status.json` updated to `"completed"`

---

## Future: PHASE ZETA (potential)

- **Upstream PR** to `level1techs/siomon` — rebase, squash, submit
- **ARM64 Windows** — `aarch64-pc-windows-msvc` target
- **WinRing0 live validation** — test SuperIO/RAPL on real hardware
- **Automated winget version bumps** — GitHub Action to create winget PRs on release
