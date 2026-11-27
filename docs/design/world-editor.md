# Design: the world editor

Status: **agreed** · 2026-11-27 · nothing here is built yet

Related: [props.md](props.md) (what a prop is; phases 1 to 3 are part of this plan),
[world-graph-and-art.md](world-graph-and-art.md) (recipes and graph nodes; its phase 1 and the
node part of phase 3 are part of this plan), [world-format.md](world-format.md) (entities, its
phase 2, is the backbone here), [world-network.md](world-network.md) (portal destinations,
dungeons, atlas edges), [controls-and-hud.md](controls-and-hud.md) (actions, the quick wheel).

## Summary

An editor in the browser at `/edit?world=<id>`: the real game (the wasm `vox-client`) in the
middle of the page, Mantine panels around it, and tools to place, move, turn and connect
portals, props and graph nodes **in the live world**. Every change goes to the running world
server, which checks it and sends it to everyone there, as block edits already work.

Two modes on one page:

- **Basic**, for players: blocks, portals and props in **their own plots**. Pair two of their
  portals, or point one at a world.
- **Advanced**, for a world's **owner and the editors they name**: the world's settings
  (sky, time, weather, hotbar, relief, water), spawn, objective, any portal destination
  (`dungeon:<name>`, atlas edges, another world), prop states, variants and channels, and the
  **graph node inspector** (add a node, write its recipe, preview it, build it).

Decisions (2026-11-27, with the owner):

1. **Everything is live.** No drafts. An edit goes to the running world; world-wide undo is the
   snapshot restore that already exists; the editor also undoes the author's own edits.
2. **Every editable thing is a world entity** (world-format phase 2): portal, spawn, objective,
   world settings, prop, graph node. One model, one pair of wire messages, one store.
3. **Advanced mode is for the world's owner and named editors** (a new per-world grant).
4. **Basic portals pair**: a player links two of their own portals (either way, across worlds or
   within one), or points one at a world's spawn as today. Same Authority cost and plot cap.
5. **The page is React panels plus a bridge** into the wasm client; Bevy draws the picking,
   ghosts and gizmos.
6. **Props (props.md phases 1 to 3) and graph nodes (vox-art phase 1, vox-graph nodes and
   expansion) are built in this plan**, not waited for.
7. **Portal destinations stay on this host's worlds**; the editor never accepts an arbitrary
   `wss://` address.

## Where things are today

| Area | Today | What the editor needs |
| --- | --- | --- |
| Level files | `data/levels/*.json`: `boxes`, `blocks`, and a `world` object (sky, time, weather, `portals`, `objective`, `hotbar`) read by `vox_core::level` | Seed only: a world's live state is its entities |
| Portals | In the level's `world.portals`; players' plot portals in the directory's `portals` table (`/api/feats/portal`: 30 Authority, 2 a plot, axis `z`, to a world's spawn), fetched by the world from `/api/world/{id}/rules` every 30 s and merged by `settings_now()` | Live: placed, moved and paired while players watch |
| How clients learn of portals | Once, in `Welcome.settings` (JSON) | A message when one changes |
| Props | None in worlds (props.md is a proposal) | Placed, moved, removed live |
| Graph nodes | None (world-graph-and-art.md is a proposal) | Added, edited, built live |
| Who edits | Blocks: plots (owner and a shared group) via `Why::Claimed`; `world_owners` (set by an operator) issues currencies | Plot builders for basic; owner and editors for advanced |
| Page to client | One way: JS calls `start(config)`; the client calls `vox_status`, `vox_ready`, `vox_error` | Commands in, events out |
| Entities on disk | `vox-format` zones hold blocks only | Entities in zones, so snapshots carry them |

## The backbone: world entities

### Types

In `vox-format` (`src/entities/`), plain serde types, **no Bevy**: `vox-format` stays free of
Bevy so the server does (world-format.md had proposed `bevy_reflect`; plain serde with explicit
stable names and versions gives the same guarantees without it).

