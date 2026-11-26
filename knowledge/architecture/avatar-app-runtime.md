---
type: Architecture
title: "Block Avatar Runtime: Bevy Schedule, Threads and State"
description: "How the streaming app is organised: the ordered Bevy system chains, worker threads, shared state, and the 120 Hz fixed simulation."
tags: [architecture, bevy, block-avatar, threads, schedule]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
code_refs: [bins/block-avatar/src/main.rs, bins/block-avatar/src/app/sim.rs, bins/block-avatar/src/character/jiggle.rs]
---

# Block Avatar Runtime: Bevy Plugins, Threads and State

`bins/block-avatar/`, one Rust process on Bevy 0.19.1. The ECS main loop owns all state. **Named worker threads** do blocking work (mic, WAV feeder, IRC, control server, viewer server, Whisper, question transcription, clip encoder, short export, ffmpeg check, webcam, audio playback, hotkeys) and reach the main loop only through `Arc<Mutex<_>>` and `mpsc`. **No async runtime** in the app. Mutex locks recover from poisoning (`unwrap_or_else(|e| e.into_inner())`).

## Startup (`main.rs`, about 50 lines)
Parses the CLI (`cli.rs`), reads settings (`settings.rs`), builds resources in `app/configure.rs`, adds Bevy default plugins plus `AvatarPlugins`.

## Plugins by pipeline (`app/plugins.rs`)
`AvatarPlugins` = `Setup`, `Frame`, `Motion`, `Network`, `WorldGlue` (plus `vox_world::input::InputPlugin`). They are grouped by *pipeline*, not by folder: several chains run systems from different folders in a set order. Check schedule changes with `tools/test/visual_test.py` (render references with the old build, `--update`, compare the new one).
- **Setup** (Startup chain): scene, model load, webcam, clips, stage, bubbles, poll labels, start card, chat, fx (sound), lighting, particles, blocks, in that order.
- **Frame** (Update): input, hotkeys, commands, guest, lip sync, body and camera, webcam and clips, HUD and published state; then, after the camera, everything on stage (throws, dress-up, Twitch moments, polls, voice effects, co-host, show, puppets, hits, sound, bubbles); the fixed-rate steps; face capture, sets, idle life, crowd and snapshots.
- **Motion** (PostUpdate after animation sampling, before transform propagation): `drive_face`, `guest::drive_face`, `puppet::faces` write face parts, joint rotations and scales over the animation, so emotes, squash and stretch layer on any clip; then `bevel::apply`, `lighting::blur` (after propagation) and `jiggle::update` (springs in 1/120 s substeps).
- **Network**: `net::receive`, particle and net cues, `lighting::update`; find body, hold still, load remotes, voice, stage; then after the faces `net::send`, `apply`, `place_guest`.
- **WorldGlue**: a Vox world inside the app (`--world`): the world client's receive/attach/controls/remesh/HUD chain before the camera, its camera and weather, sky (`outdoors`), portals and its input actions.
- **FixedUpdate at 120 Hz** (`app/sim.rs`): particles, crowd, fx step, chat fade; moving things write `Interp::curr` and `sim::blend` draws between the last two ticks. See `decisions/fixed-120hz-simulation`.

## State
`Avatar` (`model.rs`) is the central resource; feature modules own their resources (`Chat`, `Wardrobe`, `Moments`, `Polls`, `VoiceFx`, `Cohost`, `Show`, `Clips`, `Fx`, `Hotkeys`, `Guest`, `Captions`, `Stages`). Bevy limits a system to 16 parameters, so some take resource tuples.

## Layout (lines, 2026-11-26)
`app/` (configure, plugins, sim, startup, applog), `audience/` (ask, chat, guest, questions, streamchat, poll, events, cohost), `character/` (bevel 5 files, face, facecap, jiggle, puppet, wardrobe 753), `controls/` (commands 967, control 691, hotkeys), `network/net.rs` 902, `output/` (clips 994, webcam 711, cards, snapshot), `scene/` (particles 906, lighting 855, bubbles 722, crowd 580, stage 554, camera, show, sets, fx), `sound/` (audio, netvoice, voicefx). Voice analysis moved to `crates/avatar-voice/`. Windows-only: hotkeys, webcam driver, console attach.

## Build-time
`build.rs` embeds the Windows icon via `winresource`. Optional feature `whisper`. `winchk` cfg lets Windows code be type-checked on Linux: `RUSTFLAGS="--cfg winchk" cargo check -p block_avatar`.

## Smells already known
Resource tuples remain; duplicated helpers (xorshift, value noise, child-spawning) are listed in AGENTS.md. The big `main.rs` risk is largely fixed (`status/risks-and-open-issues`).

# Related Concepts
- [Voice Analysis and Lip Sync](voice-and-lipsync.md): voice analysis feeding the avatar
- [Rendering, Cameras and Outputs (Avatar App)](rendering-and-output.md): cameras and outputs
- [Avatar Control Surfaces: Control API, Commands, Settings and Files](control-surfaces.md): control page and commands
- [Audience Interaction: Chat, Dress-up, Polls, Crowd and Moments](audience-interaction.md): chat features
- [Decision: 120 Hz Fixed Step with Interpolated Drawing](../decisions/fixed-120hz-simulation.md): why effects step at 120 Hz
- [Bevy 0.19.1: How and Why It Is Used](../technologies/bevy.md): the engine
- [Decision (done): Restructure into crates/, bins/ and apps/](../decisions/restructure-into-crates-bins-apps.md): why the app is plugins and folders
