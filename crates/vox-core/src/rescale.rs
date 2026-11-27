//! Converting a world made with bigger blocks to [`crate::BLOCK_M`] blocks, keeping everything
//! its size in metres (docs/design/block-scale.md). Worlds saved at half-metre blocks are
//! converted once, with a factor of [`FROM_HALF_METRE`]: each old block fills a 2 × 2 × 2 cube,
//! and every place and size in blocks doubles.
//!
//! What does not change: prop positions (5 cm units, not blocks), prop shapes, yaws, names,
//! weather and anything else in metres. A world-graph node moves with its corner, but keeps its
//! size and recipe: a recipe is drawn one voxel per block, so it builds at the new block size
//! (rebuilding it at the old size is a job for its author).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::entity::{Component, Dest, MAX_RADIUS};
use crate::world::{CHUNK, Cell, Chunk, ChunkKey};

/// Half-metre blocks to [`crate::BLOCK_M`] blocks: each old block is this many new ones along
/// each side.
pub const FROM_HALF_METRE: i32 = crate::whole_blocks(0.5);

/// One chunk's blocks at `k` times the size: the chunks they fill, each old block a `k` × `k`
/// × `k` cube. An old chunk lands on exactly the `k`³ new chunks from `key * k` (it is `k`
/// chunks across now), and on no other chunk's, so chunks convert one at a time. Empty new
/// chunks are left out. `None` when `k` isn't 1 to 4 or the new places would overflow (keys
/// come from storage, so they are checked).
pub fn chunk(key: ChunkKey, chunk: &Chunk, k: i32) -> Option<Vec<(ChunkKey, Chunk)>> {
    if !(1..=4).contains(&k) {
        return None;
    }
    let mut out = Vec::new();
    if chunk.is_empty() {
        return Some(out);
    }
    // the first new chunk's key, and every new block's place fits an i32
    let base = [key.0[0].checked_mul(k)?, key.0[1].checked_mul(k)?, key.0[2].checked_mul(k)?];
    for v in base {
        v.checked_add(k)?.checked_mul(CHUNK)?;
    }
    let o = key.origin();
    for a in 0..k {
        for b in 0..k {
            for c in 0..k {
                let nk = ChunkKey([base[0] + a, base[1] + b, base[2] + c]);
                let no = nk.origin();
                let mut n = Chunk::default();
                for y in 0..CHUNK {
                    for z in 0..CHUNK {
                        for x in 0..CHUNK {
                            let p: Cell = [no[0] + x, no[1] + y, no[2] + z];
                            // the old block this new one is part of
                            let from: Cell = [p[0].div_euclid(k), p[1].div_euclid(k), p[2].div_euclid(k)];
                            debug_assert!((0..3).all(|i| (o[i]..o[i] + CHUNK).contains(&from[i])));
                            let id = chunk.get(from);
                            if id != 0 {
                                n.set(p, id);
                            }
                        }
                    }
                }
                if !n.is_empty() {
                    // newer than the old chunk, so nothing that cached it takes this for it
                    n.version = n.version.max(chunk.version.saturating_add(1));
                    out.push((nk, n));
                }
            }
        }
    }
    Some(out)
}

/// The chunks of a world converted by [`chunk`], all in memory (a snapshot being restored).
/// A chunk that would overflow is left out.
pub fn chunks(old: impl IntoIterator<Item = (ChunkKey, Chunk)>, k: i32) -> BTreeMap<ChunkKey, Chunk> {
    old.into_iter().filter_map(|(key, c)| chunk(key, &c, k)).flatten().collect()
}

fn times(p: [f32; 3], k: f32) -> [f32; 3] {
    p.map(|v| v * k)
}

/// One entity with its places and sizes in blocks multiplied by `k`. The result is tidied and
/// checked like any entity: a doorway that would grow past the largest size is clamped.
pub fn component(c: &Component, k: i32) -> Component {
    let f = k as f32;
    let mut c = c.clone();
    match &mut c {
        Component::Portal(p) => {
            p.at = times(p.at, f);
            p.size = p.size.map(|s| crate::portal::clamp_side(s * f));
            match &mut p.to {
                Dest::Here { arrive } | Dest::Dungeon { arrive, .. } | Dest::Url { arrive, .. } => *arrive = times(*arrive, f),
                Dest::World { .. } | Dest::Pair { .. } => {}
            }
        }
        Component::Spawn(s) => s.at = times(s.at, f),
        Component::Objective(o) => {
            o.at = times(o.at, f);
            o.radius = (o.radius * f).min(MAX_RADIUS);
        }
        Component::Settings(s) => {
            if let Ok(v) = serde_json::from_str::<Value>(&s.json) {
                s.json = settings(&v, k).to_string();
            }
        }
        // 5 cm units, the same at any block size
        Component::Prop(_) => {}
        Component::Node(n) => n.at = n.at.map(|v| v.saturating_mul(k)),
    }
    c
}

