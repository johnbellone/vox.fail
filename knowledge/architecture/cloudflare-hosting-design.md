---
type: Design
title: "Design (built, not yet live): vox.fail on Cloudflare Containers"
description: "One basic container (directory and vox-host) behind the edge Worker as a Durable Object, sleeping when idle; SQLite streamed to R2 by Litestream; the Worker serves the site; releases deploy with wrangler. Built 2026-11-27; first deploy waits on Workers Paid."
tags: [design, deployment, cloudflare, containers, litestream, r2]
generated: { by: agent/mcp, at: "2026-11-27T17:27:02Z" }
status: stable
---

Approved and built 2026-11-27; the full design and its as-built notes are in `docs/design/cloudflare-hosting.md`. It isn't live yet: the first deploy needs the Workers Paid plan, and then a `v*` tag.

## The owner's choices (2026-11-27)

- **Storage:** keep SQLite; Litestream 0.5.18 streams every `*.db` in the data folder to the R2 bucket `vox-data` and restores on start. Rewriting the directory for D1 was rejected.
- **Uptime:** the container sleeps after 15 idle minutes. World sockets are relayed and counted by the Durable Object, and an alarm keeps the container awake while any are open. Pages and game files come from the Worker, so browsing doesn't wake it.
- **Size:** the basic instance (¼ vCPU, 1 GiB), raised after measuring.
- **The dev server, tunnel and server-pull deploy are retired.** The systemd units, image, Helm chart and tunnel config stay for self-hosting.

## What's where

- **The container's start:** `deploy/docker/start.sh` writes the signing key from a secret, restores from R2 (a failed restore stops the start), then runs `litestream replicate` with `vox-supervise` (the directory, then vox-host, with production's flags from `directory.args` and `host.args`).
- **The Worker** (`apps/edge`, configured with Wrangler because `cf` 1.0 doesn't take containers):
  - serves the site, sending the 51 MB `.wasm` from its brotli or gzip copy;
  - refuses `/api/world/` and `/api/host/`;
  - sends `/api/`, `/auth/` and `/w/` to the Durable Object `VoxServers`.
- **Releases:** `release.yml` builds the image once, pushes it to Cloudflare's registry pinned by digest, and runs `wrangler deploy`.
- **Cloudflare setup:** `deploy/cloudflare-setup.sh` (zone, DNS, R2 and its token, secrets).

## Why not Containers' own storage

Container disks are temporary. Container snapshots (beta) are tied to one image version, so a release would strand the data. They also expire after 30 days.

## Still to measure

Cold start, memory on the basic instance, and the cost of a real month.

See [[decisions/edge-worker-offline-page]] and [[architecture/deployment]].
