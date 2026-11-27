# Design: ground relief and puddles

Status: **proposed** · 2026-11-27 · nothing here is built yet

Related: [world-scaling.md](world-scaling.md) (generated terrain, fixed-point rules),
[world-network.md](world-network.md) (the atlas: tiles that must agree at their edges),
[world-format.md](world-format.md) (what a world stores), `crates/vox-core/src/terrain.rs`
(the atlas's ground generator), `crates/vox-core/src/weather.rs` (the weather simulation).

## Summary

Natural ground (grass, dirt, gravel, cobblestone, stone, basalt, ash) gets **relief**: a top surface that dips a
few centimetres here and there instead of being perfectly flat. It is part of world
generation: a function of the world's seed, the block's place and its material, so every
machine sees the same dips without storing or sending anything. When it rains, water **pools
in the dips**: the weather simulation keeps how much water stands on the ground, the ground
darkens and shines as it gets wet, and puddles form in the low spots, then dry out.

The first version is drawn, not built: relief and puddles are worked out per pixel in the
block shader from world position. Meshes, collision and the wire stay as they are, except one
new weather field. A version with dips in the geometry itself is phase 5, only if the
silhouette turns out to matter.

## What we have today (checked in code)

- Every block top is exactly flat. The mesher merges neighbouring faces with the same tile and
  even light into one quad (`mesh_cells(.., merge: true)`), so a stretch of grass is a few big
  quads. Vertices are packed into 20 bytes, and the face word (`ATTRIBUTE_BLOCK_FACE`) has
  bits 3-7 and 28-31 free.
- Each corner carries ambient-occlusion shading (bits 8-15), which is darker where the corner
  touches a wall or another block.
- The block shader (`assets/shaders/block.wesl`) already does parallax from a per-tile height
  texture; its green channel is how deep each tile goes, and its blue channel is unused.
- Wet ground isn't drawn: rain falls (`rain.wesl`) but nothing it lands on changes. The weather
  knows the rain reaching each column (`Weather::rain`, mm/h; `Air::rain` at a point) and sends
  it in keyframes (`Keyframe::rain`).
- `vox-core::terrain` has seeded value noise in integers only (`lattice`, `noise`), so the same
  seed gives the same ground on every machine, native and wasm.

## Relief: part of world generation

**The function.** `vox_core::relief::depth(seed, x, z) -> u8` gives how far below the block's
top the ground is at a point, in 1/256 of a block. It is integer-only like `terrain::noise`,
but hashed in `u32` (not `u64`), so it can be mirrored exactly in WGSL. Two octaves:

- swales of 3 to 8 blocks: the low spots where water gathers;
- a finer roughness of about one block that breaks up the edges of the swales.

It depends only on world coordinates, so it runs on across blocks, chunks, zones and atlas tiles
with no seams. Atlas tiles use atlas coordinates, as `terrain::height` does.

**How much.** Each block in `data/blocks.json` gets `"relief"` (0-1, the share of the largest
dip) and `"soak"` (0-1, how much rain the ground drinks):

| Block | relief | soak | Why |
| --- | --- | --- | --- |
| grass, dirt | 1.0 | 0.6 | soft ground: dips, but drinks a lot |
| gravel, sand | 0.6 | 0.8 | drains fast; puddles only in a downpour |
| cobblestone, basalt, stone, obsidian | 0.7 | 0.1 | hard and uneven: puddles stay |
| ash, snow | 0.8 | 0.9 | soft and loose (snow's own melt is a later step) |
| planks, log, bricks, concrete, tile, glass, metal, lamp, pearl, lumen, crystal, magma | 0 | 0-0.2 | built or glowing things stay flat |

The largest dip is 6 cm (0.12 of a block). That reads in the lighting and in puddles, but never
as a hole. It goes into the tile data's blue channel, so the shader reads it with the tile it
already samples.

**Edges stay flat.** A dip fades to nothing where a corner's ambient occlusion says it touches
a wall or a neighbouring block. Ground meets walls, steps and placed blocks cleanly, with no
gaps, and still with no knowledge of neighbours in the shader.

**Per world.** The world's settings gain `"relief": { "seed": n, "amount": 0-1, "version": 1 }`.
It is on by default for generated atlas tiles, and hand-made levels opt in (sea, islands and
yard should). The seed is derived from the world id when not given. Settings already travel in
the welcome, so this needs no protocol change. `version` pins the function: changing it
changes how every world looks, so a new function is a new version, and golden tests pin the
old one's values.

**Edits.** Relief follows the block, not the save. A dirt block placed anywhere has that
spot's relief, and covering it removes its top. Nothing is stored. Players can't smooth or
roughen it (an open question below).

## Puddles: part of the weather

**Ground water** joins the simulation: `Weather::ground: Vec<f32>`, mm standing per column,
stepped with the rest:

```text
d ground / dt = rain × (1 − soak of the area) − evaporation(temperature, wind, humidity, sun)
```

It is capped at the depth the dips can hold. The soak of an area is that of its most common
ground, from the column's top blocks, worked out when the weather grid is laid over the world.
It travels in keyframes (`Keyframe::ground: Vec<u16>`), so a player who joins mid-storm sees the
same puddles as everyone else. Adding a field to the postcard keyframe changes its encoding,
which means **protocol 8**. Clients step it between keyframes, as they do the rest of the
weather.

**Two numbers reach the block shader**, read at the camera's column (weather cells are tens of
metres, so one value for the view is right):

- `wet` (0-1): rain now or recent ground water. The ground darkens (about −30% albedo on soft
  ground) and its roughness drops, so it shines.
- `fill` (0-1): ground water over the dips' capacity. A pixel whose relief depth is greater
  than `(1 − fill)` of the largest dip is under water. It becomes a flat, dark, nearly
  mirror-like surface lit through the same PBR path as the sea, with the environment map in it.
  When rain is falling it gets rings from noise, as many as the rain rate calls for.

This sets one water level for every dip in view, not a fill per basin. Because the low spots
are where the swales put them, puddles still form in the hollows, grow out from the middle,
join, and shrink back from the edges as they dry. Below 0 °C the puddles freeze: no rings,
paler and smoother. Snow filling the dips first is a later step.

## Costs

- **Vertices and meshing:** none. Faces still merge, and nothing new is meshed.
- **Pixels:** two octaves of value noise (8 hashes) and a few mixes for each top-face pixel of
  ground with relief. That is small beside the parallax steps the shader already takes.
  Measure it on the browser build (Medium quality) with the frame probe used for the fog fix.
- **Wire:** one `u16` per weather column per keyframe.
- **Server:** one more field stepped per column.

## Rollout

1. **The function:** `vox-core::relief` (u32 integer noise, amount per material), golden values
   pinned in tests, the `relief` world setting, and `relief`/`soak` in `blocks.json`. Check
   that values agree across a block, chunk, zone and atlas-tile edge.
2. **Dry relief in the shader:** a WGSL copy of the function in `block.wesl`, with normals
   tilted by its slope and a little darker in the hollows (dirt gathers there). Edges are
   pinned by ambient occlusion. Check with `tools/test/visual_test.py` references.
3. **Ground water:** `Weather::ground`, stepped and sent in keyframes (protocol 8); `wet` and
   `fill` reach the block material.
4. **Puddles:** wet darkening and shine, water below the fill line, rain rings, ice below
   0 °C. Check on a rainy copy of the islands, as the rain shader was checked.
5. **Only if wanted, geometric dips:** displace top-face corners near the camera (unmerged,
   within about 16 m) so silhouettes and feet show them; collision stays flat. Measure the
   vertex cost first.

## Hard constraints

Splashes underfoot, if added, use real CC0 recordings (no synthesised sound). The relief and
puddles add no likenesses or characters.

## Open questions

- The largest dip: 6 cm is a guess. It should be set by eye on the islands.
- Do puddles do anything? Splash sounds and footprints are cheap. Slipping is not, because it
  would need the server to know the relief (it could, being part of generation).
- Can players change relief (a rake, a paver)? That would need a per-block flag in the chunk
  data, which today holds only block ids.
- Should snow fill the dips before it covers the ground?
