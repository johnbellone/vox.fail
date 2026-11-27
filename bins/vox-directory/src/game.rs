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
//!   their world. When a player goes through a portal to another world, their world asks for a
//!   **ticket** (`/api/world/travel`): it lets them into that world only, once, within a minute,
//!   at the arrival point the world they left worked out.
//! - **Pairs**: a player portal may lead to another of its owner's portals (`pair`), in any
//!   world; the pair is two-way, so pairing to a portal that has no live pair (it leads to a
//!   world's spawn, or to a portal since gone) turns that portal around to lead back. A portal
//!   already paired to a live portal is left alone. A paired portal arrives at its other end;
//!   a gone end, at the spawn of the world it leads to.

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
/// A world's portal request id is remembered this long (a repeat after that charges again).
const REQUESTS_KEPT: u64 = 24 * 3600;
/// A portal can be taken back this soon after it was made.
const REFUND_WITHIN: u64 = 60;
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

    pub(crate) fn world(&self, id: &str) -> Res<&crate::WorldInfo> {
        self.0.worlds.iter().find(|w| w.id == id).ok_or_else(|| bad("there's no such world"))
    }

    /// A world of this directory, or one of its instances: who may report and ask for tickets.
    pub(crate) fn known_world(&self, id: &str) -> Res<()> {
        if self.0.worlds.iter().any(|w| w.id == id) || self.tile(id) {
            return Ok(());
        }
        let db = self.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let found = db.query_row("SELECT 1 FROM instances WHERE id = ?1", params![id], |_| Ok(())).optional().map_err(oops)?;
        found.ok_or_else(|| bad("there's no such world"))
    }

    /// Is `id` a tile of one of this directory's atlases (`atlas-earth-3-7`)?
    fn tile(&self, id: &str) -> bool {
        self.0.atlases.iter().any(|(name, size)| vox_core::terrain::Tile::from_id(id, name, 0, *size).is_some())
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
    // a partner elsewhere is left with no live pair (it leads to its world's spawn)
    tx.execute("UPDATE portals SET dest_key = NULL WHERE dest_key IN (SELECT id FROM portals WHERE plot = ?1)", params![r.plot]).map_err(oops)?;
    tx.execute("DELETE FROM portals WHERE plot = ?1", params![r.plot]).map_err(oops)?;
    tx.execute("DELETE FROM items WHERE kind = 'deed' AND ref = ?1", params![r.plot]).map_err(oops)?;
    tx.execute("DELETE FROM plots WHERE id = ?1", params![r.plot]).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "released": r.plot })))
}

/// A portal's name as the rules give it: its destination's name, cut so that its key (`" (7)"`)
/// still fits in `MAX_NAME` characters.
fn portal_name(key: i64, dest_name: &str) -> String {
    let tag = format!(" ({key})");
    let room = vox_core::entity::MAX_NAME.saturating_sub(tag.chars().count());
    dest_name.chars().take(room).chain(tag.chars()).collect()
}

