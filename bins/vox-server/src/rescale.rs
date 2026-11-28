//! The one-time move of a saved world to quarter-metre blocks (docs/design/block-scale.md).
//!
//! A world saved before the change holds half-metre blocks. On its first start with this build
//! it is converted in one transaction (`vox_core::rescale`): each block becomes a 2 × 2 × 2
//! cube, its entities and settings move with the blocks, and its edit logs are emptied (they
//! name half-metre cells; the restore they serve can't reach back past the change anyway). The
//! meta key [`MARK`] says it is done, so it runs once; a new world gets the mark when it is
//! made. Snapshots taken before the change are converted when restored ([`older`]).

use rusqlite::{Connection, OptionalExtension, params};
use vox_core::rescale::{self, FROM_HALF_METRE};
use vox_core::wire;
use vox_core::{Chunk, ChunkKey, entity::Component};

/// The meta key that records the world's block size once it is quarter-metre.
pub(crate) const MARK: &str = "block_m";
/// When it was converted (unix seconds): snapshots before this hold half-metre blocks.
const MARK_AT: &str = "block_m_at";

/// Converts a world saved at half-metre blocks; a world already converted, or a new one, is
/// only marked. Returns what it did, when it did anything.
pub(crate) fn migrate(db: &Connection, now: u64) -> Result<Option<String>, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let meta = |k: &str| db.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get::<_, String>(0)).optional();
    if meta(MARK).map_err(e)?.is_some() {
        return Ok(None);
    }
    // a world is made when it first stores its palette; one without has nothing to convert
    let made = meta("palette").map_err(e)?.is_some();
    let tx = db.unchecked_transaction().map_err(e)?;
    let mut what = None;
    if made {
        // one stored chunk at a time into a new table (an old chunk's new chunks are its own,
        // so none is written twice), then the new table takes the old one's place: a world
        // never holds both whole in memory
        tx.execute_batch(
            "CREATE TABLE chunks_new(x INTEGER, y INTEGER, z INTEGER, version INTEGER NOT NULL,
                 data BLOB NOT NULL, PRIMARY KEY(x, y, z));",
        )
        .map_err(e)?;
        let keys: Vec<ChunkKey> = {
            let mut q = tx.prepare("SELECT x, y, z FROM chunks").map_err(e)?;
            q.query_map([], |r| Ok(ChunkKey([r.get(0)?, r.get(1)?, r.get(2)?]))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?
        };
        let (mut blocks, mut made) = (0usize, 0usize);
        for k in &keys {
            let (version, data): (u32, Vec<u8>) = tx
                .query_row("SELECT version, data FROM chunks WHERE x = ?1 AND y = ?2 AND z = ?3", params![k.0[0], k.0[1], k.0[2]], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .map_err(e)?;
            let old = Chunk::decode(&data, version)?;
            blocks += count(*k, &old);
            let new = rescale::chunk(*k, &old, FROM_HALF_METRE).ok_or_else(|| format!("chunk {:?} is too far out to convert", k.0))?;
            for (nk, c) in new {
                tx.execute(
                    "INSERT INTO chunks_new(x, y, z, version, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![nk.0[0], nk.0[1], nk.0[2], c.version, c.encode()],
                )
                .map_err(e)?;
                made += 1;
            }
        }
        tx.execute_batch("DROP TABLE chunks; ALTER TABLE chunks_new RENAME TO chunks;").map_err(e)?;
        let was = keys.len();
        // entities: each moved with the blocks, into the zone its new place is in. A row that
        // doesn't decode is left as it is (loading skips it and says so).
        let rows: Vec<(u32, Vec<u8>)> = {
            let mut q = tx.prepare("SELECT id, data FROM entities").map_err(e)?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?
        };
        let mut moved = 0;
        for (id, data) in rows {
            let Ok(c) = wire::decode::<Component>(&data) else { continue };
            let c = rescale::component(&c, FROM_HALF_METRE);
            let (zx, zz) = crate::entities::zone_of(&c);
            tx.execute("UPDATE entities SET zone_x = ?1, zone_z = ?2, data = ?3 WHERE id = ?4", params![zx, zz, wire::encode(&c), id])
                .map_err(e)?;
            moved += 1;
        }
        let stored = tx.query_row("SELECT value FROM meta WHERE key = 'settings'", [], |r| r.get::<_, String>(0)).optional().map_err(e)?;
        if let Some(s) = stored
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
        {
            let s = rescale::settings(&v, FROM_HALF_METRE).to_string();
            tx.execute("UPDATE meta SET value = ?1 WHERE key = 'settings'", [s]).map_err(e)?;
        }
        tx.execute("DELETE FROM edits", []).map_err(e)?;
        tx.execute("DELETE FROM entity_edits", []).map_err(e)?;
        what = Some(format!(
            "converted to {} m blocks: {blocks} blocks in {was} chunks became {} chunks, {moved} entities moved",
            vox_core::BLOCK_M,
            made
        ));
    }
    tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", params![MARK, vox_core::BLOCK_M.to_string()]).map_err(e)?;
    if made {
        tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", params![MARK_AT, now.to_string()]).map_err(e)?;
    }
    tx.commit().map_err(e)?;
    Ok(what)
}

