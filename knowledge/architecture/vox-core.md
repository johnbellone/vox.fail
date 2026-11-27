---
type: Component
title: "vox-core: Shared Rules, Wire Format and Tokens"
description: "The engine-free crate that holds block chunks, edit checks, ray casts, body movement, levels, weather, the world wire format and token signing."
tags: [component, vox-core, rust, blocks]
generated: { by: agent/cli, at: "2026-11-26T23:26:41Z" }
status: stable
code_refs: [crates/vox-core/src/]
---

# vox-core: Shared Rules, Wire Format, Moves and Tokens

Path `crates/vox-core/`, 2,543 lines in 10 files, 51 unit tests. **No Bevy.** Depends on `serde`, `serde_json`, `postcard`, `ed25519-dalek`, `base64`. Used by the server, directory, relay-free clients and the avatar app.

| Module | What it holds |
| --- | --- |
| `world.rs` | Blocks in **32 x 32 x 32 chunks**, id 0 = air, per-chunk version, `Cell = [i32;3]`, `Bounds`, the checks an edit must pass. `Why`: `Outside`, `NoSuchBlock`, `NoChange`, `TooFar`, `TooFast`, `Claimed`. |
| `ray.rs` | Voxel raycast (Amanatides and Woo). |
| `body.rs` | Moving a box through blocks: solids, one-block steps, falling, ceilings. |
| `moves.rs` | **The server's check on reported positions**: a distance budget per player refilled at the fastest a player can go (11 m/s sideways, 9 up; falls to 70), capped at about two seconds for lag; spawn always reachable. Positions are in blocks. |
| `level.rs` | Level JSON: boxes (solid, hollow, round) and single blocks; the `"world"` object (sky, time, weather, camera, hotbar, portals). |
| `predict.rs` | Server-confirmed blocks plus the player's pending edits; a refused edit disappears. |
| `wire.rs` | `ToServer`/`ToClient`, postcard, `PROTOCOL = 7`. See `architecture/wire-protocol`. |
| `auth.rs` | Ed25519 tokens. See `architecture/identity-and-tokens`. |
| `weather.rs` | Atmosphere simulation. See `architecture/weather-simulation`. |

## Why it exists
So server, directory and clients run the *same* code for edit validation, prediction, movement checks and token verification. See `decisions/core-without-bevy`. It is also the home of the planned deterministic terrain generator (`architecture/world-scaling-design`).

## Rules for contributors (AGENTS.md)
Everything from the network is untrusted: checked arithmetic, reject non-finite floats, cap sizes. Code shared with wasm avoids threads and `std::time`.

# Related Concepts
- [World Wire Protocol (PROTOCOL = 7)](wire-protocol.md): its wire messages
- [Weather Simulation](weather-simulation.md): its weather model
- [Identity, Sessions and Signed Launch Tokens](identity-and-tokens.md): its token code
- [Decision: Keep vox-core Free of Bevy](../decisions/core-without-bevy.md): why it has no engine
