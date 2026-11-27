---
type: Architecture
title: "Deployment: vox.fail on Cloudflare Tunnel, and Windows Packaging"
description: "How vox.fail is hosted (one Linux box, systemd, Cloudflare Tunnel with no open ports) and how Block Avatar is packaged for Windows."
tags: [deployment, cloudflare, systemd, windows, installer]
generated: { by: agent/mcp, at: "2026-11-27T15:15:21Z" }
status: stable
code_refs: [deploy/, bins/block-avatar/installer/block-avatar.nsi, .github/workflows/windows.yml]
---

# Deployment

## vox.fail (see `deploy/README.md`)
One Linux machine runs the directory and the worlds on localhost. `cloudflared` dials out to Cloudflare and carries traffic by path, so **no inbound ports** and no certificates to manage.

| Public | Local |
| --- | --- |
| https://vox.fail/ | vox-directory 127.0.0.1:7700 (pages, sign-in, API, `/pkg`) |
| wss://vox.fail/w/7780/ws | The Void, 127.0.0.1:7780 |
| wss://vox.fail/w/7781/ws | The yard, :7781 |
| wss://vox.fail/w/7782/ws | The islands, :7782 |

`deploy/cloudflared/config.yml` has one ingress rule per world, then the directory last. Systemd units: `vox-directory.service` and `vox-world@.service` (one instance per world, reading `/etc/vox/<name>.env` for `PORT` and `SEED`). Layout on the server: `/opt/vox/{bin,web,assets,data}`, user `vox`, state in `/var/lib/vox`. Start the directory first: it creates the signing key the worlds need.

Build: `cargo build --release -p vox-directory -p vox-server -p vox-client`, `tools/dev/build_web.sh` (wasm client into `apps/site/public/pkg`) and `bun install && (cd apps/site && bun run build)`. Copy `apps/site/dist/` to `/opt/vox/web` (per `deploy/README.md`).

Directory flags in production: `--secure --behind-proxy --public-url https://vox.fail --steam --world "void=The Void=/w/7780/ws"` (and similar for yard, islands).

## Cloudflare dashboard settings that matter
SSL: Always Use HTTPS. WebSockets on. **Rocket Loader off** and Email Obfuscation off (they rewrite scripts the content policy then blocks). **Bot Fight Mode off** or skipped for `/w/` (desktop client cannot answer a challenge). One cache rule: path starting `/pkg/`, `/assets/` or `/data/` eligible for cache (Cloudflare does not cache `.wasm`, `.glb`, `.json` by default; the wasm is ~11 MB compressed). `deploy/purge-cache.sh` empties the cache after a release.

## Block Avatar on Windows
`.github/workflows/windows.yml` runs on every `v*` tag: `cargo test --release`, `cargo build --release`, NSIS installer (`bins/block-avatar/installer/block-avatar.nsi`, `makensis /DVERSION=...`) and a portable zip. Installed copy keeps user files in `%APPDATA%\Block Avatar`. The installer (Program Files, Start menu, optional desktop shortcut and a firewall rule for 7778 that has no effect, see risk 3) is only ~2.5 MB.
**This workflow has never run**, and nothing has ever run on Windows. The exe and installer are unsigned (SmartScreen will warn).

## Relay
`vox-relay` is not part of the vox.fail deployment yet; the README's run line is `cargo run --release -p vox-relay -- --listen 0.0.0.0:7779` behind TLS.

## Decision record
`decisions/cloudflare-tunnel-hosting`.

# Related Concepts
- [Decision: Host vox.fail Behind a Cloudflare Tunnel](../decisions/cloudflare-tunnel-hosting.md): why a tunnel
- [Windows Integration: Hotkeys, OBS Virtual Camera, Installer](../technologies/windows-integration.md): installer and Windows pieces
- [Decision: Pull-Based Automatic Deploys from Version Tags](../decisions/pull-based-auto-deploy.md): How releases reach the server: v* tags build a bundle, a timer on the server installs it.
- [Decision: A Cloudflare Worker Shows an Offline Page When the Server Is Down](../decisions/edge-worker-offline-page.md): What visitors see when the server is down: a Worker in front of the tunnel serves an offline page.
