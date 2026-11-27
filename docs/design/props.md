# Design: props and the prop pipeline

Status: **proposed** · 2026-11-26 · nothing here is built yet

Related: [world-graph-and-art.md](world-graph-and-art.md) (recipes, `vox-art`, the library),
[world-format.md](world-format.md) (entities, structures, entitlements),
[world-network.md](world-network.md) (dungeon templates and instances),
[controls-and-hud.md](controls-and-hud.md) (the use action).

## Summary

A **prop** is a small authored object that stands in a world: a lamp, a bench, a crate, a
lever, a rowboat. It is not a block (a prop has its own finer voxels and can have parts that
move) and not a structure (a structure is a blueprint of blocks; a prop is one thing with
behaviours). Worlds hold **instances** of props as entities, pinned to a version, the way
structures are.

The pipeline has two lanes into one library. Props are made as **recipes** (data that a person
or a language model writes and Rust interprets, from `world-graph-and-art.md`) or imported as
hand-made **MagicaVoxel files**. Both become the same thing, a `PropDef` plus a voxel grid,
and both go through the same **checks**, **review** and **library** before a world can place
them. A prop's **behaviours** (it gives light, you can sit on it, it opens) come from a small
closed vocabulary, never code, so a prop is as safe to accept from anyone as a block.

It all runs natively and in WebAssembly (`vox-art` has no Bevy, no threads, no file system), so
the browser expands a prop the same way the desktop does. No Python.

## Where things are today

- **143 props** for the avatar app in `assets/props/` (held items, set furniture, effects):
  `.glb` files with 5 cm voxels, one palette image, greedy-merged meshes (12 to 130 triangles
  each), origin at the grip. Sources are MagicaVoxel `.vox` in `source/vox/props/`; the builders
  are Python (`tools/assets/build_roasted.py`'s `write_prop` and `build_animals.py`'s `Vox`).
- **Worlds have no props.** What stands in a world is blocks (0.5 m, textured from an atlas,
  in chunk meshes). Lights are blocks: `lamp`, `fire`, `lumen` and the like, found near the
  camera by `outdoors.rs` (`LightKind`). The portal frames and the pier lanterns are built of
  blocks.
- **The plan already allows for them**: `Prop { model, at, yaw }` is among the first entity
  components in `world-format.md`, and `world-graph-and-art.md` names a `prop` scale (10 cm)
  and a `vox-art` crate. This design fills in what a prop is and how it is made.

## Blocks, structures, props

| | Block | Structure | Prop |
| --- | --- | --- | --- |
| Made of | one cell (0.5 m) | blocks, up to 64 × 64 × 64 | its own voxel grid (5 or 10 cm), up to 64 voxels a side |
| Look | atlas tile with height, glow | the blocks' | one flat palette (the avatar props' look) |
| Placed | on the 0.5 m grid | on the 0.5 m grid, quarter turns | 5 cm grid, 15° turns |
| Behaviour | none (light by flag) | none | light, seat, door, switch, readable, sound, motion |
| Edited by players | yes, one block at a time | by saving a new version | no: a prop is placed, moved, removed, never reshaped |
| Owned, shared | the world's plots | owner plus grants | catalogue props are open; player-made ones are owned |
| Use it for | terrain, walls, floors, bulk | a house, a bridge, a village | what is in the house, on the bridge, in the village |

A house is a structure; its lamp, bed, door and the letters on the table are props. If an
object is made to be walked on and built with in bulk, it is blocks; if it is an object, it is
a prop.

## What a prop is

A `PropDef` is immutable data. Its **version** is the hash of its canonical form and its voxels
(the same content-addressing as regions), and an instance pins that hash.

```json
{
  "id": "lamp.street",
  "name": "Street lamp",
  "scale": "fine",
  "size": [7, 44, 7],
  "pivot": [3, 0, 3],
  "tags": ["street", "light", "village", "outdoor"],
  "place": { "on": ["floor"], "footprint": [[0, 0]], "height": 2 },
  "collide": "footprint",
  "parts": { "lantern": { "voxels": "tagged by the recipe" } },
  "sockets": [{ "name": "light", "at": [3, 38, 3], "dir": "+y" }],
  "states": ["off", "lit"],
  "behaviours": [
    { "light": { "socket": "light", "colour": "warm", "power": 3.0, "when": "night" } }
  ],
  "variants": [{ "name": "iron" }, { "name": "copper", "paint": { "iron": "copper" } }],
  "palette_version": 1,
  "source": { "recipe": "recipe.json" }
}
```

