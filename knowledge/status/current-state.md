---
type: Status
title: Current State of main (as of 2026-11-26)
description: "A dated snapshot of main at 7c4e517: maturity, metrics, what works, what is unverified and what is not started."
tags: [status, snapshot, main]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
stale_after: 2026-12-26
---

# Current State of `main` (2026-11-26)

**Scope**: `main` = `origin/main` at `7c4e517` after the restructure and follow-on work (see `status/restructure-and-follow-on-work`). 35 commits; the first six are dated 2026-11-25, the other 29 2026-11-25 to 26.

## Maturity
Pre-release. No git tags, no GitHub releases, no open GitHub issues (public repo `johnbellone/vox.fail`). `v0.1.0` not cut.

## Metrics
| Measure | Value |
| --- | --- |
| Rust | about 45k lines (block-avatar 23.9k, vox-world 8.7k, directory 3.1k, vox-core 2.5k, avatar-voice 2.2k, server 1.8k, relay 1.1k, format 0.7k, client 0.7k, link 0.2k) |
| Site (TypeScript) | about 2.6k lines, 1 test file |
| Python tools | about 4.3k lines |
| Tests | 256 pass, 0 fail, 1 ignored (ran 2026-11-26, 2 min 20 s warm) |
| Tracked files | 984 (193 `.glb`); assets 110 MB |
| Content | 38 characters; 143 prop files; 6 sets; 3 worlds |

## What works (by evidence)
| Area | State | Evidence |
| --- | --- | --- |
| Lip sync, voice analysis, laugh/sing | Done; tested on real recordings | 21 tests in avatar-voice |
| Avatar window, sets, lighting, effects | Done; headless-verified (software render) | screenshots, visual tests |
| Chat features, polls, dress-up, crowd, question queue | Done; unit-tested against fakes | 85 app tests |
| Shorts and comics | Done; one MP4 produced under Xvfb | headless run |
| Show networking | Done; run end to end under Xvfb; relay now rate-limited | 13 relay tests |
| World server, client, portals, weather, sky | Done; server checks moves; snapshots and restore | 16 server tests |
| World format phase 1 | Done: region blobs, snapshots every 5 min, restore | 7 format tests |
| Controls and HUD phases 1-4 | Built (first person, gamepad hints, menus, palette) | 48 world tests; not hardware-verified |
| Directory: accounts, Steam/Discord, characters, economy, friends | Done; hardened Steam check, Argon2 concurrency cap | 12 tests |
| Site | React/Mantine rewrite, builds with Bun | 1 test file |
| Browser client | Verified in headless Chromium (earlier) | `tools/test/web_e2e.py` |

## Unverified or missing
- **Windows**: nothing has ever run (installer, hotkeys, webcam, paths); the release workflow has never run (no tags).
- Real GPU, real audio, live Twitch/YouTube/Kick, iPhone tracking, Whisper, relay on the internet, controllers. See `status/verification-gaps`.
- **vox.fail production**: the domain did not resolve from the analysis machine on 2026-11-26. Unconfirmed.
- Not started: Capitol world (backlog), built-in webcam face tracking, hosted relay, macOS, level editor, voice/chat/name tags in worlds, survival mode, 60 km worlds, world graph and generated art.
- beads not installed.

# Related Concepts
- [Restructure and Follow-on Work (the former branch, now on main)](restructure-and-follow-on-work.md): the restructure and follow-on commits
- [Test Results on main (2026-11-26 run)](test-results.md): measured test results
- [Verification Gaps: What Has Never Been Run](verification-gaps.md): what has never been run
- [Risks and Open Issues (re-checked 2026-11-26)](risks-and-open-issues.md): ranked risks
- [Roadmap and Next Steps](roadmap.md): what comes next
- [Documentation Drift: Where the Docs Disagree with the Code](documentation-drift.md): which docs are stale
- [Repository and Disk Hygiene Findings](repository-hygiene.md): housekeeping findings
