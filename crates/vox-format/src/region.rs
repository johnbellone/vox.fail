//! A region as one blob.
//!
//! ```text
//! "VOXR"        magic
//! u16           format version
//! i32, i32      region key (x, z, in regions)
//! u16           block palette version (which list of blocks the ids refer to)
//! u32           chunks
//! u32           compressed length
//! [u8]          zstd frame: for each chunk, in (y, z, x) order:
//!                   varint chunk x, z within the region (0-15), varint chunk y (zigzag)
//!                   varint chunk version, varint length, the run-length bytes
//! ```
//!
//! The hash (BLAKE3, first 16 bytes) is over the header and the *uncompressed* chunk data, so
//! compressing differently doesn't change a region's identity. Reading is safe on untrusted
//! input: sizes are checked before anything is allocated, decompression stops at a cap, and
//! every chunk goes through vox-core's bounds-checked decoder.

use vox_core::{Chunk, ChunkKey};

use crate::{Error, err, store::Hash};

const MAGIC: &[u8; 4] = b"VOXR";
pub const FORMAT_VERSION: u16 = 1;
/// Chunk columns along each side of a region.
pub const REGION_CHUNKS: i32 = 16;
/// Most chunks a region may hold (16 × 16 columns, 16 chunks tall).
pub const MAX_CHUNKS: u32 = 16 * 16 * 16;
/// Most bytes a region's chunk data may decompress to (a run-length chunk is at most about
/// 6 bytes a block in the worst case; real ones are under 1 KB).
pub const MAX_DATA: usize = 64 << 20;
const HEADER: usize = 4 + 2 + 4 + 4 + 2 + 4 + 4;

/// Which region: chunk columns `x * 16 .. x * 16 + 16` and the same along z.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct RegionKey {
    pub x: i32,
    pub z: i32,
}

impl std::fmt::Display for RegionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", self.x, self.z)
    }
}

/// The region a chunk is in.
pub fn region_of(k: ChunkKey) -> RegionKey {
    RegionKey { x: k.0[0].div_euclid(REGION_CHUNKS), z: k.0[2].div_euclid(REGION_CHUNKS) }
}

/// A region's chunks, ready to write or just read.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub key: RegionKey,
    pub palette_version: u16,
    /// (chunk, its version, its run-length bytes), any order (written sorted)
    pub chunks: Vec<(ChunkKey, u32, Vec<u8>)>,
}

