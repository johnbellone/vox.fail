//! A region: 32 × 32 [`crate::zone`]s (16,384 blocks, 4,096 m across), stored as a
//! small blob listing the hash of each of its zones that has blocks.
//!
//! ```text
//! "VOXG"        magic
//! u16           format version
//! i32, i32      region key (x, z, in regions)
//! u32           zones
//! for each zone, sorted by (z, x):
//!     u8, u8    zone x, z within the region (0-31)
//!     [u8; 16]  the zone blob's hash
//! ```
//!
//! A region blob is content-addressed like a zone: its hash (BLAKE3 over all of it, first 16
//! bytes) is its address. A snapshot lists region hashes, so a snapshot shares every region
//! nothing changed in, and the hashes form a tree (snapshot root, regions, zones) that two
//! copies of a world can compare from the top. Reading is safe on untrusted input: the count
//! is capped, the length must be exact, and zones must be inside the region, sorted and unique.

use std::collections::BTreeMap;

use vox_core::ChunkKey;

use crate::{
    Error, err,
    store::Hash,
    zone::{ZoneKey, zone_of},
};

const MAGIC: &[u8; 4] = b"VOXG";
pub const FORMAT_VERSION: u16 = 1;
/// Zones along each side of a region.
pub const REGION_ZONES: i32 = 32;
/// Most zones a region holds.
pub const MAX_ZONES: u32 = (REGION_ZONES * REGION_ZONES) as u32;
const HEADER: usize = 4 + 2 + 4 + 4 + 4;
const ENTRY: usize = 2 + 16;

/// Which region: zones `x * 32 .. x * 32 + 32` and the same along z.
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

impl RegionKey {
    /// From its `x_z` name (as in a manifest).
    pub fn parse(s: &str) -> Option<RegionKey> {
        let (x, z) = s.split_once('_')?;
        Some(RegionKey { x: x.parse().ok()?, z: z.parse().ok()? })
    }
}

/// The region a zone is in.
pub fn region_of_zone(z: ZoneKey) -> RegionKey {
    RegionKey { x: z.x.div_euclid(REGION_ZONES), z: z.z.div_euclid(REGION_ZONES) }
}

/// The region a chunk is in.
pub fn region_of(k: ChunkKey) -> RegionKey {
    region_of_zone(zone_of(k))
}

/// A region's zones, by key, each with its blob's hash.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub key: RegionKey,
    pub zones: BTreeMap<ZoneKey, Hash>,
}

impl Region {
    pub fn new(key: RegionKey) -> Region {
        Region { key, zones: BTreeMap::new() }
    }

