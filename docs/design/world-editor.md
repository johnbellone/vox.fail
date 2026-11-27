# Design: the world editor on the web

Status: **proposed, questions settled** · 2026-11-27 · nothing here is built yet (see *Decisions*)

Related: [world-format.md](world-format.md) (snapshots, levels as files, structures and their
rights), [world-network.md](world-network.md) (worlds by id, the host, instances),
[controls-and-hud.md](controls-and-hud.md) (input actions, menus), [props.md](props.md)
(placing props), [ground-relief.md](ground-relief.md) and
[bevy-ecosystem.md](bevy-ecosystem.md) (bevy_egui and gizmos for an editor build).

## Summary

An editor at **vox.fail/edit**: open a world you may edit in the browser and build it with
tools a player doesn't have (boxes, fills, replace, copy and paste, an eyedropper), set the
world's own settings in side panels (sky, time, weather, water, relief, spawn, portals,
cameras, hotbar), undo and go back through its history, and keep the result as a world, save
it as a level, or save part of it as a structure.

It is not a separate program. The canvas in the middle is the same WebAssembly client as on
the play page, in an **edit mode**; edits go over the same WebSocket to the same world server,
which checks every one with the same rules. The panels around it are React on the site. What a
builder does in the editor is a world edit like any other, so it is saved, snapshotted,
restorable and shared the way worlds already are.

## Who it is for

- **Builders** making their own worlds: today they can only place one block at a time, by
  hand, within reach.
- **Level authors** for the worlds that ship with the game: today that is JSON written by hand
  or a Python builder (`tools/assets/build_*.py`), then delete the world's database to see it.
- **Structure makers**: today the directory can store and share structures
  (`/api/structures`), but nothing in the game makes one from what you built.

## What we have (checked in code)

