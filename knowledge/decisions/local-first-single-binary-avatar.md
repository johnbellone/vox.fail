---
type: Decision
title: "Decision: Block Avatar Is Local-First, Voice-First, One Binary"
description: "The design principles of the streaming app: microphone drives everything, no account or cloud, every feature is a text command, failures are visible but not fatal."
tags: [decision, principles, block-avatar]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: Local-First, Voice-First, One Binary

Design principles recorded in `docs/avatar/PROJECT.md`:

1. **Voice first.** The microphone drives everything by default; keys, the control page, chat and hotkeys only add to it. No webcam, rig or VTuber toolchain is needed.
2. **One binary, local first.** No account, no cloud service, no server to run. The two web pages (control and ask) are served by the app itself.
3. **Zero-setup Twitch.** Anonymous IRC; only viewer questions use sign-in.
4. **One command path.** Every feature is a text command, so keyboard, control page, hotkeys, gags, presets and outside bots share one implementation (`architecture/control-surfaces`).
5. **Fail visibly, keep running.** Problems go to `avatar.log` and a red box on the control page; a missing optional piece (ffmpeg, Whisper, webcam driver) disables one feature, not the app.

**Consequence**: the online Vox stack is the *exception*: it adds services (directory, worlds, relay) that the avatar app only reaches optionally (`--world`, `--watch`, `--join`).
