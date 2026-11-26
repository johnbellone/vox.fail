//! What players hold and what they can do with it: currencies, the ledger, the inventory,
//! feats, and what world servers tell the directory.
//!
//! - **Authority** (`AUTH`) is the one currency every world shares. It's earned by playing
//!   (time in a world and blocks placed, reported by world servers; capped per day), by
//!   finishing raids, and 25 come with a new account. It can't be sent to anyone: it's what
//!   you spend on feats.
//! - **World currencies**: a world's owner can issue one coin for it (a feat), with a fixed
//!   supply, all minted to them. World coins can be sent between players.
//! - **Feats** cost authority: claim a plot (16 × 16 blocks where you last stood, which only
//!   you, or a group you share it with, can build in), open a portal (in your plot, to another
//!   world), issue a world currency, start a raid (your party's, on another world).
//! - Every movement of every currency is a row in the ledger: minted (no source), spent (no
//!   destination) or sent (both).
//! - The inventory holds deeds (plots), keys (portals), banners (raids started) and trophies
//!   (raids finished); the explorer also shows your coins and the wardrobe.
//! - World servers (with the directory's world secret) report each minute who is there,
//!   where they stand and how many blocks they placed, and fetch the plots and portals in
//!   their world.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ApiError, Directory, Res, bad, json_only, now, sha};

/// The currency every world shares.
pub const AUTH: &str = "AUTH";
/// Authority a new account starts with.
pub const WELCOME: i64 = 25;
/// What feats cost, in authority.
pub const COST_PLOT: i64 = 20;
pub const COST_PORTAL: i64 = 30;
pub const COST_CURRENCY: i64 = 100;
pub const COST_RAID: i64 = 10;
/// Earning: one authority per this many minutes in a world, and per this many blocks placed,
/// up to `DAILY` a day.
pub const MINUTES_PER: i64 = 10;
pub const EDITS_PER: i64 = 50;
pub const DAILY: i64 = 60;
/// A raid: how long it lasts, how many minutes at least two of the party must spend in the
/// target world together, and what each of them gets for it.
pub const RAID_LENGTH: u64 = 3600;
pub const RAID_TOGETHER: i64 = 10;
pub const RAID_REWARD: i64 = 15;
/// Raids pay out at most this many times per account per day (a throwaway pair of accounts
/// could otherwise raid in a loop for authority).
pub const RAID_REWARDS_PER_DAY: i64 = 2;
/// A plot is twice this across, and keeps this far from the world's spawn.
pub const PLOT_HALF: i32 = 8;
pub const SPAWN_CLEAR: f32 = 24.0;
const PLOTS_EACH: i64 = 3;
const PORTALS_PER_PLOT: i64 = 2;
/// Where you last stood counts for a week.
const SEEN_FOR: u64 = 7 * 24 * 3600;

fn no(code: StatusCode, s: impl Into<String>) -> ApiError {
    ApiError(code, s.into())
}

fn oops<E>(_: E) -> ApiError {
    no(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong: try again")
}

pub(crate) fn balance(db: &Connection, account: u64, code: &str) -> i64 {
    db.query_row("SELECT amount FROM balances WHERE account = ?1 AND currency = ?2", params![account as i64, code], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
}

fn add(db: &Connection, account: u64, code: &str, amount: i64) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO balances (account, currency, amount) VALUES (?1, ?2, ?3)
         ON CONFLICT(account, currency) DO UPDATE SET amount = amount + ?3",
        params![account as i64, code, amount],
    )
    .map(|_| ())
}

fn record(db: &Connection, code: &str, src: Option<u64>, dst: Option<u64>, amount: i64, memo: &str) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO ledger (at, currency, src, dst, amount, memo) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![now() as i64, code, src.map(|a| a as i64), dst.map(|a| a as i64), amount, memo],
    )
    .map(|_| ())
}

/// New money for `account` (authority earned, a world coin minted).
pub(crate) fn mint(db: &Connection, account: u64, code: &str, amount: i64, memo: &str) -> rusqlite::Result<()> {
    add(db, account, code, amount)?;
    record(db, code, None, Some(account), amount, memo)
}

/// Money spent (on a feat): gone.
fn spend(db: &Connection, account: u64, code: &str, amount: i64, memo: &str) -> Res<()> {
    let have = balance(db, account, code);
    if have < amount {
        return Err(no(StatusCode::PAYMENT_REQUIRED, format!("that takes {amount} {} and you have {have}", name_of(code))));
    }
    add(db, account, code, -amount).map_err(oops)?;
    record(db, code, Some(account), None, amount, memo).map_err(oops)
}

