//! A zone (16 × 16 chunk columns, 512 blocks or 128 m across) as one blob. Zones are the unit of storage
//! and hashing; [`crate::region`]s group them, 32 × 32 to a region.
//!
//! ```text
//! "VOXR"        magic (zones were called regions when the format was made; the bytes stay,
//!               so every blob already stored keeps its hash)
//! u16           format version: 1 (chunks only) or 2 (chunks and entities)
//! i32, i32      zone key (x, z, in zones)
//! u16           block palette version (which list of blocks the ids refer to)
//! u32           chunks
//! u32           compressed length of the chunk section
//! [u8]          zstd frame: for each chunk, in (y, z, x) order:
//!                   varint chunk x, z within the zone (0-15), varint chunk y (zigzag)
//!                   varint chunk version, varint length, the run-length bytes
//! -- version 2 only, after the chunk section --
//! u32           entities
//! u32           compressed length of the entity section
//! [u8]          zstd frame: for each entity, in id order:
//!                   u32 entity id, u16 name length, the name ("vox::Portal"),
//!                   u16 component version, u32 length, the postcard bytes of the Component
//! ```
//!
//! The hash (BLAKE3, first 16 bytes) is over the header and the *uncompressed* data, so
//! compressing differently doesn't change a zone's identity. A version 2 hash also covers the
//! entity count and the uncompressed entity records. A version 1 blob hashes exactly as it did
//! before entities existed. Reading is safe on untrusted input: sizes are checked before
//! anything is allocated, decompression stops at a cap, and every chunk and entity goes through
//! its bounds-checked decoder and validation.

use std::collections::BTreeSet;

use vox_core::{
    Chunk, ChunkKey,
    entity::{Component, EntityId},
    wire,
};

use crate::{Error, err, store::Hash};

/// Kept from when zones were called regions: changing it would change every stored hash.
const MAGIC: &[u8; 4] = b"VOXR";
/// The format new zones are written in. Version 1 (chunks only) is still read.
pub const FORMAT_VERSION: u16 = 2;
/// The format before zones carried entities; still read, and written back unchanged.
const VERSION_CHUNKS_ONLY: u16 = 1;
/// Chunk columns along each side of a zone.
pub const ZONE_CHUNKS: i32 = 16;
/// Most chunks a zone may hold (16 × 16 columns, 16 chunks tall).
pub const MAX_CHUNKS: u32 = 16 * 16 * 16;
/// Most bytes a zone's chunk data may decompress to (a run-length chunk is at most about
/// 6 bytes a block in the worst case; real ones are under 1 KB).
pub const MAX_DATA: usize = 64 << 20;
/// Most entities a zone may hold.
pub const MAX_ZONE_ENTITIES: u32 = 4_000;
/// Longest component name a record may carry, in bytes.
pub const MAX_ENTITY_NAME: usize = 64;
/// Largest postcard payload one entity may be, in bytes.
pub const MAX_ENTITY_BYTES: usize = 64 << 10;
/// Most bytes a zone's entity records may decompress to.
pub const MAX_ENTITY_DATA: usize = 64 << 20;
/// Fixed bytes of one entity record before its name and payload: id, name length, component
/// version, payload length. A count the section cannot hold is refused before allocating.
const RECORD_MIN: usize = 4 + 2 + 2 + 4;
const HEADER: usize = 4 + 2 + 4 + 4 + 2 + 4 + 4;
/// The entity section's own header: its count and its compressed length.
const ENTITY_HEADER: usize = 4 + 4;

/// Which zone: chunk columns `x * 16 .. x * 16 + 16` and the same along z.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ZoneKey {
    pub x: i32,
    pub z: i32,
}

impl std::fmt::Display for ZoneKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", self.x, self.z)
    }
}

impl ZoneKey {
    /// From its `x_z` name (as in a manifest).
    pub fn parse(s: &str) -> Option<ZoneKey> {
        let (x, z) = s.split_once('_')?;
        Some(ZoneKey { x: x.parse().ok()?, z: z.parse().ok()? })
    }
}

