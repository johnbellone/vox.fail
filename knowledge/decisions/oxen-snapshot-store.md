---
type: Decision
title: "Decision: Oxen Is the Snapshot Store, Squashed Daily"
description: "World snapshots can be commits in an Oxen repository driven through the oxen command line, with history older than a week squashed by rebuilding the repository."
tags: [decision, snapshots, oxen, vox-format, deploy]
generated: { by: agent/mcp, at: "2026-11-26T22:11:34Z" }
status: stable
---

# Decision: Oxen Is the Snapshot Store, Squashed Daily

**Decision**: with `vox-server --oxen` (or `VOX_OXEN`), a world's snapshots live in an Oxen repository: `vox_format::OxenStore` implements `BlobStore` over an Oxen working tree and commits after each snapshot and each pruning. The `oxen` command line is run as a subprocess (the owner chose it over embedding `liboxen`, which would bring RocksDB and DuckDB into the build, and over a shared `oxen-server`). The Helm chart turns it on and the container image carries the binary (version and SHA-256 pinned in the Dockerfile). Without `--oxen` the plain folder store is used.

**Squashing**: Oxen has no history rewrite, so `OxenStore::squash` rebuilds the repository: the newest commit before the cutoff (now minus a week) is the base, the later commits are replayed on top in a fresh repository, and it is swapped in. It runs only when a day of older history exists, about once a day. The owner confirmed squashing old history is acceptable.

**Why**: an earlier measurement (256 regions, 100 rounds) found Oxen fast (about 29 ms a commit) but with no space saving and no retention; the owner decided server cost does not matter and history (diffs, branches to copy a world) is worth having, so retention is done by the rebuild.

**Consequences**: each blob is held twice (working tree and Oxen's versions). Restore, `--list-snapshots` and the `Snapshots` API are unchanged. A folder of plain snapshots is adopted as the first commit, so switching on is only the flag. Tests for the Oxen paths skip without an `oxen` on the PATH (set `OXEN`).

See `architecture/world-format-design`.
