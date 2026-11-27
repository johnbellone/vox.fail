//! The world's entities (doorways, spawn, an objective, settings) as the server keeps them:
//! in memory by id, with the ones changed since the last write, and an edit log of who
//! changed what. They are written in the same SQLite transaction as the blocks (see
//! `WorldState::flush`), and only forgotten once that transaction has committed.
//!
//! Rows hold postcard data (`vox_core::wire`), the same encoding the wire and the zones use.
//! Loading checks every row again, so a bad row is skipped rather than trusted.

// `seq_of` has no caller until the entity edits (`ToServer::Put`, `Remove`) are handled
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Transaction, params};
use vox_core::{
    ChunkKey, wire,
    entity::{Component, EntityId, Portal},
    wire::EntityChange,
};

/// The entities of one world, with what is not yet written.
pub(crate) struct Entities {
    pub map: BTreeMap<EntityId, (u64, Component)>,
    /// the id the next new entity gets
    next: u32,
    /// ids changed (or removed) since the last write
    dirty: BTreeSet<EntityId>,
    /// edits since the last write
    log: Vec<EntityEdit>,
    /// set by `seed`, and cleared once the write that records it has committed
    seeded_now: bool,
}

/// One change to an entity: who made it, when, and what it was before and after (None where
/// there was nothing: a new entity has no old, a removed one no new).
pub(crate) struct EntityEdit {
    pub seq: u64,
    pub at: u64,
    pub player: String,
    pub entity: EntityId,
    pub old: Option<Component>,
    pub new: Option<Component>,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entities(id INTEGER PRIMARY KEY, zone_x INTEGER NOT NULL, zone_z INTEGER NOT NULL,
             name TEXT NOT NULL, version INTEGER NOT NULL, seq INTEGER NOT NULL, data BLOB NOT NULL);
         CREATE TABLE IF NOT EXISTS entity_edits(seq INTEGER PRIMARY KEY, at INTEGER NOT NULL, player TEXT NOT NULL,
             entity INTEGER NOT NULL, old BLOB, new BLOB);",
    )
    .map_err(|e| e.to_string())
}

impl Entities {
    pub(crate) fn new() -> Entities {
        Entities { map: BTreeMap::new(), next: 1, dirty: BTreeSet::new(), log: Vec::new(), seeded_now: false }
    }