/// The world and place of a portal that a new one may pair with. It must exist (a refunded or
/// removed key is 404, as is any key the directory never issued) and belong to `account`
/// (another player's is 403; a pair is one owner's two ends).
fn pair_end(db: &Connection, account: u64, key: i64) -> Res<(String, [f64; 3])> {
    let row: Option<(i64, String, f64, f64, f64)> = db
        .query_row("SELECT owner, world, x, y, z FROM portals WHERE id = ?1", params![key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .optional()
        .map_err(oops)?;
    let Some((owner, world, x, y, z)) = row else { return Err(no(StatusCode::NOT_FOUND, "that portal is gone")) };
    if owner != account as i64 {
        return Err(no(StatusCode::FORBIDDEN, "that portal is another player's"));
    }
    Ok((world, [x, y, z]))
}

/// Turns the other end of a pair to lead back to `world`, through `key`, unless it already has a
/// live pair. A portal with no pair, or whose `dest_key` names a portal that is gone, has no live
/// pair (it leads to a world's spawn), so it is turned around; one paired to a live portal is
/// left as it is.
fn link_back(db: &Connection, other: i64, world: &str, key: i64) -> Res<()> {
    db.execute(
        "UPDATE portals SET dest = ?1, dest_key = ?2 WHERE id = ?3
         AND (dest_key IS NULL OR dest_key NOT IN (SELECT id FROM portals))",
        params![world, key, other],
    )
    .map_err(oops)?;
    Ok(())
}

/// The next portal key. `portals.id` alone would reuse the highest key once its row is deleted
/// (SQLite without AUTOINCREMENT), and a refunded key still names the request that made it, so
/// the last key issued is kept in `counters` too. Call this inside the transaction that inserts.
fn next_portal_key(db: &Connection) -> Res<i64> {
    let issued: i64 = db
        .query_row("SELECT COALESCE((SELECT value FROM counters WHERE name = 'portal'), 0)", [], |r| r.get(0))
        .map_err(oops)?;
    let live: i64 = db.query_row("SELECT COALESCE(MAX(id), 0) FROM portals", [], |r| r.get(0)).map_err(oops)?;
    Ok(issued.max(live) + 1)
}

/// A world's own request id for a portal (`Idem`): 1 to 64 letters, digits and `._:-`.
fn idem_ok(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// A player portal's doorway as the rules show it when a request gives none: 4 wide, 6 high,
/// facing along the wall's own direction (yaw 0).
const DEFAULT_SIZE: [f64; 2] = [4.0, 6.0];
const DEFAULT_YAW: f64 = 0.0;

/// A doorway's size is 4 to 8 wide and 6 to 16 high (the editor's limits), with finite numbers.
fn size_ok(s: [f32; 2]) -> bool {
    s.iter().all(|v| v.is_finite()) && (4.0..=8.0).contains(&s[0]) && (6.0..=16.0).contains(&s[1])
}

/// Where a player portal goes: its plot (already checked to be the account's), the world, the
/// wall's axis, the doorway's place, its size and its turn (degrees).
pub(crate) struct Site<'a> {
    world: &'a str,
    plot: i64,
    axis: &'a str,
    at: [f64; 3],
    size: [f64; 2],
    yaw: f64,
}

/// The account's plot in `world` that holds the point `at` (x and z floored), if any.
fn plot_at(db: &Connection, world: &str, account: u64, at: [f64; 3]) -> Res<Option<i64>> {
    db.query_row(
        "SELECT id FROM plots WHERE world = ?1 AND owner = ?2 AND x0 <= ?3 AND ?3 < x1 AND z0 <= ?4 AND ?4 < z1",
        params![world, account as i64, at[0].floor() as i64, at[2].floor() as i64],
        |r| r.get(0),
    )
    .optional()
    .map_err(oops)
}

/// Deletes a player portal and the pointers at it: its key item goes, and a partner is left with
/// no live pair (it leads to its world's spawn). No Authority comes back; the callers decide that.
fn drop_portal(db: &Connection, account: u64, key: i64) -> Res<()> {
    db.execute("DELETE FROM items WHERE kind = 'key' AND ref = ?1 AND account = ?2", params![key, account as i64]).map_err(oops)?;
    db.execute("UPDATE portals SET dest_key = NULL WHERE dest_key = ?1", params![key]).map_err(oops)?;
    db.execute("DELETE FROM portals WHERE id = ?1", params![key]).map_err(oops)?;
    Ok(())
}

/// A portal as the rules and the answers describe it.
pub(crate) struct Made {
    key: i64,
    at: [f64; 3],
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct OpenPortal {
    world: String,
    to: String,
}

#[derive(Deserialize)]
pub(crate) struct WorldPortal {
    world: String,
    account: u64,
    at: [f32; 3],
    axis: String,
    /// the doorway's width and height (4 to 8, 6 to 16); 4 by 6 when the caller gives none
    #[serde(default)]
    size: Option<[f32; 2]>,
    /// the doorway's turn in degrees; 0 when the caller gives none
    #[serde(default)]
    yaw: Option<f32>,
    /// the world it leads to (one of `to` and `pair` is given)
    #[serde(default)]
    to: Option<String>,
    /// or the key of the other end of a pair, one of the account's portals
    #[serde(default)]
    pair: Option<i64>,
    idem: String,
}

/// A player moving or turning its own portal (`/api/world/portal/update`): where it stands now
/// and how it is shaped. The plot it stands in may change, to another of the account's.
#[derive(Deserialize)]
pub(crate) struct UpdatePortal {
    world: String,
    account: u64,
    key: i64,
    at: [f32; 3],
    axis: String,
    size: [f32; 2],
    yaw: f32,
}

/// Removing one of the account's own portals (`/api/world/portal/remove`). Nothing is refunded.
#[derive(Deserialize)]
pub(crate) struct RemovePortal {
    world: String,
    account: u64,
    key: i64,
}

#[derive(Deserialize)]
pub(crate) struct LinkPortal {
    world: String,
    account: u64,
    key: i64,
    /// the portal to pair with, or none to lead to the destination world's spawn again
    pair: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct RefundPortal {
    world: String,
    account: u64,
    key: i64,
}

impl Directory {
    /// Makes a player portal: charges Authority, enforces the cap, records the row and gives the
    /// key. The caller has checked that `site.plot` is the account's. With `idem`, the request
    /// is recorded under that id in the same transaction, so `replay` can answer a repeat.
    /// With `pair`, the portal leads to that portal's world. A pair is two-way: a portal with no
    /// live pair (it leads to a world's spawn, or to a portal since gone) is turned to lead back
    /// here; a portal already paired to a live portal is left alone.
    pub(crate) fn make_portal(&self, db: &Connection, account: u64, site: Site, to: &str, pair: Option<i64>, idem: Option<&str>) -> Res<Made> {
        let to: String = match pair {
            Some(key) => pair_end(db, account, key)?.0,
            None => to.to_string(),
        };
        if pair.is_none() && site.world == to {
            return Err(bad("a portal leads to another world"));
        }
        let to = to.as_str();
        let there = self.world(to)?.name.clone();
        let here = self.world_name(site.world);
        if spawn_of(db, to).is_none() {
            return Err(bad(format!("{there} hasn't been opened yet: try again once it's running")));
        }
        let count: i64 = db.query_row("SELECT COUNT(*) FROM portals WHERE plot = ?1", params![site.plot], |r| r.get(0)).map_err(oops)?;
        if count >= PORTALS_PER_PLOT {
            return Err(bad(format!("a plot holds {PORTALS_PER_PLOT} portals")));
        }
        spend(db, account, AUTH, COST_PORTAL, &format!("opened a portal from {here} to {there}"))?;
        let key = next_portal_key(db)?;
        db.execute(
            "INSERT INTO portals (id, world, owner, plot, x, y, z, axis, dest, dest_key, created, size_w, size_h, yaw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                key,
                site.world,
                account as i64,
                site.plot,
                site.at[0],
                site.at[1],
                site.at[2],
                site.axis,
                to,
                pair,
                now() as i64,
                site.size[0],
                site.size[1],
                site.yaw
            ],
        )
        .map_err(oops)?;
        if let Some(other) = pair {
            link_back(db, other, site.world, key)?;
        }
        db.execute("INSERT OR REPLACE INTO counters (name, value) VALUES ('portal', ?1)", params![key]).map_err(oops)?;
        give(db, account, "key", &format!("Portal to {there}"), Some(site.world), Some(key), json!({ "at": site.at, "to": to, "to_name": there }))?;
        if let Some(idem) = idem {
            db.execute(
                "INSERT INTO portal_requests (idem, world, account, portal, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![idem, site.world, account as i64, key, now() as i64],
            )
            .map_err(oops)?;
        }
        Ok(Made { key, at: site.at, name: portal_name(key, &there) })
    }

    /// The answer to a request already made under `idem`, if there was one. A world's request
    /// id used by another world or account is refused; a portal since refunded is a conflict.
    fn replay(&self, db: &Connection, idem: &str, world: &str, account: u64) -> Res<Option<Made>> {
        let row: Option<(String, i64, i64)> = db
            .query_row("SELECT world, account, portal FROM portal_requests WHERE idem = ?1", params![idem], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()
            .map_err(oops)?;
        let Some((w, a, key)) = row else { return Ok(None) };
        if w != world || a != account as i64 {
            return Err(no(StatusCode::CONFLICT, "that request id belongs to another request"));
        }
        let portal: Option<(f64, f64, f64, String)> = db
            .query_row("SELECT x, y, z, dest FROM portals WHERE id = ?1 AND world = ?2 AND owner = ?3", params![key, world, account as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()
            .map_err(oops)?;
        let Some((x, y, z, dest)) = portal else { return Err(no(StatusCode::CONFLICT, "that portal was refunded")) };
        Ok(Some(Made { key, at: [x, y, z], name: portal_name(key, &self.world_name(&dest)) }))
    }
}

pub(crate) async fn open_portal(State(d): State<Directory>, h: HeaderMap, Json(o): Json<OpenPortal>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.who(&h)?;
    d.world(&o.world)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
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
    // the doorway's sill: where you stood, on the half block
    let at = [(x * 2.0).round() / 2.0, y.floor(), (z * 2.0).round() / 2.0];
    let site = Site { world: &o.world, plot, axis: "z", at, size: DEFAULT_SIZE, yaw: DEFAULT_YAW };
    let made = d.make_portal(&tx, id, site, &o.to, None, None)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "portal": made.key, "at": made.at })))
}

/// A world server's request, made when a player places a portal (its Authority is charged here).
/// Repeats under the same `idem` answer the first one and charge nothing.
pub(crate) async fn world_portal(State(d): State<Directory>, h: HeaderMap, Json(p): Json<WorldPortal>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&p.world)?;
    if !idem_ok(&p.idem) {
        return Err(bad("a request id is 1 to 64 letters, digits and . _ : -"));
    }
    if !p.at.iter().all(|v| v.is_finite()) {
        return Err(bad("nowhere to put that portal"));
    }
    if p.axis != "x" && p.axis != "z" {
        return Err(bad("a portal's wall is along x or z"));
    }
    if p.to.is_some() == p.pair.is_some() {
        return Err(bad("a portal leads to a world (to) or pairs with a portal (pair), not both or neither"));
    }
    let size = p.size.unwrap_or([DEFAULT_SIZE[0] as f32, DEFAULT_SIZE[1] as f32]);
    if !size_ok(size) {
        return Err(bad("a portal is 4 to 8 wide and 6 to 16 high"));
    }
    let yaw = p.yaw.unwrap_or(DEFAULT_YAW as f32);
    if !yaw.is_finite() {
        return Err(bad("a portal's turn is a finite number of degrees"));
    }
    let to = p.to.clone().unwrap_or_default();
    let at = p.at.map(f64::from);
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    if let Some(made) = d.replay(&tx, &p.idem, &p.world, p.account)? {
        return Ok(Json(json!({ "key": made.key, "at": made.at, "name": made.name })));
    }
    tx.execute("DELETE FROM portal_requests WHERE created < ?1", params![now().saturating_sub(REQUESTS_KEPT) as i64]).map_err(oops)?;
    let plot = plot_at(&tx, &p.world, p.account, at)?.ok_or_else(|| bad("that portal's place is not in a plot of the player's in that world"))?;
    let site = Site { world: &p.world, plot, axis: &p.axis, at, size: size.map(f64::from), yaw: f64::from(yaw) };
    let made = d.make_portal(&tx, p.account, site, &to, p.pair, Some(&p.idem))?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "key": made.key, "at": made.at, "name": made.name })))
}

