---
type: Decision
title: "Decision (proposed): Lua (Luau) Triggers, Run by the World Server"
description: "ADR 0003: Luau through mlua on the world server only; scripts as hashed blobs and a Trigger component in the world format; a checked host API with instruction, time and memory budgets; editors only; piccolo as the pure-Rust fall-back."
tags: [decision, proposed, scripting, lua, luau, triggers, world-format]
generated: { by: agent/mcp, at: "2026-11-28T04:33:06Z" }
status: draft
---

# Decision (proposed): Lua (Luau) Triggers, Run by the World Server

**ADR 0003**, `docs/decisions/0003-lua-triggers.md`, status **proposed**, 2026-11-28. Beads epic `vox-n4k`.

## Facts it rests on (checked 2026-11-28)
- **Luau** has a sandbox mode, interrupts and memory limits, and `mlua` exposes them (`sandbox`, `set_interrupt`, `set_memory_limit`). `mlua` builds Luau's C++, and targets WebAssembly only through Emscripten, not our `wasm32-unknown-unknown`.
- **piccolo** is a pure-Rust Lua VM with fuel budgets. It is experimental and pre-1.0.
- **Lua 5.4** has no sandbox of its own.
- There is no scripting in Vox yet. Props have channels and states; dungeons have objectives.

## Decided (proposed)
1. **Luau runs on the world server only.** A script's effects are checked like a player's and reach clients as ordinary edits and entities. There is no VM in the browser.
2. **World format**:
   - **Script blobs**: Luau source, at most 64 KiB, named by hash, in the blob store and in snapshots.
   - **`Trigger` component**: what it watches (a box, a room or opening, a prop, a channel, or a timer of at least 1 s); what it listens for (enter, leave, use, state, signal, tick); the script hash and function to call; JSON `params` up to 4 KiB; and its owner.
   - **Saved state**: up to 4 KiB per trigger.
   - Levels declare triggers in `world.triggers`.
3. **Host API only**, with no `os`, `io`, `require`, network or wall clock.
   - **Reading**: blocks, entities, players, tick.
   - **Acting**: blocks, prop states, channels, notices, moving players, `Through`, objectives. Every action is checked by `vox-core` with the owner's rights.
   - **Timing and chance**: timers, and random numbers seeded by world, trigger and tick.
4. **Budgets**: one sandboxed VM per world; an interrupt for each call's instructions; 2 ms per tick shared by all scripts; 16 MB per world; caps on triggers, timers and actions. A script that keeps failing is switched off with a reason.
5. **Authors**: world owners and editors only, through a script panel on `/edit`. Scripts are shared like structures, with use and copy rights.
6. **First use**: the Crypt's altar door.
7. **piccolo** is the fall-back if browser scripts or a pure-Rust build are ever needed.

## Rejected
Lua 5.4 (no sandbox), scripts on clients, and WebAssembly modules as scripts. Fixed editor wiring stays for the simplest cases.

# Related Concepts
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): blobs and entities scripts join
- [Decision: The World Editor](world-editor.md): where scripts are written
- [Decision (proposed): BSN Scenes for Drawing; Rooms and Openings for Indoor Culling](bsn-scenes-rooms-and-openings.md): rooms and openings as trigger areas
