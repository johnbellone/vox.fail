---
type: Status
title: Roadmap and Next Steps
description: "The planned milestones from the project document, updated with what the restructure and follow-on commits already delivered."
tags: [status, roadmap, planning]
generated: { by: agent/mcp, at: "2026-11-27T03:35:47Z" }
status: stable
stale_after: 2026-12-26
---

# Roadmap

## v0.1.0: first Windows release candidate (gate: a real Windows run)
1. Tag `v0.1.0` so `windows.yml` builds the installer (it has never run).
2. Fix risks 2, 3, 4: hotkey defaults (Ctrl+Shift or F13-F24), delete the firewall section, trust only `CF-Connecting-IP` by default.
3. Test on a clean Windows 11 VM: install, first-run setup, mic, OBS window capture, virtual camera in Zoom/Discord, hotkeys during a game, a short with sound, uninstall.
4. Live Twitch test with a second account: throws, dress-up, raid, sub, bits, a viewer question through cloudflared, ban carry-over.
5. Confirm vox.fail is actually up (DNS did not resolve from the analysis machine).

## v0.2 hardening
Code signing; Whisper build in CI plus a model download step in setup; a single audio mixer; tests for command parsing and poses; control page moved to a file with a Playwright smoke test; a CI job that runs `cargo test --workspace` and the site's tests.

## Candidate features (none started)
Pre-stream checklist page; main-window live preview; running-app window icon; demo short for a store page; official YouTube/Kick integrations; hosted relay; macOS.

## Vox online
**Done**: core, server (with move checks, snapshots, restore), clients, browser client, directory, portals, weather, economy, controls and HUD phases 1-4, React site.
**Next**: world format phases 2-7 (entities, levels as files, regions on the wire, client cache, structures, owners' restore UI); health and survival worlds; voice, name tags and text chat in worlds; token audience per world; the first-position trust gap.
**Designed, not built**: worlds at 60 x 60 km (`architecture/world-scaling-design`); a world graph with recipes written by a language model (`architecture/world-graph-and-art-design`).

## Backlog, not started: January 6 Capitol world
West Front, platform, scaffolding, barricades, lawn, Reflecting Pool edge, late-afternoon winter light; new models and animations; crowd and police (defensive poses only); no real likenesses. **Open**: what players do there; whether Twitch's and YouTube's rules allow streaming it. Settle both before any modelling. See `convention/hard-constraints`.

## Process
Install beads and run `bd init`; open GitHub issues for user-facing items; keep this bundle current per `convention/work-tracking`.

# Related Concepts
- [Design (proposed): Worlds at 60 km x 60 km](../architecture/world-scaling-design.md): planned scaling work
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): next world-format phases
- [Hard Constraints Set by the Owner](../convention/hard-constraints.md): rules for the backlog Capitol world
- [Props: Phases and Open Questions](props-phases-and-open-questions.md): The proposed prop pipeline's phases and open questions.