    /// The blob, and its hash. Fails for a zone outside the region or too many zones.
    pub fn encode(&self) -> Result<(Vec<u8>, Hash), Error> {
        if self.zones.len() as u32 > MAX_ZONES {
            return err("too many zones for a region");
        }
        let mut out = Vec::with_capacity(HEADER + self.zones.len() * ENTRY);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.key.x.to_le_bytes());
        out.extend_from_slice(&self.key.z.to_le_bytes());
        out.extend_from_slice(&(self.zones.len() as u32).to_le_bytes());
        let mut sorted: Vec<_> = self.zones.iter().collect();
        sorted.sort_by_key(|(k, _)| (k.z, k.x));
        for (k, h) in sorted {
            if region_of_zone(*k) != self.key {
                return err(format!("zone {k} isn't in region {}", self.key));
            }
            out.push(k.x.rem_euclid(REGION_ZONES) as u8);
            out.push(k.z.rem_euclid(REGION_ZONES) as u8);
            out.extend_from_slice(&h.0);
        }
        let hash = Hash::from_blake3(blake3::hash(&out));
        Ok((out, hash))
    }

    /// The region's identity: its blob's hash.
    pub fn hash(&self) -> Result<Hash, Error> {
        Ok(self.encode()?.1)
    }

    /// Read a blob (from storage or from anyone): checked, never panics.
    pub fn decode(blob: &[u8]) -> Result<Region, Error> {
        if blob.len() < HEADER || &blob[..4] != MAGIC {
            return err("not a region");
        }
        let u32_at = |i: usize| u32::from_le_bytes([blob[i], blob[i + 1], blob[i + 2], blob[i + 3]]);
        let version = u16::from_le_bytes([blob[4], blob[5]]);
        if version != FORMAT_VERSION {
            return err(format!("region format {version}, this build reads {FORMAT_VERSION}"));
        }
        let key = RegionKey { x: u32_at(6) as i32, z: u32_at(10) as i32 };
        let count = u32_at(14);
        if count > MAX_ZONES {
            return err("too many zones");
        }
        if blob.len() != HEADER + count as usize * ENTRY {
            return err("the region's length is wrong");
        }
        // checked, not wrapping: a crafted key near i32::MAX must not overflow on wasm32 either
        let base = |r: i32| r.checked_mul(REGION_ZONES).ok_or_else(|| Error("a region key out of range".into()));
        let (bx, bz) = (base(key.x)?, base(key.z)?);
        let mut zones = BTreeMap::new();
        let mut last: Option<(i32, i32)> = None;
        for e in blob[HEADER..].as_chunks::<ENTRY>().0 {
            let (x, z) = (i32::from(e[0]), i32::from(e[1]));
            if x >= REGION_ZONES || z >= REGION_ZONES {
                return err("a zone outside its region");
            }
            if last.is_some_and(|l| l >= (z, x)) {
                return err("zones out of order or repeated");
            }
            last = Some((z, x));
            let mut h = [0u8; 16];
            h.copy_from_slice(&e[2..]);
            zones.insert(ZoneKey { x: bx + x, z: bz + z }, Hash(h));
        }
        Ok(Region { key, zones })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Region {
        let key = RegionKey { x: -1, z: 2 };
        let mut r = Region::new(key);
        for (i, (x, z)) in [(-32, 64), (-1, 95), (-17, 70), (-32, 95)].into_iter().enumerate() {
            r.zones.insert(ZoneKey { x, z }, Hash([i as u8 + 1; 16]));
        }
        r
    }

    #[test]
    fn regions_round_trip_and_the_hash_is_the_content() {
        let r = sample();
        let (blob, hash) = r.encode().unwrap();
        let back = Region::decode(&blob).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.hash().unwrap(), hash);
        let mut other = r.clone();
        other.zones.insert(ZoneKey { x: -2, z: 80 }, Hash([9; 16]));
        assert_ne!(other.hash().unwrap(), hash);
        assert_eq!(Region::new(r.key).encode().unwrap().0.len(), HEADER, "an empty region is just its header");
    }

    #[test]
    fn keys_nest_chunk_zone_region_with_negatives() {
        assert_eq!(region_of_zone(ZoneKey { x: 31, z: 32 }), RegionKey { x: 0, z: 1 });
        assert_eq!(region_of_zone(ZoneKey { x: -1, z: -32 }), RegionKey { x: -1, z: -1 });
        assert_eq!(region_of_zone(ZoneKey { x: -33, z: 0 }), RegionKey { x: -2, z: 0 });
        // chunk 511 is the last of zone 31, the last of region 0; chunk 512 starts region 1
        assert_eq!(region_of(ChunkKey([511, 0, -1])), RegionKey { x: 0, z: -1 });
        assert_eq!(region_of(ChunkKey([512, 9, -512])), RegionKey { x: 1, z: -1 });
        assert_eq!(region_of(ChunkKey([-513, 0, 0])), RegionKey { x: -2, z: 0 });
        assert_eq!(RegionKey::parse("-3_12"), Some(RegionKey { x: -3, z: 12 }));
        assert_eq!(RegionKey::parse("3"), None);
    }

    #[test]
    fn a_zone_outside_the_region_is_not_written() {
        let mut r = sample();
        r.zones.insert(ZoneKey { x: 0, z: 64 }, Hash([1; 16]));
        assert!(r.encode().is_err());
    }

    #[test]
    fn crafted_blobs_are_refused_not_crashed_on() {
        let (good, _) = sample().encode().unwrap();
        for n in 0..good.len() {
            assert!(Region::decode(&good[..n]).is_err(), "cut at {n}");
        }
        let mut too_many = good.clone();
        too_many[14..18].copy_from_slice(&(MAX_ZONES + 1).to_le_bytes());
        assert!(Region::decode(&too_many).is_err());
        let mut outside = good.clone();
        outside[HEADER] = 32;
        assert!(Region::decode(&outside).is_err());
        // the second entry repeating the first
        let mut repeated = good.clone();
        let first = repeated[HEADER..HEADER + ENTRY].to_vec();
        repeated[HEADER + ENTRY..HEADER + 2 * ENTRY].copy_from_slice(&first);
        assert!(Region::decode(&repeated).is_err());
        let mut far = good.clone();
        far[6..10].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(Region::decode(&far).is_err(), "a key that overflows is refused");
        assert!(Region::decode(b"VOXR\x01\x00").is_err(), "a zone blob is not a region");
    }
}
