# Design: world graphs and generated art

Status: **proposed** · 2026-11-25 · nothing here is built yet

Related: [world-scaling.md](world-scaling.md) (regions, generated terrain, streaming),
[repo-structure.md](repo-structure.md) (the `crates/`, `bins/` and `apps/` layout this uses).

## Summary

Levels are described as a **graph of nodes** ("village", "house", "road", "well") that starts
coarse and gains detail in passes. A language model works on the graph through a small set of
tools; it never places blocks or voxels itself. Each node carries a **recipe**: a structured,
typed list of building operations (walls, roof, door, windows, trim, weathering) that a Rust
interpreter turns into voxels in the project's art style. The same Rust code runs **natively and
in WebAssembly**, so the server, the desktop client and the browser can all expand a node into
the same voxels from the same recipe and seed.

Each node also carries a **vector** that places it among accepted builds in a **library**. The
vector finds similar builds to use as examples and keeps neighbouring nodes in a consistent
style. Accepted builds grow the library over time.

No Python. Everything is Rust crates in the workspace, buildable for `wasm32-unknown-unknown`.

## Why recipes, not generated code or meshes

The project's art is already made by code: the scripts in `tools/` build every set, prop,
animal and texture from primitives, a fixed palette and fixed voxel sizes. That is what gives the
art its style. This design keeps the approach but changes the medium:

