# ADR 0003: Lua (Luau) triggers in the world format, run by the world server

- **Status:** proposed
- **Date:** 2026-11-28
- **Decider:** the owner
- **Builds on:** [world-format.md](../design/world-format.md) (*Entities*, blobs);
  [world-editor.md](../design/world-editor.md) (Advanced mode);
  [ADR 0002](0002-bsn-scenes-rooms-and-openings.md) (rooms and openings)
- **Beads:** epic `vox-n4k`

## Context

The owner wants worlds to react: a door that opens when the party reaches the altar, a lift, a
trap, a light show, a message when someone walks into a room. That needs triggers and a
scripting language, and the owner asked for one based on Lua.

What exists today:

- **Props have a little behaviour.** A switch or a light listens on a numbered `channel`, and a
  prop has a `state` (0 to 7: open, lit). There is no logic between them.
- **Dungeons have an objective**, a place to reach that ends a raid. It is the only "when this
  happens, do that" in the game.
- **Everything is checked by the server**, with the rules in `vox-core`: edits, entities, moves.
  The server owns each world on one thread.
- **The browser client is `wasm32-unknown-unknown`.**
- There is no scripting in the workspace.

What the candidates offer (checked 2026-11-28):

- **Luau**, Roblox's Lua dialect (Lua 5.1 with types), was built to run untrusted code: a
  sandbox mode, interrupts that can stop a script mid-loop, and a host-set memory limit. The
  `mlua` crate exposes all three (`Lua::sandbox`, `set_interrupt`, `set_memory_limit`). It
  compiles Luau's C++ in, so it builds natively, but for WebAssembly only through Emscripten,
  not our `wasm32-unknown-unknown`.
- **piccolo** is a pure-Rust Lua VM. It is stackless (a script runs in steps with a *fuel*
  budget, roughly one unit per instruction) and aims at sandboxing. It is experimental and
  pre-1.0, with frequent API breakage.
- **Lua 5.4 through `mlua`** has no sandbox of its own. We would have to build one by hiding the
  standard library, which is easy to get wrong.

## Decision

1. **Scripts run only on the world server**, in **Luau** through `mlua`. The server is the
   authority already, so a script's effects are checked by the same rules as a player's and
   reach clients as ordinary edits, entity changes and notices. There is no second VM in the
   browser and nothing to fall out of step. A script never runs on a client.
2. **Scripts are part of the world format.**
   - **Script blobs**: Luau source text, at most 64 KiB, content-addressed like prop blobs (a
     hash names it). It is stored in the world's blob store and taken into snapshots.
   - **`Trigger`**, a new entity component:
     - a name;
     - **what it watches**: a box of cells; or a room or opening (ADR 0002); or a prop (used,
       state changed); or a channel; or a timer every N seconds (at least 1 s);
     - **what it listens for**: enter, leave, use, state, signal, tick;
     - the script's hash and the function to call;
     - a small JSON `params` (at most 4 KiB);
     - the account that placed it.

     It is stored, versioned, sent and checked like any entity. Clients get triggers too, so the
     editor can show them, but never run them.
   - **A trigger's state**: a few keys of saved values (at most 4 KiB), kept with the world, so a
     counter or a "done" flag survives a restart.
3. **Scripts act only through a host API**, with nothing else open. There is no `os`, no `io`, no
   `require` of files, no network and no wall clock.
   - **Reading**: blocks in the trigger's area, entities, the players inside, the world's tick
     and time of day.
   - **Doing**, each checked by `vox-core` with the trigger owner's rights:
     - set blocks, within the owner's plots or as an editor, counted against the edit rate;
     - set a prop's state, or send on a channel (doors, lights);
     - show a notice to the players inside;
     - move a player within the world, through the server's own move check, like `Correct`;
     - send a player through a doorway (`Through`);
     - start or end an objective.
   - **Time and chance**: run a function after N seconds. Random numbers are seeded from the
     world, the trigger and the tick, so a replay does the same.