impl Region {
    /// A region from chunks (encoded with vox-core's run-length encoding).
    pub fn from_chunks<'a>(key: RegionKey, palette_version: u16, chunks: impl IntoIterator<Item = (ChunkKey, &'a Chunk)>) -> Region {
        let chunks = chunks.into_iter().filter(|(_, c)| !c.is_empty()).map(|(k, c)| (k, c.version, c.encode())).collect();
        Region { key, palette_version, chunks }
    }

    /// The chunks, decoded.
    pub fn decode_chunks(&self) -> Result<Vec<(ChunkKey, Chunk)>, Error> {
        self.chunks.iter().map(|(k, v, d)| Chunk::decode(d, *v).map(|c| (*k, c)).map_err(Error)).collect()
    }

    fn sorted(&self) -> Vec<&(ChunkKey, u32, Vec<u8>)> {
        let mut v: Vec<_> = self.chunks.iter().collect();
        v.sort_by_key(|(k, _, _)| (k.0[1], k.0[2], k.0[0]));
        v
    }

    fn header(&self, chunks: u32, compressed: u32) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER);
        h.extend_from_slice(MAGIC);
        h.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        h.extend_from_slice(&self.key.x.to_le_bytes());
        h.extend_from_slice(&self.key.z.to_le_bytes());
        h.extend_from_slice(&self.palette_version.to_le_bytes());
        h.extend_from_slice(&chunks.to_le_bytes());
        h.extend_from_slice(&compressed.to_le_bytes());
        h
    }

    /// The uncompressed chunk data, in the order written.
    fn data(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        for (k, version, bytes) in self.sorted() {
            if region_of(*k) != self.key {
                return err(format!("chunk {:?} isn't in region {}", k.0, self.key));
            }
            varint(&mut out, k.0[0].rem_euclid(REGION_CHUNKS) as u32);
            varint(&mut out, k.0[2].rem_euclid(REGION_CHUNKS) as u32);
            varint(&mut out, zigzag(k.0[1]));
            varint(&mut out, *version);
            varint(&mut out, bytes.len() as u32);
            out.extend_from_slice(bytes);
        }
        Ok(out)
    }

    /// The region's identity: its hash (the same however it's compressed).
    pub fn hash(&self) -> Result<Hash, Error> {
        let data = self.data()?;
        let mut h = blake3::Hasher::new();
        h.update(&self.header(self.chunks.len() as u32, 0));
        h.update(&data);
        Ok(Hash::from_blake3(h.finalize()))
    }

    /// The blob, and its hash.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn encode(&self) -> Result<(Vec<u8>, Hash), Error> {
        if self.chunks.len() as u32 > MAX_CHUNKS {
            return err("too many chunks for a region");
        }
        let data = self.data()?;
        let packed = zstd::bulk::compress(&data, 9)?;
        let mut out = self.header(self.chunks.len() as u32, packed.len() as u32);
        out.extend_from_slice(&packed);
        Ok((out, self.hash()?))
    }

    /// Read a blob (from storage or from anyone): checked, never panics.
    pub fn decode(blob: &[u8]) -> Result<Region, Error> {
        if blob.len() < HEADER || &blob[..4] != MAGIC {
            return err("not a region");
        }
        let u16_at = |i: usize| u16::from_le_bytes([blob[i], blob[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([blob[i], blob[i + 1], blob[i + 2], blob[i + 3]]);
        let version = u16_at(4);
        if version != FORMAT_VERSION {
            return err(format!("region format {version}, this build reads {FORMAT_VERSION}"));
        }
        let key = RegionKey { x: u32_at(6) as i32, z: u32_at(10) as i32 };
        let palette_version = u16_at(14);
        let count = u32_at(16);
        let compressed = u32_at(20) as usize;
        if count > MAX_CHUNKS {
            return err("too many chunks");
        }
        if blob.len() != HEADER + compressed {
            return err("the region's length is wrong");
        }
        let data = unzstd(&blob[HEADER..], MAX_DATA)?;
        let mut pos = 0;
        let mut chunks = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let x = read_varint(&data, &mut pos)? as i32;
            let z = read_varint(&data, &mut pos)? as i32;
            let y = unzigzag(read_varint(&data, &mut pos)?);
            let v = read_varint(&data, &mut pos)?;
            let n = read_varint(&data, &mut pos)? as usize;
            if !(0..REGION_CHUNKS).contains(&x) || !(0..REGION_CHUNKS).contains(&z) {
                return err("a chunk outside its region");
            }
            if n > data.len() - pos {
                return err("a chunk runs past the end");
            }
            let k = ChunkKey([key.x * REGION_CHUNKS + x, y, key.z * REGION_CHUNKS + z]);
            let bytes = data[pos..pos + n].to_vec();
            // check it decodes (bounds, lengths) before anyone relies on it
            Chunk::decode(&bytes, v).map_err(Error)?;
            pos += n;
            chunks.push((k, v, bytes));
        }
        if pos != data.len() {
            return err("extra bytes after the chunks");
        }
        Ok(Region { key, palette_version, chunks })
    }
}

/// Decompress a zstd frame (pure Rust, so the browser can too), refusing more than `cap` bytes.
pub fn unzstd(src: &[u8], cap: usize) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(src).map_err(|e| Error(format!("zstd: {e}")))?;
    let mut out = Vec::new();
    let read = (&mut dec).take(cap as u64 + 1).read_to_end(&mut out).map_err(|e| Error(format!("zstd: {e}")))?;
    if read > cap {
        return err("decompresses to too much");
    }
    Ok(out)
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

fn unzigzag(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

fn varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn read_varint(b: &[u8], pos: &mut usize) -> Result<u32, Error> {
    let mut v: u32 = 0;
    for shift in (0..35).step_by(7) {
        let Some(&byte) = b.get(*pos) else { return err("truncated") };
        *pos += 1;
        v |= ((byte & 0x7f) as u32).checked_shl(shift).unwrap_or(0);
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    err("a number too long")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::{World, world::Bounds};

    fn world() -> World {
        let mut w = World::new(Bounds { lo: [-600, -64, -600], hi: [600, 128, 600] }, 9);
        let mut s = 7u32;
        for _ in 0..20000 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let c = [(s % 900) as i32 - 450, ((s >> 9) % 100) as i32 - 40, ((s >> 18) % 900) as i32 - 450];
            w.set(c, (s % 9) as u16 + 1);
        }
        w
    }

    fn regions(w: &World) -> std::collections::BTreeMap<RegionKey, Vec<ChunkKey>> {
        let mut m: std::collections::BTreeMap<RegionKey, Vec<ChunkKey>> = Default::default();
        for k in w.keys() {
            m.entry(region_of(k)).or_default().push(k);
        }
        m
    }

    #[test]
    fn regions_round_trip_with_the_same_blocks() {
        let w = world();
        for (key, ks) in regions(&w) {
            let r = Region::from_chunks(key, 3, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())));
            let (blob, hash) = r.encode().unwrap();
            let back = Region::decode(&blob).unwrap();
            assert_eq!(back.key, key);
            assert_eq!(back.palette_version, 3);
            assert_eq!(back.hash().unwrap(), hash, "the hash survives the trip");
            for (k, c) in back.decode_chunks().unwrap() {
                let orig = w.chunk(k).unwrap();
                assert_eq!(c.encode(), orig.encode(), "{k:?}");
                assert_eq!(c.version, orig.version);
            }
        }
    }

    #[test]
    fn negative_regions_and_heights_work() {
        assert_eq!(region_of(ChunkKey([-1, 0, -17])), RegionKey { x: -1, z: -2 });
        assert_eq!(region_of(ChunkKey([16, -3, 15])), RegionKey { x: 1, z: 0 });
        for v in [0, 1, -1, 1000, -1000, i32::MAX, i32::MIN] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
    }

    #[test]
    fn the_hash_ignores_order_and_compression_but_not_content() {
        let w = world();
        let (key, ks) = regions(&w).into_iter().next().unwrap();
        let a = Region::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())));
        let b = Region::from_chunks(key, 1, ks.iter().rev().map(|k| (*k, w.chunk(*k).unwrap())));
        assert_eq!(a.hash().unwrap(), b.hash().unwrap());
        let mut c = a.clone();
        c.chunks[0].1 += 1;
        assert_ne!(a.hash().unwrap(), c.hash().unwrap());
    }

    #[test]
    fn crafted_blobs_are_refused_not_crashed_on() {
        let w = world();
        let (key, ks) = regions(&w).into_iter().next().unwrap();
        let (good, _) = Region::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap()))).encode().unwrap();
        for n in 0..good.len().min(4000) {
            let _ = Region::decode(&good[..n]);
        }
        let mut wrong_count = good.clone();
        wrong_count[16..20].copy_from_slice(&(MAX_CHUNKS + 1).to_le_bytes());
        assert!(Region::decode(&wrong_count).is_err());
        let mut flipped = good.clone();
        let mid = HEADER + (good.len() - HEADER) / 2;
        flipped[mid] ^= 0x55;
        let _ = Region::decode(&flipped);
        // a frame that would decompress to more than allowed
        let bomb = zstd::bulk::compress(&vec![0u8; 1 << 20], 19).unwrap();
        assert!(unzstd(&bomb, 1000).is_err());
        assert!(Region::decode(b"VOXR").is_err());
        assert!(Region::decode(b"nope").is_err());
    }
}