- **Edits** are one block at a time: `ToServer::Edit {id, cell, block}`. The server checks the
  palette and bounds (`vox-core::world`), reach (`REACH`, 16 blocks from the eyes), a rate
  (`edit_rate`, 20 a second, as a token bucket) and plots (`Why::Claimed`), then logs each in
  `edits(seq, at, player, x, y, z, old, new)`. Old values are kept, but rows carry no
  operation id (only the player's name), and rows older than the oldest kept snapshot are
  deleted at each snapshot, so the log alone can't group or undo an operation.
- **History**: snapshots every five minutes, kept a week, with restore to a snapshot or any
  moment (`--restore <id|latest>`, `--restore-at <unix time>`), from the command line, with the
  world closed. There is no restore while a world runs.
- **Commands**: the client already takes `world:` commands as text (`place`, `remove`, `goto`,
  `look`, `fly`, `camera`, `time`, `weather`, ...) in `client/commands.rs`, and reports its
  state as JSON. Block Avatar's control page drives it this way.
- **The browser client** is started by the play page (`apps/site/src/features/play`) through
  its one export, `start(config)`, and reports back through `window.vox_status`, `vox_ready`
  and `vox_error`. Nothing else crosses between page and client.
- **World settings** (sky, time, weather, water, relief, portals, cameras, hotbar) are the
  level's JSON object, stored in the world and sent in the welcome. Two things are added at
  send time (`settings_now`): the portals players opened at the directory (they change while
  the world runs, with the directory's rules) and `voice_radius`. The weather is kept apart
  (as the world's climate), and any player can change it (`ToServer::Climate`, limited by the
  edit rate). Clients read settings once, on arrival.
- **Levels** are JSON boxes of blocks: solid, `hollow` or `round`, and single blocks
  (`vox-core::level`). A world's blocks come from its level only the first time it starts.
- **Hosting**: `vox-host` runs a fixed list of worlds (given when it starts) on one port,
  starting each on its first visitor and stopping it when idle. The only worlds it makes at
  run time are dungeon instances (`POST /api/host/instance`): in memory, without snapshots,
  from `data/levels/dungeons/`, and thrown away when empty. The directory knows the worlds
  (`/api/worlds`: its configured worlds, atlas tiles and instances) and tells each world its
  rules every 30 seconds (`/api/world/{id}/rules`: plots, portals, parties; nothing about who
  may edit).
- **Chunks** go to a player only while they join; after that, changes arrive as `Edits`.
- **Structures**: owned, versioned, with use, copy and edit grants (`vox-core::entitle`,
  `/api/structures/*`). Placing one in a world is not built.
- **Worlds** are 512 × 128 × 512 blocks by default (`Bounds::default`).

## Design

### 1. Where you edit: workshops

A **workshop** is a world you own, made for building, kept with its snapshots. This is new
work on both sides, not a reuse of instances (which live in memory and are thrown away):

- **The host** gains a way to add a persistent world while it runs, with its own database and
  snapshot folder, started on its first visitor and stopped when idle like the others, and
  still there after the host restarts (it keeps a list of its workshops on disk).
- **The directory** keeps workshops in its database, so `/api/worlds` and the world's rules
  know them; it asks the host to make one with the world secret, as it does for instances.

- `POST /api/workshops {name, from}` makes one. `from` is blank ground, a level the game ships
  (`data/levels/<name>.json`, dungeons in `data/levels/dungeons/` too), one of your workshops'
  snapshots, or a structure you may copy. Each needs its own way to seed a world.
- `GET /api/workshops` lists yours, and those shared with you. `POST /api/workshops/grant`
  shares one, `POST /api/workshops/delete` removes it.
- Rights follow structures (`vox-core::entitle`): the **owner**, plus accounts granted
  **edit** (build and change settings) or **visit** (walk round, no edits). Workshops are
  private by default.
- Limits, set by the directory: a few workshops an account (three to start) at the default
  world size.

Editing the public worlds (the Void, the sea) is out of scope at first. A level the game ships
is edited by opening it as a workshop, then exporting it back (section 6).

### 2. Edit mode in the client

A new `editor` module in `vox-world`, so it works on the desktop and in the browser alike. It
is on only in a world whose rules say this account may edit it.

- **Camera**: free flight (the client already has `fly`), no fall damage, and no reach limit
  on what you point at.
- **Selection**: a box from two corners, dragged with the mouse or typed in, drawn as an
  outline with its size in blocks. Shift widens it, and it snaps to blocks.
- **A ghost** of what a tool will do, before you commit it.
- **Tools**: place and break (as now); **box** (solid, `hollow` or `round`, the shapes a level
  file has, so exports stay readable; also walls only, which exports as four boxes); **replace** (one block for another
  inside the selection); **copy, cut and paste**, with rotate and mirror; **eyedropper**; and
  a **flood fill** across a face, bounded by the selection.
- **Readouts**: the pointed-at cell and block, the selection's corners and size.
- Keyboard and mouse first. Controller editing is later.

### 3. Edit operations on the server

One new client message carries a whole operation, so a 40 × 1 × 40 floor is one message, one
check and one step in the history, not 1,600 edits at 20 a second:

```text
ToServer::Op { id: u32, op: EditOp }
EditOp = Fill    { lo: Cell, hi: Cell, block: u16, shape: Solid | Hollow | Walls | Round(f32) }
       | Replace { lo: Cell, hi: Cell, from: u16, to: u16 }
       | Paste   { at: Cell, turn: u8, mirror: bool, blob: Vec<u8> }   // a structure blob
ToServer::Undo { op: u32 }    ToServer::Redo { op: u32 }
```

- **Expanded by `vox-core`** (a new `edit` module), so the client's prediction and the server
  agree, with checked arithmetic (it runs in wasm). Paste blobs are decoded with the same caps
  as any untrusted blob.
- **Who may edit** reaches the world with its rules: the directory's rules gain an `editors`
  list (accounts), and the world's rules event carries it. Rules refresh every 30 seconds, so a
  new editor waits up to that long and a removed one keeps editing as long; the owner can
  ask for a refresh when sharing. Only signed-in players can be editors: guests never are, so
  a world without sign-in has no ops at all.
- **Checked by the server**: the caller is an editor, the box is inside the world, a paste
  blob is at most 1 MB (well under the WebSocket's frame limit) and decodes with the usual
  caps, and the size is capped after expansion: at most 262,144 cells (64³) an operation, and a
  token bucket like the edit rate's, counted in cells (about 2 million a minute, a burst of
  one big op). Plots still hold: an op touching a cell in someone else's plot is refused
  whole. Ops replace the per-player edit rate and reach in a world you may edit; play keeps
  them.
- **Refusals** get new `Why` values (too big, too fast, not an editor), added after the
  existing ones so older values keep their numbers.
- **Applied as one batch**, and stored as an op: a new `ops(id, account, at, lo, hi, kind)`
  table, with the op's before and after kept compactly (per chunk, run-length coded, like
  the blocks themselves) rather than as one edit row per cell; a 64³ fill would otherwise be a
  quarter of a million rows written on the world's one thread.
- **Sent out**: small ops go to players as `Edits`; one that changes more than a few thousand
  cells sends the changed chunks instead. Clients take chunks only while joining today, so
  taking one mid-visit, and settling it against the editor's own prediction, is new work.
- **Undo and redo**, of your own ops only, with your rights checked again at the time:
  undoing puts back an op's old value in each cell that still holds its new value (cells
  someone changed since are left, and the editor says how many); redo is the same the other
  way. Ops can be undone as long as their world keeps them: a week, with the snapshots.