/// A world server's request to re-point one of a player's portals in that world: at another of
/// the player's portals (a pair, two-way as in `make_portal`), or back to its destination world's
/// spawn (`pair` none). Free. Re-pointing or unlinking takes the old partner's pointer with it:
/// the partner still leads to the world this portal led to, and then to that world's spawn.
pub(crate) async fn link_portal(State(d): State<Directory>, h: HeaderMap, Json(l): Json<LinkPortal>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&l.world)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let here: Option<String> = tx
        .query_row("SELECT world FROM portals WHERE id = ?1 AND owner = ?2", params![l.key, l.account as i64], |r| r.get(0))
        .optional()
        .map_err(oops)?;
    if here.as_deref() != Some(l.world.as_str()) {
        return Err(no(StatusCode::NOT_FOUND, "that portal is gone"));
    }
    if l.pair == Some(l.key) {
        return Err(bad("a portal can't pair with itself"));
    }
    let other = match l.pair {
        Some(k) => Some((k, pair_end(&tx, l.account, k)?.0)),
        None => None,
    };
    tx.execute("UPDATE portals SET dest_key = NULL WHERE dest_key = ?1 AND id != ?1", params![l.key]).map_err(oops)?;
    match other {
        Some((k, world)) => {
            tx.execute("UPDATE portals SET dest = ?1, dest_key = ?2 WHERE id = ?3", params![world, k, l.key]).map_err(oops)?;
            link_back(&tx, k, &l.world, l.key)?;
        }
        None => {
            tx.execute("UPDATE portals SET dest_key = NULL WHERE id = ?1", params![l.key]).map_err(oops)?;
        }
    }
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "key": l.key, "pair": l.pair })))
}

