---
type: Risk
title: Risks and Open Issues (re-checked 2026-11-26)
description: "The ranked risk list from docs/avatar/PROJECT.md with a fresh verdict per item against main at 7c4e517: still true, fixed by the restructure work, or not re-checked."
tags: [status, risks, security, technical-debt]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
stale_after: 2026-12-26
---

# Risks and Open Issues

Numbers follow `docs/avatar/PROJECT.md` (some are missing there). **Verdict** key: *Confirmed* = I re-read the code on `main` at `7c4e517`; *Fixed* = changed by the 29 commits (`status/restructure-and-follow-on-work`); *Doc* = from the project document, not re-checked.

## High
| # | Area | Issue | Verdict |
| --- | --- | --- | --- |
| 1 | Platform | No Windows run at all; release workflow never ran | **Confirmed** (no tags, no runs) |
| 2 | Hotkeys | Defaults use Ctrl+Alt (AltGr on many European layouts), stealing typed characters | **Confirmed** (`bins/block-avatar/src/controls/hotkeys.rs`) |

## Medium
| # | Area | Issue | Verdict |
| --- | --- | --- | --- |
| 3 | Security | Installer firewall rule for TCP 7778 is pointless: the viewer server binds 127.0.0.1 | **Confirmed** (`bins/block-avatar/installer/block-avatar.nsi` lines 80-94) |
| 4 | Security | The ask page's `client_ip()` trusts the *first* `X-Forwarded-For` entry when `CF-Connecting-IP` is absent, so viewers can spoof their address behind non-Cloudflare tunnels | **Confirmed** (`audience/ask/http.rs` ~45); relay and directory use the last entry |
| 5 | Distribution | Unsigned exe/installer: SmartScreen warns | Doc |
| 6 | Feature | Installed builds have no Whisper | Doc |
| 7 | Maintainability | `main.rs` huge, no tests, resource tuples | **Fixed** mostly: `main()` about 50 lines, plugins by pipeline, big files split into folders. Resource tuples remain (Bevy's 16-parameter limit). |
| 8 | Audio | Each sound opens its own cpal stream/thread | Doc |
| 9 | Performance | Short export decodes and re-encodes ~400 JPEG frames on a background thread | Doc |
| 16 | Network | Relay had no accounts or rate limits | **Fixed** in part: per-address show and wrong-code limits, text frames dropped; still no accounts |
| 17 | Network | ADPCM is about 2x Opus size | Doc |
| 21 | World | No ownership or undo; anyone may change any block (20 edits/s) | Partly: plots exist; owners can restore snapshots from the server CLI (UI not built) |
| 28 | Web | Browser client ~50 MB wasm (11 MB gz), WebGPU only | Doc; GPU memory reduced, wasm size not |
| 30 | Economy | A world server holding the shared secret can mint authority; reports once a minute | Doc |
| new | Auth | A sign-in token is valid on every world (no audience in the token) | Recorded by the authors in AGENTS.md |
| new | Position trust | First position after joining is trusted (portal arrival point set by the previous world) | Recorded in AGENTS.md |
| new | CI | `windows.yml` runs `cargo test --release -p block_avatar` only; `visual.yml` builds `-p block_avatar` and runs visual tests; nothing runs `cargo test --workspace` | **Confirmed** (workflow files) |

## Low
10 Timing: Bevy virtual time clamps on slow frames (Doc). 11 Control page JS in a Rust string (Doc). 12 Preview shows shorts camera only (Doc). 13 Mic list names on ALSA (Doc). **14 Not a git repo: resolved.** 15 macOS never built. 18 Relay sends host messages back to guests. 19 Host under 10 fps runs clock slow. 20 Guest's hats not shown on host. **22 Movement client-authoritative: Fixed** (`vox-core::moves`, `Correct`). 23 No voice, name tags or chat in worlds. 24 Reflections sky-only. 25 Only 12 nearest lights. 26 Portal echoes show the level as built, drawn twice. 27 Portals need the other server running. 29 Other players' wardrobe not shown; names cannot be changed. Authors' list: one-block edit rebuilds a whole chunk; weather and fog baking on the main thread; duplicated helpers; no Linux webcam output.

## Open product questions
Does the lead's slogan cap suit a public store page? Viewer questions beyond Twitch? A hosted relay? For the backlog Capitol world, Twitch/YouTube rules on real violent events (`convention/hard-constraints`).

## Hygiene risks
See `status/repository-hygiene`.

# Related Concepts
- [Verification Gaps: What Has Never Been Run](verification-gaps.md): untested areas
