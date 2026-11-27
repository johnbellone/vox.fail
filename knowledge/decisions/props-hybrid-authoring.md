---
type: Decision
title: "Decision: Props Come from Recipes and Imported MagicaVoxel Files"
description: "Prop art is authored two ways into one library: Rust recipes for most props, and hand-made MagicaVoxel files as first-class imports, both through the same checks."
tags: [decision, props, authoring, recipes, magicavoxel]
generated: { by: agent/mcp, at: "2026-11-27T03:35:24Z" }
status: stable
---

# Decision: Props Come from Recipes and Imported MagicaVoxel Files

**Decision (the owner chose "hybrid", 2026-11-26)**: prop art has two lanes into one library. Most props are **recipes** (data interpreted by Rust in `vox-art`, native and wasm). Anything a recipe cannot express comes in as a hand-made **MagicaVoxel `.vox`** file, imported as a first-class source. Both end as a `VoxelGrid` plus a `PropDef` and pass the same checks, review and budgets. The alternatives were recipes only (most reproducible, most limiting) and hand-made first (recipes later).

**Why**: the repository's art already comes from both: 143 existing props have `.vox` sources and Python builders, so a loader gets the library started on day one, while the world-graph design makes recipes the way a language model and the browser can make art without running code. A touch-up loop joins them: export a recipe result to `.vox`, fix it by hand, re-import it with `derived_from`.

**Consequences**: the importer is part of the product, not a migration tool (palette snapping, reserved-colour tags for pivot, parts and sockets, a hardened reader). The palette is the style, so an import that cannot be snapped within a distance fails with the colours named. The Python builders retire prop by prop as recipes or imports reproduce them.

See `architecture/prop-authoring-lanes`, `architecture/prop-pipeline-design` and `architecture/world-graph-and-art-design`.
