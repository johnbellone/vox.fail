//! Friends, friend groups and raid parties.
//!
//! - **Friends** are mutual: one asks, the other accepts (or both ask, which is the same).
//! - **Groups** (up to 24) are friends who build together: a plot shared with a group lets
//!   every member build in it.
//! - **Raid parties** (up to 8) go somewhere together. Their leader starts a raid on a world
//!   (a feat, in [`crate::game`]); when two or more of the party have spent ten minutes there
//!   together within the hour, the raid is done and those there get authority and a trophy.
//!
//! Groups and parties are both "crews". You invite your friends into a crew; they accept or
//! decline. Leaving passes the lead to the longest-standing member; the last to leave ends it.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ApiError, Directory, Res, bad, check_name, json_only, now};

const GROUP_MAX: i64 = 24;
const PARTY_MAX: i64 = 8;
const FRIENDS_MAX: i64 = 500;
const CREWS_EACH: i64 = 20;

fn oops<E>(_: E) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong: try again".into())
}

/// An account by the name it goes by (any case).
pub(crate) fn named(db: &Connection, name: &str) -> Res<(u64, String)> {
    db.query_row("SELECT id, name FROM accounts WHERE name = ?1", params![name.trim()], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)))
        .optional()
        .map_err(oops)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("nobody here goes by {}", name.trim())))
}

fn friends(db: &Connection, a: u64, b: u64) -> bool {
    db.query_row("SELECT 1 FROM friends WHERE a = ?1 AND b = ?2", params![a as i64, b as i64], |_| Ok(())).optional().ok().flatten().is_some()
}

fn names(db: &Connection, sql: &str, id: u64) -> Res<Vec<String>> {
    let mut st = db.prepare(sql).map_err(oops)?;
    let v = st.query_map(params![id as i64], |r| r.get(0)).map_err(oops)?.flatten().collect();
    Ok(v)
}

