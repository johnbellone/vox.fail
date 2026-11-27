---
type: Decision
title: "Decision: vox-client installers built by GitHub Actions"
description: "The desktop client ships as an NSIS installer and zip, a universal macOS dmg and a Linux AppImage and tar.gz, built by one reusable workflow, with a rolling nightly prerelease and releases on v* tags."
tags: [ci, installer, release, github-actions, packaging]
generated: { by: agent/pi-lead, at: "2026-11-27T14:10:34Z" }
status: stable
---

# Decision: vox-client installers built by GitHub Actions

**Decision**: one reusable workflow (`.github/workflows/client-build.yml`) builds vox-client for three systems and checks each package by running `vox-client --check` from outside the checkout:
- Windows: per-user NSIS installer (`bins/vox-client/package/windows/vox.nsi`, `%LOCALAPPDATA%\\Programs\\Vox`, no admin) and a portable zip.
- macOS: one universal `Vox.app` (arm64 + x86_64 via lipo) in a dmg (`package/macos/make-macos.sh`), data in `Contents/Resources`.
- Linux: AppImage (built on ubuntu-22.04 for an older glibc; appimagetool pinned to 1.9.1 with its sha256) and a tar.gz (`package/linux/make-linux.sh`), data in `usr/share/vox`.
`nightly.yml` builds the default branch daily at 05:17 UTC (or by hand with `force`), skips when the `nightly` tag already points at the commit, uploads to the rolling `nightly` prerelease and moves the tag last so a failed upload retries. `release.yml` builds on `v*` tags (version `x.y.z` or `x.y.z-pre`; `-` makes a prerelease) and always sets the title `Vox <version>`, since Block Avatar's `windows.yml` attaches to the same release.

**Why**: the payload (binary + `assets/` + `data/`, ~400 MB uncompressed) must sit where `find_root` in `bins/vox-client/src/main.rs` looks: next to the exe, `<dir>/Resources` or `<dir>/share/vox`. Installed copies keep the player's files (`controls.json`, `levels/`) in a per-user folder (`%APPDATA%\\Vox`, `~/Library/Application Support/Vox`, `$XDG_DATA_HOME/vox`), a source checkout (Cargo.toml at its root) keeps them in the checkout. One universal macOS build halves the macOS artefacts. Per-user Windows install avoids UAC.

**Consequences**: builds are unsigned until the secrets exist (WINDOWS_CERT_PFX/PASSWORD, MACOS_CERT_P12/PASSWORD, APPLE_ID/TEAM_ID/APP_PASSWORD); signing steps are gated by job-level HAVE_* flags because a step's own env is not visible to its `if:`. The macOS runner is `macos-latest` (macos-14 retired). Installed copies have no in-app place for the launch code yet (bead vox-q7c); players use `--token`. Release Windows builds have no console, so `--check`/`--version` attach to the parent console. Details for maintainers: `docs/vox/releases.md`.

Related: `architecture/deployment`, `architecture/testing-and-ci`.

# Related Concepts
- [Deployment: vox.fail on Cloudflare Tunnel, and Windows Packaging](../architecture/deployment.md): where the client packaging fits in deployment
- [Testing and Continuous Integration](../architecture/testing-and-ci.md): the workflows that build and check the installers
