---
type: Technology
title: Rust and the Cargo Workspace
description: "Edition, workspace members, build profiles, features, and target-specific dependencies."
tags: [technology, rust, cargo]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [Cargo.toml, Cargo.lock]
---

# Rust and the Cargo Workspace

- **Edition 2024** in every crate. Local toolchain 1.100.0; CI uses `dtolnay/rust-toolchain@stable`. No pinned toolchain file.
- **Workspace**: a *virtual* workspace at the root (`members = ["bins/*", "crates/*"]`, `resolver = "3"`), shared `[workspace.package]` (version, edition) and `[workspace.dependencies]` (members write `name.workspace = true`). A bare `cargo build` builds everything; use `-p <name>` for one. A bare `cargo run` is ambiguous because there are several binaries: use `-p block_avatar` (confirmed: the manual's `cargo run --release` errors out).
- **Profiles**: `dev` opt-level 1 with dependencies at 3 (so Bevy is usable in debug); `release` with `codegen-units = 16`; `wasm-release` inherits release with opt-level "s", thin LTO, strip, for the browser.
- **Feature**: `whisper` (optional) pulls `whisper-rs` 0.16 (needs CMake and a C++ compiler, plus LLVM on Windows).
- **Target-specific**: `windows-sys` 0.59 on Windows (and under `cfg(winchk)`), `libc` on unix, `winresource` build dependency on Windows, wasm-bindgen family on wasm32. Lint config allows the `winchk` cfg.
- **Style**: rustfmt-formatted. `unexpected_cfgs` warns.
- Linux system packages: `libasound2-dev libudev-dev` (and wayland/xkbcommon for CI).
- Build size: `target/` reached 13 GB; the first Bevy build takes minutes.

See `architecture/system-overview` for crate relationships.

# Related Concepts
- [Restructure and Follow-on Work](../status/restructure-and-follow-on-work.md): how the root became a virtual workspace