pub(crate) async fn people(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    let (id, _) = d.who(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let friends: Vec<Value> = {
        let mut st = db
            .prepare(
                "SELECT a.name, c.model FROM friends f JOIN accounts a ON a.id = f.b LEFT JOIN characters c ON c.account = a.id
                 WHERE f.a = ?1 ORDER BY a.name COLLATE NOCASE",
            )
            .map_err(oops)?;
        st.query_map(params![id as i64], |r| Ok(json!({ "name": r.get::<_, String>(0)?, "model": r.get::<_, Option<String>>(1)? })))
            .map_err(oops)?
            .flatten()
            .collect()
    };
    let asking = names(&db, "SELECT a.name FROM friend_requests q JOIN accounts a ON a.id = q.src WHERE q.dst = ?1 ORDER BY q.at", id)?;
    let asked = names(&db, "SELECT a.name FROM friend_requests q JOIN accounts a ON a.id = q.dst WHERE q.src = ?1 ORDER BY q.at", id)?;
    let crews: Vec<(i64, String, String, i64, Option<String>, Option<i64>, Option<i64>, i64, i64)> = {
        let mut st = db
            .prepare(
                "SELECT c.id, c.kind, c.name, c.leader, c.target, c.started, c.ends, c.together, c.done FROM crews c
                 JOIN crew_members m ON m.crew = c.id WHERE m.account = ?1 ORDER BY c.kind DESC, c.created",
            )
            .map_err(oops)?;
        st.query_map(params![id as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))
            .map_err(oops)?
            .flatten()
            .collect()
    };
    let t = now() as i64;
    let mut out = Vec::new();
    for (cid, kind, name, leader, target, started, ends, together, done) in crews {
        let members: Vec<Value> = {
            let mut st = db
                .prepare("SELECT a.id, a.name FROM crew_members m JOIN accounts a ON a.id = m.account WHERE m.crew = ?1 ORDER BY m.joined, a.id")
                .map_err(oops)?;
            st.query_map(params![cid], |r| Ok(json!({ "name": r.get::<_, String>(1)?, "leads": r.get::<_, i64>(0)? == leader })))
                .map_err(oops)?
                .flatten()
                .collect()
        };
        let invited = {
            let mut st = db.prepare("SELECT a.name FROM crew_invites i JOIN accounts a ON a.id = i.account WHERE i.crew = ?1").map_err(oops)?;
            st.query_map(params![cid], |r| r.get::<_, String>(0)).map_err(oops)?.flatten().collect::<Vec<_>>()
        };
        let raid = target.map(|w| {
            let state = match (done, ends) {
                (1, _) => "won",
                (_, Some(e)) if e <= t => "over",
                _ => "on",
            };
            json!({
                "world": w, "world_name": d.0.worlds.iter().find(|x| x.id == w).map(|x| x.name.clone()),
                "url": d.0.worlds.iter().find(|x| x.id == w).map(|x| x.url.clone()),
                "started": started, "ends": ends, "together": together, "needed": crate::game::RAID_TOGETHER, "state": state,
            })
        });
        out.push(json!({
            "id": cid, "kind": kind, "name": name, "you_lead": leader == id as i64,
            "members": members, "invited": invited, "raid": raid,
            "max": if kind == "party" { PARTY_MAX } else { GROUP_MAX },
        }));
    }
    let invites: Vec<Value> = {
        let mut st = db
            .prepare(
                "SELECT c.id, c.kind, c.name, a.name FROM crew_invites i JOIN crews c ON c.id = i.crew JOIN accounts a ON a.id = i.by
                 WHERE i.account = ?1 ORDER BY i.at",
            )
            .map_err(oops)?;
        st.query_map(params![id as i64], |r| {
            Ok(json!({ "crew": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?, "by": r.get::<_, String>(3)? }))
        })
        .map_err(oops)?
        .flatten()
        .collect()
    };
    Ok(Json(json!({ "friends": friends, "asking": asking, "asked": asked, "crews": out, "invites": invites })))
}

#[derive(Deserialize)]
pub(crate) struct Name {
    name: String,
}

pub(crate) async fn befriend(State(d): State<Directory>, h: HeaderMap, Json(n): Json<Name>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (other, oname) = named(&tx, &n.name)?;
    if other == id {
        return Err(bad("that's you"));
    }
    if friends(&tx, id, other) {
        return Ok(Json(json!({ "friends": oname })));
    }
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM friends WHERE a = ?1", params![id as i64], |r| r.get(0)).map_err(oops)?;
    if count >= FRIENDS_MAX {
        return Err(bad(format!("{FRIENDS_MAX} friends is the most")));
    }
    // they asked you already: that's a yes both ways
    let they_asked = tx.query_row("SELECT 1 FROM friend_requests WHERE src = ?1 AND dst = ?2", params![other as i64, id as i64], |_| Ok(())).optional().map_err(oops)?.is_some();
    let r = if they_asked {
        make_friends(&tx, id, other)?;
        json!({ "friends": oname })
    } else {
        tx.execute("INSERT OR IGNORE INTO friend_requests (src, dst, at) VALUES (?1, ?2, ?3)", params![id as i64, other as i64, now() as i64]).map_err(oops)?;
        json!({ "asked": oname })
    };
    tx.commit().map_err(oops)?;
    Ok(Json(r))
}

fn make_friends(db: &Connection, a: u64, b: u64) -> Res<()> {
    let t = now() as i64;
    db.execute("DELETE FROM friend_requests WHERE (src = ?1 AND dst = ?2) OR (src = ?2 AND dst = ?1)", params![a as i64, b as i64]).map_err(oops)?;
    db.execute("INSERT OR IGNORE INTO friends (a, b, since) VALUES (?1, ?2, ?3), (?2, ?1, ?3)", params![a as i64, b as i64, t]).map_err(oops)?;
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct Answer {
    name: String,
    accept: bool,
}

pub(crate) async fn answer_friend(State(d): State<Directory>, h: HeaderMap, Json(a): Json<Answer>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (other, oname) = named(&tx, &a.name)?;
    let asked = tx.query_row("SELECT 1 FROM friend_requests WHERE src = ?1 AND dst = ?2", params![other as i64, id as i64], |_| Ok(())).optional().map_err(oops)?.is_some();
    if !asked {
        return Err(bad(format!("{oname} hasn't asked to be your friend")));
    }
    if a.accept {
        make_friends(&tx, id, other)?;
    } else {
        tx.execute("DELETE FROM friend_requests WHERE src = ?1 AND dst = ?2", params![other as i64, id as i64]).map_err(oops)?;
    }
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "name": oname, "friends": a.accept })))
}

