---
type: Design
title: "Prop Checks, Contact-Sheet Review and the Style Guide"
description: "The budgets and structured checks a prop must pass, the rendered contact sheet a reviewer sees, and the style rules given to authors."
tags: [design, props, checks, review, style-guide]
generated: { by: agent/mcp, at: "2026-11-27T03:34:48Z" }
status: draft
stale_after: 2026-12-26
---

# Prop Checks, Contact-Sheet Review and the Style Guide

Part of `architecture/prop-pipeline-design`; source `docs/design/props.md`.

## Checks
`vox-art::check` returns structured reasons (kind, where, what to change) so a person or a model can fix and rerun. An **error** stops acceptance; a **warning** goes to the reviewer.

| Check | Limit | Kind |
| --- | --- | --- |
| Size | at most 64 voxels a side; the def's size matches | error |
| Voxels | 20,000 | error |
| Triangles after greedy meshing | 400 small, 1,200 medium, 3,000 large | error (half is a warning) |
| Palette | project colours only; at most 24 distinct; ramps respected | error |
| Floating parts | each part joined to the rest or to a socket that says why | error |
| Thin walls | nothing one voxel thick and more than 8 across (it shimmers) | warning |
| Pivot | on the ground plane for floor props; inside the grid always | error |
| Footprint | covered cells fit `place.footprint` and height | error |
| Sockets and parts | named, inside the grid, no duplicates; a `light` names a socket | error |
| Behaviours | known, parameters in range; a `door` has a part to move; `readable` length | error |
| Emitters | at most 4 light sockets | error |
| Silhouette | outline holds at 24, 48 and 96 px | warning |
| Proportion | against a 1.85 m character, a 2 m door, a 45 cm seat | warning |
| Names and tags | tags from the vocabulary; no real-person or trademarked names | error |

For scale: the 143 existing avatar props are 12 to 132 triangles each.

## Review
`vox-forge` renders a contact sheet in the game's own renderer: four angles, three distances, the prop beside a character and a door, in the yard by day and the dark sea at night (so emissive props show), and every state and variant. `apps/forge` shows it; a person accepts or rejects with reasons. Later a model pre-screens against the style guide, as the world-graph design plans for builds.

## Style guide (given to authors and reviewers)
- Voxels, not detail: a prop reads from 20 m away.
- Three-tone shading; one accent colour; the palette's ramps, not mixes.
- Proportions match the characters: seat 45 cm, table 75 cm, door 2 m, step 20 cm.
- One voxel of bevel on big things; small things sharp.
- Symmetry unless the object is not.
- No text in voxels (signs use `readable`); no logos.
- Emissive sparingly: a lamp's glass, an indicator, never a whole prop.
- Wear tells a story: the pier's lanterns salted, the station's panels clean.
- The hard constraints apply (`convention/hard-constraints`).

## Tools for a language model
Added to `vox-forge`'s MCP tools: `prop_new`, `prop_set_recipe`, `prop_set_def`, `prop_import`, `prop_check`, `prop_render`, `prop_accept`, `prop_reject`, and `library_search(kind = prop)` (examples to start from, and to avoid a second bench). All validate against the Rust types and cannot run code or touch files outside the project.

## Testing
Golden voxel hashes native and wasm; import of all 143 existing `.vox` files (or named failures) and a recipe-export-import round trip; known-bad props failing with expected reasons; crafted `VOXP` blobs refused; server placement, state and restore tests; visual contact sheets against approved ones.
