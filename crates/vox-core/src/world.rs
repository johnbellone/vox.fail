//! Blocks in chunks, and the checks an edit has to pass.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A chunk is CHUNK³ blocks.
pub const CHUNK: i32 = 32;
const VOL: usize = (CHUNK * CHUNK * CHUNK) as usize;

/// A block position.
pub type Cell = [i32; 3];

/// Which chunk: block position div CHUNK.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey(pub [i32; 3]);

impl ChunkKey {
    pub fn of(c: Cell) -> Self {
        ChunkKey([c[0].div_euclid(CHUNK), c[1].div_euclid(CHUNK), c[2].div_euclid(CHUNK)])
    }
    /// The chunk's lowest block.
    pub fn origin(self) -> Cell {
        [self.0[0] * CHUNK, self.0[1] * CHUNK, self.0[2] * CHUNK]
    }
    /// The chunks whose meshes a change at `c` touches: its own, plus a neighbour when `c`
    /// is on the chunk's edge (faces and corner shading look one block across).
    pub fn touched(c: Cell) -> Vec<ChunkKey> {
        let mut out = vec![ChunkKey::of(c)];
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let k = ChunkKey::of([c[0] + dx, c[1] + dy, c[2] + dz]);
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            }
        }
        out
    }
}

fn index(c: Cell) -> usize {
    let l = |v: i32| v.rem_euclid(CHUNK) as usize;
    (l(c[1]) * CHUNK as usize + l(c[2])) * CHUNK as usize + l(c[0])
}

/// CHUNK³ block ids (0 = air) and a version that goes up with every change.
///
/// Stored paletted (docs/design/chunk-streaming.md): a chunk of one block throughout (all air, all
/// stone) is just that id; otherwise a palette of the ids it holds and, for each block, an index
/// into it packed into 1, 2, 4, 8 or 16 bits. Most chunks hold a handful of kinds, so most take 4
/// to 16 KB instead of the 64 KB of one `u16` each. The encoding on the wire and on disk is the same
/// run-length form as before.
#[derive(Clone, Debug)]
pub struct Chunk {
    store: Store,
    pub version: u32,
    solid: u32,
}

#[derive(Clone, Debug)]
enum Store {
    /// every block is this one
    Uniform(u16),
    /// `palette[i]` is the id of index `i`; each block's index is `bits` wide, packed low bits
    /// first into `words`, `64 / bits` to a word (no index straddles two words)
    Packed { palette: Vec<u16>, bits: u32, words: Vec<u64> },
}

/// The narrowest packing that holds `n` palette entries.
fn bits_for(n: usize) -> u32 {
    match n {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        17..=256 => 8,
        _ => 16,
    }
}

/// Words that hold every block's index at `bits` each.
fn words_for(bits: u32) -> usize {
    VOL.div_ceil((64 / bits) as usize)
}

/// Where block `i`'s index is: (word, shift).
fn slot(i: usize, bits: u32) -> (usize, u32) {
    let per = (64 / bits) as usize;
    (i / per, (i % per) as u32 * bits)
}

impl Store {
    fn get(&self, i: usize) -> u16 {
        match self {
            Store::Uniform(b) => *b,
            Store::Packed { palette, bits, words } => {
                let (w, shift) = slot(i, *bits);
                let mask = (1u64 << *bits) - 1;
                palette[((words[w] >> shift) & mask) as usize]
            }
        }
    }

    /// Set block `i` to `b`, growing the palette (and widening the packing) when `b` is new.
    fn set(&mut self, i: usize, b: u16) {
        if let Store::Uniform(u) = *self {
            if u == b {
                return;
            }
            // two kinds now: index 0 is the old block everywhere
            *self = Store::Packed { palette: vec![u], bits: 1, words: vec![0; words_for(1)] };
        }
        let Store::Packed { palette, bits, words } = self else { unreachable!("made packed above") };
        let p = match palette.iter().position(|&x| x == b) {
            Some(p) => p,
            None => {
                palette.push(b);
                let want = bits_for(palette.len());
                if want != *bits {
                    *words = repack(words, *bits, want);
                    *bits = want;
                }
                palette.len() - 1
            }
        };
        let (w, shift) = slot(i, *bits);
        let mask = (1u64 << *bits) - 1;
        words[w] = (words[w] & !(mask << shift)) | ((p as u64) << shift);
    }
}

