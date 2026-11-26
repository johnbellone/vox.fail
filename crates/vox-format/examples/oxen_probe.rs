//! A probe for choosing between the plain snapshot store and Oxen (docs/design/world-format.md,
//! *Oxen*): builds a world of `tiles × tiles` copies of the islands, then `rounds` rounds of
//! building, each touching a few regions. Every round is written twice:
//!
//! - as a `DirStore` snapshot under `<out>/plain` (what the world server does), and
//! - as the changed region files under `<out>/rounds/<n>/<x_z>.voxr`, for a script to copy into
//!   an Oxen working tree and commit (`tools/dev/oxen_probe.sh`).
//!
//! `cargo run --release -p vox-format --example oxen_probe -- <out> [tiles 4] [rounds 20] [regions per round 3]`

use std::collections::BTreeSet;
use std::path::PathBuf;
use vox_core::world::{Bounds, CHUNK, World};
use vox_format::{DirStore, Region, RegionKey, Snapshots, region_of};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(args.get(1).expect("an output folder"));
    let n = |i: usize, d: i32| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    let (tiles, rounds, per_round) = (n(2, 4), n(3, 20), n(4, 3));

    let blocks = std::fs::read_to_string("data/blocks.json").expect("run from the repo root");
    let names = vox_core::palette_names(&blocks).expect("palette");
    let level = std::fs::read_to_string("data/levels/islands.json").expect("islands");
    let mut one = World::new(Bounds::default(), names.len() as u16);
    vox_core::level::fill(&mut one, &level, &names).expect("fill");

    // the islands, `tiles` times each way, a tile apart (512 blocks: one region)
    let span = 16 * CHUNK;
    let half = tiles * span / 2;
    let mut world = World::new(Bounds { lo: [-half, -32, -half], hi: [half, 96, half] }, one.kinds);
    for tx in 0..tiles {
        for tz in 0..tiles {
            for k in one.keys() {
                let c = one.chunk(k).unwrap().clone();
                let to = [k.0[0] + (tx * span - half + 256) / CHUNK, k.0[1], k.0[2] + (tz * span - half + 256) / CHUNK];
                world.put_chunk(vox_core::world::ChunkKey(to), c);
            }
        }
    }
    let keys: BTreeSet<RegionKey> = world.keys().into_iter().map(region_of).collect();
    let keys: Vec<RegionKey> = keys.into_iter().collect();
    eprintln!("{} blocks in {} regions", world.count(), keys.len());

    let _ = std::fs::remove_dir_all(&out);
    let snaps = Snapshots::new(DirStore::new(out.join("plain")));
    let region = |w: &World, r: RegionKey| {
        let ks: Vec<_> = w.keys().into_iter().filter(|k| region_of(*k) == r).collect();
        Region::from_chunks(r, 1, ks.into_iter().map(|k| (k, w.chunk(k).unwrap())))
    };
    let write = |round: i32, regions: &[Region]| {
        let dir = out.join("rounds").join(round.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        for r in regions {
            std::fs::write(dir.join(format!("{}.voxr", r.key)), r.encode().unwrap().0).unwrap();
        }
    };

    // round 0: the whole world
    let all: Vec<Region> = keys.iter().map(|r| region(&world, *r)).collect();
    write(0, &all);
    let mut prev = snaps.take(None, &all, 1_000_000, 0, 1).unwrap();

    // then building: a small house's worth of blocks (a 6 × 4 × 6 shell) in a few regions a round
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    for round in 1..=rounds {
        let mut touched = BTreeSet::new();
        for _ in 0..per_round {
            let r = keys[(next() % keys.len() as u64) as usize];
            let (x0, z0) = (r.x * span + (next() % (span as u64 - 8)) as i32, r.z * span + (next() % (span as u64 - 8)) as i32);
            let y0 = 8 + (next() % 16) as i32;
            for x in 0..6 {
                for z in 0..6 {
                    for y in 0..4 {
                        if x == 0 || x == 5 || z == 0 || z == 5 || y == 3 {
                            world.set([x0 + x, y0 + y, z0 + z], 1 + (round as u16 % world.kinds));
                        }
                    }
                }
            }
            touched.insert(r);
        }
        let changed: Vec<Region> = touched.iter().map(|r| region(&world, *r)).collect();
        write(round, &changed);
        prev = snaps.take(Some(&prev), &changed, 1_000_000 + round as u64 * 300, round as u64, 1).unwrap();
    }
    eprintln!("{} snapshots, latest {}", snaps.list().unwrap().len(), prev.id);
}
