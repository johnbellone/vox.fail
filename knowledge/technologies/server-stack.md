---
type: Technology
title: "Server Stack: axum, tokio, tower-http, reqwest, tiny_http"
description: Which HTTP and WebSocket libraries each service uses and why the avatar app has no async runtime.
tags: [technology, axum, tokio, http, websocket]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Server Stack

| Component | Libraries |
| --- | --- |
| vox-directory | axum 0.8, tower-http 0.6 (`fs`), tokio 1, reqwest 0.12 (rustls, for Steam/Discord checks), argon2 0.5, sha2, getrandom |
| vox-server | axum 0.8 (`ws`), tokio, futures-util, reqwest (reports to the directory), rusqlite |
| vox-relay | axum 0.8 (`ws`), tokio, futures-util, getrandom, serde; per-address limits (10 shows per 10 min, wrong codes 20 per address and 50 per show) |
| Desktop WebSocket | **`vox-link`** (a thread with reconnect and backoff, shared by the app and clients) over `tungstenite` 0.27 |
| Avatar app | **tiny_http 0.12** (control and ask pages), **ureq 3** (Twitch HTTP), **tungstenite 0.27** (blocking WebSocket, Kick, relay, world), threads and `mpsc`; **no async runtime** |
| Tests | tokio-tungstenite 0.30, reqwest with cookies |

- TLS is rustls everywhere (no OpenSSL dependency), which keeps Windows and wasm builds simple.
- Services bind `127.0.0.1` by default and rely on a front proxy (Cloudflare Tunnel) for TLS.
- The directory serves static pages from `apps/site/`, assets from `assets/` and `data/` with cache headers.
- The directory and the world servers shut down gracefully on Ctrl-C (SIGINT) and the world server saves its SQLite state; systemd units use `KillSignal=SIGINT` (world) and the relay has no signal handling.

See `architecture/vox-server`, `architecture/vox-directory`, `architecture/avatar-app-runtime`.
