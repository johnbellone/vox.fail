---
type: Decision
title: "Decision: 120 Hz Fixed Step with Interpolated Drawing"
description: "Particles, crowd and effects step at a fixed 120 Hz and are drawn as a blend of the last two ticks, so motion is frame-rate independent."
tags: [decision, simulation, bevy, timing]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: Fixed 120 Hz Simulation

**Date**: 2026-11-24 (recorded in `docs/avatar/PROJECT.md`).

**Context**: motion used per-frame virtual time, which is clamped and stalls on slow machines; effects played in slow motion at low frame rates and differed between 60 and 144 Hz.

**Decision**: things that move step by step (`particles`, `crowd`, `fx`, chat fade) run in `FixedUpdate` at 120 Hz and write `Interp::curr`; `sim::blend` draws a blend of the last two ticks. A frame catches up at most 100 ms of ticks. Springs stay in PostUpdate with 1/120 s substeps (they follow the animated pose). Closed-form motion (arcs, hat bounce, camera easing, blinks) stays per frame. Cues that arrive per frame queue until a tick takes them.

**Consequences**: identical motion tick for tick at 30, 60 and 144 fps (tested); snapshot mode can run on exact ticks, which exposed a startup red-flash bug. Not yet checked by eye on a real display at 30/60/144 fps.

See `architecture/avatar-app-runtime`.
