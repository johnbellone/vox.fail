//! Level files: boxes of blocks (solid, hollow or rounded) and single blocks, by name.
//!
//! ```json
//! { "name": "yard", "boxes": [ { "block": "grass", "from": [-20, -1, -20], "to": [20, -1, 20] } ],
//!   "blocks": [ [0, 0, 0, "stone"] ] }
//! ```
//! Later boxes overwrite earlier ones; `"air"` clears. A level may give its world's size,
//! `"bounds": {"lo": [x, y, z], "hi": [x, y, z]}` (hi exclusive), else the world takes
//! [`Bounds::default`].

use crate::entity::{Axis, Component, Dest, Objective, Portal, Settings};
use crate::world::{Bounds, Cell, World};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Deserialize)]
struct LevelFile {
    #[serde(default)]
    boxes: Vec<LevelBox>,
    #[serde(default)]
    blocks: Vec<(i32, i32, i32, String)>,
}

#[derive(Deserialize)]
struct LevelBox {
    block: String,
    from: [i32; 3],
    to: [i32; 3],
    /// only the outside of the box (walls, a hollow tree crown)
    #[serde(default)]
    hollow: bool,
    /// how many blocks thick a hollow box's shell is (at least 1)
    #[serde(default)]
    thick: Option<u8>,
    /// a rounded box (a tree crown, a bush): corners further than this from the centre, in
    /// half-sizes, are left out
    #[serde(default)]
    round: Option<f32>,
}

/// A level's world settings (its `"world"` object: sky, time, clouds) as JSON text; `{}`
/// when it has none.
pub fn settings(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("world").filter(|w| w.is_object()).map(|w| w.to_string()))
        .unwrap_or_else(|| "{}".into())
}

/// A level's entities: its doorways (`world.portals`), its objective (`world.objective`), and
/// the rest of its `world` object as settings. There is no spawn: the server works one out
/// from the ground. Each component is tidied and then checked, so an error names the one that
/// failed.
pub fn entities(text: &str) -> Result<Vec<Component>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let world = v.get("world").filter(|w| w.is_object());
    let mut out = Vec::new();
    if let Some(list) = world.and_then(|w| w.get("portals")).and_then(|p| p.as_array()) {
        for (i, p) in list.iter().enumerate() {
            // as `portal::parse` does: a doorway without a position, level or arrival is left out
            let (Some(at), Some(level), Some(arrive)) =
                (vec3(p, "at"), p.get("level").and_then(|l| l.as_str()), vec3(p, "arrive"))
            else {
                continue;
            };
            let size = p.get("size").and_then(|s| s.as_array());
            // clamped as `portal::parse` clamps, so an out-of-range size (even an infinite one) comes out the same
            let dim = |j: usize, d: f64| {
                let c = size.and_then(|s| s.get(j)).and_then(|x| x.as_f64()).unwrap_or(d) as f32;
                crate::portal::clamp_side(c)
            };
            let to = match p.get("url").and_then(|u| u.as_str()) {
                None => Dest::Here { arrive },
                Some(url) => match url.strip_prefix("dungeon:") {
                    Some(name) => Dest::Dungeon { name: name.to_string(), arrive },
                    None => Dest::Url { url: url.to_string(), arrive },
                },
            };
            out.push(Component::Portal(Portal {
                name: p.get("name").and_then(|n| n.as_str()).map_or_else(|| format!("portal {}", i + 1), String::from),
                at,
                axis: if p.get("axis").and_then(|a| a.as_str()) == Some("x") { Axis::X } else { Axis::Z },
                size: [dim(0, f64::from(crate::portal::DEFAULT_SIZE[0])), dim(1, f64::from(crate::portal::DEFAULT_SIZE[1]))],
                // a yaw too big for f32 is 0, as `portal::parse` has it
                yaw: match p.get("yaw").and_then(|y| y.as_f64()).unwrap_or(0.0) as f32 {
                    y if y.is_finite() => y,
                    _ => 0.0,
                },
                level: level.to_string(),
                to,
                key: None,
            }));
        }
    }
    if let Some(o) = world.and_then(|w| w.get("objective")) {
        let name = o.get("name").and_then(|n| n.as_str()).ok_or("the objective needs a name")?;
        let at = vec3(o, "at").ok_or("the objective needs a position, at")?;
        let radius = o.get("radius").and_then(|r| r.as_f64()).ok_or("the objective needs a radius")?;
        out.push(Component::Objective(Objective { name: name.to_string(), at, radius: radius as f32 }));
    }
    let mut json = world.and_then(|w| w.as_object()).cloned().unwrap_or_default();
    json.remove("portals");
    json.remove("objective");
    out.push(Component::Settings(Settings { json: serde_json::to_string(&json).map_err(|e| e.to_string())? }));
    for c in &mut out {
        c.tidy();
        if let Err(why) = c.validate() {
            return Err(format!("{} is not valid: {why}", c.name()));
        }
    }
    Ok(out)
}

