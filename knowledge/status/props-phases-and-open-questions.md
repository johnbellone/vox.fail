---
type: Status
title: "Props: Phases and Open Questions"
description: The six build phases for the prop pipeline and the questions still open; nothing is built yet.
tags: [status, props, roadmap, open-questions]
generated: { by: agent/mcp, at: "2026-11-27T03:35:07Z" }
status: draft
stale_after: 2026-12-26
---

# Props: Phases and Open Questions

Status as of 2026-11-26: design written (`architecture/prop-pipeline-design`), **nothing built**. Source `docs/design/props.md`.

## Phases
1. `vox-art`: grid, palette, `PropDef`, the `.vox` reader and importer, the checks and the contact-sheet render (command line). Import the existing props as the seed library.
2. Recipe operations for props and the golden tests; export to `.glb` so the Python builders can retire.
3. `VOXP` blobs, the `Prop` entity, placement and the server's checks; meshing, batching and prop occupancy in the clients.
4. Behaviours: `light`, `door`, `switch`, `seat`, `readable`, `sound`, `motion`, then `marker`.
5. The library, search and the MCP tools; a language model makes the first wave from a brief.
6. Player-made props with entitlements; containers and pickups when the inventory is on the server.

## Open questions (for the owner)
- **Scale**: is 5 cm plus 10 cm enough, or do lamp posts and trees want 25 cm as a third? More scales means more to check.
- **Walk-through**: should decorative props never block by default, so a crowded room cannot trap someone? (The check could warn on footprints that narrow a doorway.)
- **When blocks go**: remove props whose support is broken, or let them drop and keep? Removal is simplest.
- **Who places**: only plot builders and world owners, or anyone in worlds that allow building?
- **Moving parts**: state swaps and part movement suffice for doors and lids; skeletons only for creatures. Right line?
- **Player-made props**: reviewed before others can place them, or placeable by the owner at once and public after review?
- **Text**: `readable` is the one place a player's words reach other players through a prop; it needs the same filtering as names and chat.

## Dependencies
The recipe interpreter and `vox-art` crate come from `architecture/world-graph-and-art-design`; the entity and blob machinery from `architecture/world-format-design`; entitlements from `decisions/structure-entitlements`. Reusing the avatar props needs the product question in `project/characters-and-content` settled for the political-slogan ones.