4. **Budgets.** A world has one Luau VM in sandbox mode, on the world thread.
   - An interrupt stops any call that overruns its instruction budget, and the world's scripts
     share a time budget per tick (2 ms to start).
   - A memory limit per world applies (16 MB to start).
   - There are caps on triggers per zone, timers, calls per tick and actions per call.
   - A script that keeps failing or overrunning is switched off, its owner is told in the
     editor, and the log keeps the reason.
5. **Who may write them.** The world's owner and its editors, in Advanced mode, through a script
   panel on `/edit` that shows errors and can run a trigger once. Plot builders get no scripts
   at first; that is a later decision, with tighter caps. Scripts can be shared with other
   worlds' owners the way structures are, through use and copy rights.
6. **Levels declare them**: `world.triggers`, with scripts beside the level file. The Crypt's
   altar door is the first, replacing the objective's special case once it can.
7. **piccolo is the fall-back**, not the start. If browser-side scripts are ever needed (cosmetic
   effects running ahead of the server), or `mlua`'s C++ build becomes a burden, a pure-Rust VM
   with fuel is the way. The host API above is written so that it could be served by either.

## Options considered

| | Option | Verdict |
| --- | --- | --- |
| A | Luau through `mlua`, server only | **Chosen**: built for untrusted code, with sandbox, interrupts and memory limit; mature; typed. |
| B | Lua 5.4 through `mlua` | Rejected: no sandbox of its own; we'd build one by hiding the standard library. |
| C | piccolo everywhere (pure Rust, fuel) | Not now: experimental, pre-1.0. Kept as the fall-back (decision 7). |
| D | Scripts on clients too | Rejected: two VMs, results that differ, and cheating; the server is the authority. |
| E | No language: triggers wired to fixed actions in the editor | Kept *inside* A for the simplest cases (a button opens a door: channel wiring, no script); not enough for game logic. |
| F | WebAssembly modules as scripts (`wasmtime` on the server) | Rejected for now: heavier to author and to review than Lua, and a bigger runtime. |

## Consequences

- **Good:**
  - Worlds and dungeons get behaviour without server releases.
  - A script's effects go through the same checks as a player's.
  - Scripts travel with the world in snapshots and levels.
- **Costs:**
  - A C++ dependency in the server build (Luau, through `mlua`).
  - A new blob kind and a new component, so a protocol bump.
  - Script errors become an owner-facing support surface.
  - The world thread now runs untrusted code: budgets and the switch-off rule must be tested
    hard, with infinite loops, memory bombs and actions flooding the world.
- **Open:**
  - The exact host API names and types (a typed Luau definition file for the editor).
  - Whether triggers on dungeon instances are copied with the template.
  - How a script reports to the editor (logs, prints).
  - Whether builders ever get scripts.

## Follow-up beads (epic `vox-n4k`)

1. `vox-n4k.1` Spike: `mlua` with Luau in `vox-server`. It must stop an infinite loop and a
   memory bomb. Record the build time and size, and check the container still builds.
2. `vox-n4k.2` The `Trigger` component and script blobs in `vox-core` and `vox-format`, plus
   `world.triggers` in levels.
3. `vox-n4k.3` The server's trigger runtime and host API, with budgets and the switch-off rule.
4. `vox-n4k.4` Review of the three above, for security.
5. `vox-n4k.5` Editor: the script panel and the trigger tool.
6. `vox-n4k.6` The Crypt's altar door as the first scripted trigger.

## Sources

- [Luau sandboxing](https://luau.org/sandbox): sandbox mode and interrupts.
- [mlua](https://github.com/mlua-rs/mlua): Luau support, `Lua::sandbox`, `set_interrupt` and
  `set_memory_limit`; WebAssembly through `wasm32-unknown-emscripten` only; the memory-limit
  issue [#488](https://github.com/mlua-rs/mlua/issues/488).
- [piccolo](https://github.com/kyren/piccolo): a stackless pure-Rust Lua VM with fuel;
  experimental.
