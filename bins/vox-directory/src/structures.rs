//! Structures and who may do what with them (the rules are `vox_core::entitle`; this keeps the
//! records and answers for them).
//!
//! A structure is a name, an owner and **versions**: each a hash naming an immutable blob
//! (docs/design/world-format.md, *Structures*); the newest is its head. Entitlements: the
//! owner can do everything and alone grants, revokes, opens it to everyone and deletes; others
//! hold **use** (place it), **copy** (make one of their own) or **edit** (save new versions).
//!
//! - Players (signed in): `POST /api/structures` make one, `GET /api/structures` the ones they
//!   own or are shared, `GET /api/structures/{id}`, `POST …/version` (edit), `…/grant`,
//!   `…/public`, `…/copy`, `…/delete`.
//! - World servers (with the world secret): `GET /api/world/structures/{id}?account=&version=`
//!   says whether that account may place that version, before the world places it.
//!
//! Someone with no right to a structure is told it doesn't exist, not that it's forbidden:
//! its existence is the owner's business.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use vox_core::entitle::{Entitlements, Right};

use crate::{ApiError, Directory, Res, bad, json_only, now};

/// How many structures one account may own (made or copied).
pub const MAX_OWNED: i64 = 200;
/// How many versions a structure keeps in its history.
pub const MAX_VERSIONS: i64 = 500;

fn no(code: StatusCode, s: impl Into<String>) -> ApiError {
    ApiError(code, s.into())
}

fn oops<E>(_: E) -> ApiError {
    no(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong: try again")
}

fn missing() -> ApiError {
    no(StatusCode::NOT_FOUND, "there's no such structure")
}

/// A version is the hash of its blob: 32 lower-case hex digits.
fn check_version(v: &str) -> Res<String> {
    if v.len() == 32 && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        Ok(v.to_string())
    } else {
        Err(bad("a version is the 32 hex digits of its hash"))
    }
}

fn check_name(n: &str) -> Res<String> {
    let n = n.trim();
    if n.is_empty() || n.chars().count() > 48 || n.chars().any(|c| c.is_control()) {
        return Err(bad("a structure's name is 1 to 48 characters"));
    }
    Ok(n.to_string())
}

fn parse_right(s: &str) -> Res<Right> {
    Right::parse(s).ok_or_else(|| bad("a right is use, copy or edit"))
}