fn name_of(code: &str) -> &str {
    if code == AUTH { "authority" } else { code }
}

/// Something for someone's inventory.
fn give(db: &Connection, account: u64, kind: &str, name: &str, world: Option<&str>, refers: Option<i64>, detail: Value) -> Res<()> {
    db.execute(
        "INSERT INTO items (account, kind, name, world, ref, detail, acquired) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![account as i64, kind, name, world, refers, detail.to_string(), now() as i64],
    )
    .map(|_| ())
    .map_err(oops)
}

fn day() -> i64 {
    (now() / 86400) as i64
}

impl Directory {
    /// The signed-in account, or 401.
    pub(crate) fn who(&self, h: &HeaderMap) -> Res<(u64, String)> {
        self.account(h).ok_or(no(StatusCode::UNAUTHORIZED, "sign in first"))
    }

    fn world_name(&self, id: &str) -> String {
        self.0.worlds.iter().find(|w| w.id == id).map_or_else(|| id.to_string(), |w| w.name.clone())
    }

    fn world(&self, id: &str) -> Res<&crate::WorldInfo> {
        self.0.worlds.iter().find(|w| w.id == id).ok_or_else(|| bad("there's no such world"))
    }

    /// The world secret checks out.
    pub(crate) fn from_world(&self, h: &HeaderMap) -> Res<()> {
        let Some(secret) = &self.0.world_secret else { return Err(no(StatusCode::NOT_FOUND, "not here")) };
        let given = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
        // compared as hashes, so the time taken says nothing about the secret
        if given.map(|g| sha(g.trim())) == Some(sha(secret)) {
            Ok(())
        } else {
            Err(no(StatusCode::UNAUTHORIZED, "not a world of this directory"))
        }
    }

    /// Give a new account its first authority.
    pub(crate) fn welcome(&self, db: &Connection, account: u64) {
        let _ = mint(db, account, AUTH, WELCOME, "welcome to Vox");
    }
}

// ---- wallet and sending

pub(crate) async fn wallet(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    let (id, _) = d.who(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let mut balances = vec![json!({
        "code": AUTH, "name": "Authority", "world": null, "amount": balance(&db, id, AUTH), "tradeable": false,
    })];
    let mut st = db
        .prepare(
            "SELECT c.code, c.name, c.world, b.amount FROM balances b JOIN currencies c ON c.code = b.currency
             WHERE b.account = ?1 AND b.amount > 0 ORDER BY c.name",
        )
        .map_err(oops)?;
    let rows = st
        .query_map(params![id as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?)))
        .map_err(oops)?;
    for (code, name, world, amount) in rows.flatten() {
        let wn = world.as_deref().map(|w| d.world_name(w));
        balances.push(json!({ "code": code, "name": name, "world": world, "world_name": wn, "amount": amount, "tradeable": true }));
    }
    let mut st = db
        .prepare(
            "SELECT l.at, l.currency, l.src, l.dst, l.amount, l.memo, s.name, t.name FROM ledger l
             LEFT JOIN accounts s ON s.id = l.src LEFT JOIN accounts t ON t.id = l.dst
             WHERE l.src = ?1 OR l.dst = ?1 ORDER BY l.id DESC LIMIT 50",
        )
        .map_err(oops)?;
    let ledger: Vec<Value> = st
        .query_map(params![id as i64], |r| {
            let src: Option<i64> = r.get(2)?;
            let amount: i64 = r.get(4)?;
            let out = src == Some(id as i64);
            let with: Option<String> = if out { r.get(7)? } else { r.get(6)? };
            Ok(json!({
                "at": r.get::<_, i64>(0)?, "code": r.get::<_, String>(1)?,
                "amount": if out { -amount } else { amount }, "memo": r.get::<_, String>(5)?, "with": with,
            }))
        })
        .map_err(oops)?
        .flatten()
        .collect();
    let (minutes, edits, earned) = db
        .query_row("SELECT minutes, edits, earned, day FROM activity WHERE account = ?1", params![id as i64], |r| {
            let today = r.get::<_, i64>(3)? == day();
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, if today { r.get::<_, i64>(2)? } else { 0 }))
        })
        .optional()
        .map_err(oops)?
        .unwrap_or((0, 0, 0));
    Ok(Json(json!({
        "balances": balances,
        "ledger": ledger,
        "earning": {
            "today": earned, "daily": DAILY, "minutes_per": MINUTES_PER, "edits_per": EDITS_PER,
            "minutes": minutes, "edits": edits,
        },
    })))
}