    /// Reads every entity row. A row that doesn't decode or doesn't validate is skipped and
    /// named on stderr.
    pub(crate) fn load(db: &Connection) -> Result<Entities, String> {
        let e = |e: rusqlite::Error| e.to_string();
        let mut out = Entities::new();
        let mut q = db.prepare("SELECT id, seq, data FROM entities ORDER BY id").map_err(e)?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?)))
            .map_err(e)?;
        for row in rows {
            let (id, seq, data) = row.map_err(e)?;
            match wire::decode::<Component>(&data) {
                Ok(c) => match c.validate() {
                    Ok(()) => {
                        out.map.insert(EntityId(id), (u64::try_from(seq).unwrap_or(0), c));
                    }
                    Err(why) => eprintln!("entity {id} isn't valid, skipped: {why}"),
                },
                Err(why) => eprintln!("entity {id} can't be read, skipped: {why}"),
            }
        }
        out.next = out.map.keys().next_back().map_or(1, |id| id.0.saturating_add(1));
        Ok(out)
    }

    /// Adds the components of a new world (or of a world that has never had entities): ids in
    /// order, all waiting to be written with the `entities_seeded` mark, no log. The caller
    /// decides whether a world is due this (see `WorldState::open`).
    pub(crate) fn seed(&mut self, components: Vec<Component>) {
        for c in components {
            let id = EntityId(self.next);
            self.next = self.next.saturating_add(1);
            self.map.insert(id, (0, c));
            self.dirty.insert(id);
        }
        self.seeded_now = true;
    }

    /// Places `c` as entity `id`, or as a new entity when `id` is None. Returns its id, or None
    /// when a new id is needed and the counter is at u32::MAX: ids are never reused (a stored
    /// entity may hold one), so the last id is never handed out and the caller refuses the
    /// new entity instead.
    pub(crate) fn put(
        &mut self,
        seq: u64,
        at: u64,
        player: String,
        id: Option<EntityId>,
        c: Component,
    ) -> Option<EntityId> {
        let id = match id {
            Some(id) => {
                self.next = self.next.max(id.0.saturating_add(1));
                id
            }
            None => {
                let id = EntityId(self.next);
                self.next = self.next.checked_add(1)?;
                id
            }
        };
        let old = self.map.insert(id, (seq, c.clone())).map(|(_, old)| old);
        self.log.push(EntityEdit { seq, at, player, entity: id, old, new: Some(c) });
        self.dirty.insert(id);
        Some(id)
    }

    /// Removes entity `id`, returning what it was (None if there was no such entity).
    pub(crate) fn remove(&mut self, seq: u64, at: u64, player: String, id: EntityId) -> Option<Component> {
        let (_, old) = self.map.remove(&id)?;
        self.log.push(EntityEdit { seq, at, player, entity: id, old: Some(old.clone()), new: None });
        self.dirty.insert(id);
        Some(old)
    }

    /// The edit sequence number of the last change to entity `id`.
    pub(crate) fn seq_of(&self, id: EntityId) -> Option<u64> {
        self.map.get(&id).map(|(seq, _)| *seq)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The portals that carry a directory key, for the directory's list (`wanted`, one
    /// component per key): the entity holding each wanted key (the first, if there are two),
    /// and the entities to remove (a key nobody wants, or a repeat of one).
    pub(crate) fn keyed_portals(&self, wanted: &BTreeMap<i64, Component>) -> (BTreeMap<i64, EntityId>, Vec<EntityId>) {
        let mut have = BTreeMap::new();
        let mut stale = Vec::new();
        for (&id, (_, c)) in &self.map {
            let Component::Portal(Portal { key: Some(k), .. }) = c else { continue };
            if wanted.contains_key(k) && !have.contains_key(k) {
                have.insert(*k, id);
            } else {
                stale.push(id);
            }
        }
        (have, stale)
    }

    /// Every entity as a change carrying its last edit number, nearest `from` first: the
    /// settings, then the rest by zone (the zone of `from` first) and distance.
    pub(crate) fn all_from(&self, from: [f32; 3]) -> Vec<EntityChange> {
        let (hx, hz) = zone_at(from);
        let mut keyed: Vec<((u64, f32), EntityChange)> = self
            .map
            .iter()
            .map(|(&id, (seq, c))| {
                let rank = match position(c) {
                    None => (0, 0.0),
                    Some(p) => {
                        let (zx, zz) = zone_of(c);
                        let zones = (zx as i64 - hx as i64).unsigned_abs() + (zz as i64 - hz as i64).unsigned_abs();
                        (zones + 1, (0..3).map(|i| (p[i] - from[i]).powi(2)).sum::<f32>())
                    }
                };
                (rank, EntityChange { seq: *seq, entity: id, component: Some(c.clone()) })
            })
            .collect();
        keyed.sort_by(|((za, da), _), ((zb, db), _)| za.cmp(zb).then(da.total_cmp(db)));
        keyed.into_iter().map(|(_, c)| c).collect()
    }

    /// Makes `ents` the whole set (a restore): entities not in it are removed and the rest are
    /// written in the next flush. Ids keep counting up past the highest, so none is reused.
    /// The `entities_seeded` mark is left alone.
    pub(crate) fn replace(&mut self, ents: BTreeMap<EntityId, (u64, Component)>) {
        self.dirty.extend(self.map.keys().copied());
        self.dirty.extend(ents.keys().copied());
        self.next = self.next.max(ents.keys().next_back().map_or(1, |id| id.0.saturating_add(1)));
        self.map = ents;
    }

    /// Applies the entity edits after `after` and at or before `until` (the log, in edit order)
    /// to `base`: the entities as a snapshot holds them. A logged value that doesn't validate
    /// is an error, since the restore could not then be trusted.
    pub(crate) fn replay(db: &Connection, base: &mut BTreeMap<EntityId, (u64, Component)>, after: u64, until: u64) -> Result<usize, String> {
        let e = |e: rusqlite::Error| e.to_string();
        let mut q = db.prepare("SELECT seq, entity, new FROM entity_edits WHERE seq > ?1 AND at <= ?2 ORDER BY seq").map_err(e)?;
        let rows = q
            .query_map(params![after as i64, until as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, u32>(1)?, r.get::<_, Option<Vec<u8>>>(2)?))
            })
            .map_err(e)?;
        let mut applied = 0;
        for row in rows {
            let (seq, id, new) = row.map_err(e)?;
            let seq = u64::try_from(seq).map_err(|_| format!("entity edit {seq}: a bad edit number"))?;
            let id = EntityId(id);
            match new {
                None => {
                    base.remove(&id);
                }
                Some(bytes) => {
                    let c: Component = wire::decode(&bytes).map_err(|why| format!("entity edit {seq}: {why}"))?;
                    c.validate().map_err(|why| format!("entity edit {seq}: {why}"))?;
                    base.insert(id, (seq, c));
                }
            }
            applied += 1;
        }
        Ok(applied)
    }

    /// Whether anything is waiting to be written.
    pub(crate) fn changed(&self) -> bool {
        !self.dirty.is_empty() || !self.log.is_empty() || self.seeded_now
    }

    /// Writes the changed entities (or deletes the removed ones), the edit log and, after a
    /// seed, the `entities_seeded` mark, inside the caller's transaction. The mark and the seed
    /// commit together, so a world is never seeded twice. Nothing is forgotten here: call
    /// `written` after the commit.
    pub(crate) fn write(&self, tx: &Transaction) -> Result<(), String> {
        let e = |e: rusqlite::Error| e.to_string();
        if self.seeded_now {
            tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('entities_seeded', '1')", []).map_err(e)?;
        }
        for &id in &self.dirty {
            match self.map.get(&id) {
                Some((seq, c)) => {
                    let (zone_x, zone_z) = zone_of(c);
                    tx.execute(
                        "INSERT OR REPLACE INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![id.0, zone_x, zone_z, c.name(), c.version(), *seq as i64, wire::encode(c)],
                    )
                    .map_err(e)?;
                }
                None => {
                    tx.execute("DELETE FROM entities WHERE id = ?1", params![id.0]).map_err(e)?;
                }
            }
        }
        for g in &self.log {
            tx.execute(
                "INSERT OR REPLACE INTO entity_edits(seq, at, player, entity, old, new) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    g.seq as i64,
                    g.at as i64,
                    g.player,
                    g.entity.0,
                    g.old.as_ref().map(wire::encode),
                    g.new.as_ref().map(wire::encode)
                ],
            )
            .map_err(e)?;
        }
        Ok(())
    }

    /// Forgets what was written. Call only after the transaction committed, so a failed write
    /// keeps it for the next one.
    pub(crate) fn written(&mut self) {
        self.dirty.clear();
        self.log.clear();
        self.seeded_now = false;
    }
}

