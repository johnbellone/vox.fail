---
type: Design
title: "Design (proposed): vox.fail on Cloudflare Containers"
description: "One basic container (directory and vox-host) behind the edge Worker as a Durable Object, sleeping when idle; SQLite kept and streamed to R2 by Litestream; the Worker serves the site; the dev server and tunnel retired."
tags: [design, deployment, cloudflare, containers, litestream, r2]
generated: { by: agent/mcp, at: "2026-11-27T16:35:18Z" }
status: draft
---

Proposed 2026-11-27; the full design is `docs/design/cloudflare-hosting.md`. Nothing is built yet.

## The owner's choices (2026-11-27)

- **Storage:** keep SQLite; Litestream streams each database to an R2 bucket and restores on start. Rewriting the directory's roughly 24 tables and 130 queries for D1 was rejected.
- **Uptime:** the container sleeps when idle, after about 15 minutes without requests or open world sockets. Pages and game files are served by the Worker, so browsing doesn't wake it.
- **Size:** the basic instance (¼ vCPU, 1 GiB), raised after measuring.
- **The dev server and tunnel are retired.** The server-pull deploy goes; the systemd units, image and Helm chart stay for self-hosting.

## Why not Containers' own storage

Container disks are temporary. Container snapshots (beta) are tied to one image version, so a release would strand the data. They also expire after 30 days and can't be listed. So the data lives in R2, through Litestream.

## Key constraints for building

- `start.sh` must start the directory with production's flags (`--secure`, `--behind-proxy`, `--public-url`) before vox-host, which needs `directory.key.pub`.
- The Durable Object must pass `cf-connecting-ip` on, or sign-in rate limits collapse into one bucket.
- `/api/world/*` and `/api/host/*` stay refused at the Worker.
- The Durable Object switches images itself, so two containers never write the same replica.

See [[decisions/edge-worker-offline-page]], [[decisions/cloudflare-managed-tunnel]] and [[architecture/deployment]].