/// The three numbers under `k` in a level object, if they are all there.
fn vec3(v: &Value, k: &str) -> Option<[f32; 3]> {
    let a = v.get(k)?.as_array()?;
    let p = [a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32];
    // a number too big for f32 becomes infinite: no point, as `portal::parse` has it
    p.iter().all(|c| c.is_finite()).then_some(p)
}

/// The longest a box may be corner to corner, in blocks (200 m).
const MAX_BOX: f32 = crate::in_blocks(200.0);
/// A level's bounds may reach this far from 0 along x and z, in blocks (about 4,000 km: the
/// numbers stay far from overflowing), and span at most [`MAX_HEIGHT`] up.
const MAX_REACH: i32 = 1 << 24;
const MAX_HEIGHT: i32 = 4096;

/// The size a level gives its world (`"bounds"`), if it gives a sensible one: each `lo` below
/// its `hi`, within [`MAX_REACH`] of 0, and no taller than [`MAX_HEIGHT`].
pub fn bounds(text: &str) -> Option<Bounds> {
    let v: Value = serde_json::from_str(text).ok()?;
    let b = v.get("bounds")?;
    let cell = |k: &str| -> Option<Cell> {
        let a = b.get(k)?.as_array()?;
        let n = |i: usize| a.get(i)?.as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| n.abs() <= MAX_REACH);
        Some([n(0)?, n(1)?, n(2)?])
    };
    let (lo, hi) = (cell("lo")?, cell("hi")?);
    let ok = (0..3).all(|i| lo[i] < hi[i]) && hi[1] - lo[1] <= MAX_HEIGHT;
    ok.then_some(Bounds { lo, hi })
}

