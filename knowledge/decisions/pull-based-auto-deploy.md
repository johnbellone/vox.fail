---
type: Decision
title: "Decision: Pull-Based Automatic Deploys from Version Tags"
description: "Superseded 2026-11-27: vox.fail moved to Cloudflare Containers; releases deploy with wrangler from GitHub Actions. Was: a v* tag built a bundle that a timer on the server pulled and installed."
tags: [decision, deployment, cloudflare, release, ci]
generated: { by: agent/mcp, at: "2026-11-27T17:27:02Z" }
status: deprecated
---

## Decision

Production deploys of vox.fail are triggered by pushing a `v*` tag and delivered by the server pulling, not by CI pushing.

- `.github/workflows/release.yml` builds `deploy/docker/Dockerfile`, copies the image's `/opt/vox` out as `vox-linux-x86_64.tar.gz`, writes its SHA-256 and a build provenance attestation, and attaches them to the GitHub release.
- `deploy/vox-update.sh`, run every five minutes by `vox-update.timer`, installs the latest release into `/opt/vox-releases/<tag>`, switches the `/opt/vox` symlink atomically, restarts the vox units that were running (directory first), checks the directory answers, and purges Cloudflare's cache (`CF_ZONE`, `CF_TOKEN` in `/etc/vox/deploy.env`).
- A release that fails its health check is rolled back and marked failed; the timer won't retry it. `--tag` and `--rollback` hold a release until `--resume`.

## Why

The machine sits behind a Cloudflare Tunnel with no open ports. Pulling keeps it that way: GitHub holds no SSH key or Access token that can write to production. Building from the Dockerfile keeps the bundle and the container image the same build (Debian bookworm glibc). Tags rather than every push to main match the Windows release workflow and keep deploys deliberate.

## Alternatives rejected

- CI pushes over SSH through a Cloudflare Access hostname: faster, but puts a production write credential in GitHub secrets.
- Self-hosted runner on the server: compiles Bevy and wasm on the production box and runs CI code there.

See [[architecture/deployment]] and [[decisions/cloudflare-tunnel-hosting]].
