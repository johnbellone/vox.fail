---
type: Decision
title: "Decision: Zones Are 256 m, Regions Are 32 x 32 Zones"
description: "What used to be a region (16 x 16 chunk columns) is now a zone; a region is 32 x 32 zones (8,192 m, one atlas tile), stored as a hashed list of its zones' hashes."
tags: [decision, vox-format, world-format, snapshots, zones, regions]
generated: { by: agent/mcp, at: "2026-11-27T12:47:43Z" }
status: stable
---

# Decision: Zones Are 256 m, Regions Are 32 x 32 Zones

Made 2026-11-27 at the owner's request ("zones are current size of regions and regions are made up of many zones"), with the owner choosing each option below.

## What changed
- **Zone** (`vox_format::zone`: `Zone`, `ZoneKey`, `zone_of`, `ZONE_CHUNKS = 16`): 16 x 16 chunk columns, full height, 256 m (512 blocks) across. The unit of storage and hashing: one zstd-compressed, content-addressed blob. Its bytes are exactly what a region blob was (magic still `VOXR`), so every blob already stored keeps its hash.
- **Region** (`vox_format::region`: `Region`, `RegionKey`, `region_of`, `region_of_zone`, `REGION_ZONES = 32`): **32 x 32 zones, 8,192 m across, exactly one atlas tile**. Stored as a small content-addressed blob (magic `VOXG`, version, key, count, then per zone its x and z within the region and its 16-byte hash, sorted by (z, x); at most 1,024 zones; length, order, uniqueness and key overflow checked on decode).
- **Snapshots** (manifest format 2): a manifest maps region keys to region blob hashes; the root hashes those. The tree is snapshot root, regions, zones, blocks. A snapshot writes the changed zones and the regions they are in; unchanged regions are shared whole, unchanged zones inside a changed region are shared.

## Options chosen (2026-11-27)
1. Region size **32 x 32 zones** (over 16 x 16 or 8 x 8): matches the atlas tile, so a tile is one region; a 60 km world is about 8 x 8 regions and 55,000 zones.
2. A region is a **manifest level** (over a key only): it is the hash tree the scaling design wanted, and keeps manifests small for large worlds.
3. **Read old, write new** (over a clean break): format 1 manifests (no `format` field; zone hashes listed directly under `regions`) still list, restore and prune; the first snapshot after one writes region blobs.

## Consequences
- Every use of "region" in the designs that meant the 256 m unit now says zone (streaming units, `Have`, caches, entities per zone, simulation owners).
- Camera "zones" in level JSON (`cameras[].zone`) are unrelated: a box that picks a fixed shot.
- Pruning reads every kept snapshot's region blobs; an unreadable one stops pruning rather than delete what it might name.

Related: `architecture/world-format-design`, `architecture/world-scaling-design`, `decisions/oxen-snapshot-store`.

# Related Concepts
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): The format the zones and regions are stored in.
