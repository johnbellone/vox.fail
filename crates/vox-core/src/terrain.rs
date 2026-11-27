//! Generated ground for an atlas: a big map made of tiles, each an ordinary world
//! (`docs/design/world-network.md`, *The atlas*).
//!
//! The ground's height at a column depends only on the atlas's seed and the column's place on
//! the whole map (atlas coordinates), never on which tile is asking, so two tiles agree exactly
//! along their shared edge. Within a tile, positions are the world's own blocks: tile `(i, j)`'s
//! block `(x, z)` is atlas block `(i * TILE + x, j * TILE + z)`.
//!
//! Integer hashing and plain arithmetic only, so the same seed gives the same ground on every
//! machine this runs on.

use crate::{Bounds, World};

/// A tile is this many blocks across: a world's whole width (`Bounds::default`).
pub const TILE: i32 = 512;
/// Water fills up to this height (blocks).
pub const SEA: i32 = 4;

/// Which tile of which atlas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    /// the atlas's name (`earth`): part of every tile's world id
    pub atlas: String,
    pub seed: u64,
    /// the atlas is `size[0]` tiles east to west and `size[1]` north to south
    pub size: [i32; 2],
    /// this tile, 0-based: `[0, 0]` is the north-west corner
    pub at: [i32; 2],
}

/// The four ways off a tile, and how far each goes (in tiles).
pub const EDGES: [(&str, [i32; 2]); 4] = [("east", [1, 0]), ("west", [-1, 0]), ("south", [0, 1]), ("north", [0, -1])];

impl Tile {
    /// A tile from its world id, `atlas-<name>-<i>-<j>`, if that's a tile of this atlas.
    pub fn from_id(id: &str, atlas: &str, seed: u64, size: [i32; 2]) -> Option<Tile> {
        let rest = id.strip_prefix("atlas-")?.strip_prefix(atlas)?.strip_prefix('-')?;
        let (i, j) = rest.split_once('-')?;
        // plain digits only, so each tile has exactly one id
        let num = |s: &str| (!s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))).then(|| s.parse::<i32>().ok()).flatten();
        let at = [num(i)?, num(j)?];
        (at[0] < size[0] && at[1] < size[1]).then(|| Tile { atlas: atlas.into(), seed, size, at })
    }

    /// Its world id.
    pub fn id(&self) -> String {
        tile_id(&self.atlas, self.at)
    }

    /// The tile `d` tiles away, if the atlas goes that far.
    pub fn next(&self, d: [i32; 2]) -> Option<Tile> {
        let at = [self.at[0] + d[0], self.at[1] + d[1]];
        (at[0] >= 0 && at[1] >= 0 && at[0] < self.size[0] && at[1] < self.size[1]).then(|| Tile { at, ..self.clone() })
    }

    /// The ways off this tile that lead somewhere (not off the map).
    pub fn edges(&self) -> Vec<&'static str> {
        EDGES.iter().filter(|(_, d)| self.next(*d).is_some()).map(|(n, _)| *n).collect()
    }

    /// Ground height (blocks, the top solid block) at this tile's column `(x, z)`.
    pub fn height(&self, x: i32, z: i32) -> i32 {
        height(self.seed, self.at[0] * TILE + x, self.at[1] * TILE + z)
    }

    /// The world's settings for this tile (sky, and which edges lead on).
    pub fn settings(&self) -> serde_json::Value {
        serde_json::json!({
            "sky": "earth",
            "time": 12.0,
            "atlas": { "name": self.atlas, "tile": self.at, "size": self.size, "edges": self.edges() },
            // the atlas's seed and this tile's place on the map, so dips run on across edges
            "relief": { "seed": self.seed, "origin": [self.at[0].saturating_mul(TILE), self.at[1].saturating_mul(TILE)], "version": crate::relief::VERSION },
            "hotbar": ["grass", "dirt", "stone", "planks", "log", "sand", "glass", "cobblestone", "bricks"],
        })
    }
}

/// The world id of tile `at` of atlas `name`.
pub fn tile_id(name: &str, at: [i32; 2]) -> String {
    format!("atlas-{name}-{}-{}", at[0], at[1])
}