/// The zone a chunk is in.
pub fn zone_of(k: ChunkKey) -> ZoneKey {
    ZoneKey { x: k.0[0].div_euclid(ZONE_CHUNKS), z: k.0[2].div_euclid(ZONE_CHUNKS) }
}

/// A zone's chunks and entities, ready to write or just read.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub key: ZoneKey,
    pub palette_version: u16,
    /// (chunk, its version, its run-length bytes), any order (written sorted)
    pub chunks: Vec<(ChunkKey, u32, Vec<u8>)>,
    /// The entities in this zone, any order (written sorted by id). Only a version 2 zone can
    /// hold any.
    pub entities: Vec<(EntityId, Component)>,
    /// The format this zone is written in: [`FORMAT_VERSION`] when made here, 1 when read from
    /// an older blob (which keeps its hash and has no entities).
    pub version: u16,
}

impl Zone {
    /// A zone from chunks (encoded with vox-core's run-length encoding), with no entities.
    pub fn from_chunks<'a>(key: ZoneKey, palette_version: u16, chunks: impl IntoIterator<Item = (ChunkKey, &'a Chunk)>) -> Zone {
        let chunks = chunks.into_iter().filter(|(_, c)| !c.is_empty()).map(|(k, c)| (k, c.version, c.encode())).collect();
        Zone { key, palette_version, chunks, entities: Vec::new(), version: FORMAT_VERSION }
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

    /// Refuses what this zone's format cannot hold, before anything is written or hashed.
    fn check(&self) -> Result<(), Error> {
        match self.version {
            VERSION_CHUNKS_ONLY if !self.entities.is_empty() => return err("a version 1 zone can't hold entities"),
            VERSION_CHUNKS_ONLY | FORMAT_VERSION => {}
            v => return err(format!("can't write zone format {v}")),
        }
        if self.entities.len() as u32 > MAX_ZONE_ENTITIES {
            return err("too many entities for a zone");
        }
        Ok(())
    }

    fn header(&self, chunks: u32, compressed: u32) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER);
        h.extend_from_slice(MAGIC);
        h.extend_from_slice(&self.version.to_le_bytes());
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
            if zone_of(*k) != self.key {
                return err(format!("chunk {:?} isn't in zone {}", k.0, self.key));
            }
            varint(&mut out, k.0[0].rem_euclid(ZONE_CHUNKS) as u32);
            varint(&mut out, k.0[2].rem_euclid(ZONE_CHUNKS) as u32);
            varint(&mut out, zigzag(k.0[1]));
            varint(&mut out, *version);
            varint(&mut out, bytes.len() as u32);
            out.extend_from_slice(bytes);
        }
        Ok(out)
    }

    /// The uncompressed entity records, sorted by id. Each value is validated here, so an
    /// invalid entity is never written or hashed.
    fn entity_data(&self) -> Result<Vec<u8>, Error> {
        let mut sorted: Vec<&(EntityId, Component)> = self.entities.iter().collect();
        sorted.sort_by_key(|(id, _)| id.0);
        if sorted.windows(2).any(|w| w[0].0 == w[1].0) {
            return err("an entity id is listed twice");
        }
        let mut out = Vec::new();
        for (id, c) in sorted {
            c.validate().map_err(|e| Error(format!("entity {}: {e}", id.0)))?;
            let name = c.name();
            let payload = wire::encode(c);
            if payload.len() > MAX_ENTITY_BYTES {
                return err(format!("entity {} is too big", id.0));
            }
            out.extend_from_slice(&id.0.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&c.version().to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&payload);
            // decode refuses a section this big, so refuse to write one: what encodes must decode
            if out.len() > MAX_ENTITY_DATA {
                return err("the zone's entities are too big to decode");
            }
        }
        Ok(out)
    }

    /// The zone's identity: its hash (the same however it's compressed).
    pub fn hash(&self) -> Result<Hash, Error> {
        self.check()?;
        let data = self.data()?;
        let mut h = blake3::Hasher::new();
        h.update(&self.header(self.chunks.len() as u32, 0));
        h.update(&data);
        if self.version >= FORMAT_VERSION {
            // the count is fixed-width, so the records after it can't be mistaken for chunk data
            h.update(&(self.entities.len() as u32).to_le_bytes());
            h.update(&self.entity_data()?);
        }
        Ok(Hash::from_blake3(h.finalize()))
    }

    /// The blob, and its hash.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn encode(&self) -> Result<(Vec<u8>, Hash), Error> {
        self.check()?;
        if self.chunks.len() as u32 > MAX_CHUNKS {
            return err("too many chunks for a zone");
        }
        let data = self.data()?;
        let packed = zstd::bulk::compress(&data, 9)?;
        let mut out = self.header(self.chunks.len() as u32, packed.len() as u32);
        out.extend_from_slice(&packed);
        if self.version >= FORMAT_VERSION {
            let packed_entities = zstd::bulk::compress(&self.entity_data()?, 9)?;
            out.extend_from_slice(&(self.entities.len() as u32).to_le_bytes());
            out.extend_from_slice(&(packed_entities.len() as u32).to_le_bytes());
            out.extend_from_slice(&packed_entities);
        }
        Ok((out, self.hash()?))
    }

    /// Read a blob (from storage or from anyone): checked, never panics.
    pub fn decode(blob: &[u8]) -> Result<Zone, Error> {
        if blob.len() < HEADER || &blob[..4] != MAGIC {
            return err("not a zone");
        }
        let u16_at = |i: usize| u16::from_le_bytes([blob[i], blob[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([blob[i], blob[i + 1], blob[i + 2], blob[i + 3]]);
        let version = u16_at(4);
        if version != VERSION_CHUNKS_ONLY && version != FORMAT_VERSION {
            return err(format!("zone format {version}, this build reads {VERSION_CHUNKS_ONLY} and {FORMAT_VERSION}"));
        }
        let key = ZoneKey { x: u32_at(6) as i32, z: u32_at(10) as i32 };
        let palette_version = u16_at(14);
        let count = u32_at(16);
        let compressed = u32_at(20) as usize;
        if count > MAX_CHUNKS {
            return err("too many chunks");
        }
        let len_err = || Error("the zone's length is wrong".into());
        // checked, not wrapping: on wasm32 a usize is 32 bits, so a crafted length must not wrap
        let chunks_end = HEADER.checked_add(compressed).ok_or_else(len_err)?;
        // where the entity records start, and how many there are (none in a version 1 blob)
        let (entities_at, ent_count) = if version == VERSION_CHUNKS_ONLY {
            if blob.len() != chunks_end {
                return Err(len_err());
            }
            (chunks_end, 0)
        } else {
            let at = chunks_end.checked_add(ENTITY_HEADER).ok_or_else(len_err)?;
            if blob.len() < at {
                return Err(len_err());
            }
            let ent_count = u32_at(chunks_end);
            let ent_len = u32_at(chunks_end + 4) as usize;
            if ent_count > MAX_ZONE_ENTITIES {
                return err("too many entities");
            }
            if at.checked_add(ent_len) != Some(blob.len()) {
                return Err(len_err());
            }
            (at, ent_count)
        };
        // checked, not wrapping: a crafted key near i32::MAX must not overflow on wasm32 either
        let base = |v: i32| v.checked_mul(ZONE_CHUNKS).ok_or_else(|| Error("a zone key out of range".into()));
        let (bx, bz) = (base(key.x)?, base(key.z)?);
        let data = unzstd(&blob[HEADER..chunks_end], MAX_DATA)?;
        let mut pos = 0;
        let mut chunks = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let x = read_varint(&data, &mut pos)? as i32;
            let z = read_varint(&data, &mut pos)? as i32;
            let y = unzigzag(read_varint(&data, &mut pos)?);
            let v = read_varint(&data, &mut pos)?;
            let n = read_varint(&data, &mut pos)? as usize;
            if !(0..ZONE_CHUNKS).contains(&x) || !(0..ZONE_CHUNKS).contains(&z) {
                return err("a chunk outside its zone");
            }
            if n > data.len() - pos {
                return err("a chunk runs past the end");
            }
            let k = ChunkKey([bx + x, y, bz + z]);
            let bytes = data[pos..pos + n].to_vec();
            // check it decodes (bounds, lengths) before anyone relies on it
            Chunk::decode(&bytes, v).map_err(Error)?;
            pos += n;
            chunks.push((k, v, bytes));
        }
        if pos != data.len() {
            return err("extra bytes after the chunks");
        }
        let entities = if version == VERSION_CHUNKS_ONLY { Vec::new() } else { parse_entities(&blob[entities_at..], ent_count)? };
        Ok(Zone { key, palette_version, chunks, entities, version })
    }
}

/// Decodes a version 2 entity section (its zstd frame), checking every record before the
/// next is read. A name must be a component this build knows, its version must be the one
/// this build reads, and its value must pass [`Component::validate`] and re-encode to the
/// same bytes (so one entity has one blob).
fn parse_entities(src: &[u8], count: u32) -> Result<Vec<(EntityId, Component)>, Error> {
    let data = unzstd(src, MAX_ENTITY_DATA)?;
    let n = count as usize;
    if n.saturating_mul(RECORD_MIN) > data.len() {
        return err("more entities than the section holds");
    }
    let mut pos = 0;
    let mut ids = BTreeSet::new();
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = u32::from_le_bytes(take_array(&data, &mut pos)?);
        let name_len = u16::from_le_bytes(take_array(&data, &mut pos)?) as usize;
        if name_len > MAX_ENTITY_NAME {
            return err("an entity's name is too long");
        }
        let name = std::str::from_utf8(take(&data, &mut pos, name_len)?).map_err(|_| Error("an entity's name isn't text".into()))?;
        let version = u16::from_le_bytes(take_array(&data, &mut pos)?);
        let len = u32::from_le_bytes(take_array(&data, &mut pos)?) as usize;
        if len > MAX_ENTITY_BYTES {
            return err(format!("entity {id} is too big"));
        }
        let payload = take(&data, &mut pos, len)?;
        let c: Component = wire::decode(payload).map_err(|e| Error(format!("entity {id}: {e}")))?;
        if c.name() != name {
            return err(format!("entity {id}: {name:?} isn't a component this build knows"));
        }
        if c.version() != version {
            return err(format!("entity {id}: {name} version {version}, this build reads {}", c.version()));
        }
        c.validate().map_err(|e| Error(format!("entity {id}: {e}")))?;
        if wire::encode(&c) != payload {
            return err(format!("entity {id}: its data isn't in canonical form"));
        }
        if !ids.insert(id) {
            return err(format!("entity {id} is listed twice"));
        }
        out.push((EntityId(id), c));
    }
    if pos != data.len() {
        return err("extra bytes after the entities");
    }
    Ok(out)
}