#[derive(Deserialize)]
pub(crate) struct Send {
    to: String,
    code: String,
    amount: i64,
    #[serde(default)]
    memo: String,
}

pub(crate) async fn send(State(d): State<Directory>, h: HeaderMap, Json(s): Json<Send>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, name) = d.who(&h)?;
    if s.code == AUTH {
        return Err(bad("authority can't be sent: it's earned"));
    }
    if !(1..=1_000_000_000_000).contains(&s.amount) {
        return Err(bad("send a whole number, at least 1"));
    }
    let memo: String = s.memo.trim().chars().filter(|c| !c.is_control()).take(80).collect();
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (to, to_name) = crate::people::named(&tx, &s.to)?;
    if to == id {
        return Err(bad("that's you"));
    }
    let known: bool = tx.query_row("SELECT 1 FROM currencies WHERE code = ?1", params![s.code], |_| Ok(true)).optional().map_err(oops)?.is_some();
    if !known {
        return Err(bad("there's no such currency"));
    }
    let have = balance(&tx, id, &s.code);
    if have < s.amount {
        return Err(bad(format!("you have {have} {}", s.code)));
    }
    add(&tx, id, &s.code, -s.amount).map_err(oops)?;
    add(&tx, to, &s.code, s.amount).map_err(oops)?;
    let memo = if memo.is_empty() { format!("from {name} to {to_name}") } else { memo };
    record(&tx, &s.code, Some(id), Some(to), s.amount, &memo).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "sent": s.amount, "code": s.code, "to": to_name })))
}

// ---- the inventory

pub(crate) async fn inventory(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    let (id, _) = d.who(&h)?;
    let worn = d.character(id).map(|c| c.wear).unwrap_or_default();
    let mut items: Vec<Value> = Vec::new();
    {
        let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = db
            .prepare("SELECT id, kind, name, world, ref, detail, acquired FROM items WHERE account = ?1 ORDER BY acquired DESC, id DESC")
            .map_err(oops)?;
        let rows = st
            .query_map(params![id as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })
            .map_err(oops)?;
        for (iid, kind, name, world, refers, detail, acquired) in rows.flatten() {
            let wn = world.as_deref().map(|w| d.world_name(w));
            items.push(json!({
                "id": format!("i{iid}"), "kind": kind, "name": name, "world": world, "world_name": wn, "ref": refers,
                "detail": serde_json::from_str::<Value>(&detail).unwrap_or(Value::Null), "acquired": acquired,
            }));
        }
        // coins: every currency you hold
        items.push(json!({
            "id": "c-AUTH", "kind": "coin", "name": "Authority", "code": AUTH, "world": null, "amount": balance(&db, id, AUTH),
            "detail": { "about": "Earned by playing in any world. Spent on feats; it can't be sent." },
        }));
        let mut st = db
            .prepare("SELECT c.code, c.name, c.world, b.amount FROM balances b JOIN currencies c ON c.code = b.currency WHERE b.account = ?1 AND b.amount > 0")
            .map_err(oops)?;
        let rows = st
            .query_map(params![id as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?)))
            .map_err(oops)?;
        for (code, name, world, amount) in rows.flatten() {
            let wn = world.as_deref().map(|w| d.world_name(w));
            items.push(json!({
                "id": format!("c-{code}"), "kind": "coin", "name": name, "code": code, "world": world, "world_name": wn,
                "amount": amount, "detail": { "about": format!("{}'s own coin. It can be sent to anyone.", wn.unwrap_or_default()) },
            }));
        }
    }
    // the wardrobe: everyone has all of it
    for w in &d.0.catalogue.wardrobe {
        let Some(wid) = w.get("id").and_then(|v| v.as_str()) else { continue };
        items.push(json!({
            "id": format!("w-{wid}"), "kind": "wear", "name": w.get("label").cloned().unwrap_or(json!(wid)),
            "world": null, "slot": w.get("slot"), "worn": worn.iter().any(|x| x == wid),
            "detail": { "slot": w.get("slot") },
        }));
    }
    Ok(Json(json!({ "items": items })))
}

// ---- feats