pub(crate) async fn unfriend(State(d): State<Directory>, h: HeaderMap, Json(n): Json<Name>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let (other, oname) = named(&db, &n.name)?;
    db.execute(
        "DELETE FROM friends WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)",
        params![id as i64, other as i64],
    )
    .map_err(oops)?;
    // and any request either way
    db.execute("DELETE FROM friend_requests WHERE (src = ?1 AND dst = ?2) OR (src = ?2 AND dst = ?1)", params![id as i64, other as i64]).map_err(oops)?;
    Ok(Json(json!({ "removed": oname })))
}

#[derive(Deserialize)]
pub(crate) struct NewCrew {
    kind: String,
    name: String,
}

pub(crate) async fn new_crew(State(d): State<Directory>, h: HeaderMap, Json(c): Json<NewCrew>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    if c.kind != "group" && c.kind != "party" {
        return Err(bad("a group or a party"));
    }
    let name = check_name(&c.name).map_err(|e| bad(e.replace("names", "crew names")))?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM crew_members WHERE account = ?1", params![id as i64], |r| r.get(0)).map_err(oops)?;
    if count >= CREWS_EACH {
        return Err(bad(format!("{CREWS_EACH} groups and parties is the most")));
    }
    let t = now() as i64;
    tx.execute("INSERT INTO crews (kind, name, leader, created) VALUES (?1, ?2, ?3, ?4)", params![c.kind, name, id as i64, t]).map_err(oops)?;
    let crew = tx.last_insert_rowid();
    tx.execute("INSERT INTO crew_members (crew, account, joined) VALUES (?1, ?2, ?3)", params![crew, id as i64, t]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "id": crew, "kind": c.kind, "name": name })))
}

/// The crew `crew` (kind, leader, how many in it), if `account` is in it.
fn crew_of(db: &Connection, crew: i64, account: u64) -> Res<(String, String, i64, i64)> {
    let row: Option<(String, String, i64)> = db
        .query_row(
            "SELECT c.kind, c.name, c.leader FROM crews c JOIN crew_members m ON m.crew = c.id WHERE c.id = ?1 AND m.account = ?2",
            params![crew, account as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(oops)?;
    let Some((kind, name, leader)) = row else { return Err(bad("you're not in that group or party")) };
    let n: i64 = db.query_row("SELECT COUNT(*) FROM crew_members WHERE crew = ?1", params![crew], |r| r.get(0)).map_err(oops)?;
    Ok((kind, name, leader, n))
}

#[derive(Deserialize)]
pub(crate) struct Invite {
    crew: i64,
    name: String,
}

pub(crate) async fn invite(State(d): State<Directory>, h: HeaderMap, Json(i): Json<Invite>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (kind, _, leader, n) = crew_of(&tx, i.crew, id)?;
    if kind == "party" && leader != id as i64 {
        return Err(ApiError(StatusCode::FORBIDDEN, "the party's leader invites".into()));
    }
    let (other, oname) = named(&tx, &i.name)?;
    if !friends(&tx, id, other) {
        return Err(bad(format!("invite friends: {oname} isn't one yet")));
    }
    let already = tx.query_row("SELECT 1 FROM crew_members WHERE crew = ?1 AND account = ?2", params![i.crew, other as i64], |_| Ok(())).optional().map_err(oops)?.is_some();
    if already {
        return Err(bad(format!("{oname} is in it already")));
    }
    let max = if kind == "party" { PARTY_MAX } else { GROUP_MAX };
    let pending: i64 = tx.query_row("SELECT COUNT(*) FROM crew_invites WHERE crew = ?1", params![i.crew], |r| r.get(0)).map_err(oops)?;
    if n + pending >= max {
        return Err(bad(format!("a {kind} holds {max}")));
    }
    tx.execute(
        "INSERT OR IGNORE INTO crew_invites (crew, account, by, at) VALUES (?1, ?2, ?3, ?4)",
        params![i.crew, other as i64, id as i64, now() as i64],
    )
    .map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "invited": oname })))
}

