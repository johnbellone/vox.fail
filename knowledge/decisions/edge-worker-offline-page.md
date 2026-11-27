---
type: Decision
title: "Decision: A Cloudflare Worker Shows an Offline Page When the Server Is Down"
description: "vox.fail runs on a dev server for now; a Worker on vox.fail/* passes requests through and serves a static marketing page (and a JSON 503 for /api) when the tunnel or directory can't be reached."
tags: [decision, deployment, cloudflare, worker, site]
generated: { by: agent/mcp, at: "2026-11-27T15:15:18Z" }
status: stable
---

## Decision

vox.fail runs on a dev server for now, which is sometimes off. `apps/edge` is a Cloudflare Worker on `vox.fail/*` in front of the tunnel:

- While the server is up, every request (WebSockets included) is passed through unchanged.
- When Cloudflare answers 530 (no tunnel), 502, 504 or a 52x, pages get a static offline page (the home page's pitch from `apps/site/src/features/home/Pitch.tsx`, the README's screenshots, "offline right now"), served with status 503 and never cached; `/api/` gets `503` JSON with an offline message. The directory's own 503 ("busy") passes through.
- The offline page's files are Worker static assets under `/_offline/`, a path the directory never uses; `run_worker_first` keeps the Worker in charge.
- Deployed by `.github/workflows/edge.yml` on main through the `production` GitHub environment (required reviewer; `CLOUDFLARE_API_TOKEN` scoped to Workers Scripts and Workers Routes edit).

## Why

The servers can't run on Cloudflare itself (Containers have no persistent disk; Workers can't run the native servers), so the origin stays on a machine behind the tunnel, and a dev machine is sometimes down. A Worker decides per request and serves the page from Cloudflare, so visitors see what Vox is rather than error 1033. Custom error pages were not used: less control over which requests get which answer.

## Trade-offs

A token that can edit Workers on the zone could redirect all of vox.fail's traffic, so it lives only in the protected environment, behind an approval. Every request counts against the Workers plan (100,000 a day free).

See [[decisions/pull-based-auto-deploy]] and [[architecture/deployment]].