pub(crate) async fn feats(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    let (id, _) = d.who(&h)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let since = now().saturating_sub(SEEN_FOR) as i64;
    let mut st = db.prepare("SELECT world, x, y, z, at FROM whereabouts WHERE account = ?1 AND at > ?2 ORDER BY at DESC").map_err(oops)?;
    let seen: Vec<(String, f64, f64, f64, i64)> =
        st.query_map(params![id as i64, since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(oops)?.flatten().collect();
    let mut places = Vec::new();
    for (world, x, y, z, at) in seen {
        let mine = db
            .query_row(
                "SELECT id FROM plots WHERE world = ?1 AND owner = ?2 AND x0 <= ?3 AND ?3 < x1 AND z0 <= ?4 AND ?4 < z1",
                params![world, id as i64, x.floor() as i64, z.floor() as i64],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(oops)?;
        places.push(json!({
            "world": world, "world_name": d.world_name(&world), "at": [x, y, z], "seen": at, "in_your_plot": mine,
        }));
    }
    let mut st = db.prepare("SELECT o.world, c.code FROM world_owners o LEFT JOIN currencies c ON c.world = o.world WHERE o.account = ?1").map_err(oops)?;
    let owned: Vec<Value> = st
        .query_map(params![id as i64], |r| {
            let w: String = r.get(0)?;
            Ok((w, r.get::<_, Option<String>>(1)?))
        })
        .map_err(oops)?
        .flatten()
        .map(|(w, code)| json!({ "world": w, "world_name": d.world_name(&w), "currency": code }))
        .collect();
    let worlds: Vec<Value> = d.0.worlds.iter().map(|w| json!({ "id": w.id, "name": w.name })).collect();
    Ok(Json(json!({
        "authority": balance(&db, id, AUTH),
        "costs": { "plot": COST_PLOT, "portal": COST_PORTAL, "currency": COST_CURRENCY, "raid": COST_RAID },
        "places": places,
        "owned_worlds": owned,
        "worlds": worlds,
    })))
}

#[derive(Deserialize)]
pub(crate) struct ClaimPlot {
    world: String,
}

pub(crate) async fn claim_plot(State(d): State<Directory>, h: HeaderMap, Json(c): Json<ClaimPlot>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let wname = d.world(&c.world)?.name.clone();
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (x, z) = where_you_stood(&tx, id, &c.world)?;
    // `as` saturates, and so does the arithmetic: a wild position can't overflow
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let (x0, z0) = (cx.saturating_sub(PLOT_HALF), cz.saturating_sub(PLOT_HALF));
    let (x1, z1) = (cx.saturating_add(PLOT_HALF), cz.saturating_add(PLOT_HALF));
    // clear of the spawn
    if let Some(spawn) = spawn_of(&tx, &c.world) {
        let dx = (spawn[0] as f32).clamp(x0 as f32, x1 as f32) - spawn[0] as f32;
        let dz = (spawn[2] as f32).clamp(z0 as f32, z1 as f32) - spawn[2] as f32;
        if (dx * dx + dz * dz).sqrt() < SPAWN_CLEAR {
            return Err(bad(format!("too close to where people arrive: plots keep {SPAWN_CLEAR} blocks away")));
        }
    }
    let clash: Option<String> = tx
        .query_row(
            "SELECT a.name FROM plots p JOIN accounts a ON a.id = p.owner
             WHERE p.world = ?1 AND p.x0 < ?4 AND ?2 < p.x1 AND p.z0 < ?5 AND ?3 < p.z1",
            params![c.world, x0, z0, x1, z1],
            |r| r.get(0),
        )
        .optional()
        .map_err(oops)?;
    if let Some(who) = clash {
        return Err(no(StatusCode::CONFLICT, format!("that overlaps {who}'s plot")));
    }
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM plots WHERE world = ?1 AND owner = ?2", params![c.world, id as i64], |r| r.get(0)).map_err(oops)?;
    if count >= PLOTS_EACH {
        return Err(bad(format!("you have {PLOTS_EACH} plots in {wname} already")));
    }
    spend(&tx, id, AUTH, COST_PLOT, &format!("claimed a plot in {wname}"))?;
    tx.execute(
        "INSERT INTO plots (world, owner, x0, z0, x1, z1, grp, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)",
        params![c.world, id as i64, x0, z0, x1, z1, now() as i64],
    )
    .map_err(oops)?;
    let plot = tx.last_insert_rowid();
    let area = json!({ "x0": x0, "z0": z0, "x1": x1, "z1": z1 });
    give(&tx, id, "deed", &format!("Plot in {wname}"), Some(&c.world), Some(plot), json!({ "area": area, "shared_with": null }))?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "plot": plot, "area": area })))
}