- These messages are added after the existing ones, as every new message is, and they change
  the protocol: **protocol 9**.

### 4. World settings, live

Settings stop being welcome-only:

- `ToServer::Settings(json)`, allowed only for editors. The server checks it key by key (known
  keys, finite numbers, sizes, portals that lead to worlds you own or public ones) and stores
  only the world's own keys: the portals opened at the directory and `voice_radius` are added
  when sending, as now, and stripped if a client sends them back, so they never become part of
  the world. Weather in a settings message goes the existing climate way (one path, the same
  as `ToServer::Climate`). The server then sends `ToClient::Settings(json)` to everyone there.
- Clients apply new settings in place: sky, time, weather, water, relief, cameras, hotbar and
  portals. Today they read them once, on arrival.
- **Spawn, portals and cameras** are placed in the world with a gizmo and fine-tuned in the
  panel. [bevy-ecosystem.md](bevy-ecosystem.md) suggests transform-gizmo-bevy for editor
  builds, but it has no Bevy 0.20 release; until it does, a small gizmo of our own (move along
  an axis, turn about y, snapped to blocks) is enough.

### 5. The page

`/edit` on the site (React and Mantine, like the other pages), with the client's canvas in the
middle:

| Where | What |
| --- | --- |
| Top bar | The workshop's name, who else is in it, **Save as…** (level, structure), **History** |
| Left | Tools, and the blocks as a palette with their icons, searchable |
| Right | **Selection** (corners, size, block counts) and **World** tabs: Sky and time, Weather, Water, Relief, Spawn, Portals, Cameras, Hotbar |
| History drawer | Your recent ops (undo or redo any of them), and snapshots to restore |
| `/edit` with no workshop open | Your workshops and those shared with you, and **New workshop** |

**The bridge** between the page and the client is new, beside the play page's `start` and
`vox_*` callbacks:

- The client gains a second export, `command(text)`, taking the same `world:` commands as the
  control page,
  plus new `edit:` ones (`edit:tool:box`, `edit:select:x:y:z:x:y:z`, `edit:block:stone`,
  `edit:undo`, ...). Panels drive the editor through it.
- The client calls `window.vox_event(json)` when something the panels show changes: the
  selection, the pointed-at cell, the history, the settings, who is editing.
- Keyboard focus stays clear: the canvas has it while you build, and panels take it while you
  type in them. Esc returns to the canvas.

