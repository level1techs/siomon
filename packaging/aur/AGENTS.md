# AUR Packaging - Agent Context

## Purpose

PKGBUILD template for the Arch User Repository (AUR). Contains template
values for `pkgver`, `_tag`, `pkgrel`, and `b2sums` that the CI workflow
overwrites at build time via `sed` and `updpkgsums`.

## Key Sources

- `packaging/aur/README.md` — how the build process works, version format,
  and local testing instructions.
- `.github/workflows/publish-aur.yml` — the workflow implementation.
  Inline comments document why a non-root builder user is created.
- `packaging/aur/PKGBUILD` — the template itself. Inline comments document
  the architecture field requirement.
- `PACKAGING.md` — one-time setup guide for secrets, SSH keys, and AUR
  account configuration.
