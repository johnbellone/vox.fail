---
type: Design
title: "Design (phase 1 built): The World Format"
description: Regions stored as content-addressed zstd blobs with snapshots in a folder or an Oxen repository (squashed daily) and structure entitlements; phase 1 and the Oxen store are built.
tags: [design, vox-format, snapshots, storage, branch]
generated: { by: agent/mcp, at: "2026-11-26T22:12:53Z" }
status: draft
stale_after: 2026-12-26
---

# Design: The World Format

Source: `docs/design/world-format.md`, `crates/vox-format` and `bins/vox-directory/src/structures.rs` on `main`. **Phase 1 built**, plus the Oxen snapshot store and structure entitlements; later phases not.

## Shape
A world is stored and sent as **regions** (16 x 16 chunk columns). A region is one blob: magic `VOXR`, format version, region key, flags, palette version, a **zstd frame of the edited chunks** (today's run-length encoding per chunk, compressed together), an optional **entities** section (Bevy `DynamicWorld`: RON for authoring, postcard for storage and wire), and a 16-byte **BLAKE3** hash over the uncompressed sections. Blobs are **content-addressed**; a **snapshot** is a manifest of region keys to hashes whose root is the hash tree from the scaling design; unchanged regions are shared between snapshots.

## Measured (islands world, 257 chunks, ~405k blocks)
Plain 16-bit: 16,842,752 bytes. Run-length (today): 211,314. RLE + zstd per chunk: 79,500. RLE + zstd with a trained dictionary: 70,091 (optimistic). **RLE + zstd per region: 62,831** (chosen).

## Decisions (2026-11-26)
1. Player-built structures are **entities** stored as a reference to a specific **version** of a structure.
2. **Object storage** holds blobs; the live link stays the world server's WebSocket. **Oxen is the snapshot store** when asked for, squashed daily: see `decisions/oxen-snapshot-store`.
3. **World owners restore their own worlds**; snapshots are kept **a week**.
4. Structures have an owner and use, copy and edit rights: see `decisions/structure-entitlements`.

## Built
- Phase 1: region blobs for blocks only; `Snapshots` over a `BlobStore` (`DirStore` folder, or `OxenStore` with a commit per snapshot, `--oxen`); the server snapshots every 5 minutes and on close and restores (`--list-snapshots`, `--restore`, `--restore-at`, undoable). Decoding works in wasm (`ruzstd`); encoding is native (`zstd`).
- Oxen: runs the `oxen` command line; history older than the week is squashed by rebuilding the repository, about daily.
- Entitlements: `vox_core::entitle` (pure rules) and the directory's structures API; a world asks the directory before placing a structure. Placing structures in worlds is not built.

## Next phases
2 entities (portals, spawn, lights, plots out of JSON/meta into region entities, with stable type paths and migrations); 3 levels as files; 4 region blobs on the wire; 5 client cache; 6 structures (placing, saving, plot-derived, using the entitlements); 7 owners' restore UI.

## Safety rules for untrusted blobs
At most 4,096 chunks per region; each chunk decoded with bounds checks; decompressed size capped before allocation; client-sent entities deserialised against a registry holding only world components, with caps.

## Open questions
Structure size limits; who else may restore, and cooldowns; object storage (R2) as a store.

Related: `architecture/world-scaling-design`, `architecture/vox-server`, `technologies/sqlite-persistence`, `decisions/oxen-snapshot-store`, `decisions/structure-entitlements`.

# Related Concepts
- [Decision: Oxen Is the Snapshot Store, Squashed Daily](../decisions/oxen-snapshot-store.md): Oxen is the snapshot store when asked for, with history squashed daily.
- [Decision: Structures Have an Owner and Use, Copy and Edit Rights](../decisions/structure-entitlements.md): Structures have an owner and use, copy and edit rights.
