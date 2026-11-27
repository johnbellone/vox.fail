---
type: Design
title: "Design (phase 1 built): The World Format"
description: "Zones stored as content-addressed zstd blobs, grouped into regions (32 x 32 zones) whose hashed blobs a snapshot lists, in a folder or an Oxen repository (squashed daily), with structure entitlements; phase 1 and the Oxen store are built."
tags: [design, vox-format, snapshots, storage, branch]
generated: { by: agent/mcp, at: "2026-11-27T12:47:43Z" }
status: draft
stale_after: 2026-12-26
---

# Design: The World Format

Source: `docs/design/world-format.md`, `crates/vox-format` and `bins/vox-directory/src/structures.rs` on `main`. **Phase 1 built**, plus the Oxen snapshot store, structure entitlements and zones within regions; later phases not.

## Shape
A world is stored and sent as **zones** (16 x 16 chunk columns, 256 m across), grouped into **regions** (32 x 32 zones, 8,192 m, one atlas tile): see `decisions/zones-and-regions`. A zone is one blob: magic `VOXR` (kept from when zones were called regions, so stored hashes stay valid), format version, zone key, palette version, a **zstd frame of the edited chunks** (today's run-length encoding per chunk, compressed together), later an optional **entities** section (our own `ZoneEntities` blob modelled on Bevy's `DynamicWorld` but not using it, with a version per component and `vox-format` depending on `bevy_reflect` alone: RON for authoring, postcard for storage and wire; see `technologies/bevy-ecosystem`), and a 16-byte **BLAKE3** hash over the uncompressed sections. A region is a small blob (magic `VOXG`) listing its zones' hashes. Blobs are **content-addressed**; a **snapshot** is a manifest (format 2) of region keys to region hashes, with a root over them: the tree is root, regions, zones. Unchanged regions are shared whole between snapshots, unchanged zones inside a changed region too. Format 1 manifests (zone hashes listed directly) still load, restore and prune.

## Measured (islands world, 257 chunks, ~405k blocks)
Plain 16-bit: 16,842,752 bytes. Run-length (today): 211,314. RLE + zstd per chunk: 79,500. RLE + zstd with a trained dictionary: 70,091 (optimistic). **RLE + zstd per zone: 62,831** (chosen).

## Decisions (2026-11-26)
1. Player-built structures are **entities** stored as a reference to a specific **version** of a structure.
2. **Object storage** holds blobs; the live link stays the world server's WebSocket. **Oxen is the snapshot store** when asked for, squashed daily: see `decisions/oxen-snapshot-store`.
3. **World owners restore their own worlds**; snapshots are kept **a week**.
4. Structures have an owner and use, copy and edit rights: see `decisions/structure-entitlements`.
5. (2026-11-27) Zones within regions: see `decisions/zones-and-regions`.

## Built
- Phase 1: zone blobs for blocks only, region blobs over them; `Snapshots` over a `BlobStore` (`DirStore` folder, or `OxenStore` with a commit per snapshot, `--oxen`); the server snapshots the changed zones every 5 minutes and on close and restores (`--list-snapshots`, `--restore`, `--restore-at`, undoable). Decoding works in wasm (`ruzstd`); encoding is native (`zstd`).
- Oxen: runs the `oxen` command line; history older than the week is squashed by rebuilding the repository, about daily.
- Entitlements: `vox_core::entitle` (pure rules) and the directory's structures API; a world asks the directory before placing a structure. Placing structures in worlds is not built.

## Next phases
2 entities (portals, spawn, lights, plots out of JSON/meta into zone entities, with stable type paths and migrations); 3 levels as files; 4 zone blobs on the wire; 5 client cache; 6 structures (placing, saving, plot-derived, using the entitlements); 7 owners' restore UI.

## Safety rules for untrusted blobs
At most 4,096 chunks per zone and 1,024 zones per region; each chunk decoded with bounds checks; decompressed size capped before allocation; zone and region keys checked for overflow; region entries in range, sorted and unique; client-sent entities deserialised against a registry holding only world components, with caps.

## Open questions
Structure size limits; who else may restore, and cooldowns; object storage (R2) as a store.

Related: `architecture/world-scaling-design`, `architecture/vox-server`, `technologies/sqlite-persistence`, `decisions/oxen-snapshot-store`, `decisions/structure-entitlements`, `decisions/zones-and-regions`.

# Related Concepts
- [Decision: Oxen Is the Snapshot Store, Squashed Daily](../decisions/oxen-snapshot-store.md): Oxen is the snapshot store when asked for, with history squashed daily.
- [Decision: Structures Have an Owner and Use, Copy and Edit Rights](../decisions/structure-entitlements.md): Structures have an owner and use, copy and edit rights.
- [Bevy Ecosystem Crates: Use, Copy, Watch or Skip](../technologies/bevy-ecosystem.md): Why entities use our own blob instead of DynamicWorld, and what belongs in a pack.
- [Decision: Zones Are 256 m, Regions Are 32 x 32 Zones](../decisions/zones-and-regions.md): Zones are the 256 m storage unit; regions are 32 x 32 zones listed by a snapshot.
