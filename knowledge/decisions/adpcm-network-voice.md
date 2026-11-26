---
type: Decision
title: "Decision: IMA ADPCM for Network Voice (Opus Later)"
description: Show voice uses 16 kHz IMA ADPCM because it needs no C library; Opus can replace it behind a codec field.
tags: [decision, audio, networking, codec]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: ADPCM for Network Voice

**Decision**: mono 16 kHz IMA ADPCM, 20 ms packets, each carrying the coder state so a lost packet costs only 20 ms; nothing sent while quiet.

**Why**: pure Rust, trivial to build on Windows and wasm. Opus would need a C library and CMake on Windows.

**Consequences**: about twice Opus's bit rate (~8 KB/s per speaker), but acceptable for a few speakers. A codec field in the protocol allows swapping to Opus when the build can take it (risk 17). Voices are never changed or synthesised (`convention/hard-constraints`).

See `architecture/show-networking`.
