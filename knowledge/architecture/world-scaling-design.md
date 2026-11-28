---
type: Design
title: "Design (proposed): Worlds at 60 km x 60 km"
description: "The unbuilt plan to scale worlds to about 55,000 times today's size with zones and regions, generated terrain, streaming, simulation owners and caches."
tags: [design, proposed, scaling, worlds, branch]
generated: { by: agent/mcp, at: "2026-11-28T03:21:28Z" }
status: draft
stale_after: 2027-01-25
---

# Design (proposed): Worlds at 60 km x 60 km

Source: `docs/design/world-scaling.md`, dated 2026-11-25. **Status: proposed; nothing built.** Phase 1 ("measure") and the memory work are partly done on main: packed vertices and a meshing budget. Zones and regions exist in the storage format.

## Since 2026-11-27: quarter-metre blocks
Blocks are now 0.25 m ([quarter-metre-blocks-decision](quarter-metre-blocks-decision.md)). The block counts below assume 0.5 m.
- A zone (16 × 16 chunk columns) is now **128 m** across, and a region (32 × 32 zones) **4 km**. Their sizes in blocks are unchanged.
- A cubic metre holds 8x the blocks it did.
- The streaming, memory and far-terrain parts are refined for this in [chunk-streaming-design](chunk-streaming-design.md):
  - paletted chunks
  - `View` and `FarTile` messages
  - per-device draw distance (browser 64 m, desktop 128 m, mobile 32 m)
  - a sparse voxel octree for the far field, and a ray-marching prototype

  Its budgets replace the ones below.

## Problem
Before this design:
- Bounds were 512 x 128 x 512 blocks, about 1,000 chunks.
- Every chunk was **sent on join**, and everything was simulated.
- Storage was one SQLite row per chunk, and vertices were 64 bytes.
- All dirty chunks were meshed in one frame.
- Positions were `f32`, and wasm was single-threaded.

At 0.5 m blocks, 60 km is 120,000 blocks a side, about 14 million chunk columns.

## Design
- **Zones** of 16 x 16 chunk columns, full height, are the unit of storage, hashing, streaming and simulation ownership. They were 256 m at 0.5 m blocks, and are 128 m now. **Regions** of 32 x 32 zones group them in the hash tree (`decisions/zones-and-regions`).
- **Terrain is generated** from a seed on whichever machine needs it, identically on native and wasm (integer or fixed-point maths, golden tests, in `vox-core`). The server stores only edited chunks, and existing levels become overlays.
- **Streaming by area of interest**, refined in chunk-streaming-design:
  - `ToServer::View {center, radius, budget_kbps}`, `Have {zone, hash}`, `ToClient::Zone`
  - nearest first, within a bandwidth budget
  - low-detail terrain beyond the radius
  - a protocol version bump
- **Simulation owners**:
  - Cosmetic simulation (fine weather, particles) runs on one capable nearby client per zone, and other clients interpolate it.
  - Gameplay (blocks, plots, currency, harm) stays server-authoritative.
  - **Rotating authority**: epochs of 30 to 60 s, with server-chosen owners (nobody gets a zone where they gain) and one snapshot chain.
  - Snapshots are checked by redundancy, spot audits and rule checks, which needs bit-identical (fixed-point) simulation.
- **Weather in two layers**: a coarse server grid (1 km cells) plus a fine local layer in active zones.
- **Caching**: a client zone cache (disk, or IndexedDB/OPFS), validated by the hash tree (snapshot root, region hashes, zone hashes), with no Bloom filters. A **floating origin** past 1 km.
- **Original budgets** (now replaced by chunk-streaming-design):
  - browser: 160 m full-detail radius, under 1 GB, 2 Mbit/s
  - desktop: 384 m, under 3 GB, 8 Mbit/s

## Rollout
1. Measure.
2. Memory and meshes.
3. Zones, generator and streaming (protocol bump).
4. Far terrain.
5. Simulation owners and two-layer weather.
6. Client cache and hashes.
7. Floating origin.

## Open questions
- Is 128 blocks tall enough? It is 32 m now; the Void's level gives -64..384.
- How much is hand-built, and how much generated?
- Who sets a world's view-radius limit?

Related: `architecture/world-format-design`, `architecture/vox-server`, `architecture/weather-simulation`, `status/restructure-and-follow-on-work`, `decisions/zones-and-regions`.

# Related Concepts
- [Design (phase 1 built): The World Format](world-format-design.md): storage format that zones and regions need
- [Decision: Zones Are 256 m, Regions Are 32 x 32 Zones](../decisions/zones-and-regions.md): The unit names and sizes this design uses (128 m at quarter-metre blocks).
- [Chunks, streaming and draw distance](chunk-streaming-design.md): the newer plan for streaming, memory and far terrain
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): why the numbers changed
