# Changing your name

Status: approved 2026-11-27; beads filed under vox-r58. Nothing here is built yet.

A signed-in player can change the name they go by from the start page. The directory checks
the new name, limits how often it changes, and holds the old name for a while so nobody else
can take it and pass as them.

## Decisions (settled with the owner, 2026-11-27)

| Question | Decision |
| --- | --- |
| Rules for a new name | The same as sign-up: `util::check_name` (3 to 20 characters; letters, digits, single spaces, `_ - .`; starts with a letter or digit). Unique without regard to capitals (`accounts.name` is `UNIQUE COLLATE NOCASE`). |
| Rate limit | One change every 30 days. A change of capitals only (`sam` to `Sam`) is always allowed and doesn't start the 30 days. |
| The old name | Held for the account for 37 days (the 30 days plus a week, so its owner can take it back first; review finding): nobody else can sign up as it, get it from Steam or Discord sign-in, or change to it. The owner can change back to it (that counts as a change). |
| Tokens already issued | Left alone. A launch token lasts at most 24 hours (`auth::LIFETIME`) and carries the account id, which is what permissions use; it shows the old name until it runs out. The token format and the offline check don't change. The session cookie is keyed by account, so it keeps working. |
| Names stored as text | World edit logs (`edits`, `entity_edits`) get an `account` column beside `player`, so history can be shown under the current name later. `player` stays as the name used at the time. Ledger memos ("from Sam to Kim") stay as written; the ledger list already joins the current names by account id. |
| Password again? | No. The session is enough, as for changing your character. Steam and Discord accounts have no password. |

## Directory server (`bins/vox-directory`)

### Storage (`src/state.rs`)

- `accounts.renamed INTEGER` (unix seconds of the last counted change, NULL if never), added with
  the `pragma_table_info` + `ALTER TABLE` pattern already used for `portals`.
- New table `held_names (name TEXT PRIMARY KEY COLLATE NOCASE, account INTEGER NOT NULL, until INTEGER NOT NULL)`.
- `fn held_by_other(db, name, account: Option<u64>) -> bool`: a row for `name` with
  `until > now` and a different account. Expired rows are deleted when a name changes.

All checks and writes happen under the one directory database lock, so "is it free?" and
"take it" can't be split by another request.

### Held names are refused everywhere a name is taken

- `routes::signup`: a held name answers `409 that name is taken`, the same as a taken one (it
  doesn't say whose it was).
- `social.rs` new-account loop: a held name is skipped like a taken one, so it moves on to the
  next free name (`Sam 2`).
- The name change itself (below).

### `POST /api/name`

Body `{ "name": "<new name>" }`, JSON only (`json_only`), signed in by session cookie.

1. No session: `401 sign in first`.
2. Per-visitor try limiter (`d.tried`): `429 too many tries: wait a few minutes`.
3. `check_name`: `400` with its message.
4. Same name exactly: `200`, nothing changes.
5. Same name in other capitals: change it, `renamed` untouched.
6. Otherwise: if `renamed` is within 30 days, `429 you can change your name again in N days`.
7. Taken by another account, or held by another account: `409 that name is taken`.
8. Update `accounts.name`, set `renamed = now`, insert or replace `held_names (old name, me, now + 30 days)`,
   delete the hold on the new name if it was mine. One transaction.
9. Answer `200 { "name": <new>, "rename_after": <unix seconds or null> }`.

`GET /api/me` also returns `rename_after` (when the next full change is allowed, or null), so
the form can say so before anyone tries.

A test-only `Directory::backdate_rename(account, secs)` (like `backdate_portal`) lets tests see
the 30 days end.

## Site (`apps/site`)

- `api/types.ts`: `Me.rename_after?: number | null`.
- New `features/home/NameChange.tsx`, shown in `SignedIn.tsx` under the name: a "Change your
  name" link that opens a small form (text input, Save, Cancel). On save it posts to `/api/name`;
  a refusal is shown as text under the input with `messageOf(err)` (no toast only, no silent
  failure). On success it reloads `/api/me`. When `rename_after` is in the future the form says
  "You can change your name again on <date>" and still allows capitals-only changes.

## World server (`bins/vox-server`)

- `edits` and `entity_edits` get `account INTEGER` (NULL for guests and old rows), added with a
  `pragma_table_info` check at open. The in-memory logs carry the account beside the name and
  write it. Nothing reads it yet; it's there so history survives renames.

## Help (`docs/help/accounts.md`)

A short "Change your name" section: where the form is, the rules, once every 30 days (capitals
any time), the old name is kept for you for 30 days, a launch code you already copied shows the
old name until it runs out (a day), and old edits keep the name you had then.

## Tests

Directory (`tests/directory.rs`): valid change (and `/api/me` shows it, a new token carries
it); taken name, including other capitals; invalid name; no sign-in; second change within 30
days refused, allowed after `backdate_rename`; capitals-only change allowed inside the 30 days;
someone else can't sign up as or change to a held name; the owner can change back to it.
Site: the form's refusal text (a `bun test` for the message helper if the form logic is split
out). World server: an edit by a signed-in player logs its account.

Acceptance for the whole epic: `cargo test -p vox-directory`, `cargo test -p vox-server`,
`cd apps/site && bun test && bun run build`, `cargo clippy` on the touched crates with no new
warnings.

## Beads

Epic: **vox-r58 Change your name** (`gh-<n>` still to add: needs a GitHub issue, as this is a user-facing feature).

| Id | Bead | Files to change | Depends on | Acceptance |
| --- | --- | --- | --- | --- |
| vox-r58.1 | Directory: `renamed` column, `held_names`, refuse held names in sign-up and social sign-in | `bins/vox-directory/src/state.rs`, `routes.rs`, `social.rs` |  | `cargo test -p vox-directory` |
| vox-r58.5 | Directory: `POST /api/name`, `rename_after` in `/api/me`, `backdate_rename` | `bins/vox-directory/src/routes.rs`, `state.rs` | vox-r58.1 | `cargo check -p vox-directory` |
| vox-r58.2 | Directory tests for the name change and holds | `bins/vox-directory/tests/directory.rs` | vox-r58.5 | `cargo test -p vox-directory` |
| vox-r58.6 | Review: directory (security, correctness) | | vox-r58.1, vox-r58.5, vox-r58.2 | reviewer `PASS` |
| vox-r58.3 | Site: name change form with refusals as text | `apps/site/src/api/types.ts`, new `apps/site/src/features/home/NameChange.tsx`, `SignedIn.tsx` | vox-r58.5 | `cd apps/site && bun test && bun run build` |
| vox-r58.4 | World server: account id in edit logs | `bins/vox-server/src/world.rs`, `entities.rs`, `lib.rs` |  | `cargo test -p vox-server` |
| vox-r58.7 | Help: "Change your name" in accounts.md | `docs/help/accounts.md` | vox-r58.5, vox-r58.3 | `grep -n "Change your name" docs/help/accounts.md` |
| vox-r58.8 | Review: site, world server, help | | vox-r58.3, vox-r58.4, vox-r58.7 | reviewer `PASS` |
| vox-r58.9 | Lead: record the decisions in okf (`architecture/identity-and-tokens`), `okf validate --strict --drift` | `knowledge/` via `okf_*` | vox-r58.6, vox-r58.8 | validate: 0 errors, 0 warnings |

vox-r58.1 and vox-r58.4 can start at once; vox-r58.2 and vox-r58.3 can run side by side after vox-r58.5.
