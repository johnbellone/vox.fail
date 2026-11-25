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
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    blocks: Vec<u16>,
    pub version: u32,
    solid: u32,
}

impl Default for Chunk {
    fn default() -> Self {
        Chunk { blocks: vec![0; VOL], version: 0, solid: 0 }
    }
}

impl Chunk {
    pub fn get(&self, c: Cell) -> u16 {
        self.blocks[index(c)]
    }
    /// Set a block; returns the old one.
    pub fn set(&mut self, c: Cell, b: u16) -> u16 {
        let i = index(c);
        let old = std::mem::replace(&mut self.blocks[i], b);
        if old != b {
            self.version += 1;
            match (old == 0, b == 0) {
                (true, false) => self.solid += 1,
                (false, true) => self.solid -= 1,
                _ => {}
            }
        }
        old
    }
    pub fn is_empty(&self) -> bool {
        self.solid == 0
    }
    /// Run-length encoded: pairs of varint(run length) varint(id).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < VOL {
            let b = self.blocks[i];
            let mut n = 1;
            while i + n < VOL && self.blocks[i + n] == b {
                n += 1;
            }
            varint(&mut out, n as u32);
            varint(&mut out, b as u32);
            i += n;
        }
        out
    }
    pub fn decode(data: &[u8], version: u32) -> Result<Chunk, String> {
        let mut c = Chunk { version, ..Default::default() };
        let mut at = 0;
        let mut pos = 0;
        while pos < data.len() {
            let n = read_varint(data, &mut pos)? as usize;
            let b = read_varint(data, &mut pos)?;
            let b = u16::try_from(b).map_err(|_| "block id too big")?;
            if n == 0 || at + n > VOL {
                return Err("chunk data has the wrong length".into());
            }
            c.blocks[at..at + n].fill(b);
            if b != 0 {
                c.solid += n as u32;
            }
            at += n;
        }
        if at != VOL {
            return Err("chunk data is short".into());
        }
        Ok(c)
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
    /// 512 × 128 × 512 blocks (256 × 64 × 256 m at half-metre blocks), a quarter of it below 0.
    fn default() -> Self {
        Bounds { lo: [-256, -32, -256], hi: [256, 96, 256] }
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
        assert_eq!(w.edit([256, 0, 0], 1), Err(Why::Outside));
        assert_eq!(w.edit([0, -33, 0], 1), Err(Why::Outside));
        assert_eq!(w.edit([-256, -32, 255], 1), Ok(0));
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
