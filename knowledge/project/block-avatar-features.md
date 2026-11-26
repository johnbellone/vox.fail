---
type: Fact
title: "Block Avatar: Feature Inventory"
description: "The roughly 45 user-facing features of the streaming app, grouped by area, with the source file that owns each."
tags: [block-avatar, features, streaming]
generated: { by: agent/mcp, at: "2026-11-26T15:49:37Z" }
status: stable
---

# Block Avatar: Feature Inventory

Almost every feature can be triggered from four places: a key in the avatar window, a control-page button, an HTTP `GET /cmd/<command>`, or (for audience features) chat. See `architecture/control-surfaces`.

## Performance: voice to character
- **Lip sync, five styles** (Visemes, Detailed, Flap, Volume, Chatter) from loudness and tone, plus formant-based vowels. `lipsync.rs`, `voice.rs`.
- **Body language** from the voice: nods on stressed syllables, hand beats, a head tilt on a rising ending followed by a pause, brows up with pitch. `face.rs`.
- **Voice reactions**: shout flashes Rage, 30 s of quiet dozes off, wakes startled. **Laugh and sing detection**. `detect.rs`, `voicefx.rs`.
- **10 expressions, 8 gestures, emotes** (nod, shake, tilt, hop, spin, bow, shiver, lean in) layered over any clip; 100+ clips per character; Bop to the beat.
- **Speech bubbles** typed out RPG-style from Whisper, a caption file, or typed text. `bubbles.rs`.
- **Catchphrase gags**: a heard phrase fires commands (`gags.txt`).

## Look and output
- 38 characters, six voxel sets (rally, pressroom, diner, hospital, newsroom, night) or a green/magenta/studio/transparent background, auto-director camera cuts, retro pixel-sprite mode, studio lighting with shadows, bloom, depth-of-field blur, hand-made bevel normal maps.
- **Virtual webcam** (Windows): 1280x720 at 60 fps into the OBS Virtual Camera driver. `webcam.rs`.
- **Live face capture** from VMC (OSC/UDP 39539) and iFacialMocap (UDP 49983). `facecap.rs`.

## Audience
- **Chat throws** (`!tomato`, `!egg`, `!confetti`), **dress-up** (`!hat`, `!glasses`, `!hold`, `!wear`: 15 hats, 4 face pieces, 12 held items), **Twitch moments** (raid crowd, sub party, bits volley), **tug-of-war polls** (`!1`/`!2`), **viewer crowd** (chatters appear as small characters), **live co-host** (second mic).
- **Viewer questions**: viewers record a question on a web page; the streamer reviews it privately and airs it, and the viewer's chosen character asks it in the viewer's own recorded voice. `ask.rs`, `questions.rs`, `guest.rs`. See `architecture/viewer-questions`.
- **Press conference** mode: approved questions wait as a row of reporters.

## Production and capture
- **Shorts**: a third camera keeps 45 s in memory; 1080x1920 H.264+AAC MP4 with title card, caption and end card, plus a 4-panel PNG comic strip. Triggered by `E`, `!clip`, or automatically on a splat, laugh, sub, raid, gag, poll result, song or answered question. `clips.rs`, `cards.rs`. Needs ffmpeg.
- Highlight reel joins the newest 10 shorts.
- Sound effects: six real CC0 recordings. `fx.rs`.

## Operation
- Local control page with tabs (Stream, Chat, Questions, Show, Clips, Hotkeys), first-run setup, go-live check, global hotkeys (Windows), scenes (presets), Stream Deck addresses, `avatar.log` and a problems box.
- Windows installer and portable zip built by GitHub Actions.

## Shows over the network
Host a show through `vox-relay`; watch it or join as a guest with `--watch` or `--join`. See `architecture/show-networking`.