Panels are web pages rather than in-game menus because they are mostly forms, lists and text,
which the web does better and more accessibly; the pixel HUD stays for play. The desktop gets
the same edit mode with keyboard shortcuts first, and a bevy_egui shell in an editor build
later.

### 6. Keeping what you made

- **As a world**: a workshop is a world. Its snapshots appear in the History drawer and can be
  restored there. Today a restore needs the world closed and the command line, so this needs
  a restore the server can do while running (everyone in the workshop gets the world again).
- **As a level**: **Save as level** exports the workshop's blocks and settings. For the levels
  in this repository, the export is the JSON box format: blocks merged greedily into the
  largest boxes (and `hollow` and `round` ones where they fit exactly), so a file stays short
  and its diffs readable. A test checks the round trip: loading an export gives the same
  blocks. Later it is also a level file in the world format (world-format phase 3).
- **As a structure**: **Save as structure** turns the selection into a new structure, or a new
  version of one you may edit (`/api/structures`, `/api/structures/version`). The Paste tool
  lists the structures you may use, from the directory.

### 7. Working together

Several editors can be in one workshop at once: the server already orders everyone's edits.
Each sees the others' selections as coloured outlines (a small presence message), and the
History drawer marks whose op is whose. This comes after the single-builder editor works.

## Safety

- Everything an editor sends is untrusted: ops are size-capped and expanded with checked
  arithmetic, paste blobs get the caps every blob gets, and settings are checked key by key.
- Who may edit comes from the directory and is checked by the world server, not the page.
- Workshops are private unless shared. Publishing one as a public world (for the atlas, say)
  needs review and is out of scope.
- No new public endpoints on the world side. The directory's workshop calls are signed-in
  calls like the rest; its calls to the host carry the world secret, as instances' do.
- Commands from the page run with the player's authority and nothing more: the server's
  checks stay the gate.

## Rollout

Each step ends with a command that passes or fails.

1. **Edit ops**: `vox-core::edit` (shapes, replace, paste expansion, caps); `ToServer::Op`,
   `Undo`, `Redo`; batched apply, log and broadcast; protocol 9. Tests: expansion matches a
   level box of the same shape; caps; undo with and without later edits; a big op sends
   chunks.
2. **Edit mode** in `vox-world`, desktop first: selection, ghost, tools, all driven by `edit:`
   commands. Test with the client's screenshot mode and scripted commands.
3. **Workshops**: the directory's calls, rights and limits; a new host call that adds a
   persistent world at run time and keeps it across restarts; seeding from a level, a
   snapshot or a structure; the `editors` list in the rules. Tests in `vox-directory`,
   `vox-host` and `vox-server`.
4. **The page and the bridge**: `/edit`, the palette, tools, selection panel and History drawer;
   `command()` and `vox_event`. Checked in the browser.
5. **Live settings**: `ToServer::Settings` and `ToClient::Settings`, the World tabs, gizmos for
   spawn, portals and cameras.
6. **Saving**: level export (with the round-trip test), Save as structure, and restoring a
   snapshot while the workshop runs.
7. **Together**: presence and outlines.

## Decisions (2026-11-27)

The owner took the recommendations on the questions this design left open:

- **Public worlds aren't edited in place.** Anyone with the right copies one into a workshop,
  works there, and exports a level; changes to the worlds the game ships go through review as
  a pull request against `data/levels/` (and the builders in `tools/assets/` where a level is
  generated). Replacing a live public world from a workshop, by staff, after a snapshot, can
  come later.
- **Workshops: three an account**, at the default world size (512 × 128 × 512 blocks), free (no
  Authority) to begin with. Their history is the usual week of snapshots. Storage isn't
  counted against accounts until it has been measured on real workshops.
- **Visit links: yes, later.** An owner can make an unlisted, read-only link to a workshop, and
  revoke it; it comes after the seven steps above.
- **No touch editing in the first version.** The editor is for desktop browsers with a mouse
  and keyboard; on a touch device the page says so and offers play instead.
- **Props get a tool in the editor** (a Props tab beside the blocks palette) once props exist,
  not a page of their own.
