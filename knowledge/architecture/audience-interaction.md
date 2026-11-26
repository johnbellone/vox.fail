---
type: Architecture
title: "Audience Interaction: Chat, Dress-up, Polls, Crowd and Moments"
description: "How chat from Twitch, YouTube and Kick reaches the avatar and what viewers can do, with the limits that keep spam watchable."
tags: [architecture, chat, twitch, audience]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [bins/block-avatar/src/audience/chat/, bins/block-avatar/src/audience/streamchat.rs, bins/block-avatar/src/character/wardrobe.rs, bins/block-avatar/src/audience/poll.rs, bins/block-avatar/src/scene/crowd.rs, bins/block-avatar/src/audience/events.rs, bins/block-avatar/src/character/puppet.rs]
---

# Audience Interaction

## Chat sources (`chat.rs`, `streamchat.rs`)
- **Twitch**: anonymous read-only IRC as `justinfan` user. No account, token or OAuth. Parses tags for bits, subs, raids, bans (CLEARCHAT), VIP/mod. See `decisions/anonymous-twitch-irc`.
- **YouTube**: Data API v3 `liveChat/messages` with the streamer's own API key and the live video's id. Polls every 8 s by default (about a 4-hour stream's worth of the 10,000-unit daily quota; `youtube_poll_s`). Super Chats count as bits (1 dollar = 100 bits), new members as subs.
- **Kick**: Pusher websocket the way Kick's own page does it. Not an official API, can break.
- Any bot can call `GET /cmd/throw:tomato:Name`.
All platforms land in one `Chat`; viewer keys are prefixed (`tw:`, `yt:`, `kick:`) so a YouTube ban never touches a Twitch viewer of the same name.

## What viewers can do (and limits)
| Feature | Command | Limits |
| --- | --- | --- |
| Throws | `!tomato` `!egg` `!confetti` | 1 per viewer per 12 s, 0.6 s between throws, max 3 in flight. Splat on the face for ~3.5 s, flinch; thrower credited. |
| Dress-up (`wardrobe.rs`) | `!hat` `!glasses` `!hold` `!wear`, `off` | 15 hats, 4 face pieces, 12 held items; one of each at once; 30 s per-viewer cooldown; auto-remove after 10 min (`dressup_minutes`); who may: everyone, subs/VIPs, or mods. |
| Polls (`poll.rs`) | `!1`/`!2`, `!a`/`!b` | Tug-of-war rope; revote allowed; winner celebrates, loser falls. |
| Crowd (`crowd.rs`) | talk in chat; `!me dog` | Up to 8 chatters as small characters with name signs, platform-coloured borders, star for subs; leave after 3 quiet minutes. |
| Clips | `!clip` (mods) | Saves the last 20 s. |
| Vote questions up | `!up 12` | One vote per viewer, not for their own. |

## Twitch moments (`events.rs`)
Raid: one little character per 10 raiders (3-16) runs on, camera goes wide. Sub/resub/gift: confetti, victory pose, thanks. Bits: a tomato per 100 bits in a volley (up to 8), a giant one from 500.

## Moderation
Bans and timeouts from any platform mute the viewer for the duration; Twitch bans also carry into the question queue. Subject to the platform's own mod tools.

## Puppets (`puppet.rs`)
Small extra characters (raid crowd, rope pullers, press row) that load, animate and keep a tidy single face on their own.

Related: `architecture/viewer-questions`, `architecture/control-surfaces`.

# Related Concepts
- [Viewer Questions: Ask Page, Queue and Moderation](viewer-questions.md): the public question page
- [Decision: Read Twitch Chat Anonymously](../decisions/anonymous-twitch-irc.md): why chat is read anonymously
