---
type: Component
title: "vox-directory: Accounts, Characters, Economy and Web Pages"
description: "The account service and website: sign-in, characters, signed tokens, the world list, the economy and social API, and the static pages."
tags: [component, vox-directory, axum, sqlite, accounts]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
code_refs: [bins/vox-directory/src/, bins/vox-directory/tests/]
---

# vox-directory: Accounts, Characters, Economy and the Site's API

Path `bins/vox-directory/`, 3,114 lines in 11 files; 12 integration tests (`tests/directory.rs` 3, `game.rs` 5, `social.rs` 4) using stand-ins for Steam and Discord. Depends on axum 0.8, tower-http (fs), tokio, rusqlite (bundled), argon2, sha2, reqwest (rustls), `vox-core`.

## Modules
- `state.rs` (264): the directory itself: opening the database (the schema), the router, sessions, sign-in limits, visitor address.
- `routes.rs` (179): the account pages' API: sign up/in/out, who you are, catalogue, character, launch tokens, worlds, public key; response headers.
- `game.rs` (822): authority, currencies, ledger, inventory, feats, plots, portals, raids, world reports. See `architecture/economy-and-social`.
- `people.rs` (355): friends, groups, raid parties.
- `social.rs` (373): Steam OpenID 2.0 and Discord OAuth2 sign-in.
- `lib.rs`, `main.rs`, `util.rs`.

## Identity (see `architecture/identity-and-tokens`)
Name + password (Argon2id, 8-128 chars), Steam, Discord. Web session = random cookie `vox_session` (HttpOnly, SameSite=Lax, Secure when `--secure`); only its SHA-256 is stored.
**Hardening added 2026-11-26**: Steam replies must have no repeated `openid.*` keys, a response nonce, and a signature covering `op_endpoint`, `claimed_id`, `identity`, `return_to`, `response_nonce`, `assoc_handle`; password hashing (about 19 MB each) runs at most 4 at a time and waits up to 5 s before a 503; sign-in counter keys are cut to 80 characters and the table is capped at 100,000.

## HTTP API (selected)
`/api/me`, `signup`, `signin`, `signout`, `catalogue`, `character` (PUT), `token` (launch code), `worlds`, `key`, `wallet`, `send`, `inventory`, `feats` and `feats/plot|portal|currency|raid`, `plots/share|release`, `people`, `friends`, `crews`, `world/report` and `world/{id}/rules` (world servers, shared secret), `/auth/steam[/return]`, `/auth/discord[/return]`. It serves the built site (`--web`, default `web`; in production `apps/site/dist/` is copied to `/opt/vox/web`), plus `/pkg`, `/assets`, `/data`.

## Database (SQLite, 19 tables)
accounts, sessions, characters, identities, currencies, balances, ledger, items, world_owners, world_state, whereabouts, activity, plots, portals, friends, friend_requests, crews, crew_members, crew_invites.

## Flags
`--listen 127.0.0.1:7700 --db directory.db --key directory.key --web web --assets assets --data data --world "id=Name=:7780"... --download "Windows=https://..."... --secure --behind-proxy --public-url https://vox.fail --steam`. Env: `VOX_WORLD_SECRET`, `VOX_DISCORD_CLIENT_ID`, `VOX_DISCORD_CLIENT_SECRET`, `VOX_STEAM_KEY`. Operator jobs: `admin own-world <world id> <player name>`, `admin grant <player name> <authority>`.

## Proxy handling
With `--behind-proxy` the visitor address is `CF-Connecting-IP`, else the *last* `X-Forwarded-For` entry. 30 site sign-ins per address per 5 minutes. `/pkg`, `/assets`, `/data` cacheable 5 min; API and sign-ins `no-store`.

## Keys
First run makes the Ed25519 signing key `directory.key` and writes `directory.key.pub`. Both are git-ignored.

# Related Concepts
- [Identity, Sessions and Signed Launch Tokens](identity-and-tokens.md): sign-in and tokens
- [Authority, World Currencies, Feats, Plots, Portals and Raids](economy-and-social.md): authority, feats, friends
- [Server Stack: axum, tokio, tower-http, reqwest, tiny_http](../technologies/server-stack.md): axum and friends
- [Serialization, Cryptography and Randomness](../technologies/serialization-and-crypto.md): argon2, ed25519, sha2
