---
type: Decision
title: "Decision: A Live In-Browser World Editor Built on World Entities"
description: "The world editor edits the running world live through server-checked entity messages (protocol 9), with basic mode for plot builders and advanced mode for a world's owner and named editors."
tags: [decision, editor, entities, portals, props, wire, game-design]
generated: { by: agent/mcp, at: "2026-11-27T21:09:20Z" }
status: stable
---

# Decision: A Live In-Browser World Editor Built on World Entities

**Decision** (2026-11-27, with the owner): an editor at `/edit?world=<id>`: the wasm `vox-client` in the page, Mantine panels around it, a JSON command and event bridge between them. Design: `docs/design/world-editor.md` (its "As built" section lists what implementation settled); plan: beads epic `vox-tol` (six phases).

- **Everything is live**: no drafts. Edits go to the running world server, which checks them and sends them to everyone. World-wide undo is the existing snapshot restore; the editor undoes the author's own edits.
- **World entities are the backbone** (world-format phase 2): portal, spawn, objective, settings, prop and graph node are `vox_core::entity` components (plain serde, stable names and versions, no Bevy). They live in `vox-core` because the wire carries them. `vox-format` stores them in v2 zone blobs; the server keeps them in SQLite with an `entity_edits` log written in the same flush transaction.
- **Protocol 9**: `Put`, `Remove` (with `expect`, the last seq seen, refused as `Changed` when stale), `Build`, `Unbuild`, `Entities` (in batches of 512, at most 64 KB a message), `EntityRefused`. New variants are appended last; client and server ship together.
- **Rights**: basic mode is for plot builders, in their plots (the 30 s rules window, as block edits have). Advanced mode is for the world's owner and named editors (`world_editors`), checked with the directory on every edit, cached 5 s, failing closed. The page never shows rights looser than the server enforces.
- **Player portals**: identified by their directory row id (`Portal.key`). They pair by key and cost Authority as today; the charge is idempotent, re-checked, refunded on failure, and the world reconciles with the directory.
- **Portal destinations stay on this host** (`:port`, `/w/<id>/ws`, `dungeon:<name>`, a known world). The editor never accepts an arbitrary `wss://` address.
- **Server-owned values**: prop owners and portal keys come from the server (a new prop's owner is stamped, an edited prop keeps its owner). The page holds ids as JavaScript numbers, so a node's seed is capped at 2^53 - 1.
- **Builds**: at most 32,768 blocks, applied 4,096 a tick as ordinary edits. **Undo of a build is done by the server** (`Unbuild`: restores only cells still as built, counts the rest), because block edits are limited to 20 a second a player. Client and server expand a node with the same code (`vox_graph::entity`).
- **Props** (props.md phases 1 to 3): the whole footprint must be inside the world (and a builder's plot), stand on a surface its definition accepts, and keep blocks out; a prop whose support is broken is removed. The seed catalogue is built by `vox-props` from the barrel, stool and crate recipes in `source/props` (the owner's choice); `.vox` imports refuse colours further than 48² from the palette.
- **Atlas edges** close by leaving them out of `atlas.edges` in the Settings entity; the server refuses travel over a closed edge.
- **Local play**: `--editors-all` lets guests edit a directory-less world; never on a production host.

**Rejected**: drafts published over a live world; a Bevy-UI editor (forms too costly); a separate three.js preview (not the real look); `bevy_reflect` entity types (would bring Bevy into the server); a client-side build undo sent block by block (minutes at the edit rate).

# Related Concepts
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): entities are its phase 2
- [Decision: Structures Have an Owner and Use, Copy and Edit Rights](structure-entitlements.md): the same rights model for props
- [vox-client and the Web Front End](../architecture/vox-client-and-web.md): the page and the wasm client
- [Design (proposed): The World Editor on the Web](../architecture/world-editor-design.md): The proposed next step: workshops, whole operations, saving as a level or structure.
