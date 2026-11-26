---
type: Architecture
title: "Shows over the Network: Relay, Host, Guest and Viewers"
description: How the avatar app streams a live show through vox-relay so viewers and a remote guest see and hear the same scene.
tags: [architecture, networking, relay, voice, protocol]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [bins/vox-relay/src/, bins/block-avatar/src/network/net.rs, bins/block-avatar/src/sound/netvoice.rs, crates/vox-world/src/proto.rs, crates/vox-world/src/pose.rs]
---

# Shows over the Network

Code: `bins/vox-relay/` (`vox-relay`), `bins/block-avatar/src/network/net.rs`, `bins/block-avatar/src/sound/netvoice.rs`, `crates/vox-world/src/proto.rs`, `crates/vox-world/src/pose.rs`.

## Roles
- **Host** (`net:host`): registers a show with the relay (`POST /sessions {title, character, public}` returns id, join code, host token), then streams.
- **Watcher** (`--watch http://relay/<id> [--code]`): own mic and reactions off; the host's scene, character and camera take over.
- **Guest** (`--join ... --code ... --name`): keeps their own character, mic and face capture; stands on the host's left after the host clicks **Let guest on**.
- Everyone connects *out* to the relay, so nobody needs a port forward, and the host uploads one stream however many watch.

## Relay (`bins/vox-relay/src/lib.rs`)
axum + WebSocket. `GET /sessions` lists public shows with a live host; `GET /` shows them as a page. Up to 1,000 shows; expiry; wrong token or code refused. Test flags: `--simulate-delay 200 --simulate-loss 0.05`. Needs TLS in front for the internet. No accounts yet. Limits (added 2026-11-26): 10 shows per address per 10 minutes, wrong join codes 20 per address and 50 per show (so a code cannot be guessed from many addresses); text frames from guests and viewers are dropped so they cannot fake the relay's own `{"relay": ...}` notices; `--behind-proxy` counts visitors by the Cloudflare address header. Also `DELETE /sessions/{id}` ends a show.

## Wire format (`crates/vox-world/src/proto.rs`)
Every binary message = one byte the relay reads (`DROPPABLE` or `RELIABLE`) + a postcard-encoded `Msg`. Poses and voice are droppable (a newer one replaces a lost one). A pose is the avatar's final shape for one tick: every node off its rest transform, packed small (rotation 7 bytes, position/scale 6 bytes) plus a face mask and camera; typically under 250 bytes. Sent 30 times a second; the scene (character, background, set, lighting, outfit) goes when it changes and every 2 s, on real time, and the moment someone joins.

## Playout
Viewers play poses about 100-140 ms behind, interpolating; the playout clock never runs backwards and only snaps when 250 ms off (tests pin this through 40-90 ms jitter).

## Voice (`netvoice.rs`)
Voice is sent as it is (no voice changing): mono, 16 kHz, IMA ADPCM at 4 bits/sample (~8 KB/s), 20 ms packets each carrying coder state so a loss costs 20 ms of silence only. Nothing is sent while quiet. A 120 ms jitter buffer fills lost packets with silence and ignores late ones. Use headphones to avoid echo; `net_output=` routes voices to a virtual cable for OBS.

## Verified
Run end to end under Xvfb with the real relay: host (lead in the diner), watcher and guest all showed the same two-shot (mean difference 0.06/255); through simulated 200 ms delay and 5% loss the watcher still followed. Voice was verified by counting packets (no audio device was available).

## Known gaps
Guest downloads its own forwarded voice and pose (~15 KB/s wasted); hats a guest wears are not shown on the host; ADPCM is about twice Opus's size; a host under 10 fps runs its clock slow.

See `architecture/wire-protocol` for the separate world protocol.

# Related Concepts
- [Decision: IMA ADPCM for Network Voice (Opus Later)](../decisions/adpcm-network-voice.md): why voice uses ADPCM
- [World Wire Protocol (PROTOCOL = 5)](wire-protocol.md): the separate world protocol