/// A structure's entitlements, its name and its head version; 404 if there's no such structure.
fn load(db: &Connection, id: i64) -> Res<(Entitlements, String, String)> {
    let row: Option<(i64, Option<String>, String, String)> = db
        .query_row("SELECT owner, public, name, version FROM structures WHERE id = ?1", params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()
        .map_err(oops)?;
    let (owner, public, name, version) = row.ok_or_else(missing)?;
    let mut e = Entitlements::new(owner as u64);
    e.public = public.as_deref().and_then(Right::parse);
    let mut st = db.prepare("SELECT account, right FROM structure_grants WHERE structure = ?1").map_err(oops)?;
    let rows = st.query_map(params![id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(oops)?;
    for (a, r) in rows.flatten() {
        if let Some(r) = Right::parse(&r) {
            e.grants.insert(a as u64, r);
        }
    }
    Ok((e, name, version))
}

/// The structure, if `account` may do at least `need` with it: 404 if not (or there's none).
fn allowed(db: &Connection, id: i64, account: Option<u64>, need: Right) -> Res<(Entitlements, String, String)> {
    let (e, name, version) = load(db, id)?;
    if e.allows(account, need) { Ok((e, name, version)) } else { Err(missing()) }
}

fn owned_count(db: &Connection, owner: u64) -> Res<i64> {
    db.query_row("SELECT COUNT(*) FROM structures WHERE owner = ?1", params![owner as i64], |r| r.get(0)).map_err(oops)
}

#[derive(Deserialize)]
pub(crate) struct Make {
    name: String,
    version: String,
}

/// Make a structure from a version (a blob already stored): its owner is whoever's signed in.
pub(crate) async fn make(State(d): State<Directory>, h: HeaderMap, Json(m): Json<Make>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    let (name, version) = (check_name(&m.name)?, check_version(&m.version)?);
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    if owned_count(&tx, me)? >= MAX_OWNED {
        return Err(bad(format!("you have {MAX_OWNED} structures already")));
    }
    let t = now() as i64;
    tx.execute(
        "INSERT INTO structures (name, owner, public, version, derived_from, created, updated) VALUES (?1, ?2, NULL, ?3, NULL, ?4, ?4)",
        params![name, me as i64, version, t],
    )
    .map_err(oops)?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO structure_versions (structure, version, author, at) VALUES (?1, ?2, ?3, ?4)", params![id, version, me as i64, t]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "id": id, "name": name, "version": version })))
}

/// The structures you own and the ones shared with you (or open to everyone).
pub(crate) async fn mine(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    let (me, _) = d.who(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let mut st = db
        .prepare(
            "SELECT s.id, s.name, s.version, s.owner, a.name, s.public, g.right, s.derived_from FROM structures s
             JOIN accounts a ON a.id = s.owner
             LEFT JOIN structure_grants g ON g.structure = s.id AND g.account = ?1
             WHERE s.owner = ?1 OR g.account = ?1 ORDER BY s.name COLLATE NOCASE, s.id",
        )
        .map_err(oops)?;
    let rows = st
        .query_map(params![me as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })
        .map_err(oops)?;
    let mut list = Vec::new();
    for (id, name, version, owner, owner_name, public, granted, derived) in rows.flatten() {
        let mut e = Entitlements::new(owner as u64);
        e.public = public.as_deref().and_then(Right::parse);
        if let Some(r) = granted.as_deref().and_then(Right::parse) {
            e.grants.insert(me, r);
        }
        let right = e.right_of(Some(me)).map_or("none", Right::name);
        list.push(json!({ "id": id, "name": name, "version": version, "owner": owner_name, "right": right, "public": e.public.map(Right::name), "derived_from": derived }));
    }
    Ok(Json(json!({ "structures": list })))
}

/// One structure: what you may do with it; its versions if you may edit; who it's shared with
/// if it's yours.
pub(crate) async fn show(State(d): State<Directory>, h: HeaderMap, Path(id): Path<i64>) -> Res<Json<Value>> {
    let (me, _) = d.who(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let (e, name, version) = allowed(&db, id, Some(me), Right::Use)?;
    let right = e.right_of(Some(me)).map_or("none", Right::name);
    let mut out = json!({ "id": id, "name": name, "version": version, "right": right, "public": e.public.map(Right::name) });
    if e.allows(Some(me), Right::Edit) {
        let mut st = db.prepare("SELECT v.version, a.name, v.at FROM structure_versions v JOIN accounts a ON a.id = v.author WHERE v.structure = ?1 ORDER BY v.at, v.rowid").map_err(oops)?;
        let versions: Vec<Value> = st
            .query_map(params![id], |r| Ok(json!({ "version": r.get::<_, String>(0)?, "by": r.get::<_, String>(1)?, "at": r.get::<_, i64>(2)? })))
            .map_err(oops)?
            .flatten()
            .collect();
        out["versions"] = json!(versions);
    }
    if e.can_manage(Some(me)) {
        let mut st = db.prepare("SELECT a.name, g.right FROM structure_grants g JOIN accounts a ON a.id = g.account WHERE g.structure = ?1 ORDER BY a.name COLLATE NOCASE").map_err(oops)?;
        let grants: Vec<Value> = st
            .query_map(params![id], |r| Ok(json!({ "name": r.get::<_, String>(0)?, "right": r.get::<_, String>(1)? })))
            .map_err(oops)?
            .flatten()
            .collect();
        out["grants"] = json!(grants);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub(crate) struct NewVersion {
    id: i64,
    version: String,
}

/// Save a new version (needs edit): the structure's head moves to it; instances placed from
/// the old versions keep theirs.
pub(crate) async fn version(State(d): State<Directory>, h: HeaderMap, Json(v): Json<NewVersion>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    let version = check_version(&v.version)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    allowed(&tx, v.id, Some(me), Right::Edit)?;
    let have: i64 = tx.query_row("SELECT COUNT(*) FROM structure_versions WHERE structure = ?1", params![v.id], |r| r.get(0)).map_err(oops)?;
    let known: Option<i64> = tx.query_row("SELECT 1 FROM structure_versions WHERE structure = ?1 AND version = ?2", params![v.id, version], |r| r.get(0)).optional().map_err(oops)?;
    if known.is_none() {
        if have >= MAX_VERSIONS {
            return Err(bad(format!("a structure keeps {MAX_VERSIONS} versions")));
        }
        tx.execute("INSERT INTO structure_versions (structure, version, author, at) VALUES (?1, ?2, ?3, ?4)", params![v.id, version, me as i64, now() as i64]).map_err(oops)?;
    }
    // saving a version it already had makes that the head again (going back)
    tx.execute("UPDATE structures SET version = ?1, updated = ?2 WHERE id = ?3", params![version, now() as i64, v.id]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "id": v.id, "version": version })))
}

#[derive(Deserialize)]
pub(crate) struct Grant {
    id: i64,
    /// an account's name
    to: String,
    /// use, copy, edit; or null to take it away
    right: Option<String>,
}

/// Give someone a right, or take theirs away (the owner only).
pub(crate) async fn grant(State(d): State<Directory>, h: HeaderMap, Json(g): Json<Grant>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    let right = g.right.as_deref().map(parse_right).transpose()?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (mut e, _, _) = allowed(&tx, g.id, Some(me), Right::Use)?;
    let (to, to_name) = crate::people::named(&tx, &g.to)?;
    e.grant(Some(me), to, right).map_err(|w| bad(w.to_string()))?;
    match right {
        Some(r) => tx.execute(
            "INSERT INTO structure_grants (structure, account, right) VALUES (?1, ?2, ?3) ON CONFLICT(structure, account) DO UPDATE SET right = ?3",
            params![g.id, to as i64, r.name()],
        ),
        None => tx.execute("DELETE FROM structure_grants WHERE structure = ?1 AND account = ?2", params![g.id, to as i64]),
    }
    .map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "id": g.id, "to": to_name, "right": right.map(Right::name) })))
}

