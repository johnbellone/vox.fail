---
type: Decision
title: "Decision: Read Twitch Chat Anonymously"
description: Chat is read over IRC as the read-only justinfan user so streamers set only a channel name and authorise nothing.
tags: [decision, twitch, privacy, chat]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: Anonymous Twitch Chat

**Context**: most streamer tools require OAuth and a token with chat scopes.

**Decision**: connect to Twitch IRC as a `justinfan` anonymous user (read-only). The only setting is the channel name. Only the viewer-question page uses a Twitch sign-in, and only the device-code flow with a public client id (no secret): the app reads the username once and throws the token away.

**Consequences**: zero setup, no stored credentials, nothing to leak; but the app cannot send chat messages or use channel-point redemptions. YouTube needs the streamer's own API key (quota-limited); Kick uses an unofficial route that can break. Live Twitch has **not** been tested against a real channel yet.

See `architecture/audience-interaction`, `architecture/viewer-questions`.
