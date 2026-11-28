---
type: Component
title: "vox-directory: Accounts, Characters, Economy and Web Pages"
description: "The account service and website: sign-in, characters, signed tokens, the world list, the economy and social API, and the static pages."
tags: [component, vox-directory, axum, sqlite, accounts]
generated: { by: agent/mcp, at: "2026-11-28T03:24:20Z" }
status: stable
code_refs: [bins/vox-directory/src/, bins/vox-directory/tests/]
---

# vox-directory: Accounts, Characters, Economy and the Site's API

Path `bins/vox-directory/`. Integration tests use stand-ins for Steam and Discord (`tests/directory.rs`, `game.rs`, `social.rs`, `portals.rs` and others). Depends on axum 0.8, tower-http (fs), tokio, rusqlite (bundled), argon2, sha2, reqwest (rustls) and `vox-core`.

## Modules
- `state.rs`: the directory itself: opening the database (the schema and its one-time migrations), the router, sessions, sign-in limits, and the visitor's address.
- `routes.rs`: the account pages' API: sign up, sign in and out, who you are, catalogue, character, launch tokens, worlds, public key; response headers.
- `game.rs`: authority, currencies, ledger, inventory, feats, plots, portals, raids, world reports. See `architecture/economy-and-social`. Player portals are 2 to 4 m wide and 3 to 8 m high (8 to 16 by 12 to 32 blocks), defaulting to 2 × 3 m.
- `people.rs`: friends, groups, raid parties.
- `social.rs`: Steam OpenID 2.0 and Discord OAuth2 sign-in.
- `rescale.rs`: the **one-time move to quarter-metre blocks**. In one transaction it doubles every place: whereabouts, plots, player portals (and their sizes), the worlds' spawns, and the areas and places in deeds and keys. The counter `block_scale` marks it done, and a new directory is only marked.
- `structures.rs`, `editors.rs`, `lib.rs`, `main.rs`, `util.rs`.

## Identity (see `architecture/identity-and-tokens`)
Sign-in is by name and password (Argon2id, 8-128 characters), Steam or Discord. A web session is a random `vox_session` cookie (HttpOnly, SameSite=Lax, Secure when `--secure`); only its SHA-256 is stored.

**Hardening added 2026-11-26**:
- Steam replies must have no repeated `openid.*` keys, must carry a response nonce, and need a signature covering `op_endpoint`, `claimed_id`, `identity`, `return_to`, `response_nonce` and `assoc_handle`.
- Password hashing (about 19 MB each) runs at most 4 at a time, and waits up to 5 s before answering 503.
- Sign-in counter keys are cut to 80 characters, and the table is capped at 100,000.

## HTTP API (selected)
- Accounts: `/api/me`, `signup`, `signin`, `signout`, `catalogue`, `character` (PUT), `token` (launch code), `worlds`, `key`.
- Economy and social: `wallet`, `send`, `inventory`, `feats` and `feats/plot|portal|currency|raid`, `plots/share|release`, `people`, `friends`, `crews`.
- For world servers (shared secret): `world/report`, `world/{id}/rules`.
- Sign-in: `/auth/steam[/return]`, `/auth/discord[/return]`.
- It serves the built site (`--web`, default `web`; in production `apps/site/dist/` is copied to `/opt/vox/web`), plus `/pkg`, `/assets` and `/data`.

## Database (SQLite)
Tables: accounts, sessions, characters, identities, currencies, balances, ledger, items, world_owners, world_editors, world_state, whereabouts, activity, plots, portals, structures, structure_versions, structure_grants, friends, friend_requests, crews, crew_members, crew_invites, instances, instance_opens, portal_requests, counters, held_names.

## Flags
`--listen 127.0.0.1:7700 --db directory.db --key directory.key --web web --assets assets --data data --world "id=Name=:7780"... --download "Windows=https://..."... --secure --behind-proxy --public-url https://vox.fail --steam`

- Environment: `VOX_WORLD_SECRET`, `VOX_DISCORD_CLIENT_ID`, `VOX_DISCORD_CLIENT_SECRET`, `VOX_STEAM_KEY`.
- Operator jobs: `admin own-world <world id> <player name>`, `admin grant <player name> <authority>`.

## Proxy handling
- With `--behind-proxy`, the visitor's address is `CF-Connecting-IP`; otherwise it is the *last* `X-Forwarded-For` entry.
- Each address gets 30 site sign-ins per 5 minutes.
- `/pkg`, `/assets` and `/data` are cacheable for 5 minutes; the API and sign-ins are `no-store`.

## Keys
The first run makes the Ed25519 signing key `directory.key` and writes `directory.key.pub`. Both are git-ignored.

# Related Concepts
- [Identity, Sessions and Signed Launch Tokens](identity-and-tokens.md): sign-in and tokens
- [Authority, World Currencies, Feats, Plots, Portals and Raids](economy-and-social.md): authority, feats, friends
- [Server Stack: axum, tokio, tower-http, reqwest, tiny_http](../technologies/server-stack.md): axum and friends
- [Serialization, Cryptography and Randomness](../technologies/serialization-and-crypto.md): argon2, ed25519, sha2
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): why its places were doubled