fn where_you_stood(db: &Connection, account: u64, world: &str) -> Res<(f64, f64)> {
    let since = now().saturating_sub(SEEN_FOR) as i64;
    db.query_row("SELECT x, z FROM whereabouts WHERE account = ?1 AND world = ?2 AND at > ?3", params![account as i64, world, since], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()
    .map_err(oops)?
    .ok_or_else(|| bad("go into that world first: feats happen where you last stood"))
}

fn spawn_of(db: &Connection, world: &str) -> Option<[f64; 3]> {
    let s: String = db.query_row("SELECT spawn FROM world_state WHERE world = ?1", params![world], |r| r.get(0)).ok()?;
    serde_json::from_str(&s).ok()
}

#[derive(Deserialize)]
pub(crate) struct SharePlot {
    plot: i64,
    /// a group (crew) id, or none to keep it to yourself
    group: Option<i64>,
}

pub(crate) async fn share_plot(State(d): State<Directory>, h: HeaderMap, Json(s): Json<SharePlot>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    owns_plot(&tx, id, s.plot)?;
    let mut shared = Value::Null;
    if let Some(g) = s.group {
        let name: Option<String> = tx
            .query_row(
                "SELECT c.name FROM crews c JOIN crew_members m ON m.crew = c.id WHERE c.id = ?1 AND c.kind = 'group' AND m.account = ?2",
                params![g, id as i64],
                |r| r.get(0),
            )
            .optional()
            .map_err(oops)?;
        let Some(name) = name else { return Err(bad("share it with a group you're in")) };
        shared = json!({ "id": g, "name": name });
    }
    tx.execute("UPDATE plots SET grp = ?1 WHERE id = ?2", params![s.group, s.plot]).map_err(oops)?;
    tx.execute(
        "UPDATE items SET detail = json_set(detail, '$.shared_with', json(?1)) WHERE kind = 'deed' AND ref = ?2",
        params![shared.to_string(), s.plot],
    )
    .map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "plot": s.plot, "shared_with": shared })))
}

fn owns_plot(db: &Connection, account: u64, plot: i64) -> Res<String> {
    db.query_row("SELECT world FROM plots WHERE id = ?1 AND owner = ?2", params![plot, account as i64], |r| r.get(0))
        .optional()
        .map_err(oops)?
        .ok_or_else(|| bad("that isn't your plot"))
}

#[derive(Deserialize)]
pub(crate) struct ReleasePlot {
    plot: i64,
}

pub(crate) async fn release_plot(State(d): State<Directory>, h: HeaderMap, Json(r): Json<ReleasePlot>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    owns_plot(&tx, id, r.plot)?;
    // its portals close with it
    let portals: Vec<i64> = {
        let mut st = tx.prepare("SELECT id FROM portals WHERE plot = ?1").map_err(oops)?;
        st.query_map(params![r.plot], |r| r.get(0)).map_err(oops)?.flatten().collect()
    };
    for p in portals {
        tx.execute("DELETE FROM items WHERE kind = 'key' AND ref = ?1", params![p]).map_err(oops)?;
    }
    tx.execute("DELETE FROM portals WHERE plot = ?1", params![r.plot]).map_err(oops)?;
    tx.execute("DELETE FROM items WHERE kind = 'deed' AND ref = ?1", params![r.plot]).map_err(oops)?;
    tx.execute("DELETE FROM plots WHERE id = ?1", params![r.plot]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "released": r.plot })))
}

#[derive(Deserialize)]
pub(crate) struct OpenPortal {
    world: String,
    to: String,
}

