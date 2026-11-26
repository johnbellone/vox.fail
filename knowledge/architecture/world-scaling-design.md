---
type: Design
title: "Design (proposed): Worlds at 60 km x 60 km"
description: "The unbuilt plan to scale worlds to about 55,000 times today's size with regions, generated terrain, streaming, simulation owners and caches."
tags: [design, proposed, scaling, worlds, branch]
generated: { by: agent/mcp, at: "2026-11-26T15:57:59Z" }
status: draft
stale_after: 2027-01-25
---

# Design (proposed): Worlds at 60 km x 60 km

Source: `docs/design/world-scaling.md`, dated 2026-11-25. **Status: proposed; nothing built** (phase 1 "measure" and memory work are partly done on main: packed vertices, meshing budget).

## Problem
Today: bounds 512 x 128 x 512 blocks, ~1,000 chunks, **all sent on join**, all simulated, one SQLite row per chunk, 64-byte vertices, meshing all dirty chunks in one frame, `f32` positions, single-threaded wasm. At 0.5 m blocks 60 km is 120,000 blocks a side, about 14 million chunk columns.

## Design
- **Regions** = 16 x 16 chunk columns (256 m x 256 m, full height): the unit of storage, hashing, streaming and simulation ownership (about 55,000 regions).
- **Terrain is generated** from a seed on whichever machine needs it, identically on native and wasm (integer or fixed-point maths, golden tests, in `vox-core`). The server stores only edited chunks; existing levels become overlays.
- **Streaming by area of interest**: `ToServer::View {center, radius, budget_kbps}`, `Have {region, hash}`, `ToClient::Region`; nearest-first within a bandwidth budget; far low-detail terrain beyond the radius. Protocol version bump.
- **Simulation owners**: cosmetic simulation (fine weather, particles) runs on one capable nearby client per region and is interpolated by others; gameplay (blocks, plots, currency, harm) stays server-authoritative. **Rotating authority**: 30-60 s epochs, server-chosen owners (nobody gets a region where they gain), one snapshot chain, snapshots checkable by redundancy, spot audits and rule checks; needs bit-identical (fixed-point) simulation.
- **Weather in two layers**: coarse server grid (1 km cells) plus fine local layer in active regions.
- **Caching**: client region cache (disk or IndexedDB/OPFS) validated by a quadtree of region hashes; no Bloom filters. **Floating origin** past 1 km.
- Budgets (to measure): browser 160 m full-detail radius, under 1 GB, 2 Mbit/s; desktop 384 m, under 3 GB, 8 Mbit/s.

## Rollout
1 measure; 2 memory and meshes; 3 regions, generator, streaming (protocol bump); 4 far terrain; 5 simulation owners and two-layer weather; 6 client cache and hashes; 7 floating origin.

## Open questions
Is 128 blocks tall enough? How much is hand-built vs generated? Who sets a world's view-radius limit?

Related: `architecture/world-format-design`, `architecture/vox-server`, `architecture/weather-simulation`, `status/restructure-and-follow-on-work`.

# Related Concepts
- [Design (phase 1 built): The World Format](world-format-design.md): storage format that regions need
