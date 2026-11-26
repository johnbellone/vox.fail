---
type: Technology
title: Technology Stack Overview
description: "One table of every technology in the project, what it is used for, and where its details live."
tags: [technology, stack, overview]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
---

# Technology Stack Overview

| Area | Technology | Used for | Details |
| --- | --- | --- | --- |
| Language | Rust, edition 2024, one Cargo workspace | Everything except asset scripts and web pages | `technologies/rust-workspace` |
| Engine | Bevy 0.19.1 (ECS, PBR, glTF, animation) | Avatar app, world rendering, clients | `technologies/bevy` |
| Browser | WebAssembly + WebGPU via wasm-bindgen | Player client in the browser | `technologies/webassembly-webgpu` |
| Servers | axum 0.8, tokio, tower-http, reqwest | directory, world server, relay | `technologies/server-stack` |
| Storage | SQLite via rusqlite (bundled); region blobs with BLAKE3 + zstd | directory.db, world.db, world snapshots | `technologies/sqlite-persistence` |
| Audio and speech | cpal, own DSP (YIN, Burg LPC), IMA ADPCM, whisper.cpp (optional) | Lip sync, network voice, captions | `technologies/audio-and-speech` |
| Wire and crypto | postcard, serde_json, Ed25519, Argon2id, SHA-256 | Protocols, tokens, passwords | `technologies/serialization-and-crypto` |
| Media | ffmpeg (external), jpeg-encoder, png, ab_glyph, MagicaVoxel, glTF | Shorts, comics, cards, assets | `technologies/media-and-asset-tools` |
| Windows | windows-sys, OBS shared-memory camera, NSIS, winresource | Hotkeys, webcam, installer | `technologies/windows-integration` |
| Hosting | Cloudflare Tunnel (cloudflared), systemd, one Linux host | vox.fail | `architecture/deployment` |
| Front end | React 19, Mantine 9, TypeScript 7, Bun; three.js | The site: sign-in, character maker, inventory, friends, play | `architecture/vox-client-and-web` |
| Scripting | Python 3 (PIL, Playwright), bash | Asset building, tests | `architecture/asset-pipeline` |
| CI | GitHub Actions | Visual tests, Windows release | `architecture/testing-and-ci` |
| Agent tooling | okf (memory), beads (tasks, not installed yet), pi + MCP | Agent work | `technologies/dev-and-agent-tooling` |

## Platforms
- **Linux**: primary development and test platform. Everything builds and runs (256 tests pass).
- **Windows**: the target for streamers. Type-checked only (`--cfg winchk`), never run.
- **Browser**: Chromium with WebGPU verified headless (software renderer).
- **macOS**: never built.

## Versions seen on 2026-11-26
rustc/cargo 1.100.0 locally; no `rust-toolchain` file (CI uses stable); `Cargo.lock` has about 700 packages; Bevy 0.19.1.

# Related Concepts
- [Rust and the Cargo Workspace](rust-workspace.md): Rust and Cargo
- [Bevy 0.19.1: How and Why It Is Used](bevy.md): Bevy
- [Server Stack: axum, tokio, tower-http, reqwest, tiny_http](server-stack.md): HTTP and WebSocket libraries
- [SQLite Persistence (rusqlite, bundled)](sqlite-persistence.md): SQLite
- [WebAssembly and WebGPU Browser Client](webassembly-webgpu.md): WebAssembly and WebGPU
- [Audio and Speech Technology](audio-and-speech.md): audio and speech
- [Serialization, Cryptography and Randomness](serialization-and-crypto.md): serialization and crypto
- [Media, Imaging and Asset Authoring Tools](media-and-asset-tools.md): media and asset tools
- [Windows Integration: Hotkeys, OBS Virtual Camera, Installer](windows-integration.md): Windows integration
- [Developer and Agent Tooling: okf, beads, pi, MCP](dev-and-agent-tooling.md): okf, beads, pi, MCP