/// Where an entity is, in blocks (feet). A settings entity has no place.
pub(crate) fn position(c: &Component) -> Option<[f32; 3]> {
    match c {
        Component::Portal(p) => Some(p.at),
        Component::Spawn(s) => Some(s.at),
        Component::Objective(o) => Some(o.at),
        Component::Settings(_) => None,
    }
}

/// The zone an entity lies in, by its position (a settings entity has none: zone 0, 0).
fn zone_of(c: &Component) -> (i32, i32) {
    position(c).map_or((0, 0), zone_at)
}

/// The zone an entity is written in in a snapshot (see `zone_of`).
pub(crate) fn zone_key(c: &Component) -> vox_format::ZoneKey {
    let (x, z) = zone_of(c);
    vox_format::ZoneKey { x, z }
}

/// The zone holding a position in blocks: the cell it is in, then the chunk and zone holding that cell.
fn zone_at(at: [f32; 3]) -> (i32, i32) {
    let cell = [at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32];
    let zone = vox_format::zone_of(ChunkKey::of(cell));
    (zone.x, zone.z)
}

/// A doorway the directory lists for a world (in the level format, with the directory's `key`):
/// the key and the portal entity holding it. None when there is no key or the doorway is not a
/// valid one. The level format's own reading is used, so a doorway is judged as a level's is.
pub(crate) fn from_directory(v: &serde_json::Value) -> Option<(i64, Component)> {
    let key = v.get("key")?.as_i64()?;
    let text = serde_json::json!({ "world": { "portals": [v] } }).to_string();
    let portal = vox_core::level::entities(&text).ok()?.into_iter().find_map(|c| match c {
        Component::Portal(p) => Some(p),
        _ => None,
    })?;
    Some((key, Component::Portal(Portal { key: Some(key), ..portal })))
}

