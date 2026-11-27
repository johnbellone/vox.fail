---
type: Decision
title: "Decision: Account Name Changes with a 30-Day Limit and Held Names"
description: "Players change their name through POST /api/name: same rules as sign-up, once per 30 days, old name held 37 days, issued tokens left to expire."
tags: [decision, accounts, vox-directory, auth, names]
generated: { by: agent/mcp, at: "2026-11-27T22:11:19Z" }
status: stable
---

## Context

Players could not change the name they go by after sign-up (or after Steam or Discord sign-in picked one). Names appear in signed launch tokens, which worlds check offline, and as text in world edit logs and ledger memos. Design: `docs/design/name-change.md`. Settled with the owner on 2026-11-27 (bead epic vox-r58, GitHub issue #24).

## Decision

- `POST /api/name` (bins/vox-directory/src/routes.rs `rename`) changes the signed-in account's name. Rules are sign-up's `check_name`; names are unique under SQLite `NOCASE`.
- A full change is allowed once per 30 days (`NAME_HOLD`, `accounts.renamed`). A capitals-only change is free, judged with ASCII case folding (`eq_ignore_ascii_case`) to match `NOCASE` exactly; a non-ASCII case change counts in full.
- The old name is held in `held_names` for 37 days (`NAME_HOLD + NAME_GRACE`), so its owner has a week after the limit ends to take it back. Sign-up, Steam/Discord account creation and other renames all refuse held names with the same text as a taken name.
- All checks read the current name inside one transaction under the directory lock, so concurrent requests cannot both judge against a stale name.
- Tokens already issued are not revoked: they show the old name for at most a day (`auth::LIFETIME`); permissions use the account id. No change to the token format or the offline check.
- World edit logs (`edits`, `entity_edits`) gained a nullable `account` column; `player` stays the name at the time. Ledger memos stay as written.
- No password re-entry: the session is enough (Steam/Discord accounts have none).

## Consequences

- The try limiter for renames is shared with sign-in and sign-up per visitor.
- History can later be shown under current names through the edit logs' account column.

# Related Concepts
- [Identity, Sessions and Signed Launch Tokens](../architecture/identity-and-tokens.md): where names and tokens are described
