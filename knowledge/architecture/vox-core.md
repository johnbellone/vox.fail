---
type: Component
title: "vox-core: Shared Rules, Wire Format and Tokens"
description: "The engine-free crate that holds block chunks, edit checks, ray casts, body movement, levels, weather, the world wire format and token signing."
tags: [component, vox-core, rust, blocks]
generated: { by: agent/mcp, at: "2026-11-27T20:44:34Z" }
status: stable
code_refs: [crates/vox-core/src/]
---

# vox-core: Shared Rules, Wire Format, Moves and Tokens

Path `crates/vox-core/`, about 6,200 lines in 16 files. **No Bevy.** Depends on `serde`, `serde_json`, `postcard`, `ed25519-dalek`, `base64`. Used by the server, directory, relay-free clients and the avatar app.

| Module | What it holds |
| --- | --- |
| `world.rs` | Blocks in **32 x 32 x 32 chunks**, id 0 = air, per-chunk version, `Cell = [i32;3]`, `Bounds`, the checks an edit must pass. `Why`: `Outside`, `NoSuchBlock`, `NoChange`, `TooFar`, `TooFast`, `Claimed`, `Blocked` (a placed prop stands there; last, as the wire numbers variants). |
| `ray.rs` | Voxel raycast (Amanatides and Woo). |
| `body.rs` | Moving a box through blocks: solids, one-block steps, falling, ceilings. Props add their own cells beside the blocks: `Occupancy`, `with_props`, and `prop_footprint_cells(at, yaw, footprint, height)` (the prop's 0.5 m footprint cells relative to its pivot's floor cell, turned to the nearest quarter turn, `height` blocks up), the same on the server and in the client's prediction. |
| `moves.rs` | **The server's check on reported positions**: a distance budget per player refilled at the fastest a player can go (11 m/s sideways, 9 up; falls to 70), capped at about two seconds for lag; spawn always reachable. Positions are in blocks. |
| `level.rs` | Level JSON: boxes (solid, hollow, round) and single blocks; the `"world"` object (sky, time, weather, camera, hotbar, portals). |
| `predict.rs` | Server-confirmed blocks plus the player's pending edits; a refused edit disappears. |
| `entity.rs` | **Entities**: the things placed in a world beside its blocks. `Component` is `Portal`, `Spawn`, `Objective`, `Settings`, `Prop`, `Node` (variant order is part of the format: new ones go last). `Prop {prop: [u8;16] (the definition's hash), at (5 cm units; `prop_cell` floors to blocks, 10 units a block), yaw (0..24, 15 degree steps), variant, state (0..=7), channel, text (<= 140), owner}`. `Node {kind, detail (0..=4), at, size (each side 1..=64), parent, recipe (JSON, <= 16 KiB), seed}`; `MAX_NODE_SEED` is 2^53 - 1 so a JavaScript page can send a seed back unchanged. `Component::validate` is the gate for anything from the network (finite numbers, ranges, size caps, no control characters); `tidy` repairs authored input and never replaces it. `EntityWhy` is the refusal reason (see `architecture/wire-protocol`). |
| `edit.rs` | **Who may change an entity and whether the change is allowed**: pure functions `check_put` and `check_remove` over a `Ctx` the caller fills (role `Builder` or `Editor`, account, feet, bounds, plots, zone entity counts, `ground`). Caps: 4,000 entities a zone, 2 portals and 64 props a plot, reach 16 blocks. Prop rules go through closures in `Ctx`: `known_prop`, `prop_cells` (every cell the prop covers, checked for bounds and a builder's plot), `prop_footprint_free` (no solid block or other prop under it; leaves out the prop being changed) and `prop_supported`. Support is `prop_stands` over the prop's `Surface`s (`Support`: `Floor`, `Wall`, `Ceiling`, `Table`, `Water`; `support_cells` says which cells to look at); a floating prop is refused `NoSupport`, an occupied footprint `Blocked`, and a change that keeps the placement (text or state only) skips both. Server, directory and tests run the same checks. |
| `wire.rs` | `ToServer`/`ToClient`, postcard, `PROTOCOL = 9`. See `architecture/wire-protocol`. |
| `auth.rs` | Ed25519 tokens. See `architecture/identity-and-tokens`. |
| `weather.rs` | Atmosphere simulation. See `architecture/weather-simulation`. |

## Why it exists
So server, directory and clients run the *same* code for edit validation, prediction, movement checks and token verification. See `decisions/core-without-bevy`. It is also the home of the deterministic terrain generator (`terrain`, `architecture/world-scaling-design`).

## Rules for contributors (AGENTS.md)
Everything from the network is untrusted: checked arithmetic, reject non-finite floats, cap sizes. Code shared with wasm avoids threads and `std::time`.

# Related Concepts
- [World Wire Protocol (PROTOCOL = 9)](wire-protocol.md): its wire messages
- [Weather Simulation](weather-simulation.md): its weather model
- [Identity, Sessions and Signed Launch Tokens](identity-and-tokens.md): its token code
- [Decision: Keep vox-core Free of Bevy](../decisions/core-without-bevy.md): why it has no engine
- [Decision: The World Editor](../decisions/world-editor.md): the entity and edit rules it serves