/// Changes split into `Entities` messages: at most `wire::ENTITIES_PER_MESSAGE` to a message,
/// and none that would encode over `wire::MAX_MESSAGE`. A change's encoded size adds up exactly
/// inside a message; the header room covers the tag and the count.
pub(crate) fn batches(changes: Vec<EntityChange>) -> Vec<Vec<EntityChange>> {
    const HEADER: usize = 8;
    let mut out = Vec::new();
    let mut cur: Vec<EntityChange> = Vec::new();
    let mut bytes = 0usize;
    for c in changes {
        let n = wire::encode(&c).len();
        if !cur.is_empty() && (cur.len() >= wire::ENTITIES_PER_MESSAGE || bytes + n + HEADER > wire::MAX_MESSAGE) {
            out.push(std::mem::take(&mut cur));
            bytes = 0;
        }
        bytes += n;
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::{Axis, Dest, Portal, Settings, Spawn};

    fn portal(name: &str, at: [f32; 3]) -> Component {
        Component::Portal(Portal {
            name: name.into(),
            at,
            axis: Axis::Z,
            size: [4.0, 6.0],
            yaw: 0.0,
            level: "void".into(),
            to: Dest::Here { arrive: [0.0, 0.0, 0.0] },
            key: None,
        })
    }

    #[test]
    fn entities_persist() {
        let mut db = Connection::open_in_memory().unwrap();
        // the meta table is the world's (WorldState::open makes it); the seed mark goes there
        db.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
        create_tables(&db).unwrap();

        let mut e = Entities::new();
        e.seed(vec![
            Component::Settings(Settings { json: r#"{"sky":"earth"}"#.into() }),
            Component::Spawn(Spawn { at: [0.5, 1.0, 0.5], yaw: 0.0 }),
            portal("yard", [0.0, 0.0, -6.0]),
        ]);
        assert!(e.changed());
        // a new portal (id 4) in a zone further out, and the spawn (id 2) removed
        let new = e.put(5, 100, "Ann".into(), None, portal("sea", [300.0, 2.0, 40.0])).unwrap();
        assert_eq!(new, EntityId(4));
        assert!(e.remove(6, 101, "Ann".into(), EntityId(2)).is_some());
        assert_eq!(e.seq_of(new), Some(5));

        let t = db.transaction().unwrap();
        e.write(&t).unwrap();
        t.commit().unwrap();
        e.written();
        assert!(!e.changed());

        // a bad row is skipped on load: wrong bytes, and a spawn with a non-finite position
        let bad_spawn = wire::encode(&Component::Spawn(Spawn { at: [f32::NAN, 0.0, 0.0], yaw: 0.0 }));
        db.execute(
            "INSERT INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (77, 0, 0, 'vox::Portal', 1, 0, x'ff00')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (78, 0, 0, 'vox::Spawn', 1, 0, ?1)",
            [bad_spawn],
        )
        .unwrap();

        let back = Entities::load(&db).unwrap();
        assert_eq!(back.map, e.map);
        assert_eq!(back.map.len(), 3);
        assert!(!back.map.contains_key(&EntityId(77)));
        assert_eq!(back.next, 5);

        // the log has the new portal (no old) and the removed spawn (no new)
        let count = |sql: &str| -> i64 { db.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM entity_edits"), 2);
        assert_eq!(count("SELECT count(*) FROM entity_edits WHERE entity = 4 AND old IS NULL AND new IS NOT NULL"), 1);
        assert_eq!(count("SELECT count(*) FROM entity_edits WHERE entity = 2 AND old IS NOT NULL AND new IS NULL"), 1);
        // the zone of the far portal: x 300 is block 300, chunk 9, zone 0; z 40 is chunk 1, zone 0
        assert_eq!(count("SELECT zone_x FROM entities WHERE id = 4"), 0);
        // and a portal at x 600 lands in zone 1
        assert_eq!(zone_of(&portal("far", [600.0, 0.0, 0.0])), (1, 0));
    }

    #[test]
    fn ids_are_not_reused_past_the_last() {
        let mut e = Entities::new();
        e.next = u32::MAX;
        assert_eq!(e.put(1, 0, "Ann".into(), None, portal("last", [0.0, 0.0, 0.0])), None);
        assert!(e.map.is_empty() && e.log.is_empty() && !e.changed());
        // an explicit id still lands, and the counter stays put (no wrap, no reuse)
        assert_eq!(e.put(2, 0, "Ann".into(), Some(EntityId(9)), portal("here", [0.0, 0.0, 0.0])), Some(EntityId(9)));
        assert_eq!(e.put(3, 0, "Ann".into(), None, portal("again", [0.0, 0.0, 0.0])), None);
    }
}
