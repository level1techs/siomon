# AUR Packaging - Agent Context

## What This Directory Contains

`PKGBUILD` is an Arch Linux package build script for the AUR. It is a
**template** — the CI workflow overwrites `pkgver`, `_tag`, `pkgrel`, and
`b2sums` at build time via `sed` and `updpkgsums`.

## Workflow

The workflow at `.github/workflows/publish-aur.yml` builds inside an
`archlinux:base-devel` container. Key details:

- `makepkg` refuses to run as root. A `builder` user is created and all
  `makepkg`/`updpkgsums` calls use `su builder -c "..."`.
- The AUR API at `https://aur.archlinux.org/rpc/v5/info?arg[]=siomon` is
  queried to auto-detect the current version and increment `pkgrel` when the
  `pkgver` matches. New upstream versions reset `pkgrel` to `1`.
- SSH to the AUR uses `GIT_SSH_COMMAND` with `StrictHostKeyChecking=accept-new`
  rather than `ssh-keyscan` or config files (more reliable in containers).
- The `arch=()` field must list compiled architectures (`x86_64 aarch64`), not
  `any` — `namcap` will reject ELF binaries in an `any` package.
- `depends=('glibc' 'libgcc')` are the runtime dependencies.
- The `package()` function must install the LICENSE file to
  `$pkgdir/usr/share/licenses/$pkgname/LICENSE`.

## Version Format

AUR versions follow `{pkgver}-{pkgrel}` (e.g., `0.2.2-1`). The `_tag` field
holds the full commit hash for reproducibility.
