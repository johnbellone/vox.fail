---
type: Decision
title: "Decision: Zones Are 256 m, Regions Are 32 x 32 Zones"
description: "What used to be a region (16 x 16 chunk columns) is now a zone (128 m at 0.25 m blocks); a region is 32 x 32 zones (4 km), stored as a hashed list of its zones' hashes."
tags: [decision, vox-format, world-format, snapshots, zones, regions]
generated: { by: agent/mcp, at: "2026-11-28T03:21:43Z" }
status: stable
---

# Decision: Zones Are 256 m, Regions Are 32 x 32 Zones

Made 2026-11-27 at the owner's request ("zones are current size of regions and regions are made up of many zones"). The owner chose each option below.

## Since quarter-metre blocks (2026-11-27)
The units are counted in chunks, so their sizes in blocks are unchanged. In metres they halved: a zone is **128 m** (512 blocks) and a region **4,096 m**. An atlas tile is 1,024 blocks (256 m), so it is 2 × 2 zones, not a region. The format, magic bytes and hashes are unchanged. See [quarter-metre-blocks-decision](../architecture/quarter-metre-blocks-decision.md).

## What changed
- **Zone** (`vox_format::zone`: `Zone`, `ZoneKey`, `zone_of`, `ZONE_CHUNKS = 16`):
  - 16 x 16 chunk columns, full height, 512 blocks across (256 m at the time).
  - The unit of storage and hashing: one zstd-compressed, content-addressed blob.
  - Its bytes are exactly what a region blob was (magic still `VOXR`), so every blob already stored keeps its hash.
- **Region** (`vox_format::region`: `Region`, `RegionKey`, `region_of`, `region_of_zone`, `REGION_ZONES = 32`):
  - 32 x 32 zones (8,192 m at the time, 4,096 m now).
  - Stored as a small content-addressed blob: magic `VOXG`, version, key, count, then for each zone its x and z within the region and its 16-byte hash, sorted by (z, x).
  - At most 1,024 zones. Length, order, uniqueness and key overflow are checked on decode.
- **Snapshots** (manifest format 2):
  - A manifest maps region keys to region blob hashes, and the root hashes those. The tree is snapshot root, regions, zones, blocks.
  - A snapshot writes the changed zones and the regions they are in.
  - Unchanged regions are shared whole, and unchanged zones inside a changed region are shared.

## Options chosen (2026-11-27)
1. Region size **32 x 32 zones** (over 16 x 16 or 8 x 8). A 60 km world was about 8 x 8 regions and 55,000 zones at 0.5 m blocks.
2. A region is a **manifest level** (over a key only). It is the hash tree the scaling design wanted, and keeps manifests small for large worlds.
3. **Read old, write new** (over a clean break). Format 1 manifests (no `format` field; zone hashes listed directly under `regions`) can still be listed, restored and pruned. The first snapshot after one writes region blobs.

## Consequences
- Every use of "region" in the designs that meant the 16 x 16-column unit now says zone: streaming units, `Have`, caches, entities per zone, simulation owners.
- Camera "zones" in level JSON (`cameras[].zone`) are unrelated: a box that picks a fixed shot.
- Pruning reads every kept snapshot's region blobs. If one can't be read, pruning stops rather than delete what that blob might name.

Related: `architecture/world-format-design`, `architecture/world-scaling-design`, `decisions/oxen-snapshot-store`.

# Related Concepts
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): The format the zones and regions are stored in.
- [Quarter-metre blocks](../architecture/quarter-metre-blocks-decision.md): why the sizes in metres halved
