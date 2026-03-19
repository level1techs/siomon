# Launchpad PPA Packaging - Agent Context

## What This Directory Contains

The `debian/` directory holds Debian packaging templates. These are **not used
directly** — the CI workflow copies them into a build directory, then overwrites
`debian/changelog` and uses `sed` to adjust `debian/control` and `debian/rules`
per Ubuntu series.

## Workflow

The workflow at `.github/workflows/publish-ppa.yml` runs on `ubuntu-latest`.
Key details:

- **Source-only builds**: `debuild -S` creates source packages without
  compiling. Launchpad's build farm does the actual compilation in clean
  chroots. This means all series can be built on a single runner.
- **Cargo vendoring**: Dependencies are vendored into the orig tarball.
  Windows binaries (`.dll`, `.a`) are stripped, and `.cargo-checksum.json`
  files are updated with `jq` to remove their entries.
- **Version auto-increment**: The Launchpad REST API is queried at
  `https://api.launchpad.net/1.0/~{user}/+archive/ubuntu/{ppa}?ws.op=getPublishedSources&source_name=siomon`
  to find the highest existing repack suffix (`+dsN`) and revision for the
  upstream version. For new upstream versions (no existing entries), repack
  starts at 1. For re-runs of the same version, repack is incremented by 1.
  Revision always resets to 1. There are no manual `revision` or `repack`
  inputs.
- **Version format**: `{upstream}+ds{repack}-0ppa{revision}~{series}1`
  (e.g., `0.2.2+ds1-0ppa1~noble1`). The `+ds{repack}` suffix indicates a
  repacked orig tarball (Debian convention for modified upstream sources).
- **Series handling**: Noble (24.04) uses versioned Rust packages
  (`cargo-1.85`, `rustc-1.85`) because its default Rust is too old. Other
  series use the standard `cargo`/`rustc` packages.
- **GPG signing**: Non-interactive via `gpg --batch --pinentry-mode loopback`.
  Passphrase is optional — the workflow conditionally adds `--passphrase-file`
  only when `PKG_GPG_PASSPHRASE` is non-empty.
- **Orig tarball uniqueness**: For re-runs of the same upstream version, the
  repack suffix is incremented so each run produces a unique orig tarball
  name. This avoids Launchpad's rejection of re-uploads with different
  content under the same tarball name.
- **GPG keyserver propagation**: Before uploading, the workflow checks if the
  signing GPG key is retrievable from `keyserver.ubuntu.com`. If not, it
  publishes the key with `--send-keys`, stores retry parameters (including a
  `started_at` timestamp) in a `PPA_GPG_RETRY` repository variable, and
  dispatches `gpg-keyserver-retry.yml`. That workflow uses a `gpg-retry-delay`
  GitHub environment with a 20-minute wait timer — the runner is not allocated
  during the wait, so there is no runner cost. Once the key is available, the
  retry workflow deletes the variable and dispatches the full PPA workflow. If
  the key isn't available yet, the workflow dispatches itself for the next
  check. A 6-hour timeout prevents infinite retries. The `gpg-retry-delay`
  environment must be created once in repo Settings → Environments with a
  20-minute wait timer protection rule.

## Key Files

- `debian/control` — Build-Depends uses generic `cargo, rustc (>= 1.85)` as
  a template. The workflow `sed`-replaces this per series.
- `debian/rules` — Uses `cargo build --frozen --release --all-features` and
  manually installs the binary. References `cargo` generically; `sed`-replaced
  to `cargo-1.85` for Noble.
- `debian/copyright` — DEP-5 format covering MIT, Apache-2.0, BSD-2-Clause,
  BSD-3-Clause, ISC, and Zlib licenses from vendored crates.
- `debian/changelog` — Placeholder only. Overwritten by the workflow with
  series-specific version and timestamp.
- `debian/source/format` — `3.0 (quilt)`.
- `debian/source/options` — `extend-diff-ignore = "\.orig$"` to avoid noise
  from backup files.