/// Every index in `words` (packed at `from` bits) packed again at `to` bits.
fn repack(words: &[u64], from: u32, to: u32) -> Vec<u64> {
    let mut out = vec![0u64; words_for(to)];
    let mask = (1u64 << from) - 1;
    for i in 0..VOL {
        let (w, shift) = slot(i, from);
        let v = (words[w] >> shift) & mask;
        let (w2, shift2) = slot(i, to);
        out[w2] |= v << shift2;
    }
    out
}

impl Default for Chunk {
    fn default() -> Self {
        Chunk { store: Store::Uniform(0), version: 0, solid: 0 }
    }
}

/// Two chunks are equal when they hold the same blocks at the same version, however each is stored.
impl PartialEq for Chunk {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.solid == other.solid && (0..VOL).all(|i| self.store.get(i) == other.store.get(i))
    }
}

impl Chunk {
    pub fn get(&self, c: Cell) -> u16 {
        self.store.get(index(c))
    }
    /// Set a block; returns the old one.
    pub fn set(&mut self, c: Cell, b: u16) -> u16 {
        let i = index(c);
        let old = self.store.get(i);
        if old != b {
            self.store.set(i, b);
            self.version += 1;
            match (old == 0, b == 0) {
                (true, false) => self.solid += 1,
                (false, true) => self.solid -= 1,
                _ => {}
            }
            // emptied: nothing left to keep but air
            if self.solid == 0 {
                self.store = Store::Uniform(0);
            }
        }
        old
    }
    pub fn is_empty(&self) -> bool {
        self.solid == 0
    }
    /// Bytes this chunk's blocks take in memory (for measuring; not the encoded size).
    pub fn heap_bytes(&self) -> usize {
        match &self.store {
            Store::Uniform(_) => 0,
            Store::Packed { palette, words, .. } => palette.len() * 2 + words.len() * 8,
        }
    }
    /// Run-length encoded: pairs of varint(run length) varint(id).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Store::Uniform(b) = self.store {
            varint(&mut out, VOL as u32);
            varint(&mut out, b as u32);
            return out;
        }
        let mut i = 0;
        while i < VOL {
            let b = self.store.get(i);
            let mut n = 1;
            while i + n < VOL && self.store.get(i + n) == b {
                n += 1;
            }
            varint(&mut out, n as u32);
            varint(&mut out, b as u32);
            i += n;
        }
        out
    }
    pub fn decode(data: &[u8], version: u32) -> Result<Chunk, String> {
        // read the runs first, checked, so a bad chunk is refused before anything is built
        let mut runs: Vec<(usize, u16)> = Vec::new();
        let mut at = 0;
        let mut pos = 0;
        while pos < data.len() {
            let n = read_varint(data, &mut pos)? as usize;
            let b = read_varint(data, &mut pos)?;
            let b = u16::try_from(b).map_err(|_| "block id too big")?;
            if n == 0 || n > VOL - at {
                return Err("chunk data has the wrong length".into());
            }
            runs.push((n, b));
            at += n;
        }
        if at != VOL {
            return Err("chunk data is short".into());
        }
        let solid = runs.iter().filter(|(_, b)| *b != 0).map(|(n, _)| *n as u32).sum();
        // one kind throughout: nothing to store but the id
        if runs.iter().all(|(_, b)| *b == runs[0].1) {
            return Ok(Chunk { store: Store::Uniform(runs[0].1), version, solid });
        }
        let mut palette: Vec<u16> = Vec::new();
        for (_, b) in &runs {
            if !palette.contains(b) {
                palette.push(*b);
            }
        }
        let bits = bits_for(palette.len());
        let mut words = vec![0u64; words_for(bits)];
        let mut i = 0;
        for (n, b) in runs {
            let p = palette.iter().position(|&x| x == b).unwrap_or(0) as u64;
            for j in i..i + n {
                let (w, shift) = slot(j, bits);
                words[w] |= p << shift;
            }
            i += n;
        }
        Ok(Chunk { store: Store::Packed { palette, bits, words }, version, solid })
    }
}