```rust
pub struct EntityId(pub u32);          // unique in its world; the server allocates

pub enum Component {                   // closed set; each variant has a stable name and a version
    Portal(Portal),                    // "vox::Portal" v1
    Spawn(Spawn),                      // "vox::Spawn" v1
    Objective(Objective),              // "vox::Objective" v1
    Settings(Settings),                // "vox::Settings" v1: sky, time, weather, hotbar, relief, water, floor, cameras, edges
    Prop(Prop),                        // "vox::Prop" v1 (props.md)
    Node(Node),                        // "vox::Node" v1 (vox-graph)
}

pub struct Portal {
    pub name: String,                  // at most 32 chars, control characters stripped
    pub at: [f32; 3], pub axis: Axis, pub size: [f32; 2], pub yaw: f32,
    pub level: String,                 // the echo it shows
    pub to: Dest,
    pub owner: Option<u64>,            // a player's portal (plot), or None (the world's own)
    pub plot: Option<i64>,
}

pub enum Dest {
    Here { arrive: [f32; 3] },         // somewhere else in this world
    World { world: String },           // a world's spawn (today's plot portals)
    Pair { world: String, portal: EntityId },   // a player's other portal
    Dungeon { name: String },          // "dungeon:<name>"
    Url { url: String, arrive: [f32; 3] },      // ":7781" or "/w/<id>/ws" only (see Security)
}
```

`vox_core::portal::Door` stays the geometry (what the client draws and the server checks);
`Door::from(&Portal, resolved_arrive)` replaces parsing JSON settings. The level JSON's
`world.portals` becomes portal entities when a world is seeded, and `settings_now()` and the
JSON merge go away.

### On the wire (protocol 9)

Added last, so older variants keep their first byte (the `old_variants_keep_their_first_byte`
test grows):

```rust
ToServer::Put { id: u32, entity: Option<EntityId>, component: Component }  // None: make a new one
ToServer::Remove { id: u32, entity: EntityId }
ToServer::Build { id: u32, node: EntityId }                                // expand a node into blocks
ToClient::Entities(Vec<EntityChange>)  // on join (all of them, after Ready) and to everyone on change
ToClient::EntityRefused { id: u32, why: EntityWhy }

pub struct EntityChange { pub seq: u64, pub entity: EntityId, pub component: Option<Component> } // None: removed
```

`id` is the sender's own number, echoed back, as with `Edit`. Changes share the world's edit
sequence (`seq`), so blocks and entities are seen in one order. `Welcome.settings` stays for
older clients, filled from the `Settings` entity.

### Rules (`vox-core`)

The checks are pure functions in `vox-core` (new module `vox_core::edit`), so the client
predicts with the same rules and the server enforces them. `vox-core` cannot depend on
`vox-format` (the dependency runs the other way), so the rules take plain values (positions,
sizes, plot rectangles, who is asking); `vox-format`'s types call them.

`EntityWhy`: `NotYours` (not your plot, not an editor), `Outside`, `TooFar`, `TooMany` (a cap),
`TooBig` (a string, a recipe), `Blocked` (footprint not free, no standing room), `BadDest`,
`NoSuchProp`, `CantAfford`, `TooFast`.

### Store

The server keeps entities in a new SQLite table beside `chunks`:
`entities(id INTEGER PRIMARY KEY, zone_x, zone_z, name TEXT, version INTEGER, data BLOB)`
(postcard), and writes them into each zone blob's entity part when it snapshots. The zone blob
gains an entity section after the blocks, so its format version goes to 2; version 1 blobs
read as "no entities". The hash covers both.

## Who may edit what