/// The next `n` bytes, advancing the position; refused (not panicked) past the end.
fn take<'a>(b: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    let end = pos.checked_add(n).filter(|&e| e <= b.len()).ok_or_else(|| Error("an entity runs past the end".into()))?;
    let s = &b[*pos..end];
    *pos = end;
    Ok(s)
}

fn take_array<const N: usize>(b: &[u8], pos: &mut usize) -> Result<[u8; N], Error> {
    take(b, pos, N)?.try_into().map_err(|_| Error("an entity runs past the end".into()))
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
    use vox_core::{
        World,
        entity::{Axis, Dest, Objective, Portal, Settings, Spawn},
        world::Bounds,
    };

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

    fn zones(w: &World) -> std::collections::BTreeMap<ZoneKey, Vec<ChunkKey>> {
        let mut m: std::collections::BTreeMap<ZoneKey, Vec<ChunkKey>> = Default::default();
        for k in w.keys() {
            m.entry(zone_of(k)).or_default().push(k);
        }
        m
    }

    /// One zone of the test world, with chunks and no entities.
    fn a_zone() -> Zone {
        let w = world();
        let (key, ks) = zones(&w).into_iter().next().unwrap();
        Zone::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())))
    }

    fn entities() -> Vec<(EntityId, Component)> {
        vec![
            (EntityId(9), Component::Settings(Settings { json: r#"{"sky":"earth","time":16.5}"#.into() })),
            (
                EntityId(2),
                Component::Portal(Portal {
                    name: "void".into(),
                    at: [0.0, 0.0, 11.5],
                    axis: Axis::Z,
                    size: [8.0, 16.0],
                    yaw: 0.0,
                    level: "void".into(),
                    to: Dest::World { world: "yard".into() },
                    key: Some(7),
                }),
            ),
            (EntityId(5), Component::Objective(Objective { name: "the altar".into(), at: [0.0, 0.0, -9.0], radius: 3.0 })),
            (EntityId(7), Component::Spawn(Spawn { at: [1.0, 2.0, 3.0], yaw: 90.0 })),
        ]
    }

    /// A version 1 blob written by the encoder before entities existed (a six-block test zone,
    /// palette 3), with the hash it had then.
    const V1_FIXTURE: &[u8] = &[
        0x56, 0x4f, 0x58, 0x52, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x23, 0x00, 0x00, 0x00, 0x28, 0xb5, 0x2f, 0xfd, 0x20, 0x1a, 0xd1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x15, 0x40, 0x00,
        0x01, 0x01, 0x01, 0x02, 0x01, 0x03, 0x01, 0x04, 0x01, 0x05, 0x9e, 0x48, 0x00, 0x01, 0x02, 0x9c, 0xb7, 0x01, 0x00,
    ];
    const V1_HASH: &str = "2e34dd26017d5022d13a0d1276e0d3f7";

    #[test]
    fn zones_round_trip_with_the_same_blocks() {
        let w = world();
        for (key, ks) in zones(&w) {
            let r = Zone::from_chunks(key, 3, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())));
            let (blob, hash) = r.encode().unwrap();
            let back = Zone::decode(&blob).unwrap();
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
    fn negative_zones_and_heights_work() {
        assert_eq!(zone_of(ChunkKey([-1, 0, -17])), ZoneKey { x: -1, z: -2 });
        assert_eq!(zone_of(ChunkKey([16, -3, 15])), ZoneKey { x: 1, z: 0 });
        for v in [0, 1, -1, 1000, -1000, i32::MAX, i32::MIN] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
    }

    #[test]
    fn the_hash_ignores_order_and_compression_but_not_content() {
        let w = world();
        let (key, ks) = zones(&w).into_iter().next().unwrap();
        let a = Zone::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())));
        let b = Zone::from_chunks(key, 1, ks.iter().rev().map(|k| (*k, w.chunk(*k).unwrap())));
        assert_eq!(a.hash().unwrap(), b.hash().unwrap());
        let mut c = a.clone();
        c.chunks[0].1 += 1;
        assert_ne!(a.hash().unwrap(), c.hash().unwrap());
    }

    #[test]
    fn crafted_blobs_are_refused_not_crashed_on() {
        let w = world();
        let (key, ks) = zones(&w).into_iter().next().unwrap();
        let (good, _) = Zone::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap()))).encode().unwrap();
        for n in 0..good.len().min(4000) {
            let _ = Zone::decode(&good[..n]);
        }
        let mut wrong_count = good.clone();
        wrong_count[16..20].copy_from_slice(&(MAX_CHUNKS + 1).to_le_bytes());
        assert!(Zone::decode(&wrong_count).is_err());
        let mut flipped = good.clone();
        let mid = HEADER + (good.len() - HEADER) / 2;
        flipped[mid] ^= 0x55;
        let _ = Zone::decode(&flipped);
        // a frame that would decompress to more than allowed
        let bomb = zstd::bulk::compress(&vec![0u8; 1 << 20], 19).unwrap();
        assert!(unzstd(&bomb, 1000).is_err());
        assert!(Zone::decode(b"VOXR").is_err());
        let mut far = good.clone();
        far[6..10].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(Zone::decode(&far).is_err(), "a key that overflows is refused");
        assert!(Zone::decode(b"nope").is_err());
    }

    #[test]
    fn a_version_1_blob_reads_with_no_entities_and_keeps_its_hash() {
        let z = Zone::decode(V1_FIXTURE).unwrap();
        assert_eq!(z.version, VERSION_CHUNKS_ONLY);
        assert!(z.entities.is_empty());
        assert!(!z.chunks.is_empty());
        assert_eq!(z.hash().unwrap().hex(), V1_HASH);
        let (blob, hash) = z.encode().unwrap();
        assert_eq!(blob, V1_FIXTURE, "written back as version 1, byte for byte");
        assert_eq!(hash.hex(), V1_HASH);
    }

    #[test]
    fn a_version_1_zone_cannot_hold_entities() {
        let mut z = Zone::decode(V1_FIXTURE).unwrap();
        z.entities = entities();
        assert!(z.encode().is_err());
        assert!(z.hash().is_err());
    }

    #[test]
    fn version_2_round_trips_portal_settings_objective_and_spawn() {
        let mut z = a_zone();
        z.entities = entities();
        let (blob, hash) = z.encode().unwrap();
        let back = Zone::decode(&blob).unwrap();
        assert_eq!(back.version, FORMAT_VERSION);
        let mut want = entities();
        want.sort_by_key(|(id, _)| id.0);
        assert_eq!(back.entities, want);
        assert_eq!(back.hash().unwrap(), hash, "the hash survives the trip");
        assert_eq!(back.chunks.len(), z.chunks.len());
    }

    #[test]
    fn the_largest_entity_section_encodes_and_decodes() {
        // the most a zone may hold: MAX_ZONE_ENTITIES settings of the largest size each. Its
        // uncompressed section must be under the decode cap, or the zone could be written and
        // never read back.
        let json = format!(r#"{{"x":"{}"}}"#, "a".repeat(vox_core::entity::MAX_SETTINGS - 8));
        let mut z = Zone::from_chunks(ZoneKey { x: 0, z: 0 }, 1, std::iter::empty());
        z.entities = (1..=MAX_ZONE_ENTITIES).map(|id| (EntityId(id), Component::Settings(Settings { json: json.clone() }))).collect();
        let (blob, hash) = z.encode().unwrap();
        let back = Zone::decode(&blob).unwrap();
        assert_eq!(back.entities.len(), MAX_ZONE_ENTITIES as usize);
        assert_eq!(back.hash().unwrap(), hash);
        // the caps keep the section under the cap, so the encoder's own refusal is a backstop
        assert!(z.entity_data().unwrap().len() <= MAX_ENTITY_DATA);
    }

    #[test]
    fn changing_an_entity_changes_the_hash() {
        let mut a = a_zone();
        a.entities = entities();
        let before = a.hash().unwrap();
        let mut b = a.clone();
        if let Component::Objective(o) = &mut b.entities[2].1 {
            o.radius = 4.0;
        }
        assert_ne!(before, b.hash().unwrap());
        let mut c = a.clone();
        c.entities.pop();
        assert_ne!(before, c.hash().unwrap());
        let mut d = a.clone();
        d.entities.reverse();
        assert_eq!(before, d.hash().unwrap(), "order does not matter");
    }

    /// A version 2 zone with no chunks and the given raw entity section.
    fn zone_with_section(count: u32, raw: &[u8]) -> Vec<u8> {
        let no_chunks = zstd::bulk::compress(&[], 9).unwrap();
        let packed = zstd::bulk::compress(raw, 9).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(no_chunks.len() as u32).to_le_bytes());
        out.extend_from_slice(&no_chunks);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        out.extend_from_slice(&packed);
        out
    }

    fn record(id: u32, name: &[u8], version: u16, payload: &[u8]) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&id.to_le_bytes());
        r.extend_from_slice(&(name.len() as u16).to_le_bytes());
        r.extend_from_slice(name);
        r.extend_from_slice(&version.to_le_bytes());
        r.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        r.extend_from_slice(payload);
        r
    }

    fn spawn_payload() -> Vec<u8> {
        wire::encode(&Component::Spawn(Spawn { at: [1.0, 2.0, 3.0], yaw: 90.0 }))
    }

    #[test]
    fn a_hand_built_entity_section_is_read_when_it_is_valid() {
        let blob = zone_with_section(1, &record(1, b"vox::Spawn", 1, &spawn_payload()));
        let z = Zone::decode(&blob).unwrap();
        assert_eq!(z.entities, vec![(EntityId(1), Component::Spawn(Spawn { at: [1.0, 2.0, 3.0], yaw: 90.0 }))]);
    }

    #[test]
    fn crafted_entity_sections_are_refused_not_crashed_on() {
        // a count over the cap, with nothing after it: refused before anything is allocated
        assert!(Zone::decode(&zone_with_section(MAX_ZONE_ENTITIES + 1, &[])).is_err());
        // a count the section cannot hold
        assert!(Zone::decode(&zone_with_section(2, &record(1, b"vox::Spawn", 1, &spawn_payload()))).is_err());
        // a record whose length runs past the end (the length field sits after id, name and version)
        let mut past = record(1, b"vox::Spawn", 1, &[1, 2, 3]);
        past[18..22].copy_from_slice(&1000u32.to_le_bytes());
        assert!(Zone::decode(&zone_with_section(1, &past)).is_err());
        // a name that is not a component this build knows
        assert!(Zone::decode(&zone_with_section(1, &record(1, b"vox::Nope", 1, &spawn_payload()))).is_err());
        // a payload that is not postcard for its component
        assert!(Zone::decode(&zone_with_section(1, &record(1, b"vox::Spawn", 1, &[0xff]))).is_err());
        // a value that fails validate (a non-finite yaw)
        let nan = wire::encode(&Component::Spawn(Spawn { at: [0.0; 3], yaw: f32::NAN }));
        assert!(Zone::decode(&zone_with_section(1, &record(1, b"vox::Spawn", 1, &nan))).is_err());
        // a version this build does not know
        assert!(Zone::decode(&zone_with_section(1, &record(1, b"vox::Spawn", 2, &spawn_payload()))).is_err());
        // a payload over the byte cap
        let huge = vec![0u8; MAX_ENTITY_BYTES + 1];
        assert!(Zone::decode(&zone_with_section(1, &record(1, b"vox::Spawn", 1, &huge))).is_err());
        // a name over the cap
        let long = vec![b'a'; MAX_ENTITY_NAME + 1];
        assert!(Zone::decode(&zone_with_section(1, &record(1, &long, 1, &spawn_payload()))).is_err());
        // the same id twice
        let twice = [record(1, b"vox::Spawn", 1, &spawn_payload()), record(1, b"vox::Spawn", 1, &spawn_payload())].concat();
        assert!(Zone::decode(&zone_with_section(2, &twice)).is_err());
        // bytes after the last record
        let mut trailing = record(1, b"vox::Spawn", 1, &spawn_payload());
        trailing.push(0);
        assert!(Zone::decode(&zone_with_section(1, &trailing)).is_err());
        // bytes after the entity section
        let mut good = a_zone();
        good.entities = entities();
        let (blob, _) = good.encode().unwrap();
        let mut junk = blob.clone();
        junk.push(0);
        assert!(Zone::decode(&junk).is_err());
        for n in 0..blob.len().min(4000) {
            let _ = Zone::decode(&blob[..n]);
        }
    }
}
