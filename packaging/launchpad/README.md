# Launchpad PPA Packaging

This directory contains the Debian packaging templates for building source
packages that are uploaded to a
[Launchpad PPA](https://launchpad.net/~level1techs/+archive/ubuntu/siomon).

## Files

```
debian/
  changelog    -- Template placeholder, overwritten per series by the workflow
  control      -- Package metadata and build dependencies
  copyright    -- DEP-5 format copyright file
  rules        -- Build rules (cargo build + manual install)
  source/
    format     -- Source format (3.0 quilt)
    options    -- dpkg-source options
```

## How It Works

The GitHub Actions workflow (`.github/workflows/publish-ppa.yml`) runs on
`ubuntu-latest` and:

1. Clones the upstream repo at the release tag and vendors all Rust
   dependencies with `cargo vendor`.
2. Removes Windows-only binaries (`.dll`, `.a`) from vendored crates and
   updates `.cargo-checksum.json` files to match.
3. **Queries the Launchpad API**
   (`https://api.launchpad.net/1.0/~{user}/+archive/ubuntu/{ppa}?ws.op=getPublishedSources`)
   to find the highest published repack suffix (`+dsN`) and revision for
   the upstream version. Increments the repack by 1 and resets revision to 1.
4. Creates an orig tarball (`siomon_{version}+ds{repack}.orig.tar.gz`).
5. For each Ubuntu series, copies the `debian/` templates, writes a
   series-specific changelog, adjusts build dependencies (Noble uses
   `cargo-1.85`/`rustc-1.85`), and runs `debuild -S`.
6. Signs all `.changes` files with GPG via `debsign`.
7. **Ensures the GPG key is on `keyserver.ubuntu.com`** — checks
   retrievability; if missing, publishes the key and dispatches the
   `gpg-keyserver-retry.yml` workflow, which uses a `gpg-retry-delay`
   environment with a 20-minute wait timer (no runner cost during
   wait). The retry workflow self-dispatches until the key propagates
   (6-hour timeout), then re-dispatches the PPA workflow.
8. Uploads to the PPA via `dput` over SFTP (when upload is enabled).

## Version Auto-Increment

The workflow queries the Launchpad REST API before building to find the
highest published repack suffix (`+dsN`) and revision for the current
upstream version across all series. For a new upstream version (no existing
entries), repack starts at 1. For re-runs of the same version, repack is
incremented by 1 to produce a fresh orig tarball name. Revision always
resets to 1. There are no manual `revision` or `repack` inputs — everything
is fully automatic.

Version format: `{upstream}+ds{repack}-0ppa{revision}~{series}1`

Example progression:
- First publish of `0.2.2`: `0.2.2+ds1-0ppa1~noble1`
- Re-run: `0.2.2+ds2-0ppa1~noble1`
- Re-run again: `0.2.2+ds3-0ppa1~noble1`

## Series-Specific Handling

| Series | Rust Packages | Cargo Binary |
|--------|--------------|--------------|
| Noble (24.04) | `cargo-1.85`, `rustc-1.85` | `cargo-1.85` |
| Others (e.g., Questing) | `cargo`, `rustc` | `cargo` |

The workflow uses `sed` to adjust `debian/control` (Build-Depends) and
`debian/rules` (cargo binary name) per series.

## Local Testing

To build a source package locally:

```bash
# Install dependencies
sudo apt install devscripts debhelper fakeroot cargo rustc

# From a directory containing the orig tarball and extracted source
debuild -S -sa -d -us -uc
```

See [PACKAGING.md](../../PACKAGING.md) for full setup and secrets
configuration.
