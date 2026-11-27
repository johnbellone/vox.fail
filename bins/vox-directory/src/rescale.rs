//! The one-time move of the directory's places to quarter-metre blocks
//! (docs/design/block-scale.md).
//!
//! Plots, player portals, where people last stood, the worlds' spawns, and the places written
//! into deeds and keys were all in half-metre blocks. On the first start with this build each
//! is doubled, in one transaction, so a plot covers the same ground as before (16 × 16 blocks
//! became 32 × 32; new plots are 64 × 64). The counter [`MARK`] says it is done, so it runs once.
//!
//! Structures are not touched here: their blocks live in blobs (named by hash), not in this
//! database.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

/// The counter that records the conversion (its value is the factor applied).
const MARK: &str = "block_scale";

/// Converts the places in a directory made with half-metre blocks; marks a new one. Returns
/// whether anything was converted.
pub(crate) fn migrate(db: &Connection) -> Result<bool, String> {
    let e = |e: rusqlite::Error| e.to_string();
    if db.query_row("SELECT value FROM counters WHERE name = ?1", [MARK], |r| r.get::<_, i64>(0)).optional().map_err(e)?.is_some() {
        return Ok(false);
    }
    let k = vox_core::rescale::FROM_HALF_METRE;
    let tx = db.unchecked_transaction().map_err(e)?;
    let mut changed = 0;
    changed += tx.execute("UPDATE whereabouts SET x = x * ?1, y = y * ?1, z = z * ?1", [k]).map_err(e)?;
    changed += tx.execute("UPDATE plots SET x0 = x0 * ?1, z0 = z0 * ?1, x1 = x1 * ?1, z1 = z1 * ?1", [k]).map_err(e)?;
    // a doorway with no size of its own takes the default, which is already at the new size
    changed += tx
        .execute(
            "UPDATE portals SET x = x * ?1, y = y * ?1, z = z * ?1, size_w = size_w * ?1, size_h = size_h * ?1",
            [k],
        )
        .map_err(e)?;
    let f = f64::from(k);
    let times = |v: &Value| v.as_f64().map(|n| Value::from(n * f));
    // the spawns the worlds last reported (they report again within a minute anyway)
    let spawns: Vec<(String, String)> = {
        let mut q = tx.prepare("SELECT world, spawn FROM world_state").map_err(e)?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?
    };
    for (world, spawn) in spawns {
        let Ok(Value::Array(a)) = serde_json::from_str::<Value>(&spawn) else { continue };
        let a: Vec<Value> = a.iter().map(|v| times(v).unwrap_or_else(|| v.clone())).collect();
        changed += tx.execute("UPDATE world_state SET spawn = ?1 WHERE world = ?2", params![Value::Array(a).to_string(), world]).map_err(e)?;
    }
    // what a deed and a key say about their place
    let items: Vec<(i64, String)> = {
        let mut q = tx.prepare("SELECT id, detail FROM items WHERE kind IN ('deed', 'key')").map_err(e)?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?
    };
    for (id, detail) in items {
        let Ok(mut d) = serde_json::from_str::<Value>(&detail) else { continue };
        if let Some(area) = d.get_mut("area").and_then(|a| a.as_object_mut()) {
            for key in ["x0", "z0", "x1", "z1"] {
                if let Some(n) = area.get(key).and_then(|v| v.as_i64()) {
                    area.insert(key.into(), Value::from(n.saturating_mul(i64::from(k))));
                }
            }
        }
        if let Some(at) = d.get_mut("at").and_then(|a| a.as_array_mut()) {
            for v in at.iter_mut() {
                if let Some(n) = times(v) {
                    *v = n;
                }
            }
        }
        changed += tx.execute("UPDATE items SET detail = ?1 WHERE id = ?2", params![d.to_string(), id]).map_err(e)?;
    }
    tx.execute("INSERT INTO counters (name, value) VALUES (?1, ?2)", params![MARK, k]).map_err(e)?;
    tx.commit().map_err(e)?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL);
             CREATE TABLE whereabouts (account INTEGER NOT NULL, world TEXT NOT NULL, x REAL NOT NULL, y REAL NOT NULL,
                 z REAL NOT NULL, at INTEGER NOT NULL, PRIMARY KEY (account, world));
             CREATE TABLE plots (id INTEGER PRIMARY KEY, world TEXT NOT NULL, owner INTEGER NOT NULL, x0 INTEGER NOT NULL,
                 z0 INTEGER NOT NULL, x1 INTEGER NOT NULL, z1 INTEGER NOT NULL, grp INTEGER, created INTEGER NOT NULL);
             CREATE TABLE portals (id INTEGER PRIMARY KEY, world TEXT NOT NULL, owner INTEGER NOT NULL, plot INTEGER NOT NULL,
                 x REAL NOT NULL, y REAL NOT NULL, z REAL NOT NULL, axis TEXT NOT NULL, dest TEXT NOT NULL,
                 dest_key INTEGER, created INTEGER NOT NULL, size_w REAL, size_h REAL, yaw REAL);
             CREATE TABLE world_state (world TEXT PRIMARY KEY, spawn TEXT NOT NULL, level TEXT NOT NULL, reported INTEGER NOT NULL);
             CREATE TABLE items (id INTEGER PRIMARY KEY, account INTEGER NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL,
                 world TEXT, ref INTEGER, detail TEXT NOT NULL, acquired INTEGER NOT NULL);",
        )
        .unwrap();
        db
    }

    #[test]
    fn places_double_once() {
        let db = db();
        db.execute_batch(
            "INSERT INTO whereabouts VALUES (1, 'void', 40.5, 1.0, -3.25, 0);
             INSERT INTO plots VALUES (1, 'void', 1, 32, -8, 48, 8, NULL, 0);
             INSERT INTO portals VALUES (1, 'void', 1, 1, 40.0, 1.0, 0.0, 'x', 'yard', NULL, 0, 5.0, 7.0, 90.0);
             INSERT INTO portals VALUES (2, 'void', 1, 1, 41.0, 1.0, 0.0, 'x', 'yard', NULL, 0, NULL, NULL, NULL);
             INSERT INTO world_state VALUES ('void', '[0.5,1.0,0.5]', 'void', 0);
             INSERT INTO items VALUES (1, 1, 'deed', 'Plot in The Void', 'void', 1, '{\"area\":{\"x0\":32,\"z0\":-8,\"x1\":48,\"z1\":8},\"shared_with\":null}', 0);
             INSERT INTO items VALUES (2, 1, 'key', 'Portal to The Yard', 'void', 1, '{\"at\":[40.0,1.0,0.0],\"to\":\"yard\"}', 0);
             INSERT INTO items VALUES (3, 1, 'trophy', 'Raided', 'void', 1, '{\"party\":\"x\"}', 0);",
        )
        .unwrap();
        assert!(migrate(&db).unwrap());
        let at: (f64, f64, f64) = db.query_row("SELECT x, y, z FROM whereabouts", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(at, (81.0, 2.0, -6.5));
        let plot: (i64, i64, i64, i64) = db.query_row("SELECT x0, z0, x1, z1 FROM plots", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(plot, (64, -16, 96, 16));
        let sized: (f64, f64, f64, f64) = db.query_row("SELECT x, size_w, size_h, yaw FROM portals WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(sized, (80.0, 10.0, 14.0, 90.0));
        let plain: (f64, Option<f64>) = db.query_row("SELECT x, size_w FROM portals WHERE id = 2", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(plain, (82.0, None), "no size of its own: the new default");
        let spawn: String = db.query_row("SELECT spawn FROM world_state", [], |r| r.get(0)).unwrap();
        assert_eq!(spawn, "[1.0,2.0,1.0]");
        let detail = |id: i64| -> Value { serde_json::from_str(&db.query_row("SELECT detail FROM items WHERE id = ?1", [id], |r| r.get::<_, String>(0)).unwrap()).unwrap() };
        assert_eq!(detail(1)["area"], serde_json::json!({"x0": 64, "z0": -16, "x1": 96, "z1": 16}));
        assert_eq!(detail(2)["at"], serde_json::json!([80.0, 2.0, 0.0]));
        assert_eq!(detail(3), serde_json::json!({"party": "x"}));
        // once only
        assert!(!migrate(&db).unwrap());
        let x: f64 = db.query_row("SELECT x FROM whereabouts", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 81.0);
    }

    #[test]
    fn a_new_directory_is_only_marked() {
        let db = db();
        assert!(!migrate(&db).unwrap());
        let v: i64 = db.query_row("SELECT value FROM counters WHERE name = ?1", [MARK], |r| r.get(0)).unwrap();
        assert_eq!(v, 2);
    }
}