| | Basic | Advanced |
| --- | --- | --- |
| Who | the plot's builders (owner and its shared group) | the world's owner and its editors |
| Where | inside that plot (the whole footprint and the arrival point) | anywhere in the world |
| Blocks | as today | as today, and not stopped by plots (`Why::Claimed` doesn't apply to owner/editors) |
| Portals | place, move, turn, resize (4 × 6 to 8 × 16), remove; `World` or `Pair` destinations; cost and cap as today | any destination; no cost; no cap but the zone's |
| Spawn, objective, settings | no | yes, including closing an atlas tile's edges (`Settings.edges`; the edge check refuses `edge:*` through a closed one) |
| Props | catalogue props, and their own (entitlement `use`); 64 a plot | any prop they may use; states, variants, channels, `readable` text |
| Graph nodes | no | add, edit, build |

- **World editors**: a new directory table `world_editors(world, account)` with
  `POST /api/worlds/editors` (owner only: grant, revoke) and the list included in
  `/api/world/{id}/rules` (`owner`, `editors`). The world checks each edit against the rules
  it last fetched, so a revoke takes effect within 30 s; the world also refetches on demand
  when an advanced edit comes from someone not on its list (at most once a minute per account).
- **Guests** edit nothing in the editor. The editor needs a sign-in token.
- **Basic portals cost Authority** as today: on a `Put` that makes a player portal the world
  asks the directory (`POST /api/world/portal`, world secret: account, plot, position, dest),
  which charges, enforces the cap, records the row and gives the key item; only then does the
  world apply the entity. Moving or removing your own portal is free; removing refunds nothing.
  `/api/feats/portal` stays for the inventory page and goes through the same function.
- **Pairing**: a `Pair` destination must be a portal of the same owner, in a world the
  directory knows. The directory keeps both rows; it resolves the arrival point at travel time
  (`/api/world/travel` already makes the ticket and the arrival), so moving the other end needs
  no change here. A pair whose other end is gone leads to that world's spawn.

## In the page

### The page

`apps/site/src/entries/edit.tsx` and `pages/edit.html`, a new `features/editor/`:

```
┌────────────────────────────────────────────────────────────────────────────┐
│ World: The Yard ▾   Basic │ Advanced      Undo  Redo      ● live, 3 here  │
├──────────────┬────────────────────────────────────────────┬────────────────┤
│ Tools        │                                            │ Inspector      │
│  Select      │                                            │  Portal "home" │
│  Blocks      │         the game (vox-client, wasm)        │  at  12 0 -4   │
│  Portal      │     picking, ghosts, gizmos drawn here     │  axis z  yaw 0 │
│  Prop        │                                            │  size 8 × 16   │
│  Node (adv.) │                                            │  to: Pair ▾    │
│ Palette /    │                                            │  [Remove]      │
│ catalogue    │                                            │                │
├──────────────┴────────────────────────────────────────────┴────────────────┤
│ Entities: portals · props · nodes · spawn · objective     (click to fly)  │
└────────────────────────────────────────────────────────────────────────────┘
```

Mantine owns the forms, lists, destination pickers and the recipe editor (a JSON text area
checked against the recipe schema). Advanced controls are hidden, not just disabled, for
someone who may not use them; the server checks anyway.

### The client in edit mode

`StartConfig` gains `edit: true`. The client joins as a player (others see the author) and
switches to an **edit camera**: fly freely, no gravity (moves are still checked by
`vox-core::moves`, which allows flying speed), the quick wheel replaced by the active tool. In
`vox-world`, a new `editor/` module adds:

- **Picking**: the voxel ray (`vox-core::ray`) against blocks, portal frames, prop boxes and
  node outlines.
- **Ghosts**: what a tool would place, drawn translucent; red when the rules (`vox_core::edit`)
  say no, with the reason.
- **Gizmos**: move along an axis or on the ground plane, snapping to the half block (portals),
  5 cm (props) or the block (nodes); turn by 90° (portals, nodes) or 15° (props); resize
  handles on portals.
- **Sending**: `Put`, `Remove`, `Build` on the client's one WebSocket, so the server orders them
  with everyone's edits. Applied changes come back as `Entities`.

### The bridge

```rust
#[wasm_bindgen] pub fn editor_command(json: String);   // JS -> client
// client -> JS: window.vox_editor(json)
```

One schema, defined in Rust (`vox-world::editor::bridge`: `Command` and `Event`, serde,
`#[serde(tag = "type")]`), with the TypeScript types in `apps/site/src/game/editor.ts`. A test
in `vox-world` writes sample JSON for every variant to `apps/site/src/game/editor.samples.json`
and a `bun test` parses each against the TypeScript types, so the two can't drift silently.

Commands: `tool` (select, blocks, portal, prop, node), `block` (palette choice), `prop`
(catalogue choice), `select` (an entity), `set` (a field on the selected entity, the whole
component), `remove`, `fly_to`, `undo`, `redo`, `build` (a node). Events: `ready`
(world, palette, catalogue, your rights), `entities` (the list, then changes), `selected`,
`refused` (with the reason in words), `here` (who is in the world).

Nothing secret crosses the bridge: the token is already in the page that started the client.
The bridge takes commands only from the page itself (`editor_command` is a Rust function the
page calls; there is no `postMessage` listener).

### Undo

The editor keeps the author's own changes as pairs (before, after), up to 100. Undo sends the
`before` as a `Put` (or a `Remove` for something made); if someone else has changed that entity
since, the undo is refused and the step dropped. Undoing a `Build` sends the blocks' old values
as ordinary edits. World-wide undo is the owner's snapshot restore
(`--restore-at`, and later on the site, world-format phase 7).

## Props in the editor

Built as props.md phases 1 to 3 describe (no change to that design):

1. `crates/vox-art`: grid, palette, seeded RNG, `PropDef`, the `.vox` reader and importer, the
   checks; the existing `source/vox/props/` imported as the seed catalogue.
2. Prop recipe operations and golden tests (native and wasm).
3. `VOXP` blobs, the `Prop` entity, the server's placement checks (props.md *Placing*),
   meshing, batching and prop occupancy in the clients.

Behaviours (phase 4) come after: the editor sets `state`, `variant` and `channel` fields that
the entity already holds, and the inspector shows them, but only `light` and `motion` need to
work for a first release; the rest follow props.md phase 4.

The catalogue panel lists accepted props (name, tags, a thumbnail from the contact sheet).
Player-made props and their entitlements are props.md phase 6, after this plan.

## Graph nodes in the editor

The minimum of world-graph-and-art.md the inspector needs:

- `vox-art` phase 1: grid, palette, RNG, five operations (`box`, `roof`, `cut`, `openings`,
  `trim`), evaluation, checks, export to level blocks; golden tests on both targets.
- `crates/vox-graph`: `Node` (kind, detail, footprint, params, recipe, seed), `Link`, the
  constraints (children inside parents, no overlaps) and `expand` (node to blocks, through
  `vox-art`). No language model tools, no library, no `vox-forge`.

In advanced mode: **add** a node (a kind and a footprint, drawn as an outline), **edit** its
recipe (the JSON form, checked against the schema `vox-art` generates), **preview** (the client
expands it in wasm and draws it as a ghost), **build** (`ToServer::Build`: the server expands
the same recipe and seed, and applies the blocks as ordinary ordered edits by that player, so
they are in the edit log and snapshots). Expansion is deterministic, so the preview is what
gets built. Links between nodes are shown and stored, but only `contains` is enforced for now.

The seam for later: `vox-forge`'s MCP tools (world-graph-and-art.md phase 4) can drive the same
`Put` and `Build` messages, so a language model would edit a live world through the same rules.

## Security

- **Every `Put`, `Remove` and `Build` is checked on the server**, whatever the client showed:
  rights (re-checked on every edit), bounds and reach, caps, finite floats, sizes.
- **Caps**: 4,000 entities a zone; 64 props a plot; portals as today (2 a plot) for players;
  strings (names 32, `readable` 140, recipe JSON 16 KB) with control characters stripped;
  recipes at most 200 operations and a voxel budget of 64 × 64 × 64 blocks per build; `Put`
  10 a second per player, `Build` one per 10 seconds per world.
- **Portal destinations stay on this host**: `Url` accepts only `":<port>"` or `"/w/<id>/ws"`;
  `World` and `Pair` must name a world the directory knows; `Dungeon` a template the host has.
  Atlas edges are not portals anyone places (they come from the tile); an owner may only close
  or open them (`Settings.edges`). A whole `ws://` or
  `wss://` address is refused (`BadDest`): the client would connect there with a ticket.
- **Spawn and objective**: the spawn needs room to stand (`vox-core::body`'s character height
  free, over a solid block or the world's `floor`); the objective must be inside the world. A `Pair` end inside someone
  else's plot is refused.
- **Untrusted blobs**: `Component` decoding has the same caps as zones (entity counts, string
  lengths); unknown component names and newer versions are refused, not guessed.
- **Text**: portal names and `readable` text go through the same filter as names.
- **The world secret** stays on localhost (`/api/world/*`); the new `/api/world/portal` is
  under it and refused by the tunnel config like the others.

## Wasm and the browser

- Shared code (`vox-core::edit`, `vox-format::entities`, `vox-art`, `vox-graph`,
  `vox-world::editor`) has no threads, no blocking I/O, no `std::time` clocks, and uses checked
  arithmetic on sizes from the network or a recipe (wasm32 `usize` is 32 bits and release
  builds wrap).
- Recipe expansion for a preview runs within the per-frame meshing budget (5 ms in the
  browser), spread over frames; a preview over budget shows its outline first.
- The editor page loads the same `public/pkg` client; no second build.

## Testing

| What | How | Command |
| --- | --- | --- |
| Rules | unit tests in `vox_core::edit` for each `EntityWhy` | `cargo test -p vox-core edit` |
| Entities | round trips, version-1 zone blobs still load, crafted blobs refused | `cargo test -p vox-format entities` |
| Wire | round trips, first bytes of old variants unchanged | `cargo test -p vox-core wire` |
| Server | one test per refusal; a change reaches a second player; restore brings entities back; plot portal charges Authority once | `cargo test -p vox-server editor` |
| Directory | editors grant and revoke; `/api/world/portal`; pairing | `cargo test -p vox-directory editors` |
| vox-art, vox-graph | golden voxel hashes, native and wasm | `cargo test -p vox-art`, `cargo test -p vox-graph` |
| Bridge | Rust samples parsed by the TypeScript types | `cargo test -p vox-world bridge && (cd apps/site && bun test)` |
| Page | builds | `cd apps/site && bun run build` |
| End to end | Playwright: sign in, open `/edit`, place a portal, a second page sees it | `tools/test/editor_e2e.py` |

## Phases

Each ships on its own; dependencies in brackets.

1. **Entities** (world-format phase 2): `vox-format::entities`, the zone blob v2, the SQLite
   table, seeding portals, spawn, objective and settings from level files, protocol 9
   (`Put`, `Remove`, `Entities`, `EntityRefused`), clients reading portals from entities.
2. **Rights**: `vox_core::edit` rules, `world_editors`, rules carrying owner and editors,
   `/api/world/portal` and pairing.
3. **Bridge and page**: edit camera, picking, ghosts, gizmos, the bridge, `/edit` with portals,
   blocks, spawn, objective and settings. [1, 2] (The first usable editor.)
4. **Props** (props.md phases 1 to 3), then the prop tool and inspector. [1]
5. **Graph nodes** (vox-art phase 1, vox-graph), then the node inspector and `Build`. [1, 3;
   shares vox-art with 4]
6. **Undo, the entity list, the end-to-end test, help pages** (`docs/help/editor.md`). [3]

## Open questions

- Should `Build` make a **structure** version (world-format phase 6) instead of loose blocks, so
  a built node can be rebuilt or copied? Loose blocks first; a structure once phase 6 exists.
- Should a world owner be able to make **another player's** portal pair across into their
  world refuse (a "no incoming portals" setting)?
- Does the editor need a **read-only** mode for anyone (watch the editing, no rights), or is
  `/play` enough?
- The edit camera flies: should a world be able to switch flying off for non-editors?
