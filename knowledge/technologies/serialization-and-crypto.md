---
type: Technology
title: "Serialization, Cryptography and Randomness"
description: "Libraries for the binary wire format, JSON, signing, password hashing and random numbers."
tags: [technology, postcard, serde, ed25519, argon2, security]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Serialization and Cryptography

| Need | Library | Where |
| --- | --- | --- |
| Binary wire format | `postcard` 1 (compact, serde-based) | world protocol (`vox-core/wire.rs`), show protocol (`crates/vox-world/proto.rs`), token claims |
| JSON | `serde_json` 1 | levels, data files, API, `/state` (built with `json!`, no format strings) |
| Derive | `serde` 1 | everywhere |
| Token signing | `ed25519-dalek` 2 + `base64` 0.22 (URL-safe, no padding) | `vox-core/auth.rs` |
| Password hashing | `argon2` 0.5 (Argon2id) | directory |
| Hashing | `sha2` 0.10 | session cookie storage |
| Content hashing | `blake3` 1 (first 16 bytes name a region blob) | `vox-format` |
| Compression | `zstd` 0.14 (native encode and decode), `ruzstd` 0.8 (pure-Rust decode, also in wasm) | `vox-format` |
| Randomness | `getrandom` 0.3 (`wasm_js` on wasm) | tokens, ids, salts |

Postcard has no schema evolution, so any message change needs a `PROTOCOL` bump (`architecture/wire-protocol`). See `architecture/identity-and-tokens`.
