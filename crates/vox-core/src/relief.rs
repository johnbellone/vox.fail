//! Ground relief: the shallow dips in natural ground, part of world generation
//! (`docs/design/ground-relief.md`).
//!
//! How far below a block's top the ground is at a point depends only on the world's relief seed
//! and the point's place, never on what is stored, so every machine works out the same dips and
//! nothing is saved or sent but the seed. Two octaves of value noise: swales on a 4-block
//! lattice (the hollows where water gathers) and roughness on a 1-block lattice that breaks up
//! their edges.
//!
//! Places are in [`SUB`]ths of a block. Hashing is `u32` and the rest is plain integer
//! arithmetic, so the block shader (`assets/shaders/block.wesl`, `relief_depth`) can mirror it:
//! it uses bit-for-bit the same lattice hashes, and its smooth (floating-point) blend comes
//! within about 1/255 of these values. The lattice spacings are powers of two that divide an
//! atlas tile (512 blocks), and a world's origin is kept to whole swales, so the origin moves
//! the lattice by whole steps (exactly, in the shader too) and tiles join without seams.
//!
//! [`VERSION`] pins the function: changing it changes how every world with relief looks, so a
//! different function is a new version, and the golden values in the tests hold this one.

/// The function this build draws. A world asking for another version gets no relief.
pub const VERSION: u32 = 1;
/// Places are in this many parts of a block.
pub const SUB: i32 = 16;
/// The swales' lattice, in blocks (a power of two dividing an atlas tile).
pub const SWALE: i32 = 4;
/// The roughness's lattice, in blocks.
pub const ROUGH: i32 = 1;
/// The deepest dip, in blocks (6 cm at half-metre blocks), before the block's and the world's
/// own amounts scale it.
pub const MAX_DIP: f32 = 0.12;
/// The share of the noise's range left flat: dips start above it.
const FLAT: i64 = 29_491; // 0.45 of 65,535

/// A world's relief, from its settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relief {
    pub seed: u32,
    /// 0-1: scales every dip
    pub amount: f32,
    /// where this world's block (0, 0) is on the bigger map it belongs to (an atlas tile's
    /// origin, in blocks; always a whole number of swales); (0, 0) for a world on its own
    pub origin: [i32; 2],
}

impl Relief {
    /// The relief a world's settings ask for (`"relief": {"seed", "amount", "version",
    /// "origin"}`), or `None` when they don't, or ask for a version this build doesn't draw.
    /// A missing seed is made from the world's id, so a world keeps its dips.
    pub fn from_settings(settings: &serde_json::Value, world: &str) -> Option<Relief> {
        let r = settings.get("relief")?;
        let version = r.get("version").and_then(|v| v.as_u64()).unwrap_or(u64::from(VERSION));
        if version != u64::from(VERSION) {
            return None;
        }
        let seed = r.get("seed").and_then(|v| v.as_u64()).map(|s| mix32(s as u32 ^ (s >> 32) as u32)).unwrap_or_else(|| seed_of(world));
        let amount = r.get("amount").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
        if !amount.is_finite() || amount <= 0.0 {
            return None;
        }
        // whole swales only: the shader moves its lattice by whole steps (atlas tiles, 512
        // blocks, always are)
        let at = |i: usize| {
            let v = r.get("origin").and_then(|o| o.get(i)).and_then(|v| v.as_i64()).and_then(|v| i32::try_from(v).ok()).unwrap_or(0);
            v.div_euclid(SWALE) * SWALE
        };
        Some(Relief { seed, amount: amount.min(1.0), origin: [at(0), at(1)] })
    }

    /// The dip at block place `(x, z)` of this world (in [`SUB`]ths), 0 (none) to 255 (the
    /// deepest), before the block's own amount. `None` past the edge of the numbers.
    pub fn depth_at(&self, x: i32, z: i32) -> Option<u8> {
        let ox = self.origin[0].checked_mul(SUB)?.checked_add(x)?;
        let oz = self.origin[1].checked_mul(SUB)?.checked_add(z)?;
        Some(depth(self.seed, ox, oz))
    }
}

/// A seed from a world's id (FNV-1a, then mixed).
pub fn seed_of(world: &str) -> u32 {
    let h = world.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    mix32(h)
}

/// A 32-bit mix (the "lowbias32" finaliser): every input bit moves about half the output.
pub fn mix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// A value in 0..65536 for lattice point `(x, z)` of octave `o`.
pub fn lattice(seed: u32, o: u32, x: i32, z: i32) -> u32 {
    mix32(seed ^ mix32((x as u32).wrapping_mul(0x9e37_79b1) ^ mix32((z as u32) ^ o.wrapping_mul(0x632b_e5ab)))) >> 16
}

/// Smooth value noise at `(x, z)` (in [`SUB`]ths) on a lattice `step` blocks apart, 0..65536.
fn noise(seed: u32, o: u32, x: i32, z: i32, step: i32) -> i64 {
    let s = i64::from(step) * i64::from(SUB);
    let (x, z) = (i64::from(x), i64::from(z));
    // lattice indices fit an i32: x / s with |x| < 2^31 and s >= 16
    let (x0, z0) = (x.div_euclid(s) as i32, z.div_euclid(s) as i32);
    let (fx, fz) = (x.rem_euclid(s) * 1024 / s, z.rem_euclid(s) * 1024 / s);
    // smoothstep in 0..1024
    let ease = |t: i64| t * t * (3 * 1024 - 2 * t) / (1024 * 1024);
    let (u, v) = (ease(fx), ease(fz));
    let l = |dx: i32, dz: i32| i64::from(lattice(seed, o, x0.wrapping_add(dx), z0.wrapping_add(dz)));
    let top = l(0, 0) + (l(1, 0) - l(0, 0)) * u / 1024;
    let bottom = l(0, 1) + (l(1, 1) - l(0, 1)) * u / 1024;
    top + (bottom - top) * v / 1024
}