pub(crate) async fn open_portal(State(d): State<Directory>, h: HeaderMap, Json(o): Json<OpenPortal>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let here = d.world(&o.world)?.name.clone();
    let there = d.world(&o.to)?.name.clone();
    if o.world == o.to {
        return Err(bad("a portal leads to another world"));
    }
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    if spawn_of(&tx, &o.to).is_none() {
        return Err(bad(format!("{there} hasn't been opened yet: try again once it's running")));
    }
    let since = now().saturating_sub(SEEN_FOR) as i64;
    let (x, y, z): (f64, f64, f64) = tx
        .query_row("SELECT x, y, z FROM whereabouts WHERE account = ?1 AND world = ?2 AND at > ?3", params![id as i64, o.world, since], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()
        .map_err(oops)?
        .ok_or_else(|| bad("go into that world first: feats happen where you last stood"))?;
    let plot: i64 = tx
        .query_row(
            "SELECT id FROM plots WHERE world = ?1 AND owner = ?2 AND x0 <= ?3 AND ?3 < x1 AND z0 <= ?4 AND ?4 < z1",
            params![o.world, id as i64, x.floor() as i64, z.floor() as i64],
            |r| r.get(0),
        )
        .optional()
        .map_err(oops)?
        .ok_or_else(|| bad("stand in a plot of yours to open a portal there"))?;
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM portals WHERE plot = ?1", params![plot], |r| r.get(0)).map_err(oops)?;
    if count >= PORTALS_PER_PLOT {
        return Err(bad(format!("a plot holds {PORTALS_PER_PLOT} portals")));
    }
    spend(&tx, id, AUTH, COST_PORTAL, &format!("opened a portal from {here} to {there}"))?;
    // the doorway's sill: where you stood, on the half block
    let at = [(x * 2.0).round() / 2.0, y.floor(), (z * 2.0).round() / 2.0];
    tx.execute(
        "INSERT INTO portals (world, owner, plot, x, y, z, axis, dest, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'z', ?7, ?8)",
        params![o.world, id as i64, plot, at[0], at[1], at[2], o.to, now() as i64],
    )
    .map_err(oops)?;
    let portal = tx.last_insert_rowid();
    give(&tx, id, "key", &format!("Portal to {there}"), Some(&o.world), Some(portal), json!({ "at": at, "to": o.to, "to_name": there }))?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "portal": portal, "at": at })))
}

#[derive(Deserialize)]
pub(crate) struct Issue {
    world: String,
    code: String,
    name: String,
    supply: i64,
}

pub(crate) async fn issue_currency(State(d): State<Directory>, h: HeaderMap, Json(i): Json<Issue>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let wname = d.world(&i.world)?.name.clone();
    let code = i.code.trim().to_uppercase();
    if !(3..=5).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_uppercase()) || code == AUTH {
        return Err(bad("a currency's code is 3 to 5 letters (and not AUTH)"));
    }
    let name = i.name.trim().to_string();
    let n = name.chars().count();
    if !(3..=24).contains(&n) || !name.chars().all(|c| c.is_alphanumeric() || " '-".contains(c)) {
        return Err(bad("a currency's name is 3 to 24 letters, digits, spaces, ' or -"));
    }
    if !(1..=1_000_000_000).contains(&i.supply) {
        return Err(bad("the supply is 1 to 1,000,000,000"));
    }
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let owner: Option<i64> = tx.query_row("SELECT account FROM world_owners WHERE world = ?1", params![i.world], |r| r.get(0)).optional().map_err(oops)?;
    if owner != Some(id as i64) {
        return Err(no(StatusCode::FORBIDDEN, format!("only {wname}'s owner can issue its currency")));
    }
    let has: Option<String> = tx.query_row("SELECT code FROM currencies WHERE world = ?1", params![i.world], |r| r.get(0)).optional().map_err(oops)?;
    if let Some(c) = has {
        return Err(no(StatusCode::CONFLICT, format!("{wname} has its currency already ({c})")));
    }
    if tx.query_row("SELECT 1 FROM currencies WHERE code = ?1", params![code], |_| Ok(())).optional().map_err(oops)?.is_some() {
        return Err(no(StatusCode::CONFLICT, "that code is taken"));
    }
    spend(&tx, id, AUTH, COST_CURRENCY, &format!("issued {name} ({code}) for {wname}"))?;
    tx.execute(
        "INSERT INTO currencies (code, name, world, issuer, supply, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![code, name, i.world, id as i64, i.supply, now() as i64],
    )
    .map_err(oops)?;
    mint(&tx, id, &code, i.supply, &format!("{name} issued for {wname}")).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "code": code, "name": name, "supply": i.supply })))
}

#[derive(Deserialize)]
pub(crate) struct StartRaid {
    party: i64,
    world: String,
}