fn varint(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(data: &[u8], pos: &mut usize) -> Result<u32, String> {
    let mut v: u32 = 0;
    for shift in (0..35).step_by(7) {
        let byte = *data.get(*pos).ok_or("chunk data ends mid-number")?;
        *pos += 1;
        v |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("bad number in chunk data".into())
}

/// The blocks a world may use: lo inclusive, hi exclusive.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub lo: Cell,
    pub hi: Cell,
}

impl Default for Bounds {
    /// 1024 × 256 × 1024 blocks (256 × 64 × 256 m), a quarter of it below 0.
    fn default() -> Self {
        Bounds { lo: [-512, -64, -512], hi: [512, 192, 512] }
    }
}

impl Bounds {
    pub fn contains(&self, c: Cell) -> bool {
        (0..3).all(|i| c[i] >= self.lo[i] && c[i] < self.hi[i])
    }
}

/// Why an edit wasn't made.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// outside the world
    Outside,
    /// not a block in this world's palette
    NoSuchBlock,
    /// it's already that block
    NoChange,
    /// further than a player can reach
    TooFar,
    /// too many edits too fast
    TooFast,
    /// inside someone's plot (claimed at the directory) that you can't build in
    Claimed,
    /// a placed prop stands there (new variants go at the end: the wire numbers them)
    Blocked,
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Why::Outside => "that's outside the world",
            Why::NoSuchBlock => "there's no such block here",
            Why::NoChange => "it's already that block",
            Why::TooFar => "that's out of reach",
            Why::TooFast => "slow down",
            Why::Claimed => "that's someone's plot",
            Why::Blocked => "something is in the way",
        })
    }
}

/// A world's blocks.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub bounds: Bounds,
    /// how many block kinds there are (ids 1..=kinds)
    pub kinds: u16,
    chunks: HashMap<ChunkKey, Chunk>,
}

impl World {
    pub fn new(bounds: Bounds, kinds: u16) -> Self {
        World { bounds, kinds, chunks: HashMap::new() }
    }
    pub fn get(&self, c: Cell) -> u16 {
        self.chunks.get(&ChunkKey::of(c)).map_or(0, |ch| ch.get(c))
    }
    pub fn solid(&self, c: Cell) -> bool {
        self.get(c) != 0
    }
    /// Set a block without any checks; returns the old one.
    pub fn set(&mut self, c: Cell, b: u16) -> u16 {
        let k = ChunkKey::of(c);
        if b == 0 && !self.chunks.contains_key(&k) {
            return 0;
        }
        self.chunks.entry(k).or_default().set(c, b)
    }
    /// The checks on an edit, without making it: returns the block that's there now.
    pub fn check(&self, c: Cell, b: u16) -> Result<u16, Why> {
        if !self.bounds.contains(c) {
            return Err(Why::Outside);
        }
        if b > self.kinds {
            return Err(Why::NoSuchBlock);
        }
        let old = self.get(c);
        if old == b {
            return Err(Why::NoChange);
        }
        Ok(old)
    }
    /// Check an edit and make it; returns the old block.
    pub fn edit(&mut self, c: Cell, b: u16) -> Result<u16, Why> {
        let old = self.check(c, b)?;
        self.set(c, b);
        Ok(old)
    }
    pub fn chunk(&self, k: ChunkKey) -> Option<&Chunk> {
        self.chunks.get(&k)
    }
    /// Put a whole chunk in (one that arrived over the network or from storage).
    pub fn put_chunk(&mut self, k: ChunkKey, c: Chunk) {
        self.chunks.insert(k, c);
    }
    /// Chunks with any blocks in, sorted (so they go out in the same order every time).
    pub fn keys(&self) -> Vec<ChunkKey> {
        let mut v: Vec<_> = self.chunks.iter().filter(|(_, c)| !c.is_empty()).map(|(k, _)| *k).collect();
        v.sort();
        v
    }
    /// How many blocks aren't air.
    pub fn count(&self) -> usize {
        self.chunks.values().map(|c| c.solid as usize).sum()
    }
    /// The top of the highest solid block in the column at (x, z), if any, at or below `y`.
    pub fn ground(&self, x: i32, z: i32, from_y: i32) -> Option<i32> {
        (self.bounds.lo[1]..=from_y.min(self.bounds.hi[1] - 1)).rev().find(|&y| self.solid([x, y, z])).map(|y| y + 1)
    }
}