- **Scale** is `fine` (5 cm voxels, the characters' scale: held items, small decoration) or
  `prop` (10 cm: furniture, boats, trees). Chosen by size, not taste: under a metre, fine.
  A prop is at most 64 voxels a side (3.2 m fine, 6.4 m at 10 cm); anything bigger is a
  structure.
- **Pivot** is the voxel column that sits on the placement point (the floor under it; for a
  wall prop, the back; for a ceiling prop, the top). Placement, snapping and turning use it.
- **Place** says what it may stand on (`floor`, `wall`, `ceiling`, `table`, `water`) and the
  blocks it needs free (the footprint in 0.5 m cells and a height), which is also what
  `collide` can block.
- **Collide** is `none` (walk through: a rug, a flower), `footprint` (its cells are blocked), or
  `boxes` (a few boxes in the def, for a bench you can walk under the back of).
- **Parts** are named groups of voxels the recipe or the import tags, so a state can show,
  hide, move or spin them (a door's leaf, a lamp's glass, a fan's blades).
- **Sockets** are named points with a direction: where light comes from, where you sit, where
  another prop may attach (a candle on a table's `top` socket), where a held prop's grip is.
- **States** are small named integers (`off` and `lit`, `closed` and `open`). A state chooses
  which parts show and where moving parts rest. The world keeps each instance's state.
- **Variants** are palette swaps (`paint`) and a seed for the few props that scatter (leaves),
  so one def gives a family: iron, copper and painted lamp posts.
- **Behaviours** are the closed vocabulary below. A def lists which it has; a prop has no other
  way to do anything.

### Behaviours (a closed set)

| Behaviour | What it does | Who runs it |
| --- | --- | --- |
| `light` | a light at a socket: colour, power, `when` (always, night, or in a state); fire-type ones flicker | clients (the nearest few, as lamps and fires are picked now) |
| `seat` | a socket where a character sits, with the pose clip's name | the server decides who sits where; clients play the clip |
| `door` | a state with a swing or slide of named parts; closed blocks its footprint, open doesn't | server (state, collision), clients (motion) |
| `switch` | toggles its own state, or every prop on its `channel` in the world (a lever opens a gate) | server |
| `readable` | up to 140 characters of text shown when used (a sign, a note) | server stores, filters and sends |
| `sound` | plays one named sound from the real-recording set, looped or on use | clients |
| `motion` | `spin`, `swing`, `bob`, `sway` (with the wind) or `flicker`: looks only, deterministic from the clock | clients |
| `marker` | an objective or anchor for dungeons and quests (a place to reach, a thing to use) | server |

Later, with the server-side inventory: `container` (slots) and `pickup` (becomes an item).

## The pipeline

```
 brief ──► source ──► normalise ──► evaluate ──► check ──► render ──► review ──► accept
 (a line)  recipe or   one grid +    voxels      structured  contact     person     │
           .vox        PropDef       (native =   reasons     sheet       (later a   ▼
                                     wasm)       back to     (4 angles,  model      library ──► pack ──► worlds
                                                 the author  3 sizes,    pre-       (vox-library)  (hash-addressed
                                                             in a scene) screens)                blobs)
```

Every stage takes and returns data, so any stage can be rerun from the stored source, and a
language model drives it through tools (below) the same way it drives the world graph.

### Lane A: recipes

A prop recipe is the world-graph recipe (`world-graph-and-art.md`) at `fine` or `prop` scale,
with operations that suit objects rather than buildings. The interpreter owns proportions and
edge treatment; the author says what, not which voxel.

| Operation | Does |
| --- | --- |
| `box`, `slab`, `sphere`, `cyl` | solids (round ones are voxel-true: no half voxels) |
| `lathe` | a profile revolved about the pivot's axis: vases, barrels, lamps, mugs, bells |
| `legs`, `frame`, `rail` | repeated supports and frames: tables, chairs, beds, fences, ladders |
| `stairs`, `ramp` | steps and slopes |
| `mirror`, `repeat`, `ring` | symmetry and repetition (a rule of the style: most things are symmetric) |
| `cut`, `hollow` | openings, drawers, the inside of a crate |
| `part` | tag what the next operations make as a named part |
| `socket` | a named point and direction |
| `bevel` | the style's edge: one voxel of rounding on big props, none on small |
| `shade` | the project's three-tone rule on each material (base, shade, highlight) |
| `scatter` | seeded leaves, grass blades, pebbles, within a shape |
| `weather` | seeded wear: chips, moss, rust, snow on top, salt on the sea's props |

Only named materials from the project palette (and their ramps); randomness only through the
seeded RNG, so the same recipe, seed and variant give the same voxels on every machine.

### Lane B: hand-made `.vox` files

MagicaVoxel stays a first-class source: some objects are better drawn than described, and the
existing 143 props already are. An import:

1. **Reads** the file (a hardened reader, as `avatar-voice` has for WAV: sizes capped, indices
   checked).
2. **Orients** (MagicaVoxel is z-up; the world is y-up) and **finds the scale** from the def
   (`fine` or `prop`), not from the file.
3. **Snaps the palette**: each colour to the nearest project palette entry, with the distance
   reported; past a limit the import fails with the colours named, rather than guessing. (The
   palette is the style: this is how an imported prop is made to belong.)
4. **Finds the pivot** from a marker voxel in a reserved colour (and falls back to the base
   centre), and reads **parts** and **sockets** from other reserved colours, so a person can
   tag them in MagicaVoxel without leaving it.
5. **Cleans**: drops stray voxels (islands of one), reports floating parts instead of dropping
   them.
6. Keeps the file as the `source`, and the def beside it holds everything the file can't say
   (behaviours, states, tags).

**The touch-up loop** joins the lanes: `vox-forge` can export any recipe's result as `.vox`; a
person opens it, fixes what a recipe can't, and imports it back as a new source with
`derived_from` pointing to the recipe. The recipe stays for variants; the hand edit is a
recorded override.

**Seeding the library**: the usable existing props (bed, ladder, boat, lectern, chairs,
lawnchair, pine, brick, can, burger, ball, book, binoculars, bag, paper, phone, rake, hammer,
mic, coffee, notepad, pen, gavel, flag stand, podium and more) come in through Lane B from
`source/vox/props/`, so the first library is not empty. The politically specific ones (placards,
vote cards, rally signs) stay with the avatar app until the open product question in
`project/characters-and-content` is settled.

### Normalise and evaluate

Both lanes end as a `VoxelGrid` (sparse, palette indices, the project's grid type from
`vox-art`) and a `PropDef`. Evaluation is deterministic; the golden tests hash the voxels on
native and in wasm.

### Checks

`vox-art::check` returns structured reasons (kind, where, what to change) so an author, human
or model, can fix and rerun. Failing any **error** stops acceptance; **warnings** go to the
reviewer.

| Check | Limit | Kind |
| --- | --- | --- |
| Size | at most 64 voxels a side; the def's `size` matches | error |
| Voxels | 20,000 | error |
| Triangles after greedy meshing | 400 (small), 1,200 (medium), 3,000 (large) by size class | error; half is a warning |
| Palette | only project colours; at most 24 distinct; ramps respected | error |
| Floating parts | every part connected to the rest or to a socket that says why | error |
| Thin walls | nothing a single voxel thick that is more than 8 voxels across (it shimmers) | warning |
| Pivot | on the ground plane for floor props; inside the grid always | error |
| Footprint | the cells the voxels cover fit `place.footprint` and `height` | error |
| Sockets and parts | named, inside the grid, no duplicates; a `light` behaviour names a socket | error |
| Behaviours | known, parameters in range; a `door` has a part to move; `readable` text length | error |
| Emitters | at most 4 light sockets | error |
| Silhouette | the render at 24, 48 and 96 px keeps its outline (the reviewer's contact sheet marks it) | warning |
| Proportion | against the scale reference (a 1.85 m character, a 2 m door, a 45 cm seat) | warning |
| Names and tags | tags from the vocabulary; no real-person or trademarked names | error |

### Render and review

`vox-forge` renders a **contact sheet** in the game's own renderer: four angles, the three
distances, the prop beside a character and a door for scale, in the yard by day and in the dark
sea at night (so emissive props show), and each state and variant. `apps/forge` shows it; a
person accepts or rejects with reasons. Later a model can pre-screen against the style guide
and reference images, as `world-graph-and-art.md` plans for builds.

**Style guide for props** (given to authors and reviewers):

- Voxels, not detail: a prop reads from 20 m away; if it needs single-voxel noise to be
  recognised, it is the wrong scale.
- Shade by the three-tone rule; one accent colour; the palette's ramps, not mixes.
- Proportions match the characters (1.85 m tall, 5 cm voxels): a chair seat at 45 cm, a table
  at 75 cm, a door at 2 m, a step at 20 cm.
- Round big things (one voxel of bevel); leave small things sharp.
- Symmetry unless the object isn't.
- No text in voxels (signs use `readable`); no logos.
- Emissive sparingly: a lamp's glass, a window's pane, an indicator, never a whole prop.
- Wear tells a story (the pier's lanterns are salted, the station's panels clean).
- The hard constraints apply (`convention/hard-constraints`: no real-person likenesses or
  trademarked characters; sounds are real recordings; the medic's cross is green).

### Library and shipping

- **Source** lives in the repository: `data/props/<id>/` holds `def.json`, `recipe.json` or
  `source.vox`, and the accepted review sheet. This is what is edited and reviewed.
- **The library** (`vox-library`, kind `prop`) indexes accepted props by id, tags, scale, size
  and the recipe vector for search ("something to sit on, wooden, small").
- **Built blobs**: each accepted version is built to a `VOXP` blob (the def, then the grid
  run-length coded and zstd-compressed, the same machinery as region blobs, BLAKE3-addressed),
  verified with the same caps as untrusted regions. Blobs are served by hash over HTTP from the
  directory's files (the edge already caches `/assets/`), so a client fetches only the props a
  region names, once. A pack of all accepted props (the 143 avatar props are about 3 MB as
  GLBs; these will be far smaller) ships with the desktop app.
- **Versions**: an instance pins a hash. Changing a prop makes a new version; old ones stay as
  long as any world refers to them. The palette only grows (`palette_version`), so old props
  stay valid.
- **The avatar app** keeps reading `.glb`: `vox-art`'s exporter writes the same GLB the Python
  builders do today, so the two apps share one source and one library, and the Python builders
  retire as each prop is ported (the porting plan in `world-graph-and-art.md`).

## In a world

### The entity

```
Prop { prop: "<hash>", at: (x, y, z), yaw: 0..23, variant: 0, state: 0 }
```

`at` is in 5 cm units (an `i32` triple: exact, compact, the same on every machine); `yaw` in 15°
steps; `state` and `variant` small integers. It is one more world component in `vox-format`,
stored in its region's entities, so snapshots, Oxen history, restores and instances carry props
without further work. Props inside a structure are part of its blob, in the structure's own
coordinates.

### Placing

A player with building rights (a plot's builders, or anyone in a world that allows it) places a
prop from the quick wheel or inventory. The server checks, in this order: the prop's hash is a
known accepted one, the player may use it (entitlements: catalogue props are open to every
signed-in account; a player-made one needs `use`), the position is within reach and in the
world's bounds, the footprint is free of solid blocks and other props' footprints, a surface it
accepts is under (or beside, or over) it, and the plot and region caps hold (to begin with 64
props a plot, 4,000 a region). Moving and removing are the same checks.

### Seeing it

Clients expand an instance from its blob into a small mesh (cached by hash) with the avatar
props' look: the shared palette image, flat shading. To keep thousands cheap:

- **Static props** of a region are merged into one mesh per material per chunk and rebuilt
  with the chunk, under the same per-frame budget as chunk meshing (8 ms desktop, 5 ms
  browser).
- **Repeats** (a row of fence, a street's lamps) are instanced.
- **Moving parts** are separate nodes only on props with `door` or `motion`, and only within
  a short distance; farther, the prop is its resting mesh.
- **Distance**: small props fade out by size (a mug at 30 m, a boat at 300 m).
- **Light**: the nearest few `light` sockets join the lamps and fires the client already picks
  (`outdoors.rs`), so props light the world as blocks do.

### Standing in the way

Blocks stay the only thing in chunk data. A prop with `collide: footprint` adds its cells to a
**prop occupancy** layer that `vox-core::body` and the placement checks read next to the blocks;
both server and client derive it from the same instances, so prediction agrees. A closed `door`
blocks and an open one doesn't. If the blocks a prop stands on are broken, the prop is removed
(and returned to its owner's inventory once there is one).

### Using it

The `use` action (the HUD's prompt) targets the nearest prop under the cursor within reach
(the voxel raycast, against prop boxes). The server decides: is it in reach, does the
behaviour exist, may this player (plot rules), is the actor within the rate limit; then it
changes the instance's state, sets who sits where, or sends the `readable` text, and tells the
world. States persist.

### Props and dungeons

Dungeon templates (`world-network.md`) are worlds frozen as snapshots, dressed with props:
`switch` and `door` on a channel are the puzzles (a lever opens a gate, a plate lowers a
bridge), `marker` is the objective a raid is won on, and torches and braziers are the light.
Because props are in the region blobs, an instance gets them for free.

## Tools for a language model

Added to `vox-forge`'s MCP tools (`world-graph-and-art.md`), all validating against the Rust
types and none able to run code or touch files outside the project:

- `prop_new(id, brief, scale, tags)`: a def and an empty recipe, with the style guide
- `prop_set_recipe(id, recipe)` and `prop_set_def(id, def)`: build and check; returns reasons
  or a render reference
- `prop_import(id, vox_path)`: Lane B, with the palette snap's report
- `prop_check(id)`, `prop_render(id, views)`
- `prop_accept(id)`, `prop_reject(id, reasons)`
- `library_search(kind = prop, text_or_vector)`: examples to start from, and to avoid making a
  second bench

## Fit with the other designs

- **world-graph-and-art**: this is its `prop` scale made concrete; `vox-art` gets `prop` and
  its operations; `vox-library` gets kind `prop`; a node's `dressing` detail level is where a
  graph places props.
- **world-format**: `Prop` is an entity component; blobs are content-addressed with the same
  caps; props travel in structure blobs and snapshots (and so in Oxen history).
- **Entitlements**: `vox_core::entitle` already is the rule for who may use, copy and edit a
  blueprint; a player-made prop is the same kind of record in the directory (owner, grants,
  versions), so there is one rule, not two.
- **world-network**: dungeon dressing and objectives, above.
- **Browser**: no threads, no blocking, blobs fetched by hash, meshes built in the same budget
  as chunks.

## Testing

- **Golden tests**: fixed recipes and seeds with stored voxel hashes, native and wasm.
- **Import tests**: the 143 existing `.vox` files import (or fail with named reasons); a
  round trip (recipe, export, import) keeps the voxels.
- **Check tests**: known-bad props (floating part, over budget, off-palette, thin wall) fail with
  the expected reasons.
- **Untrusted blobs**: crafted `VOXP` blobs (oversized grid, bad indices, huge part counts)
  are refused, as regions' are.
- **Server tests**: placement refused for each rule above, state changes and sitting, caps, and
  that a restore brings props back.
- **Visual tests**: contact sheets compared against approved ones, with the setup the avatar app
  already has.

## Phases

1. `vox-art`: grid, palette, `PropDef`, the `.vox` reader and importer, the checks and the
   contact-sheet render (command line). Import the existing props as the seed library.
2. Recipe operations for props and the golden tests; export to `.glb` (so the Python builders
   can retire).
3. `VOXP` blobs, the `Prop` entity, placement and the server's checks; meshing, batching and
   prop occupancy in the clients.
4. Behaviours: `light`, `door`, `switch`, `seat`, `readable`, `sound`, `motion`, then `marker`.
5. The library, search, and the MCP tools; a language model makes the first wave from a brief.
6. Player-made props with entitlements; containers and pickups when the inventory is on the
   server.

## Open questions

- **Scale**: is 5 cm (fine) plus 10 cm enough, or do the world's lamp posts and trees want 25 cm
  as a third? (More scales means more to check.)
- **Walk-through**: should decorative props never block by default, so a crowded room can't
  trap someone? (The design says `collide` is the author's choice; the check could warn on
  footprints that narrow a doorway.)
- **When blocks go**: removed props, or props that drop and keep? Removal is simplest.
- **Who places**: only plot builders and world owners, or anyone in worlds that allow building?
- **Moving parts**: states that swap and move whole parts are enough for doors and lids; a
  skeleton (as characters have) is only needed for things like a creature. Is that line right?
- **Player-made props**: reviewed before others can place them, or placeable by their owner at
  once and public only after review?
- **Text**: `readable` is the one place a player's words reach other players through a prop;
  it needs the same filtering as names and chat.

---

# The catalogue

Seventy props, grouped by where they are wanted. **Size**: S (under 30 cm, fine), M (to 1.5 m,
fine), L (to 3 m, 10 cm), XL (to 6 m, 10 cm). **Does**: the behaviours. ★ marks the **first
wave**, twelve props chosen so that between them they exercise every part of the pipeline.

## Light

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| ★ Street lamp | L | `light` (night) | iron post, glass lantern part; variants iron, copper |
| ★ Lantern (hanging) | S | `light`, `motion` swing | hangs from a `hang` socket; for piers and porches |
| Candle | S | `light` flicker | `table` surface; wax drips by `weather` |
| Wall torch | M | `light` flicker | wall; fire colours; the dungeon staple |
| ★ Brazier | M | `light` flicker, `switch` (lit, out) | bowl on three legs; lights from fire block colours |
| String lights | M | `light` ×4 | a swag between two sockets, repeated |
| Floor lamp | M | `light` | for the yard's house and the station |
| Moon lamp | M | `light` cold, `motion` bob | the void's floating light: pearl sphere on a thread |

## Home and village

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| ★ Bench | M | `seat` ×2 | wood, iron ends; seats two; pose clip `sit` |
| Chair | M | `seat` | four legs; variants for a dining set |
| Table | M | `top` socket | candles and props attach |
| Bed | M | `seat` (lie) | seeded from `bed.vox` |
| ★ Door (wood) | M | `door` swing | leaf part, frame; closed blocks its cells |
| Window shutters | M | `door` swing ×2 | wall prop that opens over a glass block |
| Mailbox | M | `readable`, `door` | the lid opens; text is the house's name |
| Fence section | M | collide boxes | repeats; gate variant is a `door` |
| Well | L | `motion` (bucket bob) | stone ring and a roofed frame |
| ★ Signpost | M | `readable` | arms by `part`; text on use |
| Flower pot | S | `sway` | seeded flowers; colourways |
| Rug | S | collide none | thin; checks the 1-voxel rule on purpose |
| Bookshelf | M | none (later `container`) | books by `repeat` |
| Wardrobe | L | `door` | later holds clothes |

## Nature

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| ★ Pine | L | `sway` | seeded layers, snow variant for the hill |
| Oak | XL | `sway` | trunk and a leaf mass by `scatter` |
| Bush | M | `sway` | berries by variant |
| Boulder | M | collide footprint | seeded, moss by `weather` |
| Reeds | M | `sway` | clumps; for shores |
| Mushrooms | S | `light` faint (variant) | glowing variant for the islands' hollows |
| Stump | S | `seat` | a seat made by nature |
| Driftwood | M | none | for the dark sea's shallows |
| Kelp | L | `sway` (water) | `water` surface; the swell sways it |

## Dark sea (the pier, the wreck, the lighthouse)

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| Pier lantern | M | `light`, `motion` swing | the lantern-lit pier's own; salted by `weather` |
| ★ Barrel | M | `top` socket | `lathe`, iron hoops; the tutorial for `lathe` |
| ★ Crate | M | stackable (`top`) | seeded variants, a lid that opens (state) |
| Fishing net | M | `motion` sway | draped on a rail |
| Anchor | M | collide footprint | iron; chain by `repeat` |
| Ship's wheel | M | `motion` spin on use | a `switch` of its own state |
| ★ Rowboat | L | `seat` ×2 | seeded from `boat.vox`; sits on `water` |
| Buoy | S | `motion` bob, `light` | red and white; blinks |
| Lighthouse lamp | L | `light` (rotating), `motion` spin | the sweep over the sea; sits in a structure |

## Station (orbit)

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| ★ Console | M | `readable`, `light` | status text on use; indicator lights |
| Airlock hatch | L | `door` slide, `switch` | cycles: a two-state door with a short sound |
| Handrail | M | collide none | along the truss, repeats |
| Oxygen tank | M | none | variants by colour band |
| Cargo container | L | `door` | stackable; the lander's cargo |
| Viewport shutter | L | `door` slide | over the dome's glass |
| Solar panel (small) | L | `motion` (slow turn) | tracks the sun by the clock |
| Antenna | L | `motion` spin, `light` blink | the mast's dish as a prop |
| Cryopod | L | `door`, `light` | lid opens; frost by `weather` |
| Tether reel | S | `motion` | hangs from a `hang` socket |

## Islands (dark fantasy)

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| Crystal cluster | M | `light`, `motion` pulse | seeded; colour variants: cold blue, ember red |
| Banner | M | `sway` | a `slab` of cloth hung from a rail; colourways |
| Chain | M | `sway` | `repeat`; hangs from a socket |
| Rune stone | M | `light`, `readable` | text on use: the islands' lore |
| Fire bowl | M | `light` flicker | a smaller brazier for the bridges |
| Gate (iron) | L | `door` slide, `switch` channel | opened by a lever across the bridge |
| Altar | M | `marker` | where an objective is placed |
| Bone pile | S | none | scattered; tasteful, never a figure of a person |

## Dungeon and play

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| ★ Lever | M | `switch` (channel) | the puzzle's input; two states; a click sound |
| Pressure plate | S | `switch` (stand on) | collide none; needs the server to see who stands |
| Portcullis | L | `door` slide | the puzzle's output |
| Chest | M | `door`, later `container` | lid opens; the first container |
| Objective beacon | M | `marker`, `light` | a pillar of light where a raid is won |
| Sconce | M | `light` flicker | wall; the dungeon's torches |
| Cobweb | S | none | corner prop; translucent |
| Teleport pad | S | `light`, `marker` | a portal's floor mark in a dungeon |

## Void (ethereal)

| Prop | Size | Does | Notes |
| --- | --- | --- | --- |
| Floating pearl | S | `motion` bob, `light` | the circle's drifting companions |
| Echo frame | L | none | a doorframe with an inner glow; a prop version of a portal's frame |
| Monolith marker | XL | none | the far slabs, as props so the void can scatter them |
| Door in the fog | L | `door` | the scattered doorframes, a door that opens onto nothing |

## The first wave

Twelve props, picked to cover the pipeline:

| Prop | What it proves |
| --- | --- |
| Barrel | `lathe`, the simplest recipe; `top` socket |
| Bench | symmetry, `legs`, `seat` behaviour with two sockets |
| Signpost | `part` and `readable` text |
| Street lamp | a `light` behaviour tied to night; variants by `paint` |
| Brazier | flicker, a two-state `switch`, fire colours |
| Door (wood) | `door` with a moving part and collision that changes |
| Lever | a `switch` on a channel, used with the door |
| Pine | `scatter`, `sway` with the wind, seeds, the snow variant |
| Console | emissive indicators, `readable` status text |
| Lantern (hanging) | `hang` socket, `motion` swing, attaches to another prop |
| Rowboat | imported from `boat.vox` (Lane B), `water` surface, `seat` ×2 |
| Crate | stacking by `top` sockets, lid state, `weather` |

When these twelve pass the checks, look right on the contact sheet, and behave in the yard, the
dark sea and the station, the pipeline is proven and the rest of the catalogue is work, not
research.
