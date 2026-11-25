//! Level files: boxes of blocks (solid, hollow or rounded) and single blocks, by name.
//!
//! ```json
//! { "name": "yard", "boxes": [ { "block": "grass", "from": [-20, -1, -20], "to": [20, -1, 20] } ],
//!   "blocks": [ [0, 0, 0, "stone"] ] }
//! ```
//! Later boxes overwrite earlier ones; `"air"` clears.

use crate::world::{Cell, World};
use serde::Deserialize;
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

/// A level's blocks: position -> id, where id is the index in `names` plus one.
pub fn load(text: &str, names: &[String]) -> Result<HashMap<Cell, u16>, String> {
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
    let mut cells = HashMap::new();
    let mut put = |p: Cell, b: u16| {
        if b == 0 {
            cells.remove(&p);
        } else {
            cells.insert(p, b);
        }
    };
    for bx in &f.boxes {
        let b = id(&bx.block)?;
        let lo: [i32; 3] = std::array::from_fn(|i| bx.from[i].min(bx.to[i]));
        let hi: [i32; 3] = std::array::from_fn(|i| bx.from[i].max(bx.to[i]));
        let size: [f32; 3] = std::array::from_fn(|i| (hi[i] - lo[i]) as f32);
        if (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() > 400.0 {
            return Err(format!("a {} box is too big", bx.block));
        }
        let centre: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) as f32 * 0.5);
        let half: [f32; 3] = std::array::from_fn(|i| (size[i] * 0.5 + 0.5).max(0.5));
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    let p = [x, y, z];
                    let inside = (0..3).all(|i| p[i] > lo[i] && p[i] < hi[i]);
                    if bx.hollow && inside {
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
    for (x, y, z, name) in &f.blocks {
        put([*x, *y, *z], id(name)?);
    }
    Ok(cells)
}

/// Put a level's blocks into a world (those outside its bounds are left out); returns how
/// many went in.
pub fn fill(world: &mut World, text: &str, names: &[String]) -> Result<usize, String> {
    let cells = load(text, names)?;
    let mut n = 0;
    for (c, b) in cells {
        if world.bounds.contains(c) {
            world.set(c, b);
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn world_settings() {
        assert_eq!(settings(r#"{"world":{"sky":"void"},"boxes":[]}"#), r#"{"sky":"void"}"#);
        assert_eq!(settings(r#"{"boxes":[]}"#), "{}");
        assert_eq!(settings("nonsense"), "{}");
    }

    #[test]
    fn errors() {
        assert!(load(r#"{"boxes":[{"block":"lava","from":[0,0,0],"to":[0,0,0]}]}"#, &names()).is_err());
        assert!(load(r#"{"boxes":[{"block":"stone","from":[0,0,0],"to":[500,0,0]}]}"#, &names()).is_err());
        assert!(load("not json", &names()).is_err());
    }

    fn load_level(name: &str) -> (World, Vec<String>, String, usize) {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
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
        assert!(n > 500 && n < 50_000, "{n}");
        assert_eq!(w.ground(0, 0, 95), None);
        let s: serde_json::Value = serde_json::from_str(&settings(&text)).unwrap();
        assert_eq!(s["sky"], "void");
        assert_eq!(s["floor"], 0);
        assert_eq!(s["cameras"][0]["name"], "gate");
        let lumen = names.iter().position(|n| n == "lumen").unwrap() as u16 + 1;
        assert_eq!(w.get([22, 14, 0]), lumen);
    }

    #[test]
    fn the_islands_load() {
        let (w, names, text, n) = load_level("islands");
        assert!(n > 300_000, "{n}");
        // you arrive standing on the plaza, with nothing overhead
        assert_eq!(w.ground(0, 0, 95), Some(0));
        let fire = names.iter().position(|n| n == "fire").unwrap() as u16 + 1;
        assert!(w.keys().len() > 100);
        assert!(settings(&text).contains("islands"));
        // braziers burn round the plaza
        assert_eq!(w.get([13, 4, 13]), fire);
    }

    #[test]
    fn the_yard_loads() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let pal = std::fs::read_to_string(base.join("data/blocks.json")).unwrap();
        let names = crate::palette_names(&pal).unwrap();
        let text = std::fs::read_to_string(base.join("data/levels/yard.json")).unwrap();
        let mut w = World::new(Bounds::default(), names.len() as u16);
        let n = fill(&mut w, &text, &names).unwrap();
        assert!(n > 1000, "{n}");
        assert_eq!(w.count(), n);
        // the character stands at the origin, on the ground
        assert_eq!(w.ground(0, 0, 20), Some(0));
    }
}
