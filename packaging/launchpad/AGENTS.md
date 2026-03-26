# Launchpad PPA Packaging - Agent Context

## Purpose

Debian packaging templates for building source packages uploaded to a
Launchpad PPA. The `debian/` files here are **templates** — the CI workflow
copies them into a build directory, then modifies them per Ubuntu series.

## Key Sources

- `packaging/launchpad/README.md` — how the build process works, version
  format, series-specific handling, and local testing instructions.
- `.github/workflows/publish-ppa.yml` — the workflow implementation.
  Inline comments document key gotchas: template `sed` modifications,
  source-only builds, orig tarball uniqueness via repack suffix, and the
  GPG keyserver retry mechanism.
- `.github/workflows/gpg-keyserver-retry.yml` — GPG key propagation retry
  loop (dispatched automatically when needed).
- `PACKAGING.md` — one-time setup guide for secrets, GPG keys, Launchpad
  account, and GitHub environment configuration.