pub(crate) async fn start_raid(State(d): State<Directory>, h: HeaderMap, Json(s): Json<StartRaid>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    let wname = d.world(&s.world)?.name.clone();
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (pname, leader, ends, done): (String, i64, Option<i64>, i64) = tx
        .query_row("SELECT name, leader, ends, done FROM crews WHERE id = ?1 AND kind = 'party'", params![s.party], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()
        .map_err(oops)?
        .ok_or_else(|| bad("there's no such party"))?;
    if leader != id as i64 {
        return Err(no(StatusCode::FORBIDDEN, "only the party's leader starts a raid"));
    }
    if done == 0 && ends.is_some_and(|e| e > now() as i64) {
        return Err(no(StatusCode::CONFLICT, "your party is raiding already"));
    }
    spend(&tx, id, AUTH, COST_RAID, &format!("{pname} raids {wname}"))?;
    let t = now() as i64;
    tx.execute(
        "UPDATE crews SET target = ?1, started = ?2, ends = ?3, together = 0, done = 0 WHERE id = ?4",
        params![s.world, t, t + RAID_LENGTH as i64, s.party],
    )
    .map_err(oops)?;
    give(&tx, id, "banner", &format!("Banner of {pname}"), Some(&s.world), Some(s.party), json!({ "raid_on": wname, "started": t }))?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "party": s.party, "target": s.world, "ends": t + RAID_LENGTH as i64 })))
}

// ---- world servers

#[derive(Deserialize)]
pub struct Report {
    pub world: String,
    #[serde(default)]
    pub level: String,
    pub spawn: [f64; 3],
    pub players: Vec<Presence>,
}

#[derive(Deserialize)]
pub struct Presence {
    pub account: u64,
    pub at: Option<[f64; 3]>,
    #[serde(default)]
    pub edits: i64,
}

pub(crate) async fn report(State(d): State<Directory>, h: HeaderMap, Json(r): Json<Report>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&r.world)?;
    let t = now() as i64;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let last: Option<i64> = tx.query_row("SELECT reported FROM world_state WHERE world = ?1", params![r.world], |r| r.get(0)).optional().map_err(oops)?;
    if last.is_some_and(|l| t - l < d.0.report_every as i64) {
        return Err(no(StatusCode::TOO_MANY_REQUESTS, "one report a minute"));
    }
    tx.execute(
        "INSERT INTO world_state (world, spawn, level, reported) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(world) DO UPDATE SET spawn = ?2, level = ?3, reported = ?4",
        params![r.world, json!(r.spawn).to_string(), r.level, t],
    )
    .map_err(oops)?;
    let wname = d.world_name(&r.world);
    let mut here: Vec<u64> = Vec::new();
    let mut earned_total = 0;
    for p in r.players.iter().take(1000) {
        let exists = tx.query_row("SELECT 1 FROM accounts WHERE id = ?1", params![p.account as i64], |_| Ok(())).optional().map_err(oops)?.is_some();
        if !exists || here.contains(&p.account) {
            continue;
        }
        here.push(p.account);
        if let Some(at) = p.at.filter(|a| a.iter().all(|v| v.is_finite())) {
            tx.execute(
                "INSERT INTO whereabouts (account, world, x, y, z, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(account, world) DO UPDATE SET x = ?3, y = ?4, z = ?5, at = ?6",
                params![p.account as i64, r.world, at[0], at[1], at[2], t],
            )
            .map_err(oops)?;
        }
        earned_total += earn(&tx, p.account, p.edits.clamp(0, 1200), &wname)?;
    }
    let finished = raids(&tx, &r.world, &wname, &here)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "players": here.len(), "earned": earned_total, "raids_finished": finished })))
}

/// A minute in a world with `edits` blocks placed: authority as it adds up. Returns how much.
fn earn(db: &Connection, account: u64, edits: i64, world: &str) -> Res<i64> {
    let today = day();
    let (mut minutes, mut placed, mut earned, was) = db
        .query_row("SELECT minutes, edits, earned, day FROM activity WHERE account = ?1", params![account as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
        })
        .optional()
        .map_err(oops)?
        .unwrap_or((0, 0, 0, today));
    if was != today {
        earned = 0;
    }
    minutes += 1;
    placed += edits;
    let due = minutes / MINUTES_PER + placed / EDITS_PER;
    minutes %= MINUTES_PER;
    placed %= EDITS_PER;
    let got = due.min(DAILY - earned).max(0);
    earned += got;
    db.execute(
        "INSERT INTO activity (account, minutes, edits, day, earned) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(account) DO UPDATE SET minutes = ?2, edits = ?3, day = ?4, earned = ?5",
        params![account as i64, minutes, placed, today, earned],
    )
    .map_err(oops)?;
    if got > 0 {
        mint(db, account, AUTH, got, &format!("playing in {world}")).map_err(oops)?;
    }
    Ok(got)
}