/// A player moves, turns or reshapes one of their own portals in a world (`free`: no Authority).
/// The new place must be in one of the account's plots in that world; when that is a different
/// plot from the portal's own, the cap of portals per plot applies to the new one. Partners see
/// the new place on their next rules fetch, since they arrive at this row.
pub(crate) async fn update_portal(State(d): State<Directory>, h: HeaderMap, Json(u): Json<UpdatePortal>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&u.world)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let row: Option<(i64, i64, String)> = tx
        .query_row("SELECT owner, plot, world FROM portals WHERE id = ?1", params![u.key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .map_err(oops)?;
    let Some((owner, plot, world)) = row else { return Err(no(StatusCode::NOT_FOUND, "that portal is gone")) };
    if owner != u.account as i64 {
        return Err(no(StatusCode::FORBIDDEN, "that portal is another player's"));
    }
    if world != u.world {
        return Err(no(StatusCode::NOT_FOUND, "that portal is gone"));
    }
    if !u.at.iter().all(|v| v.is_finite()) {
        return Err(bad("nowhere to put that portal"));
    }
    if u.axis != "x" && u.axis != "z" {
        return Err(bad("a portal's wall is along x or z"));
    }
    if !size_ok(u.size) {
        return Err(bad("a portal is 4 to 8 wide and 6 to 16 high"));
    }
    if !u.yaw.is_finite() {
        return Err(bad("a portal's turn is a finite number of degrees"));
    }
    let at = u.at.map(f64::from);
    let target = plot_at(&tx, &u.world, u.account, at)?.ok_or_else(|| no(StatusCode::FORBIDDEN, "that place is not in a plot of yours in that world"))?;
    if target != plot {
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM portals WHERE plot = ?1", params![target], |r| r.get(0)).map_err(oops)?;
        if count >= PORTALS_PER_PLOT {
            return Err(bad(format!("a plot holds {PORTALS_PER_PLOT} portals")));
        }
    }
    tx.execute(
        "UPDATE portals SET x = ?1, y = ?2, z = ?3, axis = ?4, plot = ?5, size_w = ?6, size_h = ?7, yaw = ?8 WHERE id = ?9",
        params![at[0], at[1], at[2], u.axis, target, f64::from(u.size[0]), f64::from(u.size[1]), f64::from(u.yaw), u.key],
    )
    .map_err(oops)?;
    // the key item says where it opens, so it follows the portal
    let items: Vec<(i64, String)> = {
        let mut st = tx.prepare("SELECT id, detail FROM items WHERE kind = 'key' AND ref = ?1").map_err(oops)?;
        st.query_map(params![u.key], |r| Ok((r.get(0)?, r.get(1)?))).map_err(oops)?.flatten().collect()
    };
    for (id, detail) in items {
        let mut v: Value = serde_json::from_str(&detail).unwrap_or_else(|_| json!({}));
        v["at"] = json!(at);
        tx.execute("UPDATE items SET detail = ?1 WHERE id = ?2", params![v.to_string(), id]).map_err(oops)?;
    }
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "key": u.key, "at": at })))
}

