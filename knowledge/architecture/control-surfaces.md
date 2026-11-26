---
type: Architecture
title: "Avatar Control Surfaces: Control API, Commands, Settings and Files"
description: "The local control page and HTTP API, the single text command language shared by keys, hotkeys and bots, the settings file and the user folder."
tags: [architecture, control, api, commands, settings]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [bins/block-avatar/src/controls/control.rs, bins/block-avatar/src/controls/commands.rs, bins/block-avatar/src/controls/hotkeys.rs, bins/block-avatar/src/app/applog.rs]
---

# Control Surfaces

## One command language
Every action is a text command `verb` or `verb:argument`, parsed once (`commands.rs`: `parse_cmd`, `apply_cmd`, `apply_extra`). Keys, the control page, `GET /cmd/<command>`, Stream Deck buttons, global hotkeys, scene presets, catch-phrase gags and chat bots all use the same path. Examples: `face:2`, `play:Victory`, `emote:spin`, `throw:tomato:Viewer`, `say:Hello`, `set:diner`, `poll:start:60:Best snack?|Pizza|Tacos`, `clip`, `ask:next`, `preset:load:Late show`, `world:weather:storm`. Full list: `docs/avatar/manual.md`, `docs/avatar/STREAMDECK.md`.

## Control API (127.0.0.1:7777, `--port`)
`GET /` (page: inline HTML, CSS, JS in a Rust string, tabs Stream/Chat/Questions/Show/Clips/Hotkeys), `GET /cmd/<cmd>`, `GET /state` (JSON, polled each second), `GET /questions`, `GET /q/<id>.wav`, `GET /mics`, `GET /checks`, `GET /preview.jpg` (shorts camera, only while recording).

**Origin guard** `allowed(host, fetch_site, port)`: Host must be empty or `127.0.0.1`, `localhost`, `[::1]` at this port; `Sec-Fetch-Site` must be empty, `same-origin` or `none`. This blocks DNS rebinding and other websites driving the avatar. Empty headers are allowed on purpose (curl, Stream Deck, OBS dock). The page has **no login**; any local process can drive the avatar and read question recordings. Accepted for a single-user desktop tool.

## Settings: `avatar_settings.txt`
`key=value` lines in the user folder (~59 keys: `model`, `lip_mode`, `background`, `gate_db`, `twitch_channel`, `whisper_model`, `ask_*`, `cohost_*`, `webcam*`, `vowels`, ...). Absence means first run. `save_keys` updates given keys and keeps the rest.

## User folder (`applog.rs`)
Chosen in order: `BLOCK_AVATAR_HOME`, the program folder if writable (source or portable run), then `%APPDATA%\Block Avatar` or `~/.local/share/block-avatar`. Holds settings, `avatar.log` (+ `avatar.prev.log`), `hotkeys.txt`, `gags.txt`, `sounds/`, `questions/`, `clips/`, `whisper/`, `presets.json`, `levels/`.

## Global hotkeys (Windows, `hotkeys.rs`)
Combos in `hotkeys.txt` each run one command, registered with Win32 `RegisterHotKey` on a worker thread; defaults are Ctrl+Alt+... Known issue: Ctrl+Alt is AltGr on many European layouts, so defaults steal characters.

## CLI flags
`--model --mic --list-mics --transparent --port --ask-port --wav --analyze --webcam --webcam-size --webcam-fps --webcam-dump --cohost-wav --no-browser --watch --join --world --snapshot ...`.

## First-run setup and go-live check
Five steps at `/?setup`: mic, character, vowel tuning, Twitch channel, dependency checks (OBS virtual camera, ffmpeg, Whisper). The go-live check runs these, then watches 5 s for frame rate, mic level, chat, webcam and recording.

Related: `architecture/audience-interaction`, `architecture/viewer-questions`.

# Related Concepts
- [Block Avatar: Feature Inventory](../project/block-avatar-features.md): features reachable by commands
- [Decision: Block Avatar Is Local-First, Voice-First, One Binary](../decisions/local-first-single-binary-avatar.md): design principles behind one command path