#[derive(Deserialize)]
pub(crate) struct Public {
    id: i64,
    /// use or copy; or null to close it
    right: Option<String>,
}

/// Open a structure to every signed-in account to use or copy, or close it again (the owner
/// only; never to edit).
pub(crate) async fn public(State(d): State<Directory>, h: HeaderMap, Json(p): Json<Public>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    let right = p.right.as_deref().map(parse_right).transpose()?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let (mut e, _, _) = allowed(&db, p.id, Some(me), Right::Use)?;
    e.set_public(Some(me), right).map_err(|w| bad(w.to_string()))?;
    db.execute("UPDATE structures SET public = ?1 WHERE id = ?2", params![right.map(Right::name), p.id]).map_err(oops)?;
    Ok(Json(json!({ "id": p.id, "public": right.map(Right::name) })))
}

#[derive(Deserialize)]
pub(crate) struct Copy {
    id: i64,
    /// the copy's name (the original's by default)
    name: Option<String>,
}

/// Make a structure of your own from one you may copy: the same head version, owned by you,
/// with a note of where it came from. It's yours to change; the original is unaffected.
pub(crate) async fn copy(State(d): State<Directory>, h: HeaderMap, Json(c): Json<Copy>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (_, original, version) = allowed(&tx, c.id, Some(me), Right::Copy)?;
    let name = match &c.name {
        Some(n) => check_name(n)?,
        None => original,
    };
    if owned_count(&tx, me)? >= MAX_OWNED {
        return Err(bad(format!("you have {MAX_OWNED} structures already")));
    }
    let t = now() as i64;
    tx.execute(
        "INSERT INTO structures (name, owner, public, version, derived_from, created, updated) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?5)",
        params![name, me as i64, version, c.id, t],
    )
    .map_err(oops)?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO structure_versions (structure, version, author, at) VALUES (?1, ?2, ?3, ?4)", params![id, version, me as i64, t]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "id": id, "name": name, "version": version, "derived_from": c.id })))
}

#[derive(Deserialize)]
pub(crate) struct Delete {
    id: i64,
}

/// Delete a structure (the owner only). Copies others made keep their own record.
pub(crate) async fn delete(State(d): State<Directory>, h: HeaderMap, Json(x): Json<Delete>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (me, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (e, _, _) = allowed(&tx, x.id, Some(me), Right::Use)?;
    if !e.can_manage(Some(me)) {
        return Err(bad("only the owner can delete a structure"));
    }
    tx.execute("DELETE FROM structure_grants WHERE structure = ?1", params![x.id]).map_err(oops)?;
    tx.execute("DELETE FROM structure_versions WHERE structure = ?1", params![x.id]).map_err(oops)?;
    tx.execute("UPDATE structures SET derived_from = NULL WHERE derived_from = ?1", params![x.id]).map_err(oops)?;
    tx.execute("DELETE FROM structures WHERE id = ?1", params![x.id]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "deleted": x.id })))
}

#[derive(Deserialize)]
pub(crate) struct Ask {
    /// the signed-in account placing it (none: a guest)
    account: Option<u64>,
    /// a version to place (the head if left out)
    version: Option<String>,
}

/// For a world server (with the world secret): may this account place this structure, and
/// which version? A world asks before placing one. 404 when the account has no right to it.
pub(crate) async fn check(State(d): State<Directory>, h: HeaderMap, Path(id): Path<i64>, Query(q): Query<Ask>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let (e, name, head) = allowed(&db, id, q.account, Right::Use)?;
    let version = match &q.version {
        Some(v) => {
            let v = check_version(v)?;
            let known: Option<i64> = db.query_row("SELECT 1 FROM structure_versions WHERE structure = ?1 AND version = ?2", params![id, v], |r| r.get(0)).optional().map_err(oops)?;
            // only those who may edit see (or place) the older versions: others get the head
            if known.is_none() || !e.allows(q.account, Right::Edit) && v != head {
                return Err(missing());
            }
            v
        }
        None => head,
    };
    let right = e.right_of(q.account).map_or("none", Right::name);
    Ok(Json(json!({ "id": id, "name": name, "version": version, "right": right })))
}