| Option | Problem |
| --- | --- |
| The model writes Python build scripts (today's approach) | Not wasm; needs a sandbox to run untrusted code; cannot run in the browser |
| The model writes Rust compiled to wasm modules | Needs a compiler in the pipeline and a wasm runtime to sandbox each module; slow and heavy |
| An image or 3D model service | Off-style, not reproducible, not editable |
| **The model writes a recipe (data) that Rust interprets** | Chosen: no code execution, validated by type, deterministic, runs everywhere |

A recipe is data, so it is checked by deserialising it against the Rust types. Invalid recipes
are rejected before anything runs, and a recipe cannot do anything the interpreter does not
offer. The interpreter is the style.

## Concepts

- **Graph**: a level, or a region of a large world. Nodes and links.
- **Node**: one thing in the world, with:
  - `kind`: house, road, tree, well, village, island and so on (a fixed, versioned vocabulary)
  - `detail`: how far it has been refined (see below)
  - `footprint` and `parent`: where it may build; children stay inside their parent
  - `params`: what must be true (size, floors, doors facing the path)
  - `vec`: style and intent as a vector (see *Library and vectors*)
  - `recipe`: the building operations, once the node is detailed enough
  - `seed`: so expansion is reproducible
- **Link**: a relation between nodes: `contains`, `next_to`, `faces`, `connects` (a road or
  path), `portal_to`.
- **Detail levels**, refined in order: `intent` (just a kind and a description) → `layout`
  (footprint and position) → `massing` (outline and height) → `structure` (a recipe with walls,
  roof, openings) → `dressing` (trim, props, weathering). A node at any level can be drawn: at
  `intent` and `layout` as a block outline, from `massing` as its shape.
- **Recipe**: an ordered list of typed operations on a voxel grid at one of the project's
  scales.
- **Scale**: `character` (5 cm voxels), `prop` (10 cm), `world` (0.5 m blocks). A house you walk
  into is `world` scale, compiled to level blocks; a lamp on its porch is a `prop`.

## Crate layout

Following [repo-structure.md](repo-structure.md):

```
crates/
  vox-art/                 voxels in the project's style; no Bevy; native and wasm
    src/
      lib.rs
      scale.rs             Scale (character, prop, world) and grid sizes
      grid.rs              VoxelGrid: sparse chunks of palette indices, bounds, iteration
      palette.rs           the project palette and named materials; style limits
      rng.rs               one seeded RNG (replaces the copies in the avatar app and bevel)
      recipe/
        mod.rs             Recipe, Op (serde + JSON Schema), versioning
        ops/               one file per operation: box, extrude, roof, cut, openings,
                           trim, stairs, scatter, weather, mirror, repeat
        eval.rs            interpret a recipe into a VoxelGrid, deterministically
      check.rs             footprint, palette, voxel and triangle budget, floating parts,
                           reachable doors; returns structured reasons
      mesh.rs              greedy meshing with packed vertices (shared later with vox-world)
      export/
        vox.rs             MagicaVoxel .vox (so artists can open and touch up results)
        glb.rs             glTF binary for props and sets
        level.rs           level boxes for world-scale structures (today's level format)
    tests/                 golden tests: recipe in, voxel hash out, same on native and wasm

  vox-graph/               the world graph; no Bevy; native and wasm
    src/
      lib.rs
      node.rs              Node, Kind, Detail, Params, Footprint
      link.rs              Link and relations
      graph.rs             Graph: add, remove, query by area, children
      ops.rs               the operations a language model may call (see below)
      constraints.rs       children inside parents, no overlaps, roads meet at region edges
      expand.rs            node to recipe to voxels or level blocks, using vox-art
      store.rs             serialised form, versioned; templates pinned by version

  vox-library/             accepted builds and their vectors; native and wasm (read side)
    src/
      lib.rs
      entry.rs             Entry: recipe, vector, kind, scale, review notes, render reference
      index.rs             nearest-neighbour search (brute-force cosine first; an index later)
      embed.rs             the Embedder trait: how a vector is made (see below)

bins/
  vox-forge/               the authoring tool (native)
    src/
      main.rs              command line: graph new, node add, refine, build, check, render
      mcp.rs               the same commands as an MCP server, so a language model can drive it
      render.rs            headless render of a build for review (uses vox-world; feature
                           "render")
      review.rs            accept or reject, with reasons; accepted builds go into the library

apps/
  forge/                   review UI (Bun, TypeScript): the graph, renders of builds, accept and
                           reject with reasons; talks to vox-forge over local HTTP and loads
                           vox-art built for wasm to preview recipes in the browser
```

Dependency direction: `vox-art` ← `vox-graph` ← `vox-forge`; `vox-library` depends on
`vox-art`. `vox-core` uses `vox-graph` and `vox-art` for world generation; `vox-world` (and so
both clients) uses them to expand nodes locally. Nothing in `vox-art`, `vox-graph` or
`vox-library` depends on Bevy, threads, the file system or the network, so they build for wasm
unchanged; file and network access live in the apps.

## A recipe

JSON is the exchange format (what a language model writes and the tool stores); the Rust types
are the definition, and a JSON Schema generated from them is given to the model.

```json
{
  "version": 1,
  "scale": "world",
  "seed": 7,
  "ops": [
    { "box":     { "from": [0, 0, 0], "to": [10, 6, 8], "mat": "brick", "hollow": true } },
    { "roof":    { "style": "stepped", "over": [0, 0, 10, 8], "mat": "plank", "overhang": 1 } },
    { "cut":     { "door": { "face": "+z", "at": 4, "width": 2, "height": 4 } } },
    { "windows": { "faces": ["+x", "-x"], "every": 3, "size": [1, 2], "sill": 2, "mat": "glass" } },
    { "trim":    { "edges": "corners", "mat": "stone" } },
    { "weather": { "amount": 0.15, "mats": ["moss", "dirt"] } }
  ]
}
```

Rules the interpreter enforces, which are the style:

- Only named materials from the palette; colour comes from materials, not raw values.
- Integer coordinates on the node's scale; nothing outside the node's footprint.
- Operations are high-level (a roof, a row of windows), so proportions and edge treatment
  come from the interpreter, not from the model's guesses.
- Randomness only through the seeded RNG, so the same recipe and seed give the same voxels on
  every machine and target.

## The pipeline

1. **Plan the graph.** The model, through `vox-forge`'s tools, adds nodes and links at `intent`
   and `layout` detail for a region or level ("a village of six houses around a well, a road to
   the north portal").
2. **Retrieve.** For each node to refine, find the nearest library entries by vector and kind.
   They go to the model as examples.
3. **Write the recipe.** The model returns a recipe for the node. It is deserialised and
   type-checked; failures go straight back with the error.
4. **Build.** `vox-art` evaluates the recipe into voxels.
5. **Check.** `vox-art::check` tests footprint, palette, budget, floating parts and reachable
   doors; `vox-graph::constraints` tests the node against its neighbours. Failures go back to
   step 3 with structured reasons.
6. **Render and review.** `vox-forge` renders the build in the game's own renderer, and the
   review UI in `apps/forge` shows it. At first a person accepts or rejects it there; later an image-understanding model can pre-screen against the
   style guide and reference images.
7. **Accept.** The recipe and vector are stored on the node and, if it is good, added to the
   library.

The graph is the source of truth. The model's output is frozen into it when accepted, so later
expansion never calls a model and is always reproducible.

## Tools the language model gets

Exposed by `vox-forge` as MCP tools (and as command-line commands for scripting):

- `graph_open`, `graph_summary(area)`: read a level or region at a chosen level of detail
- `node_add(kind, parent, params, description)`, `node_link(from, to, relation)`
- `node_refine(id, target_detail)`: returns what the next detail level needs, nearest library
  examples and the recipe schema
- `node_set_recipe(id, recipe)`: build and check; returns problems or a render reference
- `node_accept(id)`, `node_reject(id, reasons)`
- `library_search(kind, text_or_vector)`

Every tool validates its input against the Rust types; the model cannot write files, run code
or reach the network through them.

## Library and vectors

- Each library entry is a recipe plus a vector, kind, scale and review notes.
- **The vector first describes recipes, not images.** A deterministic embedder built from the
  recipe itself (materials used, proportions, operation counts, roof style) works on day one,
  needs no model, and runs in wasm. A text embedding of the node's description can be added
  alongside it, computed by the native tool and stored with the entry.
- `Embedder` is a trait, so a learned embedder can replace or join these later.
- Neighbouring nodes are steered towards nearby vectors, which keeps a village consistent.
- Accepted recipes are the training data if a learned recipe generator is ever wanted; because
  they are structured, they are far easier to learn from than meshes or images.

## Fit with the world-scaling plan

- **Generation everywhere.** `vox-graph::expand` is the generator that
  [world-scaling.md](world-scaling.md) needs. The server stores graphs and recipes, which are
  small, and clients expand them into chunks locally, in the browser too.
- **Streaming.** A region's graph replaces most of its chunk data on the wire. Player edits are
  stored on top, as before, and win over generated blocks.
- **Far terrain.** Nodes at `massing` detail give the low-detail silhouette for distant regions
  without expanding them.
- **Refine ahead of players.** The authoring tool can refine nodes where players are heading;
  regions no one has reached can stay at `layout` detail.

## Porting the existing tools

The Python scripts stay as they are until each kind of asset has a recipe that reproduces it.

1. Port the shared toolkit (`Vox` in `tools/build_animals.py`, `write_prop` in
   `tools/build_roasted.py`) into `vox-art` operations and exporters.
2. Reproduce one existing asset from a recipe (a stage set from `tools/build_sets.py`) and
   compare voxel for voxel with today's `.vox` file.
3. Move each builder in turn; delete a Python script once its recipes match.

Character rigging (`tools/build_animals.py` builds bodies on the shared skeleton) is out of
scope for recipes at first; it stays a separate tool until there is a reason to port it.

## Testing

- **Golden tests**: fixed recipes and seeds with stored voxel hashes, run natively and under
  wasm (for example with `wasm-bindgen-test`) in CI, so the two targets never disagree.
- **Schema tests**: every operation round-trips through JSON; the JSON Schema given to the model
  is generated from the types and checked in.
- **Check tests**: known-bad recipes (outside the footprint, floating roof, unreachable door) fail
  with the expected reasons.
- **Visual tests**: rendered builds compared against approved images, using the visual test
  setup the avatar app already has.

## Phases

1. `vox-art` with grid, palette, RNG, five operations (box, roof, cut, openings, trim),
   evaluation, checks and `.vox` and level export; golden tests on both targets.
2. Reproduce one existing set from a recipe.
3. `vox-graph` with nodes, links, detail levels, constraints and expansion; one level (the yard)
   described as a graph and compiled to today's level format.
4. `vox-forge` command line and MCP tools; a language model refines the yard's house from
   `intent` to `structure`, with human review.
5. `vox-library` with the recipe embedder and search; accepted builds feed examples.
6. Connect to world scaling: graphs stored per region, expanded by clients.

## Open questions

- Which language model provider drives `vox-forge`? The design leaves it open: any client that
  speaks MCP can.
- How fine should operations be? Too coarse limits variety; too fine and the model is placing
  voxels again. Start coarse and add operations when reviews show a need.
- Who may run the authoring tool on a public world: only you, world owners, or players through a
  moderated request?
- Should player builds enter the library? It needs consent and moderation.
