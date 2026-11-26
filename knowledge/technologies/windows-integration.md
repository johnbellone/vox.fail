---
type: Technology
title: "Windows Integration: Hotkeys, OBS Virtual Camera, Installer"
description: "The Windows-only pieces, how they work, and the fact that none of them has run on Windows."
tags: [technology, windows, obs, nsis, installer]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
code_refs: [bins/block-avatar/src/controls/hotkeys.rs, bins/block-avatar/src/output/webcam.rs, bins/block-avatar/build.rs, bins/block-avatar/installer/block-avatar.nsi]
---

# Windows Integration

All of this is type-checked on Linux (`RUSTFLAGS="--cfg winchk" cargo check`) and unit-tested where possible, but **has never run on Windows**.

- **Global hotkeys** (`hotkeys.rs`): Win32 `RegisterHotKey` on a thread with a message queue; `WM_APP` re-registers. Combos in `hotkeys.txt`. Review item: possible race between creating the queue and publishing the thread id.
- **Virtual webcam** (`webcam.rs`): writes NV12 frames into OBS Studio's shared-memory queue (`OBSVirtualCamVideo`) byte-for-byte like OBS's `shared-memory-queue.c`; unit-tested against a port of OBS's reader. The "OBS Virtual Camera" DirectShow device must be registered by installing OBS (start and stop its camera once). Only one writer at a time. Apps using only Windows' newest camera system may not list it.
- **Console**: attaches a console for `--list-mics` (`windows-sys` `Win32_System_Console`).
- **Icon and version info**: `build.rs` with `winresource` embeds `assets/icon.ico`.
- **Installer**: NSIS (`bins/block-avatar/installer/block-avatar.nsi`): Program Files, Start menu, optional desktop shortcut, optional firewall rule for TCP 7778 (pointless: the server binds localhost), "Your files" shortcut to `%APPDATA%\Block Avatar`, uninstall leaves user files. It ships `docs/avatar/manual.md` as `README.md`. Built by `.github/workflows/windows.yml`.
- **Windows dev setup** (documented in `docs/avatar/manual.md`): VS Build Tools (C++ workload), rustup, OBS, ffmpeg (all via winget), then `cargo test --release`.

Verification checklist for a first Windows run is in `status/verification-gaps`.