#[derive(Deserialize)]
pub(crate) struct CrewAnswer {
    crew: i64,
    accept: bool,
}

pub(crate) async fn answer_invite(State(d): State<Directory>, h: HeaderMap, Json(a): Json<CrewAnswer>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let gone = tx.execute("DELETE FROM crew_invites WHERE crew = ?1 AND account = ?2", params![a.crew, id as i64]).map_err(oops)?;
    if gone == 0 {
        return Err(bad("there's no such invitation"));
    }
    if a.accept {
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM crew_members WHERE account = ?1", params![id as i64], |r| r.get(0)).map_err(oops)?;
        if count >= CREWS_EACH {
            return Err(bad(format!("{CREWS_EACH} groups and parties is the most")));
        }
        tx.execute("INSERT OR IGNORE INTO crew_members (crew, account, joined) VALUES (?1, ?2, ?3)", params![a.crew, id as i64, now() as i64])
            .map_err(oops)?;
    }
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "crew": a.crew, "joined": a.accept })))
}

#[derive(Deserialize)]
pub(crate) struct Leave {
    crew: i64,
}

pub(crate) async fn leave(State(d): State<Directory>, h: HeaderMap, Json(l): Json<Leave>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (kind, _, leader, n) = crew_of(&tx, l.crew, id)?;
    tx.execute("DELETE FROM crew_members WHERE crew = ?1 AND account = ?2", params![l.crew, id as i64]).map_err(oops)?;
    if n <= 1 {
        // the last one out: it ends, and plots shared with it go back to their owners alone
        tx.execute("DELETE FROM crew_invites WHERE crew = ?1", params![l.crew]).map_err(oops)?;
        tx.execute("DELETE FROM crews WHERE id = ?1", params![l.crew]).map_err(oops)?;
        if kind == "group" {
            tx.execute("UPDATE plots SET grp = NULL WHERE grp = ?1", params![l.crew]).map_err(oops)?;
            tx.execute("UPDATE items SET detail = json_set(detail, '$.shared_with', json('null')) WHERE kind = 'deed' AND json_extract(detail, '$.shared_with.id') = ?1", params![l.crew])
                .map_err(oops)?;
        }
    } else if leader == id as i64 {
        let next: i64 = tx.query_row("SELECT account FROM crew_members WHERE crew = ?1 ORDER BY joined, account LIMIT 1", params![l.crew], |r| r.get(0)).map_err(oops)?;
        tx.execute("UPDATE crews SET leader = ?1 WHERE id = ?2", params![next, l.crew]).map_err(oops)?;
    }
    // a plot of yours shared with a group you've left stays yours, unshared
    if kind == "group" {
        tx.execute("UPDATE plots SET grp = NULL WHERE grp = ?1 AND owner = ?2", params![l.crew, id as i64]).map_err(oops)?;
        tx.execute(
            "UPDATE items SET detail = json_set(detail, '$.shared_with', json('null'))
             WHERE kind = 'deed' AND account = ?2 AND json_extract(detail, '$.shared_with.id') = ?1",
            params![l.crew, id as i64],
        )
        .map_err(oops)?;
    }
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "left": l.crew })))
}
