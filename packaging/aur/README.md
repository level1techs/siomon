# AUR Packaging

This directory contains the PKGBUILD template for the
[Arch User Repository (AUR)](https://aur.archlinux.org/packages/siomon).

## Files

- `PKGBUILD` — Arch Linux package build script. Contains template values for
  `pkgver`, `_tag`, `pkgrel`, and `b2sums` that are updated automatically by
  the CI workflow at build time.

## How It Works

The GitHub Actions workflow (`.github/workflows/publish-aur.yml`) runs in an
`archlinux:base-devel` container and:

1. Resolves the release tag to a full commit hash via `git ls-remote`.
2. **Queries the AUR API** (`https://aur.archlinux.org/rpc/v5/info`) to
   determine the currently published version. If the upstream version
   (`pkgver`) already exists, `pkgrel` is incremented automatically. For a
   new upstream version, `pkgrel` resets to `1`.
3. Updates the PKGBUILD with the resolved `pkgver`, `_tag`, and `pkgrel`.
4. Recomputes checksums with `updpkgsums`.
5. Builds the package with `makepkg -s` (which runs `prepare()`, `build()`,
   `check()`, and `package()`).
6. Generates `.SRCINFO` and validates with `namcap`.
7. Pushes the updated PKGBUILD and .SRCINFO to the AUR git repo via SSH.

## Version Auto-Increment

The workflow fetches the current AUR version before building. The version
comparison logic:

- **Same `pkgver`**: increment `pkgrel` (e.g., `0.2.2-1` becomes `0.2.2-2`)
- **New `pkgver`**: reset `pkgrel` to `1` (e.g., `0.3.0-1`)

This ensures re-running the workflow for the same tag always produces a
publishable update without manual intervention.

## Local Testing

To test the PKGBUILD locally on an Arch Linux system:

```bash
cd packaging/aur
makepkg -s --noconfirm
namcap PKGBUILD
namcap *.pkg.tar.*
```

See [PACKAGING.md](../../PACKAGING.md) for full setup and secrets
configuration.