/// How deep the ground dips at `(x, z)` (in [`SUB`]ths of a block, on the whole map), 0 (flat,
/// at the block's top) to 255 (the deepest, [`MAX_DIP`] before the amounts). Rounded bowls:
/// the noise above [`FLAT`], squared.
pub fn depth(seed: u32, x: i32, z: i32) -> u8 {
    let v = (noise(seed, 0, x, z, SWALE) * 3 + noise(seed, 1, x, z, ROUGH)) / 4;
    let t = ((v - FLAT).max(0) * 255 / (65_535 - FLAT)).min(255);
    (t * t / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These pin version 1. If one changes, every world with relief looks different: make a
    /// new version instead.
    #[test]
    fn golden_values() {
        assert_eq!(mix32(0), 0);
        assert_eq!(mix32(1), 1_753_845_952);
        assert_eq!(lattice(7, 0, 0, 0), lattice(7, 0, 0, 0));
        let got: Vec<u8> = [(0, 0), (8, 8), (100, -37), (-5000, 2222), (123_456, -654_321), (16, 0), (17, 0), (40, 200), (-77, -300), (300, 300), (1000, 31), (-2048, 64)]
            .iter()
            .map(|&(x, z)| depth(0x5eed, x, z))
            .collect();
        assert_eq!(got, GOLDEN.to_vec(), "relief version 1 changed");
    }
    const GOLDEN: [u8; 12] = [0, 0, 20, 58, 0, 0, 0, 18, 44, 20, 0, 0];

    #[test]
    fn there_are_dips_and_flats_in_fair_shares() {
        let (mut flat, mut deep, mut n) = (0, 0, 0);
        for x in (0..64 * SUB).step_by(3) {
            for z in (0..64 * SUB).step_by(5) {
                let d = depth(42, x, z);
                n += 1;
                flat += (d == 0) as i32;
                deep += (d > 128) as i32;
            }
        }
        let (flat, deep) = (flat as f32 / n as f32, deep as f32 / n as f32);
        assert!((0.25..0.8).contains(&flat), "flat share {flat}");
        assert!((0.005..0.2).contains(&deep), "deep share {deep}");
    }

    #[test]
    fn it_is_smooth_no_steps_between_neighbours() {
        // the most one sixteenth of a block can change it
        let mut worst = 0;
        for x in -300..300 {
            for z in [-41, 0, 7, 99] {
                let a = i32::from(depth(9, x, z));
                worst = worst.max((a - i32::from(depth(9, x + 1, z))).abs()).max((a - i32::from(depth(9, x, z + 1))).abs());
            }
        }
        assert!(worst <= 40, "a step of {worst}/255 in a sixteenth of a block");
    }

    #[test]
    fn seeds_differ_and_the_same_seed_agrees() {
        let a: Vec<u8> = (0..400).map(|i| depth(1, i * 7, i * 3)).collect();
        let b: Vec<u8> = (0..400).map(|i| depth(2, i * 7, i * 3)).collect();
        assert_ne!(a, b);
        assert_eq!(a, (0..400).map(|i| depth(1, i * 7, i * 3)).collect::<Vec<u8>>());
        assert_ne!(seed_of("sea"), seed_of("islands"));
    }

    #[test]
    fn atlas_tiles_join_where_their_edges_meet() {
        // tile (1, 0)'s block 0 is tile (0, 0)'s block 512: the same place, the same dip
        let t0 = Relief { seed: 5, amount: 1.0, origin: [0, 0] };
        let t1 = Relief { seed: 5, amount: 1.0, origin: [512, 0] };
        for z in (0..512 * SUB).step_by(37) {
            for dx in 0..SUB {
                assert_eq!(t0.depth_at(512 * SUB + dx, z), t1.depth_at(dx, z));
            }
        }
        // far out the numbers end, rather than wrap
        let far = Relief { seed: 5, amount: 1.0, origin: [i32::MAX / 8, 0] };
        assert_eq!(far.depth_at(0, 0), None);
    }

    #[test]
    fn settings_ask_for_it() {
        let s = serde_json::json!({ "relief": { "amount": 0.5 } });
        let r = Relief::from_settings(&s, "sea").unwrap();
        assert_eq!((r.seed, r.amount, r.origin), (seed_of("sea"), 0.5, [0, 0]));
        let s = serde_json::json!({ "relief": { "seed": 77, "origin": [1024, -512] } });
        let r = Relief::from_settings(&s, "x").unwrap();
        assert_eq!((r.amount, r.origin), (1.0, [1024, -512]));
        assert_eq!(Relief::from_settings(&serde_json::json!({}), "x"), None, "off unless asked for");
        assert_eq!(Relief::from_settings(&serde_json::json!({ "relief": { "version": 2 } }), "x"), None);
        assert_eq!(Relief::from_settings(&serde_json::json!({ "relief": { "amount": 0 } }), "x"), None);
        assert_eq!(Relief::from_settings(&serde_json::json!({ "relief": { "amount": "lots" } }), "x").map(|r| r.amount), Some(1.0));
        // an origin between swales is moved down to one, both ways round
        let o = |v: serde_json::Value| Relief::from_settings(&serde_json::json!({ "relief": { "origin": v } }), "x").unwrap().origin;
        assert_eq!(o(serde_json::json!([2, -3])), [0, -4]);
        assert_eq!(o(serde_json::json!([-512, 4099])), [-512, 4096]);
    }
}