/// Was a snapshot taken at `taken_at` (unix seconds) before this world was converted, so that
/// it holds half-metre blocks?
pub(crate) fn older(db: &Connection, taken_at: u64) -> bool {
    db.query_row("SELECT value FROM meta WHERE key = ?1", [MARK_AT], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|at| taken_at < at)
}

fn count(k: ChunkKey, c: &Chunk) -> usize {
    let o = k.origin();
    let n = vox_core::CHUNK;
    (0..n).flat_map(|y| (0..n).flat_map(move |z| (0..n).map(move |x| [o[0] + x, o[1] + y, o[2] + z]))).filter(|p| c.get(*p) != 0).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::{Settings, Spawn};

    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE chunks(x INTEGER, y INTEGER, z INTEGER, version INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY(x, y, z));
             CREATE TABLE edits(seq INTEGER PRIMARY KEY, at INTEGER NOT NULL, player TEXT NOT NULL, account INTEGER,
                 x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, old INTEGER NOT NULL, new INTEGER NOT NULL);",
        )
        .unwrap();
        crate::entities::create_tables(&db).unwrap();
        db
    }

    #[test]
    fn a_half_metre_world_is_converted_once() {
        let db = db();
        let mut c = Chunk::default();
        c.set([1, 2, 3], 5);
        db.execute("INSERT INTO chunks VALUES (0, 0, 0, 1, ?1)", [c.encode()]).unwrap();
        db.execute("INSERT INTO meta VALUES ('palette', '[\"stone\"]'), ('settings', '{\"floor\": 2}')", []).unwrap();
        db.execute("INSERT INTO edits VALUES (1, 0, 'Ann', NULL, 1, 2, 3, 0, 5)", []).unwrap();
        let spawn = Component::Spawn(Spawn { at: [300.5, 1.0, 0.5], yaw: 0.0 });
        let settings = Component::Settings(Settings { json: r#"{"floor": 2}"#.into() });
        for (id, c) in [(1, &spawn), (2, &settings)] {
            db.execute(
                "INSERT INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (?1, 0, 0, ?2, 1, 0, ?3)",
                params![id, c.name(), wire::encode(c)],
            )
            .unwrap();
        }
        let did = migrate(&db, 1_000).unwrap();
        assert!(did.is_some_and(|w| w.contains("1 blocks in 1 chunks")));
        // the block is a 2 × 2 × 2 cube where it was
        let (n, data): (i64, Vec<u8>) = db.query_row("SELECT COUNT(*), data FROM chunks", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(n, 1);
        let c = Chunk::decode(&data, 0).unwrap();
        assert_eq!((c.get([2, 4, 6]), c.get([3, 5, 7]), c.get([4, 4, 6])), (5, 5, 0));
        // the spawn moved, into the zone it is in now (x = 601 is past the first zone, 512 blocks)
        let (zx, data): (i32, Vec<u8>) = db.query_row("SELECT zone_x, data FROM entities WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(wire::decode::<Component>(&data).unwrap(), Component::Spawn(Spawn { at: [601.0, 2.0, 1.0], yaw: 0.0 }));
        assert_eq!(zx, 1);
        let s: String = db.query_row("SELECT value FROM meta WHERE key = 'settings'", [], |r| r.get(0)).unwrap();
        assert_eq!(s, r#"{"floor":4}"#);
        let logged: i64 = db.query_row("SELECT COUNT(*) FROM edits", [], |r| r.get(0)).unwrap();
        assert_eq!(logged, 0, "the half-metre edit log is gone");
        assert!(older(&db, 999) && !older(&db, 1_000));
        // and never again
        assert_eq!(migrate(&db, 2_000).unwrap(), None);
        let n: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_new_world_is_only_marked() {
        let db = db();
        assert_eq!(migrate(&db, 1_000).unwrap(), None);
        let mark: String = db.query_row("SELECT value FROM meta WHERE key = ?1", [MARK], |r| r.get(0)).unwrap();
        assert_eq!(mark, "0.25");
        assert!(!older(&db, 0), "nothing to convert in its snapshots");
    }
}