/// Removes one of a player's own portals in a world: the portal, its key and the pointers at it
/// go, and nothing is refunded (a portal taken back within a minute is `refund_portal`'s). A
/// portal that is someone else's, or gone, is `removed: false`.
pub(crate) async fn remove_portal(State(d): State<Directory>, h: HeaderMap, Json(r): Json<RemovePortal>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&r.world)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let mine = tx
        .query_row("SELECT 1 FROM portals WHERE id = ?1 AND world = ?2 AND owner = ?3", params![r.key, r.world, r.account as i64], |_| Ok(()))
        .optional()
        .map_err(oops)?
        .is_some();
    if !mine {
        return Ok(Json(json!({ "removed": false })));
    }
    drop_portal(&tx, r.account, r.key)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "removed": true })))
}

/// Takes back a portal a world made a moment ago (its plot or rights went before the answer
/// came): the portal and its key go, and the Authority comes back. Only the account that made it,
/// in the world that made it, and only within `REFUND_WITHIN` seconds. The request id stays
/// behind, so a repeat of it is still answered (409) and charges nothing.
pub(crate) async fn refund_portal(State(d): State<Directory>, h: HeaderMap, Json(r): Json<RefundPortal>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.world(&r.world)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    // any portal of this account in this world made in the last minute may go: the world secret's holder is trusted to say which
    let recent = now().saturating_sub(REFUND_WITHIN) as i64;
    let mine = tx
        .query_row(
            "SELECT 1 FROM portals WHERE id = ?1 AND world = ?2 AND owner = ?3 AND created >= ?4",
            params![r.key, r.world, r.account as i64, recent],
            |_| Ok(()),
        )
        .optional()
        .map_err(oops)?
        .is_some();
    if !mine {
        return Ok(Json(json!({ "refunded": false })));
    }
    drop_portal(&tx, r.account, r.key)?;
    mint(&tx, r.account, AUTH, COST_PORTAL, &format!("portal {} placed and then refused: refunded", r.key)).map_err(oops)?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "refunded": true })))
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
    // a world, or a dungeon (`dungeon:crypt`), whose raid is won by reaching its objective
    let wname = match s.world.strip_prefix(DUNGEON) {
        Some(t) if dungeon_name(t) && d.0.instance_host.is_some() => target_name(&d, &s.world),
        Some(_) => return Err(bad("there's no such dungeon")),
        None => d.world(&s.world)?.name.clone(),
    };
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
    d.known_world(&r.world)?;
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
        finish_raid(db, party, &pname, world, wname, &present, members.len())?;
        finished += 1;
    }
    Ok(finished)
}

/// A raid is won: it's done, and everyone of the party who was `present` gets the reward (up to
/// `RAID_REWARDS_PER_DAY` a day) and a trophy.
fn finish_raid(db: &Connection, party: i64, pname: &str, world: &str, wname: &str, present: &[u64], members: usize) -> Res<()> {
    let t = now() as i64;
    db.execute("UPDATE crews SET done = 1 WHERE id = ?1", params![party]).map_err(oops)?;
    for &m in present {
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
        give(db, m, "trophy", &format!("Raided {wname}"), Some(world), Some(party), json!({ "party": pname, "finished": t, "with": members }))?;
    }
    Ok(())
}