/// Raids on `world` going on now, with `here` in it: a minute together for each party with
/// two or more of its members here; finished at `RAID_TOGETHER` minutes. Returns how many
/// finished.
fn raids(db: &Connection, world: &str, wname: &str, here: &[u64]) -> Res<usize> {
    let t = now() as i64;
    let parties: Vec<(i64, String)> = {
        let mut st = db.prepare("SELECT id, name FROM crews WHERE kind = 'party' AND target = ?1 AND done = 0 AND ends > ?2").map_err(oops)?;
        st.query_map(params![world, t], |r| Ok((r.get(0)?, r.get(1)?))).map_err(oops)?.flatten().collect()
    };
    let mut finished = 0;
    for (party, pname) in parties {
        let members: Vec<u64> = {
            let mut st = db.prepare("SELECT account FROM crew_members WHERE crew = ?1").map_err(oops)?;
            st.query_map(params![party], |r| r.get::<_, i64>(0)).map_err(oops)?.flatten().map(|a| a as u64).collect()
        };
        let present: Vec<u64> = members.iter().copied().filter(|m| here.contains(m)).collect();
        if present.len() < 2 {
            continue;
        }
        db.execute("UPDATE crews SET together = together + 1 WHERE id = ?1", params![party]).map_err(oops)?;
        let together: i64 = db.query_row("SELECT together FROM crews WHERE id = ?1", params![party], |r| r.get(0)).map_err(oops)?;
        if together < RAID_TOGETHER {
            continue;
        }
        db.execute("UPDATE crews SET done = 1 WHERE id = ?1", params![party]).map_err(oops)?;
        finished += 1;
        for m in present {
            let paid: i64 = db
                .query_row(
                    "SELECT count(*) FROM ledger WHERE dst = ?1 AND currency = ?2 AND memo LIKE '% raided %' AND at > ?3",
                    params![m as i64, AUTH, t - 86_400],
                    |r| r.get(0),
                )
                .map_err(oops)?;
            if paid < RAID_REWARDS_PER_DAY {
                mint(db, m, AUTH, RAID_REWARD, &format!("{pname} raided {wname}")).map_err(oops)?;
            }
            give(db, m, "trophy", &format!("Raided {wname}"), Some(world), Some(party), json!({ "party": pname, "finished": t, "with": members.len() }))?;
        }
    }
    Ok(finished)
}

/// What a world server enforces and shows: who may build where, and the portals players
/// opened (in the level format, `world/src/portals.rs`).
pub(crate) async fn rules(State(d): State<Directory>, h: HeaderMap, Path(world): Path<String>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&world)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut st = db.prepare("SELECT id, owner, x0, z0, x1, z1, grp FROM plots WHERE world = ?1").map_err(oops)?;
    let rows: Vec<(i64, i64, i64, i64, i64, i64, Option<i64>)> = st
        .query_map(params![world], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
        .map_err(oops)?
        .flatten()
        .collect();
    let mut plots = Vec::new();
    for (pid, owner, x0, z0, x1, z1, grp) in rows {
        let mut builders = vec![owner];
        if let Some(g) = grp {
            let members = groups.entry(g).or_insert_with(|| {
                db.prepare("SELECT account FROM crew_members WHERE crew = ?1")
                    .and_then(|mut st| st.query_map(params![g], |r| r.get(0)).map(|rows| rows.flatten().collect()))
                    .unwrap_or_default()
            });
            builders.extend(members.iter().filter(|m| **m != owner));
        }
        plots.push(json!({ "id": pid, "x0": x0, "z0": z0, "x1": x1, "z1": z1, "builders": builders }));
    }
    let mut st = db.prepare("SELECT p.id, p.x, p.y, p.z, p.axis, p.dest, s.level, s.spawn FROM portals p JOIN world_state s ON s.world = p.dest WHERE p.world = ?1").map_err(oops)?;
    let portals: Vec<Value> = st
        .query_map(params![world], |r| {
            Ok((r.get::<_, i64>(0)?, [r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?], r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?))
        })
        .map_err(oops)?
        .flatten()
        .filter_map(|(pid, at, axis, dest, level, spawn)| {
            let w = d.0.worlds.iter().find(|w| w.id == dest)?;
            let arrive: Value = serde_json::from_str(&spawn).ok()?;
            Some(json!({
                "name": format!("{} ({pid})", w.name), "at": at, "axis": axis, "size": [4, 6],
                "level": level, "url": w.url, "arrive": arrive, "yaw": 0,
            }))
        })
        .collect();
    Ok(Json(json!({ "plots": plots, "portals": portals })))
}