/// A level's blocks, put one at a time into `put(cell, id)` in the file's order (id 0 is air:
/// clear the cell), only those inside `within`. Boxes are clipped to `within` before their
/// cells are visited, so a small region of a big level costs only that region. Every block
/// name is checked before anything is put, so a bad level puts nothing.
///
/// Nothing here holds the level's blocks: at quarter-metre blocks the sea is 34 million of
/// them, which a map of every block can't afford (docs/design/chunk-streaming.md).
pub fn apply(text: &str, names: &[String], within: Bounds, mut put: impl FnMut(Cell, u16)) -> Result<(), String> {
    let f: LevelFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let id = |name: &str| -> Result<u16, String> {
        if name == "air" {
            Ok(0)
        } else {
            names
                .iter()
                .position(|n| n == name)
                .map(|i| i as u16 + 1)
                .ok_or_else(|| format!("no block called \"{name}\""))
        }
    };
    // names and sizes first
    let mut boxes = Vec::with_capacity(f.boxes.len());
    for bx in &f.boxes {
        let b = id(&bx.block)?;
        let lo: [i32; 3] = std::array::from_fn(|i| bx.from[i].min(bx.to[i]));
        let hi: [i32; 3] = std::array::from_fn(|i| bx.from[i].max(bx.to[i]));
        // in i64: the corners come from a file, and their difference can overflow an i32
        let size: [f32; 3] = std::array::from_fn(|i| (i64::from(hi[i]) - i64::from(lo[i])) as f32);
        if (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() > MAX_BOX {
            return Err(format!("a {} box is too big", bx.block));
        }
        boxes.push((bx, b, lo, hi, size));
    }
    let singles: Vec<(Cell, u16)> = f.blocks.iter().map(|(x, y, z, name)| Ok(([*x, *y, *z], id(name)?))).collect::<Result<_, String>>()?;
    for (bx, b, lo, hi, size) in boxes {
        let centre: [f32; 3] = std::array::from_fn(|i| (lo[i] as f32 + hi[i] as f32) * 0.5);
        let half: [f32; 3] = std::array::from_fn(|i| (size[i] * 0.5 + 0.5).max(0.5));
        let t = i32::from(bx.thick.unwrap_or(1).max(1));
        // the part of the box inside the region (hi inclusive here, exclusive in Bounds)
        let first: [i32; 3] = std::array::from_fn(|i| lo[i].max(within.lo[i]));
        let last: [i32; 3] = std::array::from_fn(|i| hi[i].min(within.hi[i].saturating_sub(1)));
        // a hollow box's middle: more than `t` in from every face (in i64, as for `size`)
        let middle = |p: Cell| (0..3).all(|i| {
            let (v, t) = (i64::from(p[i]), i64::from(t));
            v >= i64::from(lo[i]) + t && v <= i64::from(hi[i]) - t
        });
        for x in first[0]..=last[0] {
            for y in first[1]..=last[1] {
                for z in first[2]..=last[2] {
                    let p = [x, y, z];
                    if bx.hollow && middle(p) {
                        continue;
                    }
                    if let Some(r) = bx.round {
                        let d2: f32 = (0..3).map(|i| ((p[i] as f32 - centre[i]) / half[i]).powi(2)).sum();
                        if d2.sqrt() > r {
                            continue;
                        }
                    }
                    put(p, b);
                }
            }
        }
    }
    for (c, b) in singles {
        if within.contains(c) {
            put(c, b);
        }
    }
    Ok(())
}

/// Bounds that take in every block a level can have: for reading a whole level.
pub fn everywhere() -> Bounds {
    Bounds { lo: [i32::MIN; 3], hi: [i32::MAX; 3] }
}

/// A level's blocks as a map: position -> id, where id is the index in `names` plus one. For
/// tests and small levels; a world is filled with [`fill`], which keeps no map.
pub fn load(text: &str, names: &[String]) -> Result<HashMap<Cell, u16>, String> {
    let mut cells = HashMap::new();
    apply(text, names, everywhere(), |p, b| {
        if b == 0 {
            cells.remove(&p);
        } else {
            cells.insert(p, b);
        }
    })?;
    Ok(cells)
}

/// Put a level's blocks into a world (those outside its bounds are left out), straight into
/// its chunks; returns how many blocks the world gained.
pub fn fill(world: &mut World, text: &str, names: &[String]) -> Result<usize, String> {
    let before = world.count();
    let within = world.bounds;
    apply(text, names, within, |c, b| {
        world.set(c, b);
    })?;
    Ok(world.count().saturating_sub(before))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portal::Door;
    use crate::world::Bounds;

    fn names() -> Vec<String> {
        ["grass", "stone", "leaves"].map(String::from).to_vec()
    }

    #[test]
    fn boxes_hollow_round_and_blocks() {
        let text = r#"{
            "boxes": [
                { "block": "grass", "from": [0, 0, 0], "to": [2, 0, 2] },
                { "block": "stone", "from": [10, 0, 0], "to": [12, 2, 2], "hollow": true },
                { "block": "leaves", "from": [20, 0, 0], "to": [24, 4, 4], "round": 1.0 }
            ],
            "blocks": [[1, 0, 1, "air"], [5, 5, 5, "stone"]]
        }"#;
        let c = load(text, &names()).unwrap();
        assert_eq!(c.get(&[0, 0, 0]), Some(&1));
        assert_eq!(c.get(&[1, 0, 1]), None, "air clears");
        assert_eq!(c.get(&[5, 5, 5]), Some(&2));
        assert_eq!(c.get(&[11, 1, 1]), None, "hollow middle");
        assert_eq!(c.get(&[10, 1, 1]), Some(&2));
        assert_eq!(c.get(&[22, 2, 2]), Some(&3), "round centre");
        assert_eq!(c.get(&[20, 0, 0]), None, "round corner");
        assert_eq!(c.values().filter(|&&b| b == 1).count(), 8);
    }

    #[test]
    fn a_thick_hollow_box_keeps_a_shell_that_thick() {
        let text = r#"{"boxes": [{ "block": "stone", "from": [0, 0, 0], "to": [5, 5, 5], "hollow": true, "thick": 2 }]}"#;
        let c = load(text, &names()).unwrap();
        assert_eq!(c.get(&[1, 1, 1]), Some(&2), "the shell's inner layer");
        assert_eq!(c.get(&[2, 2, 2]), None, "the middle");
        assert_eq!(c.get(&[3, 3, 3]), None, "the middle");
        assert_eq!(c.get(&[4, 4, 4]), Some(&2), "the shell's inner layer, far side");
        assert_eq!(c.len(), 6 * 6 * 6 - 2 * 2 * 2);
    }

    #[test]
    fn a_region_of_a_level_is_that_region_of_the_whole() {
        let text = r#"{
            "boxes": [
                { "block": "grass", "from": [-20, -1, -20], "to": [20, -1, 20] },
                { "block": "stone", "from": [-3, 0, -3], "to": [6, 5, 6], "hollow": true, "thick": 2 },
                { "block": "leaves", "from": [2, 2, 2], "to": [9, 9, 9], "round": 0.9 },
                { "block": "air", "from": [0, -1, 0], "to": [1, -1, 1] }
            ],
            "blocks": [[4, 4, 4, "stone"], [30, 0, 0, "stone"]]
        }"#;
        let whole = load(text, &names()).unwrap();
        let region = Bounds { lo: [-2, -1, 1], hi: [5, 4, 8] };
        let mut part = HashMap::new();
        apply(text, &names(), region, |c, b| {
            assert!(region.contains(c), "{c:?} is outside the region");
            if b == 0 { part.remove(&c); } else { part.insert(c, b); }
        })
        .unwrap();
        let want: HashMap<Cell, u16> = whole.into_iter().filter(|(c, _)| region.contains(*c)).collect();
        assert!(!want.is_empty());
        assert_eq!(part, want);
    }

    #[test]
    fn a_level_with_a_bad_block_puts_nothing() {
        let text = r#"{"boxes": [{ "block": "stone", "from": [0, 0, 0], "to": [3, 3, 3] }, { "block": "lava", "from": [0, 0, 0], "to": [0, 0, 0] }]}"#;
        let mut put = 0;
        assert!(apply(text, &names(), everywhere(), |_, _| put += 1).is_err());
        assert_eq!(put, 0);
        // corners far apart are refused, not overflowed
        let wide = r#"{"boxes": [{ "block": "stone", "from": [-2147483648, 0, 0], "to": [2147483647, 0, 0] }]}"#;
        assert!(load(wide, &names()).is_err());
    }

    #[test]
    fn a_level_may_give_its_bounds() {
        let b = bounds(r#"{"bounds": {"lo": [-100, -8, -50], "hi": [100, 40, 50]}}"#).unwrap();
        assert_eq!((b.lo, b.hi), ([-100, -8, -50], [100, 40, 50]));
        assert_eq!(bounds(r#"{"boxes": []}"#), None, "none given: the default");
        for bad in [
            r#"{"bounds": {"lo": [10, 0, 0], "hi": [10, 5, 5]}}"#,
            r#"{"bounds": {"lo": [0, 0, 0], "hi": [5, 5000, 5]}}"#,
            r#"{"bounds": {"lo": [0, 0, 0], "hi": [99999999, 5, 5]}}"#,
            r#"{"bounds": {"lo": [0, 0], "hi": [5, 5, 5]}}"#,
            r#"{"bounds": {"lo": [0, 0, 0.5], "hi": [5, 5, 5]}}"#,
        ] {
            assert_eq!(bounds(bad), None, "{bad}");
        }
    }

    #[test]
    fn world_settings() {
        assert_eq!(settings(r#"{"world":{"sky":"void"},"boxes":[]}"#), r#"{"sky":"void"}"#);
        assert_eq!(settings(r#"{"boxes":[]}"#), "{}");
        assert_eq!(settings("nonsense"), "{}");
    }

    #[test]
    fn errors() {
        assert!(load(r#"{"boxes":[{"block":"lava","from":[0,0,0],"to":[0,0,0]}]}"#, &names()).is_err());
        assert!(load(r#"{"boxes":[{"block":"stone","from":[0,0,0],"to":[900,0,0]}]}"#, &names()).is_err());
        assert!(load("not json", &names()).is_err());
    }

    fn load_level(name: &str) -> (World, Vec<String>, String, usize) {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pal = std::fs::read_to_string(base.join("data/blocks.json")).unwrap();
        let names = crate::palette_names(&pal).unwrap();
        let text = std::fs::read_to_string(base.join(format!("data/levels/{name}.json"))).unwrap();
        let mut w = World::new(Bounds::default(), names.len() as u16);
        let n = fill(&mut w, &text, &names).unwrap();
        (w, names, text, n)
    }

    #[test]
    fn the_void_loads() {
        let (w, names, text, n) = load_level("void");
        // a flat, nearly empty space: the floor is endless (not blocks), you arrive on it with
        // nothing overhead, and the pillars of the Circle glow at the top
        assert!(n > 4_000 && n < 400_000, "{n}");
        assert_eq!(w.ground(0, 0, 191), None);
        // 25.6 km across, so there is room to build
        let b = bounds(&text).expect("the void gives its bounds");
        assert_eq!((b.lo[0], b.hi[0], b.lo[2], b.hi[2]), (-51_200, 51_200, -51_200, 51_200));
        let s: serde_json::Value = serde_json::from_str(&settings(&text)).unwrap();
        assert_eq!(s["sky"], "void");
        assert_eq!(s["floor"], 0);
        assert_eq!(s["cameras"][0]["name"], "gate");
        let lumen = names.iter().position(|n| n == "lumen").unwrap() as u16 + 1;
        assert_eq!(w.get([44, 28, 0]), lumen);
    }

    #[test]
    fn the_islands_load() {
        let (w, names, text, n) = load_level("islands");
        assert!(n > 2_400_000, "{n}");
        // you arrive standing on the plaza, with nothing overhead
        assert_eq!(w.ground(0, 0, 191), Some(0));
        let fire = names.iter().position(|n| n == "fire").unwrap() as u16 + 1;
        assert!(w.keys().len() > 100);
        assert!(settings(&text).contains("islands"));
        // braziers burn round the plaza
        assert_eq!(w.get([26, 8, 26]), fire);
    }

    #[test]
    fn the_yard_loads() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pal = std::fs::read_to_string(base.join("data/blocks.json")).unwrap();
        let names = crate::palette_names(&pal).unwrap();
        let text = std::fs::read_to_string(base.join("data/levels/yard.json")).unwrap();
        let mut w = World::new(Bounds::default(), names.len() as u16);
        let n = fill(&mut w, &text, &names).unwrap();
        assert!(n > 1000, "{n}");
        assert_eq!(w.count(), n);
        // the character stands at the origin, on the ground
        assert_eq!(w.ground(0, 0, 40), Some(0));
    }

    fn entities_of(path: &str) -> Vec<Component> {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(base.join(path)).unwrap();
        entities(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn every_level_file_converts() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut seen = 0;
        for dir in ["data/levels", "data/levels/dungeons"] {
            for entry in std::fs::read_dir(base.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "json") {
                    let rel = path.strip_prefix(&base).unwrap().to_string_lossy().into_owned();
                    entities_of(&rel);
                    seen += 1;
                }
            }
        }
        assert!(seen >= 7, "found {seen} level files");
    }

    #[test]
    fn the_yard_has_one_doorway_to_the_void() {
        let all = entities_of("data/levels/yard.json");
        let portals: Vec<&Portal> = all
            .iter()
            .filter_map(|c| match c {
                Component::Portal(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(portals.len(), 1);
        assert!(matches!(&portals[0].to, Dest::Url { url, .. } if url == ":7780"));
    }

    #[test]
    fn the_crypt_has_its_altar_and_a_doorway() {
        let all = entities_of("data/levels/dungeons/crypt.json");
        let objective = all.iter().find_map(|c| match c {
            Component::Objective(o) => Some(o),
            _ => None,
        });
        let objective = objective.expect("an objective");
        assert_eq!(objective.name, "the altar");
        assert_eq!(objective.radius, 6.0);
        assert!(all.iter().any(|c| matches!(c, Component::Portal(_))));
    }

    #[test]
    fn the_void_settings_keep_the_sky_and_lose_the_portals() {
        let all = entities_of("data/levels/void.json");
        let json = all
            .iter()
            .find_map(|c| match c {
                Component::Settings(s) => Some(s.json.as_str()),
                _ => None,
            })
            .expect("settings");
        let s: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(s["sky"], "void");
        assert!(s.get("portals").is_none());
    }

    #[test]
    fn a_dungeon_url_is_a_dungeon_destination() {
        let text = r#"{ "world": { "portals": [
            {"name": "down", "at": [0, 0, 5], "level": "dungeons/crypt", "url": "dungeon:crypt", "arrive": [0, 0, 1]}
        ] } }"#;
        let all = entities(text).unwrap();
        let Component::Portal(p) = &all[0] else { panic!("a portal first") };
        assert_eq!(p.to, Dest::Dungeon { name: "crypt".into(), arrive: [0.0, 0.0, 1.0] });
    }

    #[test]
    fn entities_agree_with_parse() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut seen = 0;
        for dir in ["data/levels", "data/levels/dungeons"] {
            for entry in std::fs::read_dir(base.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if !path.extension().is_some_and(|e| e == "json") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let v: Value = serde_json::from_str(&text).unwrap();
                let world = v.get("world").cloned().unwrap_or(Value::Null);
                let doors = crate::portal::parse(&world);
                let all = entities(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let portals: Vec<&Portal> = all
                    .iter()
                    .filter_map(|c| match c {
                        Component::Portal(p) => Some(p),
                        _ => None,
                    })
                    .collect();
                assert_eq!(portals.len(), doors.len(), "{}", path.display());
                for (p, door) in portals.iter().zip(&doors) {
                    let Some((url, arrive)) = p.travel() else {
                        panic!("{}: {:?}", path.display(), p.to)
                    };
                    assert_eq!(Door::from_portal(p, url, arrive), *door, "{} {}", path.display(), p.name);
                }
                seen += 1;
            }
        }
        assert!(seen >= 7, "found {seen} level files");
    }

    #[test]
    fn a_doorway_with_a_number_too_big_for_f32_is_left_out() {
        // as `portal::parse` does: the doorway is skipped, and a non-finite yaw is 0
        let text = r#"{ "world": { "portals": [
            {"name": "far", "at": [1e300, 0, 0], "level": "far", "url": ":7781", "arrive": [0, 0, 0]},
            {"name": "spun", "at": [0, 0, 5], "level": "spun", "url": ":7781", "arrive": [0, 0, 0], "yaw": 1e300},
            {"name": "near", "at": [0, 0, 5], "level": "near", "url": ":7781", "arrive": [0, 0, 0]}
        ] } }"#;
        let all = entities(text).unwrap();
        let portals: Vec<&Portal> = all
            .iter()
            .filter_map(|c| match c {
                Component::Portal(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(portals.len(), 2);
        assert_eq!(portals[0].name, "spun");
        assert_eq!(portals[0].yaw, 0.0);
        assert_eq!(portals[1].name, "near");
    }
}
