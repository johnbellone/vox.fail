---
type: Architecture
title: "Viewer Questions: Ask Page, Queue and Moderation"
description: "How viewers record a question on the public page, how the streamer reviews and airs it in the viewer's own voice, and the security controls on that page."
tags: [architecture, security, viewer-questions, moderation]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
governance: constraint
code_refs: [bins/block-avatar/src/audience/ask/, bins/block-avatar/src/audience/questions/, bins/block-avatar/src/audience/guest/]
---

# Viewer Questions

The only streamer-side surface designed to face the internet. Code: `bins/block-avatar/src/audience/ask/` (page and API, port 7778), `bins/block-avatar/src/audience/questions/` (queue), `bins/block-avatar/src/audience/guest/` (the on-stage guest).

## Flow
1. Viewer signs in with Twitch (device-code flow at twitch.tv/activate; the app reads the username once and discards the token) or, if `ask_names=true`, types a name (shown as *unverified*).
2. Picks a character (with portrait), records up to 30 s (`ask_max_secs`), listens back, sends.
3. It lands as **New**. The streamer listens privately on the control page. **Approve** stays greyed out until it has been played.
4. **On air** (or `A`): the viewer's character pops in beside the host, camera widens to a two-shot, the recording plays through the default output (or `guest_output=`, e.g. a virtual cable) and drives the guest's mouth, brows and head. The host's own mic is ignored meanwhile so speaker spill does not move the host's mouth (`guest_mutes_host`). **Done** thanks them; **Cut** stops instantly.
5. The voice is the viewer's own, never modified or synthesised (a hard constraint).

## Limits
30 s per question; 1 waiting and 3 per stream per viewer; 60 s between sends; 200 in the queue. Upload size is capped at (max secs + 1) x 48,000 x 2 + 4 KB with a hard 60 s ceiling, checked before the body is read.

## Security controls
- Headers on every response: CSP (`default-src 'self'; media-src 'self' blob:; ... frame-ancestors 'none'`), `X-Frame-Options: DENY`, `nosniff`, `no-referrer`, `no-store`, Permissions-Policy allowing only the microphone.
- Sessions: random token in an `x-ask-session` header kept in `sessionStorage`; no cookies, so no CSRF. Max 5,000 sessions.
- Rate limits per client address: 120 requests/min, 5 typed-name sign-ins, 6 device-code sign-ins and 6 uploads per 10 min.
- Nothing airs unheard: New, listen, Approve, On air. The page never plays one viewer's question to another.
- Bans by account (and by address for typed names); Twitch chat bans carry over; stored in `questions/banned.txt`. Blocked words flag transcripts and hide matching names on stream (`questions/blocked_words.txt`).
- Retention: recording deleted when done, skipped or banned; everything dropped after 24 h; tokens never stored.
- Viewer-supplied text must be rendered by `textContent`, never `innerHTML` (a review item).

## Known weaknesses
`client_ip()` trusts `CF-Connecting-IP`, or else the *first* `X-Forwarded-For` entry when the peer is loopback (`ask.rs` ~line 172). Behind a tunnel that only appends XFF a viewer can pick their own address and defeat per-address limits and typed-name bans. (The directory instead reads the last XFF entry.) The Windows installer still offers a firewall rule for 7778 although the server binds 127.0.0.1. See `status/risks-and-open-issues`.

Setup details (Twitch client id, tunnel with `cloudflared tunnel --url http://localhost:7778`) are in `docs/avatar/manual.md`.

# Related Concepts
- [Risks and Open Issues (re-checked 2026-11-26)](../status/risks-and-open-issues.md): known weaknesses of the ask page
