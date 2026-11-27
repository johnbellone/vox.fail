//! World editors: the accounts a world's owner lets edit it in the advanced editor (the rights
//! are docs/design/world-editor.md, *Who may edit what*; this keeps the list and answers for it).
//!
//! - Players (signed in): `POST /api/worlds/editors` (the owner grants or revokes a name) and
//!   `GET /api/worlds/editors?world=` (the owner or an editor sees the names).
//! - World servers (with the world secret): `GET /api/world/{id}/editor?account=` says whether
//!   that account may make advanced edits there: its owner, or one of its editors.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ApiError, Directory, Res, bad, json_only, people};

/// How many editors one world may have (the owner is not counted: they may always edit).
pub const MAX_EDITORS: usize = 32;

fn no(code: StatusCode, s: impl Into<String>) -> ApiError {
    ApiError(code, s.into())
}

fn oops<E>(_: E) -> ApiError {
    no(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong: try again")
}

/// The account that owns `world`, if one does.
pub(crate) fn owner_of(db: &Connection, world: &str) -> Res<Option<u64>> {
    let owner: Option<i64> = db.query_row("SELECT account FROM world_owners WHERE world = ?1", params![world], |r| r.get(0)).optional().map_err(oops)?;
    Ok(owner.map(|a| a as u64))
}

/// The names of the world's editors, by name.
fn editor_names(db: &Connection, world: &str) -> Res<Vec<String>> {
    let mut st = db
        .prepare("SELECT a.name FROM world_editors e JOIN accounts a ON a.id = e.account WHERE e.world = ?1 ORDER BY a.name COLLATE NOCASE")
        .map_err(oops)?;
    let names = st.query_map(params![world], |r| r.get(0)).map_err(oops)?.flatten().collect();
    Ok(names)
}

/// The account ids of the world's editors (for the rules a world server fetches).
pub(crate) fn editor_ids(db: &Connection, world: &str) -> Res<Vec<u64>> {
    let mut st = db.prepare("SELECT account FROM world_editors WHERE world = ?1 ORDER BY account").map_err(oops)?;
    let ids = st.query_map(params![world], |r| r.get::<_, i64>(0)).map_err(oops)?.flatten().map(|a| a as u64).collect();
    Ok(ids)
}

#[derive(Deserialize)]
pub(crate) struct Change {
    world: String,
    /// the account's name, as players know each other
    name: String,
    grant: bool,
}

/// The owner of a world grants or revokes someone's right to edit it in the advanced editor.
pub(crate) async fn change(State(d): State<Directory>, h: HeaderMap, Json(c): Json<Change>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    d.world(&c.world)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    if owner_of(&db, &c.world)? != Some(me) {
        return Err(no(StatusCode::FORBIDDEN, "only the world's owner grants editors"));
    }
    let (who, name) = people::named(&db, &c.name)?;
    if who == me {
        return Err(bad("the owner already may edit their world"));
    }
    if c.grant {
        let already: bool = db
            .query_row("SELECT 1 FROM world_editors WHERE world = ?1 AND account = ?2", params![c.world, who as i64], |_| Ok(()))
            .optional()
            .map_err(oops)?
            .is_some();
        if !already {
            let count: i64 = db.query_row("SELECT COUNT(*) FROM world_editors WHERE world = ?1", params![c.world], |r| r.get(0)).map_err(oops)?;
            if count as usize >= MAX_EDITORS {
                return Err(bad(format!("a world has {MAX_EDITORS} editors at most")));
            }
            db.execute("INSERT INTO world_editors (world, account) VALUES (?1, ?2)", params![c.world, who as i64]).map_err(oops)?;
        }
    } else {
        db.execute("DELETE FROM world_editors WHERE world = ?1 AND account = ?2", params![c.world, who as i64]).map_err(oops)?;
    }
    Ok(Json(json!({ "world": c.world, "changed": name, "granted": c.grant, "editors": editor_names(&db, &c.world)? })))
}

#[derive(Deserialize)]
pub(crate) struct WorldQuery {
    world: String,
}

/// The names of a world's editors, for its owner and its editors.
pub(crate) async fn list(State(d): State<Directory>, h: HeaderMap, Query(q): Query<WorldQuery>) -> Res<Json<Value>> {
    let (me, _) = d.who(&h)?;
    d.world(&q.world)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let is_editor: bool = db
        .query_row("SELECT 1 FROM world_editors WHERE world = ?1 AND account = ?2", params![q.world, me as i64], |_| Ok(()))
        .optional()
        .map_err(oops)?
        .is_some();
    if owner_of(&db, &q.world)? != Some(me) && !is_editor {
        return Err(no(StatusCode::FORBIDDEN, "only the world's owner and its editors see who edits it"));
    }
    Ok(Json(json!({ "world": q.world, "editors": editor_names(&db, &q.world)? })))
}

#[derive(Deserialize)]
pub(crate) struct Ask {
    /// the signed-in account making the edit (none: a guest, who may edit nothing here)
    account: Option<u64>,
}

/// For a world server (with the world secret): may this account make advanced edits in the
/// world? 404 when there's no such world; `false` for a guest or anyone not listed.
pub(crate) async fn check(State(d): State<Directory>, h: HeaderMap, Path(world): Path<String>, Query(q): Query<Ask>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.known_world(&world).map_err(|_| no(StatusCode::NOT_FOUND, "there's no such world"))?;
    let Some(account) = q.account else {
        return Ok(Json(json!({ "editor": false })));
    };
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let editor = owner_of(&db, &world)? == Some(account)
        || db
            .query_row("SELECT 1 FROM world_editors WHERE world = ?1 AND account = ?2", params![world, account as i64], |_| Ok(()))
            .optional()
            .map_err(oops)?
            .is_some();
    Ok(Json(json!({ "editor": editor })))
}