/// What a raid's target is called: a world's name, or "the Crypt" for `dungeon:crypt`.
fn target_name(d: &Directory, target: &str) -> String {
    match target.strip_prefix(DUNGEON) {
        Some(t) => {
            let mut c = t.replace(['-', '_'], " ").chars().collect::<Vec<_>>();
            if let Some(f) = c.first_mut() {
                *f = f.to_ascii_uppercase();
            }
            format!("the {}", c.into_iter().collect::<String>())
        }
        None => d.world_name(target),
    }
}

/// Is this a dungeon's name (`crypt`, as in `dungeon:crypt`)?
fn dungeon_name(t: &str) -> bool {
    !t.is_empty() && t.len() <= 32 && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

#[derive(Deserialize)]
pub struct Reached {
    /// the instance whose objective was reached
    pub world: String,
    /// who was in it then (signed-in players)
    pub players: Vec<u64>,
}

/// An instance's objective was reached (its world server says so): a raid on that dungeon by
/// the instance's party is won, by those of the party who were there.
pub(crate) async fn objective(State(d): State<Directory>, h: HeaderMap, Json(r): Json<Reached>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    let mut db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    let tx = db.transaction().map_err(oops)?;
    let (party, template): (i64, String) = tx
        .query_row("SELECT party, template FROM instances WHERE id = ?1", params![r.world], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()
        .map_err(oops)?
        .ok_or_else(|| bad("objectives are for dungeons' instances"))?;
    let target = format!("{DUNGEON}{template}");
    let raid: Option<String> = tx
        .query_row(
            "SELECT name FROM crews WHERE id = ?1 AND kind = 'party' AND target = ?2 AND done = 0 AND ends > ?3",
            params![party, target, now() as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(oops)?;
    let Some(pname) = raid else { return Ok(Json(json!({ "raid": false }))) };
    let members: Vec<u64> = {
        let mut st = tx.prepare("SELECT account FROM crew_members WHERE crew = ?1").map_err(oops)?;
        st.query_map(params![party], |row| row.get::<_, i64>(0)).map_err(oops)?.flatten().map(|a| a as u64).collect()
    };
    let mut present: Vec<u64> = r.players.iter().copied().filter(|p| members.contains(p)).collect();
    present.sort_unstable();
    present.dedup();
    if present.is_empty() {
        return Ok(Json(json!({ "raid": false })));
    }
    let tname = target_name(&d, &target);
    finish_raid(&tx, party, &pname, &target, &tname, &present, members.len())?;
    tx.commit().map_err(oops)?;
    Ok(Json(json!({ "raid": true, "won_by": present })))
}

/// What a world server enforces and shows: who may build where, and the portals players
/// opened (in the level format, `world/src/portals.rs`).
pub(crate) async fn rules(State(d): State<Directory>, h: HeaderMap, Path(world): Path<String>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.known_world(&world)?;
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
    // a paired portal arrives at its other end (when that still exists) and names it as `pair`;
    // otherwise it arrives at its destination world's spawn
    let mut st = db
        .prepare(
            "SELECT p.id, p.x, p.y, p.z, p.axis, p.dest, s.level, s.spawn, p.dest_key, o.x, o.y, o.z,
                    COALESCE(p.size_w, 4.0), COALESCE(p.size_h, 6.0), COALESCE(p.yaw, 0.0)
             FROM portals p JOIN world_state s ON s.world = p.dest
             LEFT JOIN portals o ON o.id = p.dest_key AND o.world = p.dest
             WHERE p.world = ?1",
        )
        .map_err(oops)?;
    let portals: Vec<Value> = st
        .query_map(params![world], |r| {
            let other: Option<[f64; 3]> = match (r.get::<_, Option<f64>>(9)?, r.get::<_, Option<f64>>(10)?, r.get::<_, Option<f64>>(11)?) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            };
            Ok((
                r.get::<_, i64>(0)?,
                [r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?],
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<i64>>(8)?,
                other,
                [r.get::<_, f64>(12)?, r.get::<_, f64>(13)?],
                r.get::<_, f64>(14)?,
            ))
        })
        .map_err(oops)?
        .flatten()
        .filter_map(|(pid, at, axis, dest, level, spawn, dest_key, other, size, yaw)| {
            let w = d.0.worlds.iter().find(|w| w.id == dest)?;
            // a portal's name is at most MAX_NAME characters (the server drops a longer one), so
            // the world's name is cut to fit beside " (pid)"
            let name = portal_name(pid, &w.name);
            let mut entry = json!({
                "key": pid, "name": name, "at": at, "axis": axis, "size": size,
                "level": level, "url": w.url, "yaw": yaw,
            });
            match (dest_key, other) {
                (Some(pair), Some(end)) => {
                    entry["pair"] = json!(pair);
                    entry["arrive"] = json!(end);
                }
                _ => {
                    entry["arrive"] = serde_json::from_str::<Value>(&spawn).ok()?;
                }
            }
            Some(entry)
        })
        .collect();
    // Raid parties with two or more members, so a world can let them talk to each other. The
    // caps keep one large or spammed table from making this response unbounded. Newest first
    // (`c.id DESC`), so when there are more than the cap the newest parties are the ones kept.
    // Finished parties are not filtered out: a finished party can still talk.
    let mut st = db
        .prepare(
            "SELECT m.crew, m.account FROM crew_members m WHERE m.crew IN (
                 SELECT c.id FROM crews c JOIN crew_members n ON n.crew = c.id
                 WHERE c.kind = 'party' GROUP BY c.id HAVING COUNT(*) >= 2 ORDER BY c.id DESC LIMIT ?1)
             ORDER BY m.crew, m.joined, m.account",
        )
        .map_err(oops)?;
    let mut parties: Vec<Vec<i64>> = Vec::new();
    let mut last = None;
    for row in st.query_map(params![PARTIES_MAX as i64], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(oops)?.flatten() {
        if last != Some(row.0) {
            parties.push(Vec::new());
            last = Some(row.0);
        }
        if let Some(p) = parties.last_mut().filter(|p| p.len() < PARTY_MEMBERS_MAX) {
            p.push(row.1);
        }
    }
    // who may edit the world in the advanced editor (see `editors`)
    let owner: Option<u64> = crate::editors::owner_of(&db, &world)?;
    let editors = crate::editors::editor_ids(&db, &world)?;
    Ok(Json(json!({ "plots": plots, "portals": portals, "parties": parties, "owner": owner, "editors": editors })))
}

// ---- tickets

#[derive(Deserialize)]
pub struct Trip {
    /// the world they're leaving (its id)
    pub world: String,
    pub account: u64,
    /// where the portal leads: its url as the level gives it (`":7781"`, `"/w/7781/ws"`, a
    /// whole address) or a world's id
    pub to: String,
    /// where they arrive there, blocks
    pub arrive: [f32; 3],
}

/// What tells worlds apart in an address: the `<x>` of `/w/<x>/`, else the port. Two addresses
/// for the same world (`":7781"` from a level, `"/w/7781/ws"` on the directory's list) agree.
pub fn way(url: &str) -> Option<String> {
    let u = url.trim();
    if let Some(i) = u.find("/w/") {
        let rest = &u[i + 3..];
        let seg = rest.split('/').next().unwrap_or("");
        return (!seg.is_empty()).then(|| seg.to_string());
    }
    let authority = u.split_once("://").map_or(u, |(_, r)| r).split('/').next().unwrap_or("");
    let port = authority.rsplit_once(':').map(|(_, p)| p)?;
    (!port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())).then(|| port.to_string())
}

/// A ticket for a player going through a portal (see the module's docs). Asked by the world
/// they're leaving, which checked they were in the doorway; answered with the world it leads
/// to, its address and the ticket.
pub(crate) async fn travel(State(d): State<Directory>, h: HeaderMap, Json(t): Json<Trip>) -> Res<Json<Value>> {
    d.from_world(&h)?;
    d.known_world(&t.world)?;
    if !t.arrive.iter().all(|v| v.is_finite()) {
        return Err(bad("nowhere to arrive"));
    }
    let (to_id, to_url) = match t.to.strip_prefix(DUNGEON) {
        Some(template) => {
            let id = instance_for(&d, t.account, template).await?;
            (id.clone(), format!("/w/{id}/ws"))
        }
        // a tile of an atlas (over its edge): known by its name, not listed
        None if way(&t.to).is_some_and(|w| d.tile(&w)) => {
            let id = way(&t.to).unwrap_or_default();
            (id.clone(), format!("/w/{id}/ws"))
        }
        None => {
            let w = d
                .0
                .worlds
                .iter()
                .find(|w| w.id == t.to || (way(&t.to).is_some() && way(&t.to) == way(&w.url)))
                .ok_or_else(|| no(StatusCode::NOT_FOUND, "that portal leads to no world of this directory"))?;
            (w.id.clone(), w.url.clone())
        }
    };
    let name: String = {
        let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT name FROM accounts WHERE id = ?1", params![t.account as i64], |r| r.get(0))
            .optional()
            .map_err(oops)?
            .ok_or_else(|| bad("there's no such account"))?
    };
    let character = d.character(t.account).unwrap_or_default();
    let now = now();
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(oops)?;
    let ticket = vox_core::auth::Ticket {
        claims: vox_core::auth::Claims { account: t.account, name, character, issued: now, expires: now + vox_core::auth::TICKET_LIFETIME },
        world: to_id.clone(),
        arrive: t.arrive,
        nonce: u64::from_le_bytes(nonce),
    };
    Ok(Json(json!({ "world": to_id, "url": to_url, "ticket": vox_core::auth::sign_ticket(&ticket, &d.0.key) })))
}

// ---- instances

/// A portal whose url is `dungeon:<name>` leads to the party's own copy of that dungeon.
pub const DUNGEON: &str = "dungeon:";
/// How long an instance lasts, s: as long as a raid.
pub const INSTANCE_LENGTH: u64 = RAID_LENGTH;
/// Copies of dungeons a party may have made in an hour, new or afresh (so it can't roll copies,
/// reset one by leaving it, or fill the host).
pub const INSTANCES_PER_HOUR: i64 = 3;

/// The instance of dungeon `template` for `account`'s party: the one it has (made again on the
/// host if the host has let it go), or a new one, if the party may have another.
async fn instance_for(d: &Directory, account: u64, template: &str) -> Res<String> {
    if !dungeon_name(template) {
        return Err(bad("no such dungeon"));
    }
    let Some(host) = d.0.instance_host.clone() else { return Err(no(StatusCode::NOT_FOUND, "there are no dungeons here")) };
    let Some(secret) = d.0.world_secret.clone() else { return Err(no(StatusCode::NOT_FOUND, "there are no dungeons here")) };
    let t = now() as i64;
    let (party, id, ends, new) = {
        let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let party: i64 = db
            .query_row(
                "SELECT c.id FROM crews c JOIN crew_members m ON m.crew = c.id WHERE m.account = ?1 AND c.kind = 'party' ORDER BY m.joined DESC LIMIT 1",
                params![account as i64],
                |r| r.get(0),
            )
            .optional()
            .map_err(oops)?
            .ok_or_else(|| no(StatusCode::FORBIDDEN, "dungeons are for raid parties: go with yours"))?;
        let have: Option<(String, i64)> = db
            .query_row(
                "SELECT id, ends FROM instances WHERE party = ?1 AND template = ?2 AND ends > ?3 ORDER BY created DESC LIMIT 1",
                params![party, template, t],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(oops)?;
        match have {
            Some((id, ends)) => (party, id, ends, false),
            None => {
                let mut b = [0u8; 6];
                getrandom::fill(&mut b).map_err(oops)?;
                let id = format!("{template}-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>());
                (party, id, t + INSTANCE_LENGTH as i64, true)
            }
        }
    };
    let minutes = ((ends - t).max(60) / 60) as u32;
    let ask = |make: bool| {
        d.0.http
            .post(format!("{host}/api/host/instance"))
            .bearer_auth(&secret)
            .json(&json!({ "id": id, "template": template, "minutes": minutes, "make": make }))
            .send()
    };
    // one it has: is the host still holding it? (an instance left empty is thrown away)
    if !new {
        let r = ask(false).await.map_err(|_| no(StatusCode::SERVICE_UNAVAILABLE, "the dungeons can't be reached just now"))?;
        match r.status().as_u16() {
            200 => return Ok(id),
            410 => {}
            _ => return Err(no(StatusCode::SERVICE_UNAVAILABLE, "that dungeon can't be opened just now")),
        }
    }
    // a copy is made, new or afresh (which is a reset): each counts against the hour's limit
    {
        let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let opened: i64 = db
            .query_row("SELECT count(*) FROM instance_opens WHERE party = ?1 AND at > ?2", params![party, t - 3600], |r| r.get(0))
            .map_err(oops)?;
        if opened >= INSTANCES_PER_HOUR {
            return Err(no(StatusCode::TOO_MANY_REQUESTS, "your party has opened enough dungeons for now: try again later"));
        }
    }
    let r = ask(true).await.map_err(|_| no(StatusCode::SERVICE_UNAVAILABLE, "the dungeons can't be reached just now"))?;
    match r.status().as_u16() {
        200 => {}
        404 => return Err(no(StatusCode::NOT_FOUND, "no such dungeon")),
        _ => return Err(no(StatusCode::SERVICE_UNAVAILABLE, "that dungeon can't be opened just now")),
    }
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    db.execute("INSERT INTO instance_opens (party, at) VALUES (?1, ?2)", params![party, t]).map_err(oops)?;
    if new {
        db.execute("INSERT INTO instances (id, template, party, created, ends) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, template, party, t, ends])
            .map_err(oops)?;
    }
    Ok(id)
}
/// Most raid parties a world's rules list, and most members shown for each.
const PARTIES_MAX: usize = 500;
const PARTY_MEMBERS_MAX: usize = 24;