/// A world's settings with its places in blocks multiplied by `k`: the floor, the fixed
/// cameras' zones (their `at` and `look` are metres), doorways and objectives still kept in
/// settings by older worlds, and the relief's origin. Relief made for half-metre blocks is
/// moved to the version that draws the same ground at the new size.
pub fn settings(v: &Value, k: i32) -> Value {
    let mut v = v.clone();
    let Some(o) = v.as_object_mut() else { return v };
    let int = |x: &Value| x.as_i64().map(|n| Value::from(n.saturating_mul(i64::from(k))));
    let num = |x: &Value| x.as_f64().map(|n| Value::from(n * f64::from(k)));
    let list = |x: &mut Value, scale: &dyn Fn(&Value) -> Option<Value>| {
        if let Some(a) = x.as_array_mut() {
            for e in a.iter_mut() {
                if let Some(n) = scale(e) {
                    *e = n;
                }
            }
        }
    };
    if let Some(f) = o.get("floor").and_then(int) {
        o.insert("floor".into(), f);
    }
    if let Some(cams) = o.get_mut("cameras").and_then(|c| c.as_array_mut()) {
        for cam in cams {
            if let Some(zone) = cam.get_mut("zone").and_then(|z| z.as_object_mut()) {
                // the zone takes in its `to` block, like a box: the new zone ends k - 1 further
                if let Some(from) = zone.get_mut("from") {
                    list(from, &num);
                }
                if let Some(to) = zone.get_mut("to") {
                    list(to, &|x| x.as_f64().map(|n| Value::from(n * f64::from(k) + f64::from(k - 1))));
                }
            }
        }
    }
    if let Some(portals) = o.get_mut("portals").and_then(|p| p.as_array_mut()) {
        for p in portals {
            for key in ["at", "arrive", "size"] {
                if let Some(x) = p.get_mut(key) {
                    list(x, &num);
                }
            }
        }
    }
    if let Some(obj) = o.get_mut("objective").and_then(|x| x.as_object_mut()) {
        if let Some(at) = obj.get_mut("at") {
            list(at, &num);
        }
        if let Some(r) = obj.get("radius").and_then(num) {
            obj.insert("radius".into(), r);
        }
    }
    if let Some(relief) = o.get_mut("relief").and_then(|r| r.as_object_mut()) {
        if let Some(origin) = relief.get_mut("origin") {
            list(origin, &int);
        }
        // version 1 was this ground at half-metre blocks; a missing version means "this build's"
        if relief.get("version").and_then(|v| v.as_u64()).is_none_or(|v| v == 1) {
            relief.insert("version".into(), Value::from(crate::relief::VERSION));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Axis, Node, Objective, Portal, Prop, Settings, Spawn};
    use crate::world::{Bounds, World};

    #[test]
    fn each_block_fills_a_cube() {
        let mut w = World::new(Bounds::default(), 4);
        w.set([0, 0, 0], 1);
        w.set([-1, 3, 31], 2);
        let old: Vec<_> = w.keys().into_iter().map(|k| (k, w.chunk(k).unwrap().clone())).collect();
        let new = chunks(old, 2);
        let mut n = World::new(Bounds::default(), 4);
        for (k, c) in new {
            n.put_chunk(k, c);
        }
        assert_eq!(n.count(), 16);
        for d in [[0, 0, 0], [1, 1, 1], [0, 1, 0]] {
            assert_eq!(n.get(d), 1);
            assert_eq!(n.get([-2 + d[0], 6 + d[1], 62 + d[2]]), 2);
        }
        assert_eq!(n.get([2, 0, 0]), 0);
        assert_eq!(n.get([-3, 6, 62]), 0);
        // the block at z = 31 (the first chunk along z) lands at z = 62 and 63, in the second
        assert_eq!(ChunkKey::of([-2, 6, 62]), ChunkKey([-1, 0, 1]));
        assert!(n.chunk(ChunkKey([-1, 0, 1])).is_some());
        // a chunk whose new places would overflow is refused, not wrapped
        let mut far = Chunk::default();
        far.set([0, 0, 0], 1);
        assert!(chunk(ChunkKey([i32::MAX / 64, 0, 0]), &far, 2).is_none());
        assert!(chunk(ChunkKey([0, 0, 0]), &far, 5).is_none());
    }

    #[test]
    fn entities_keep_their_size_in_metres() {
        let portal = Component::Portal(Portal {
            name: "gate".into(),
            at: [1.5, 0.0, -39.5],
            axis: Axis::Z,
            size: [8.0, 100.0],
            yaw: 90.0,
            level: "yard".into(),
            to: Dest::Url { url: ":7781".into(), arrive: [0.0, 1.0, 11.5] },
            key: Some(3),
        });
        let Component::Portal(p) = component(&portal, 2) else { panic!() };
        assert_eq!(p.at, [3.0, 0.0, -79.0]);
        assert_eq!(p.size, [16.0, crate::portal::MAX_SIDE], "clamped to the largest doorway");
        assert_eq!(p.to, Dest::Url { url: ":7781".into(), arrive: [0.0, 2.0, 23.0] });
        assert_eq!((p.yaw, p.key), (90.0, Some(3)));

        let spawn = component(&Component::Spawn(Spawn { at: [0.5, 1.0, 0.5], yaw: 10.0 }), 2);
        assert_eq!(spawn, Component::Spawn(Spawn { at: [1.0, 2.0, 1.0], yaw: 10.0 }));
        let obj = component(&Component::Objective(Objective { name: "altar".into(), at: [0.0, 0.0, -8.0], radius: 3.0 }), 2);
        assert_eq!(obj, Component::Objective(Objective { name: "altar".into(), at: [0.0, 0.0, -16.0], radius: 6.0 }));

        let prop = Component::Prop(Prop { prop: [1; 16], at: [55, 10, -3], yaw: 6, variant: 0, state: 0, channel: None, text: None, owner: Some(7) });
        assert_eq!(component(&prop, 2), prop, "5 cm units don't change");

        let node = Node { kind: "house".into(), detail: 1, at: [4, 0, -2], size: [8, 6, 8], parent: None, recipe: None, seed: 1 };
        let Component::Node(n) = component(&Component::Node(node), 2) else { panic!() };
        assert_eq!((n.at, n.size), ([8, 0, -4], [8, 6, 8]));
        // every converted entity still passes the checks
        for c in [Component::Portal(p), spawn, obj, prop, Component::Node(n)] {
            assert_eq!(c.validate(), Ok(()), "{c:?}");
        }
    }

    #[test]
    fn settings_move_with_the_blocks() {
        let v = serde_json::json!({
            "sky": "void",
            "floor": 3,
            "camera": {"yaw": 0, "zoom": 14},
            "cameras": [{"name": "gate", "zone": {"from": [-14, 0, -36], "to": [14, 10, -20]}, "at": [5.5, 2.2, -6.0]}],
            "relief": {"amount": 1.0, "origin": [512, -1024]},
            "weather": {"ground": -16.0},
        });
        let s = settings(&v, 2);
        assert_eq!(s["floor"], 6);
        assert_eq!(s["camera"]["zoom"], 14, "metres");
        assert_eq!(s["cameras"][0]["zone"]["from"], serde_json::json!([-28.0, 0.0, -72.0]));
        assert_eq!(s["cameras"][0]["zone"]["to"], serde_json::json!([29.0, 21.0, -39.0]));
        assert_eq!(s["cameras"][0]["at"], serde_json::json!([5.5, 2.2, -6.0]), "metres");
        assert_eq!(s["relief"]["origin"], serde_json::json!([1024, -2048]));
        assert_eq!(s["relief"]["version"], crate::relief::VERSION);
        assert_eq!(s["weather"]["ground"], -16.0);
        // through the entity too
        let c = component(&Component::Settings(Settings { json: v.to_string() }), 2);
        let Component::Settings(c) = c else { panic!() };
        assert_eq!(serde_json::from_str::<Value>(&c.json).unwrap(), s);
        // relief a newer build made is left at its version
        let newer = settings(&serde_json::json!({"relief": {"version": 3}}), 2);
        assert_eq!(newer["relief"]["version"], 3);
    }
}
