---
type: Decision
title: "Decision: Voice Chat in Worlds (Nearby and Raid Party), Relayed by vox-server"
description: "World voice reuses Block Avatar's IMA ADPCM voice in a new pure crate vox-voice, travels as droppable wire messages (PROTOCOL 7), and the world server alone decides who hears each packet."
tags: [decision, voice, networking, vox-server, vox-client]
generated: { by: agent/mcp, at: "2026-11-27T11:17:07Z" }
status: stable
---

# Decision

Players in a Vox world can talk to **nearby** players and to their **raid party**. Questions, shorts, webcam, Twitch and merging vox-relay are out of scope.

## Where the code lives
- New crate `crates/vox-voice`: pure Rust, no Bevy, no threads, no clocks, wasm32-safe. It holds the IMA ADPCM codec, the resampler, the jitter buffer, the 20 ms packetizer and the silence gate with its hangover, all moved out of `bins/block-avatar/src/sound/netvoice.rs`. `block_avatar` depends on it (no copy stays).
- `avatar-voice` stays as the analysis crate (lip sync, pitch, formants); `vox-world` depends on both.
- Device code (cpal capture and playback) stays in the apps and `vox-world`, behind `cfg(not(target_arch = "wasm32"))`.

## Wire (vox-core, PROTOCOL 7)
- `ToServer::Voice(VoicePacket)` and `ToClient::Voice { player, packet }`, added last so older messages keep their numbers; `ToClient::Voice` is droppable.
- `VoicePacket { channel: u8 (0 nearby, 1 party), seq: u32, samples: u16, pred: i16, index: u8, data: Vec<u8> }`, 16 kHz mono ADPCM, 20 ms (320 samples, 160 bytes). The codec byte is dropped: ADPCM is the only codec (see the ADPCM decision).
- The relay's `proto::Voice` stays as it is for shows.

## Who hears (server-authoritative)
- The sender's channel is checked, not trusted; the audience is computed from server-known state only.
- **Nearby:** other players in this world within `voice_radius` blocks (default 32, flag `--voice-radius`) of the sender's checked position (`Player::at`).
- **Party:** other players in this world whose directory account is in the same raid party (`crews.kind = 'party'`, any party with two or more members, raiding or not) as the sender's account. No distance limit.
- Guests (no account) can use nearby voice but are in no party. Watchers neither send nor receive voice.
- The sender never receives their own packet.

## Party membership reaches the world
`GET /api/world/{id}/rules` (localhost, `VOX_WORLD_SECRET`) gains `"parties": [[account, ...], ...]`: member accounts of each party with two or more members, capped. The server rebuilds `account -> party` on each rules fetch (every 30 s). A world with no directory has no parties and still does nearby voice.

## Validation and limits (server)
Hello first; `data.len() == samples / 2` with `samples == 320`; `index <= 88`; `channel <= 1`; at most 60 packets per second per player with a small burst (a client sends 50); a position must be known (`at` is Some); everything else is dropped silently. Voice is never stored or logged.

## Client
- Mic is opt-in (off at startup). Push-to-talk (an input action on keyboard and controller) or open mic with the silence gate; the transmit channel is chosen by the player (nearby / party) and shown on the HUD with a speaking indicator.
- Playback: nearby voices are attenuated by distance and panned left or right by bearing; party voice is non-spatial. Per-player mute. Input and output volume and device in the menu.
- Remote and local mouths are driven from voice through `avatar-voice` lip sync.

## Browser (wasm32)
Compiles with voice behind cfg. Decided in the client bead whether the browser gets Web Audio voice or shows "voice is not available in the browser yet"; the stub is acceptable for this goal.

## Why
Reusing the proven ADPCM voice keeps bandwidth at about 8 KB/s per talker. Computing audiences on the server keeps a client from listening where it should not, and from broadcasting to everyone.

# Related Concepts
- [Decision: IMA ADPCM for Network Voice (Opus Later)](adpcm-network-voice.md): the codec
- [World Wire Protocol (PROTOCOL = 5)](../architecture/wire-protocol.md): the messages this extends
- [Voice Analysis and Lip Sync](../architecture/voice-and-lipsync.md): the analysis the mouths use
- [Authority, World Currencies, Feats, Plots, Portals and Raids](../architecture/economy-and-social.md): raid parties


# As built (2026-11-26)
- `crates/vox-voice` (adpcm, resample, jitter, packet) holds the shared code; `block_avatar` uses it.
- Wire: `VoicePacket` has `pub` fields; `ToServer::Voice` and `ToClient::Voice` are last in their enums; PROTOCOL 7 (6 was travel, which landed on main first; voice goes after `Through` and `Travel`).
- Server: `voice_audience` (pure, unit-tested), per-player bucket 60/s burst 100, per-connection cap 120/s and size check, `--voice-radius` (default 32). The directory rules carry `parties` (newest 500 parties, 24 members each, refreshed every 30 s).
- Client: input actions PushToTalk (B, D-pad right) and VoiceChannel (N, keyboard only: every pad button already had a game action). The client fades nearby voices over a fixed 32 blocks (a server with another radius mismatches).
- Browser: stub only; Web Audio voice is a follow-up.
- Review catches fixed: speaker positions are metres on the wire and blocks in the listener (divide by block size); the listener faces `cam_yaw`, not the character's yaw; the HUD marker holds 0.3 s.
- Known gaps: the mic stream stays open while voice is on (push-to-talk only gates sending); muting does not flush audio already queued (up to 0.4 s); starting the devices blocks the main thread briefly; no real microphone or speakers were available, so device paths are covered by compilation and unit tests, and the audio path by `bins/vox-server/tests/voice_e2e.rs` (a real recording through a real server).

# Finished (2026-11-27)
Brought onto main (Bevy 0.20.1) and landed, with three of the known gaps closed:
- **The radius**: the world's settings carry `voice_radius` (the server's `--voice-radius`); the client fades nearby voices over that distance (`radius_from`: finite, above zero, at most 512, else 32). No more mismatch.
- **Mute is immediate**: `Playback::forget` drops a muted player's queued audio, instead of letting up to 0.4 s play out.
- **No frame is blocked opening devices**: each device opens on its own thread (`voice/device.rs`) and the plugin polls (speakers first, the mic only once they are open). The speaker stream is now made and dropped on that thread too, so the `unsafe impl Send` for `Playback` is gone.
- Still open: voice in the browser (the stub says so); the mic stays open while voice is on (push to talk gates sending, as documented); not yet tried with a real microphone and speakers on this machine (devices are covered by compilation and unit tests, the audio path by `voice_e2e`).
