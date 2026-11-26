---
type: Technology
title: WebAssembly and WebGPU Browser Client
description: "How the Rust client is compiled for the browser, its size, browser requirements and the verified test setup."
tags: [technology, wasm, webgpu, browser]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# WebAssembly and WebGPU

- Target `wasm32-unknown-unknown`; `wasm-bindgen` generates JS glue (`--target web --no-typescript`); `tools/dev/build_web.sh` sets `RUSTFLAGS='--cfg getrandom_backend="wasm_js"'` and falls back to `-Zbuild-std` when the target is not installed.
- Bevy with `webgpu` feature; `web-sys` WebSocket for the world link; files fetched before start (no disk).
- Output `apps/site/public/pkg/` (git-ignored, built per release): ~50 MB wasm, ~11 MB gzipped; pre-compressed (`.gz`, `.br`) and served as such by the directory.
- **Requires WebGPU** (current Chrome, Edge, Safari, Firefox claimed). No WebGL2 fallback.
- Verified only in headless Chromium on SwiftShader with `tools/test/web_e2e.py`. Never checked on a real GPU or on mobile.
- Cloudflare must cache `/pkg/` (see `architecture/deployment`).

See `architecture/vox-client-and-web`. Risk 28 (size, WebGPU-only) is in `status/risks-and-open-issues`.
