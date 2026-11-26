---
type: Status
title: "Verification Gaps: What Has Never Been Run"
description: "Everything that is written and unit-tested but never exercised on its real target, with the way to verify each one."
tags: [status, verification, windows, risk]
generated: { by: agent/mcp, at: "2026-11-26T15:57:12Z" }
status: stable
stale_after: 2026-12-26
---

# Verification Gaps

The largest single risk: the target platform for streamers, **Windows, has never run any of this code**.

| Item | Why not yet | How to verify |
| --- | --- | --- |
| Windows install, exe icon, no-console subsystem, `%APPDATA%` folder, hotkeys | No Windows host or toolchain used | Tag `v0.1.0`, let `windows.yml` build, install on a clean Windows 11 VM |
| OBS virtual camera end to end | Needs Windows and the OBS driver | Pick "OBS Virtual Camera" in Zoom/Discord; measure with `ffmpeg -f dshow ...` (see `docs/avatar/manual.md`) |
| Live Twitch (IRC events, device-code login), YouTube, Kick | Sandbox network allowlist | A test channel and a second account |
| Whisper transcription | Not in the default build | `--features whisper` with `ggml-base.en.bin` |
| Real GPU and frame rate under load | Software renderer only (1-5 fps) | 60 fps target with webcam and shorts camera on |
| Sound output | No audio device in the test environment | Listen to effects; check they reach a short |
| Network voices heard | Packets counted, not heard | Two machines, headphones |
| Relay on the internet behind TLS | Sandbox | Deploy behind Cloudflare, use `--watch https://...` |
| Mouse look and controllers in worlds | Xvfb has no pointer lock or gamepad | Desktop with mouse and pad |
| Fixed-step motion at 30/60/144 fps by eye | Only tests, software render | Real display |
| Face capture from iPhone/VMC apps | No real trackers | iFacialMocap, VSeeFace |
| Browser client on a real GPU, other browsers, mobile | Only headless Chromium on SwiftShader | Manual test matrix |
| vox.fail itself | DNS did not resolve from this machine | Check Cloudflare and DNS; visit the site |
| macOS | Never built | Out of scope until decided |

## Suggested first release test matrix (from docs/PROJECT.md)
Windows 10 and 11; NVIDIA, AMD and Intel GPUs; OBS installed or not.

See `status/risks-and-open-issues`, `status/roadmap`.