/// A 64-bit mix (splitmix64's finaliser).
fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A value in 0..65536 for lattice point `(x, z)` of octave `o`.
fn lattice(seed: u64, o: u32, x: i64, z: i64) -> i64 {
    let h = mix(seed ^ mix((x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (z as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f) ^ u64::from(o) << 56));
    (h >> 48) as i64
}

/// Smooth value noise at `(x, z)` with lattice spacing `step` (a power of two), in 0..65536,
/// in whole numbers only.
fn noise(seed: u64, o: u32, x: i32, z: i32, step: i32) -> i64 {
    let (x, z, s) = (i64::from(x), i64::from(z), i64::from(step));
    let (x0, z0) = (x.div_euclid(s), z.div_euclid(s));
    let (fx, fz) = (x.rem_euclid(s) * 1024 / s, z.rem_euclid(s) * 1024 / s);
    // smoothstep in 0..1024
    let ease = |t: i64| t * t * (3 * 1024 - 2 * t) / (1024 * 1024);
    let (u, v) = (ease(fx), ease(fz));
    let a = lattice(seed, o, x0, z0);
    let b = lattice(seed, o, x0 + 1, z0);
    let c = lattice(seed, o, x0, z0 + 1);
    let d = lattice(seed, o, x0 + 1, z0 + 1);
    let top = a + (b - a) * u / 1024;
    let bottom = c + (d - c) * u / 1024;
    top + (bottom - top) * v / 1024
}

/// Ground height at atlas column `(x, z)`, blocks: plains and hills, with lakes and seas below
/// `SEA`, from about -14 to 62.
pub fn height(seed: u64, x: i32, z: i32) -> i32 {
    // broad land and sea, then hills, then roughness
    let n = noise(seed, 0, x, z, 512) * 6 + noise(seed, 1, x, z, 128) * 3 + noise(seed, 2, x, z, 32);
    // 0..655360 → about -14..62
    (n * 76 / 655_360 - 14) as i32
}

/// Fill `world` with tile `t`'s ground: stone, then dirt, then grass (sand at the shore and
/// under water), and water up to `SEA`. Blocks are looked up by name in `palette`; one that's
/// missing is left out.
pub fn fill(world: &mut World, t: &Tile, palette: &[String]) {
    let id = |name: &str| palette.iter().position(|n| n == name).map(|i| i as u16 + 1);
    let (stone, dirt, grass, sand, water) = (id("stone"), id("dirt"), id("grass"), id("sand"), id("water"));
    let b: Bounds = world.bounds;
    for x in b.lo[0]..b.hi[0] {
        for z in b.lo[2]..b.hi[2] {
            let h = t.height(x, z).clamp(b.lo[1], b.hi[1] - 1);
            let shore = h <= SEA + 1;
            for y in b.lo[1]..=h {
                let block = if y == h {
                    if shore { sand } else { grass }
                } else if y >= h - 3 {
                    if shore { sand } else { dirt }
                } else {
                    stone
                };
                if let Some(block) = block {
                    world.set([x, y, z], block);
                }
            }
            if let Some(water) = water {
                for y in h + 1..=SEA.min(b.hi[1] - 1) {
                    world.set([x, y, z], water);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(at: [i32; 2]) -> Tile {
        Tile { atlas: "earth".into(), seed: 7, size: [4, 3], at }
    }

    #[test]
    fn tiles_and_their_ids() {
        let t = Tile::from_id("atlas-earth-2-1", "earth", 7, [4, 3]).unwrap();
        assert_eq!(t.at, [2, 1]);
        assert_eq!(t.id(), "atlas-earth-2-1");
        for bad in ["atlas-earth-4-0", "atlas-earth-0-3", "atlas-earth--1-0", "atlas-earth-01-0", "atlas-mars-0-0", "atlas-earth-0", "atlas-earth-a-0", "void"] {
            assert!(Tile::from_id(bad, "earth", 7, [4, 3]).is_none(), "{bad}");
        }
        // the corner leads east and south only; the middle every way
        assert_eq!(tile([0, 0]).edges(), ["east", "south"]);
        assert_eq!(tile([1, 1]).edges().len(), 4);
        assert_eq!(tile([3, 2]).next([1, 0]), None);
        assert_eq!(tile([0, 0]).next([1, 0]).unwrap().at, [1, 0]);
    }

    #[test]
    fn ground_agrees_across_edges() {
        let (a, b) = (tile([0, 0]), tile([1, 0]));
        // tile 0's column x is tile 1's column x - TILE: the same place
        for z in [-256, -3, 0, 100, 255] {
            for x in [250, 255, 256, 300] {
                assert_eq!(a.height(x, z), b.height(x - TILE, z));
            }
        }
        // the same every time, different with another seed, and in range
        assert_eq!(height(7, 1000, -2000), height(7, 1000, -2000));
        let hs: Vec<i32> = (0..2000).map(|i| height(7, i * 37, i * 53)).collect();
        assert!(hs.iter().all(|h| (-16..=64).contains(h)), "{:?}", hs.iter().minmax());
        assert!(hs.iter().any(|&h| h < SEA) && hs.iter().any(|&h| h > SEA + 10), "some sea, some hills");
        assert!((0..200).any(|i| height(7, i * 41, 0) != height(8, i * 41, 0)));
        // neighbouring columns don't jump (walkable ground)
        for i in 0..500 {
            assert!((height(7, i, 77) - height(7, i + 1, 77)).abs() <= 3);
        }
    }

    trait MinMax {
        fn minmax(self) -> (i32, i32);
    }
    impl<'a, I: Iterator<Item = &'a i32>> MinMax for I {
        fn minmax(self) -> (i32, i32) {
            self.fold((i32::MAX, i32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
        }
    }

    #[test]
    fn filled_ground() {
        let palette: Vec<String> = ["grass", "dirt", "stone", "sand", "water"].map(String::from).to_vec();
        let mut w = World::new(Bounds { lo: [-8, -32, -8], hi: [8, 96, 8] }, palette.len() as u16);
        let t = tile([1, 1]);
        fill(&mut w, &t, &palette);
        for (x, z) in [(-8, -8), (0, 0), (7, 7)] {
            let h = t.height(x, z);
            // `ground` is where you'd stand: on the top block, or on the water if that counts
            let top = w.ground(x, z, 95).unwrap();
            assert!(top == h + 1 || (h < SEA && top == SEA + 1), "column {x},{z}: {top} for {h}");
        }
    }
}
