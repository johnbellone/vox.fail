---
type: Design
title: "Prop Definition: Fields, Scales and Behaviours"
description: "What a PropDef holds (scale, pivot, placement, collision, parts, sockets, states, variants) and the closed set of behaviours a prop may have."
tags: [design, props, propdef, behaviours, vox-art]
generated: { by: agent/mcp, at: "2026-11-27T03:35:47Z" }
status: draft
stale_after: 2026-12-26
---

# Prop Definition: Fields, Scales and Behaviours

Part of `architecture/prop-pipeline-design`; source `docs/design/props.md`.

## Three kinds of thing in a world
| | Block | Structure | Prop |
| --- | --- | --- | --- |
| Made of | one 0.5 m cell | blocks, up to 64 x 64 x 64 | its own voxel grid (5 or 10 cm), up to 64 voxels a side |
| Look | atlas tile with height and glow | the blocks' | one flat palette (the avatar props' look) |
| Placed | 0.5 m grid | 0.5 m grid, quarter turns | 5 cm grid, 15 degree turns |
| Behaviour | none (light by flag) | none | light, seat, door, switch, readable, sound, motion |
| Edited | one block at a time | by saving a new version | placed, moved, removed, never reshaped |
| Owned | the world's plots | owner plus grants | catalogue props open; player-made ones owned |

A house is a structure; its lamp, bed and door are props.

## A PropDef (immutable; its version is the hash of its canonical form and voxels)
- **id, name, tags** (tags from a vocabulary, for search).
- **scale**: `fine` (5 cm voxels: held items, small decoration) or `prop` (10 cm: furniture, boats, trees), chosen by size (under a metre: fine). At most 64 voxels a side; bigger is a structure.
- **size, pivot**: bounding box in voxels; the voxel column that sits on the placement point (floor under it; back for a wall prop; top for a ceiling prop).
- **place**: what it may stand on (`floor`, `wall`, `ceiling`, `table`, `water`), the footprint in 0.5 m cells and a height.
- **collide**: `none`, `footprint` (its cells are blocked) or `boxes` (a few boxes).
- **parts**: named voxel groups that states show, hide, move or spin (a door's leaf, a lamp's glass).
- **sockets**: named points with a direction: light source, sit point, attach point for another prop, held-item grip.
- **states**: small named integers (off/lit, closed/open); the world keeps each instance's state.
- **variants**: palette swaps (`paint`) and a seed for the few props that scatter; one def gives a family.
- **palette_version**: the palette only grows, so old props stay valid.
- **source**: a recipe or a `.vox` file.

## Behaviours: a closed vocabulary (no code)
| Behaviour | Does | Run by |
| --- | --- | --- |
| `light` | light at a socket: colour, power, when (always, night, state); fire types flicker | clients (nearest few) |
| `seat` | a socket where a character sits, with the pose clip's name | server decides who sits where |
| `door` | a state with a swing or slide of named parts; closed blocks its footprint | server (state, collision), clients (motion) |
| `switch` | toggles its own state or every prop on its channel (a lever opens a gate) | server |
| `readable` | up to 140 characters shown on use | server stores, filters, sends |
| `sound` | one named sound from the real-recording set, looped or on use | clients |
| `motion` | spin, swing, bob, sway (with the wind) or flicker: looks only, deterministic from the clock | clients |
| `marker` | an objective or anchor for dungeons and quests | server |

Later, with the server-side inventory: `container` (slots) and `pickup`.

Why closed: a prop is as safe to accept from anyone as a block; see `decisions/props-closed-behaviours`.

# Related Concepts
- [Decision: Prop Behaviours Are a Closed Vocabulary, Never Code](../decisions/props-closed-behaviours.md): Why behaviours are a closed set.
- [Decision: Props Are a Third Kind of Thing, Beside Blocks and Structures](../decisions/props-third-kind-of-thing.md): Why props are neither blocks nor structures.