/// Edits allowed per second (a refill rate) with a burst allowance.
#[derive(Clone, Debug)]
pub struct Bucket {
    rate: f32,
    burst: f32,
    tokens: f32,
    last: f64,
}

impl Bucket {
    pub fn new(rate: f32, burst: f32) -> Self {
        Bucket { rate, burst, tokens: burst, last: f64::NAN }
    }
    /// Take one token at time `now` (seconds); false if there isn't one.
    pub fn take(&mut self, now: f64) -> bool {
        if !self.last.is_nan() {
            self.tokens = (self.tokens + (now - self.last).max(0.0) as f32 * self.rate).min(self.burst);
        }
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cell of a chunk at `origin`, in a fixed order.
    fn cells(origin: Cell) -> impl Iterator<Item = Cell> {
        (0..CHUNK).flat_map(move |y| (0..CHUNK).flat_map(move |z| (0..CHUNK).map(move |x| [origin[0] + x, origin[1] + y, origin[2] + z])))
    }

    #[test]
    fn a_chunk_holds_what_was_set_at_every_packing_width() {
        // 2, 3, 5, 17 and 257 kinds cross each width: 1, 2, 4, 8 and 16 bits
        for kinds in [2u16, 3, 5, 17, 257, 1000] {
            let origin = [-64, -32, 96];
            let mut c = Chunk::default();
            let mut want = Vec::new();
            for (i, cell) in cells(origin).enumerate() {
                let b = (i as u16).wrapping_mul(7919) % kinds;
                c.set(cell, b);
                want.push(b);
            }
            for (cell, b) in cells(origin).zip(&want) {
                assert_eq!(c.get(cell), *b, "{kinds} kinds at {cell:?}");
            }
            let solid = want.iter().filter(|b| **b != 0).count();
            assert_eq!(c.solid as usize, solid);
            // the encoding round-trips, and a decoded chunk is the same chunk
            let back = Chunk::decode(&c.encode(), c.version).unwrap();
            assert_eq!(back, c, "{kinds} kinds");
            for (cell, b) in cells(origin).zip(&want) {
                assert_eq!(back.get(cell), *b);
            }
        }
    }

    #[test]
    fn negative_coordinates_land_in_their_own_chunk_cells() {
        let mut c = Chunk::default();
        c.set([-1, -1, -1], 5);
        c.set([-32, -32, -32], 6);
        assert_eq!(c.get([-1, -1, -1]), 5);
        assert_eq!(c.get([31, 31, 31]), 5, "the same cell of a chunk, one chunk over");
        assert_eq!(c.get([-32, -32, -32]), 6);
        assert_eq!(c.get([0, 0, 0]), 6);
        assert_eq!(c.get([-2, -1, -1]), 0);
    }

    #[test]
    fn one_kind_throughout_takes_nothing_and_an_emptied_chunk_goes_back_to_that() {
        let empty = Chunk::default();
        assert_eq!(empty.heap_bytes(), 0);
        // a solid chunk of stone from the wire stays uniform
        let stone = Chunk::decode(&[0x80, 0x80, 0x02, 3], 1).unwrap();
        assert_eq!(stone.heap_bytes(), 0);
        assert_eq!(stone.get([5, 6, 7]), 3);
        assert_eq!(stone.solid as usize, VOL);
        assert_eq!(stone.encode(), vec![0x80, 0x80, 0x02, 3]);
        // two kinds: one bit a block, 4 KB
        let mut c = Chunk::default();
        c.set([1, 2, 3], 9);
        assert_eq!(c.heap_bytes(), 4 + VOL / 8);
        // cleared again: back to nothing
        c.set([1, 2, 3], 0);
        assert!(c.is_empty());
        assert_eq!(c.heap_bytes(), 0);
        assert_eq!(c.get([1, 2, 3]), 0);
        assert_eq!(c.version, 2, "each change counts");
    }

    #[test]
    fn a_few_kinds_take_a_fraction_of_the_dense_size() {
        let mut c = Chunk::default();
        for cell in cells([0, 0, 0]) {
            let b = match cell[1] {
                0..8 => 1,
                8..10 => 2,
                10 => 3,
                _ => 0,
            };
            c.set(cell, b);
        }
        // four kinds: two bits a block, 8 KB instead of 64 KB
        assert!(c.heap_bytes() <= VOL / 4 + 16, "{}", c.heap_bytes());
    }

    #[test]
    #[ignore = "a timing, run by hand: cargo test -p vox-core --release -- --ignored chunk_speed --nocapture"]
    fn chunk_speed_against_dense() {
        let mut c = Chunk::default();
        let mut dense = vec![0u16; VOL];
        let t = std::time::Instant::now();
        for round in 0..20u16 {
            for (i, cell) in cells([0, 0, 0]).enumerate() {
                c.set(cell, (i as u16 ^ round) % 12);
            }
        }
        let set_paletted = t.elapsed();
        let t = std::time::Instant::now();
        for round in 0..20u16 {
            for (i, cell) in cells([0, 0, 0]).enumerate() {
                dense[index(cell)] = (i as u16 ^ round) % 12;
            }
        }
        let set_dense = t.elapsed();
        let t = std::time::Instant::now();
        let mut sum = 0u64;
        for _ in 0..20 {
            for cell in cells([0, 0, 0]) {
                sum += c.get(cell) as u64;
            }
        }
        let get_paletted = t.elapsed();
        let t = std::time::Instant::now();
        let mut sum2 = 0u64;
        for _ in 0..20 {
            for cell in cells([0, 0, 0]) {
                sum2 += dense[index(cell)] as u64;
            }
        }
        let get_dense = t.elapsed();
        assert_eq!(sum, sum2);
        println!("set: paletted {set_paletted:?}, dense {set_dense:?}; get: paletted {get_paletted:?}, dense {get_dense:?}");
    }

    #[test]
    fn chunk_decode_rejects_runs_past_the_end_without_overflowing() {
        // a run of 1, then a run of u32::MAX: `at + n` would wrap on a 32-bit target
        let data = [1, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 1];
        assert!(Chunk::decode(&data, 0).is_err());
    }

    #[test]
    fn chunk_keys_round_down() {
        assert_eq!(ChunkKey::of([0, 0, 0]), ChunkKey([0, 0, 0]));
        assert_eq!(ChunkKey::of([31, 31, 31]), ChunkKey([0, 0, 0]));
        assert_eq!(ChunkKey::of([-1, 32, -33]), ChunkKey([-1, 1, -2]));
        assert_eq!(ChunkKey([-1, 1, -2]).origin(), [-32, 32, -64]);
    }

    #[test]
    fn touched_chunks_at_edges() {
        assert_eq!(ChunkKey::touched([5, 5, 5]).len(), 1);
        assert_eq!(ChunkKey::touched([0, 5, 5]).len(), 2);
        assert_eq!(ChunkKey::touched([31, 0, 31]).len(), 8);
    }

    #[test]
    fn set_get_and_count() {
        let mut w = World::new(Bounds::default(), 16);
        assert_eq!(w.set([-1, -1, -1], 3), 0);
        assert_eq!(w.set([40, 2, 0], 4), 0);
        assert_eq!(w.get([-1, -1, -1]), 3);
        assert_eq!(w.get([40, 2, 0]), 4);
        assert_eq!(w.get([0, 0, 0]), 0);
        assert_eq!(w.count(), 2);
        assert_eq!(w.set([-1, -1, -1], 0), 3);
        assert_eq!(w.count(), 1);
        assert_eq!(w.keys(), vec![ChunkKey([1, 0, 0])]);
        // clearing air in a chunk that doesn't exist doesn't make one
        w.set([500, 0, 0], 0);
        assert!(w.chunk(ChunkKey::of([500, 0, 0])).is_none());
    }

    #[test]
    fn versions_go_up_only_on_change() {
        let mut c = Chunk::default();
        c.set([1, 2, 3], 5);
        c.set([1, 2, 3], 5);
        assert_eq!(c.version, 1);
        c.set([1, 2, 3], 0);
        assert_eq!(c.version, 2);
        assert!(c.is_empty());
    }

    #[test]
    fn rle_round_trip() {
        let mut c = Chunk::default();
        assert_eq!(c.encode().len(), 4); // one run of 32768 air
        for x in 0..32 {
            for z in 0..32 {
                c.set([x, 0, z], 1);
                c.set([x, 1, z], if (x + z) % 2 == 0 { 2 } else { 300 });
            }
        }
        c.set([31, 31, 31], 7);
        let data = c.encode();
        assert!(data.len() < 4000, "{}", data.len());
        let back = Chunk::decode(&data, c.version).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn bad_chunk_data_is_refused() {
        assert!(Chunk::decode(&[], 0).is_err());
        assert!(Chunk::decode(&[0x80], 0).is_err());
        let mut short = Vec::new();
        varint(&mut short, 100);
        varint(&mut short, 1);
        assert!(Chunk::decode(&short, 0).is_err());
        let mut long = Chunk::default().encode();
        long.extend([1, 1]);
        assert!(Chunk::decode(&long, 0).is_err());
    }

    #[test]
    fn edit_checks() {
        let mut w = World::new(Bounds::default(), 16);
        assert_eq!(w.edit([0, 0, 0], 3), Ok(0));
        assert_eq!(w.edit([0, 0, 0], 3), Err(Why::NoChange));
        assert_eq!(w.edit([0, 0, 0], 17), Err(Why::NoSuchBlock));
        assert_eq!(w.edit([512, 0, 0], 1), Err(Why::Outside));
        assert_eq!(w.edit([0, -65, 0], 1), Err(Why::Outside));
        assert_eq!(w.edit([-512, -64, 511], 1), Ok(0));
        assert_eq!(w.edit([0, 0, 0], 0), Ok(3));
    }

    #[test]
    fn ground_finds_top() {
        let mut w = World::new(Bounds::default(), 16);
        w.set([2, -1, 2], 1);
        w.set([2, 3, 2], 1);
        assert_eq!(w.ground(2, 2, 10), Some(4));
        assert_eq!(w.ground(2, 2, 2), Some(0));
        assert_eq!(w.ground(3, 2, 10), None);
    }

    #[test]
    fn bucket_limits_rate() {
        let mut b = Bucket::new(20.0, 20.0);
        let ok = (0..30).filter(|_| b.take(0.0)).count();
        assert_eq!(ok, 20);
        assert!(!b.take(0.01));
        assert!(b.take(0.06));
        // a long pause refills only to the burst
        let ok = (0..30).filter(|_| b.take(100.0)).count();
        assert_eq!(ok, 20);
    }
}
