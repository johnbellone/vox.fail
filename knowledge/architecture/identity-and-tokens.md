---
type: Architecture
title: "Identity, Sessions and Signed Launch Tokens"
description: "How people sign in, how the directory proves who they are to world servers without a call-back, and what secrets exist."
tags: [architecture, security, auth, ed25519, argon2]
generated: { by: agent/mcp, at: "2026-11-27T22:11:30Z" }
status: stable
governance: constraint
code_refs: [crates/vox-core/src/auth.rs, bins/vox-directory/src/lib.rs, bins/vox-directory/src/social.rs]
---

# Identity, Sessions and Tokens

## Sign-in methods (`bins/vox-directory/src/lib.rs`, `social.rs`)
- **Name and password**: Argon2id; 8-128 characters; a dummy hash is checked for unknown names so a failure takes the same time.
- **Steam**: OpenID 2.0. The reply is verified by asking Steam (`check_authentication`), and must come from Steam's endpoint, for our return address, with a state matching a 10-minute cookie. With `VOX_STEAM_KEY` a new account takes its Steam name.
- **Discord**: OAuth2 code flow, `identify` scope only, same state check. Needs `VOX_DISCORD_CLIENT_ID` and `_SECRET`; redirect `<public-url>/auth/discord/return`.
- An `identities` table maps a site's id to an account. First sign-in makes an account named after the person (cleaned, nearest free name that isn't held); when already signed in, "Link your account" adds the site. Site-made accounts have no password.

## Changing your name
`POST /api/name` (`routes.rs` `rename`): sign-up's rules, one full change per 30 days (capitals-only, ASCII-folded, any time), the old name held for its owner 37 days in `held_names` and refused to everyone else at sign-up, social sign-in and rename. Tokens already issued keep the old name until they expire. See [Decision: Account Name Changes](../decisions/account-name-changes.md).

## Web session
Random token in cookie `vox_session` (HttpOnly, SameSite=Lax, Secure with `--secure`); only its SHA-256 is stored. Cross-site requests cannot send it without asking first.

## Launch token for worlds (`crates/vox-core/src/auth.rs`)
Format `v1.<claims>.<signature>`, both parts base64url without padding. Claims (postcard): account number, display name, `Character {model, wear[]}`, issued, expires. Signed with the directory's **Ed25519** key; `LIFETIME = 24 h`. World servers hold only the public key (`--directory-key`), so checking needs no network call. The browser gets one automatically; the desktop client takes it as `--token` ("Copy launch code").

## Server-to-server secret
`VOX_WORLD_SECRET` (same value in the directory and each world) authenticates world reports and plot/portal fetches. A world server holding it can mint authority, a known economy risk (risk 30).

## Secrets and where they live
`directory.key` (private signing key, git-ignored, `/var/lib/vox/` in production), `directory.key.pub`, `/etc/vox/directory.env` (mode 600: Discord id and secret, optional Steam key, world secret), `/etc/vox/worlds.env` (world secret). Never commit them.

## Viewer page identity (separate system)
The streamer's ask page uses Twitch device-code login or unverified typed names; see `architecture/viewer-questions`.

# Related Concepts
- [Decision: Signed Tokens Instead of Server Call-backs](../decisions/ed25519-signed-launch-tokens.md): rationale for signed tokens
- [Decision: Account Name Changes](../decisions/account-name-changes.md): renames, limits and held names
