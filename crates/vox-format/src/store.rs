//! Where a world's blobs and snapshots live.
//!
//! [`BlobStore`] is the little an object store offers: put, get, delete and list by key. [`DirStore`]
//! keeps them in a folder; an S3 or R2 bucket (or Oxen, see the design) implements the same four
//! calls. On top of it, [`Snapshots`]:
//!
//! - zone blobs and region blobs (a region lists its zones' hashes) are stored once, under
//!   `blobs/<first two of hash>/<hash>`;
//! - a snapshot is a [`Manifest`] at `snapshots/<id>.json`: every region's hash, the edit number it
//!   was taken at, a serial number (so two in one second never collide), and a root hash;
//! - a new snapshot writes only the zones that changed and the regions they are in (the rest are
//!   shared with the one before);
//! - [`Snapshots::prune`] drops snapshots older than the retention (a week), always keeping the
//!   newest, and deletes blobs no remaining snapshot names.
//!
//! Snapshots from before regions held zones (manifest format 1, no `format` field) list zone
//! hashes directly under `regions`; they still list, restore and prune, and the first snapshot
//! taken after one writes the region blobs.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::{
    Error, err,
    region::Region,
    zone::{Zone, ZoneKey},
};
// taking a snapshot (native only) groups zones into regions
#[cfg(not(target_arch = "wasm32"))]
use crate::region::{RegionKey, region_of_zone};

/// A blob's identity: the first 16 bytes of its BLAKE3 hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash(pub [u8; 16]);

impl Hash {
    pub fn from_blake3(h: blake3::Hash) -> Hash {
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.as_bytes()[..16]);
        Hash(out)
    }
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
    pub fn from_hex(s: &str) -> Result<Hash, Error> {
        if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return err("not a hash");
        }
        let mut out = [0u8; 16];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|e| Error(e.to_string()))?;
        }
        Ok(Hash(out))
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex())
    }
}

impl Serialize for Hash {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        let s = String::deserialize(d)?;
        Hash::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// The calls an object store offers. Keys are `/`-separated paths of letters, digits, `_`,
/// `-` and `.`.
pub trait BlobStore {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), Error>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    fn delete(&self, key: &str) -> Result<(), Error>;
    /// Every key starting with `prefix`.
    fn list(&self, prefix: &str) -> Result<Vec<String>, Error>;
    /// A snapshot (or a pruning) has been written: stores with a history (Oxen) commit it,
    /// at `at` (unix seconds) with a note. The others have nothing to do.
    fn commit(&self, _at: u64, _note: &str) -> Result<(), Error> {
        Ok(())
    }
    /// Drop history before `cutoff` (unix seconds), keeping what came after; returns how many
    /// commits went. Stores without a history have none to drop.
    fn squash(&self, _cutoff: u64) -> Result<usize, Error> {
        Ok(0)
    }
}

fn check_key(key: &str) -> Result<(), Error> {
    let ok = !key.is_empty()
        && key.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && part.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)));
    if ok { Ok(()) } else { err(format!("not a key: {key}")) }
}

/// Blobs in a folder.
pub struct DirStore {
    root: PathBuf,
}

impl DirStore {
    pub fn new(root: impl Into<PathBuf>) -> DirStore {
        DirStore { root: root.into() }
    }
}

impl BlobStore for DirStore {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        check_key(key)?;
        let path = self.root.join(key);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // whole or not at all: write aside, then move into place
        let tmp = path.with_extension("partial");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        check_key(key)?;
        match std::fs::read(self.root.join(key)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn delete(&self, key: &str) -> Result<(), Error> {
        check_key(key)?;
        match std::fs::remove_file(self.root.join(key)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let mut out = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_none_or(|x| x != "partial") {
                    let Ok(rel) = p.strip_prefix(&self.root) else { continue };
                    let key = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                    if key.starts_with(prefix) {
                        out.push(key);
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }
}

/// The manifest format written now: `regions` names region blobs.
pub const MANIFEST_FORMAT: u16 = 2;

fn format_one() -> u16 {
    1
}

/// One snapshot: what every region was at a moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// [`MANIFEST_FORMAT`]; 1 (the field missing) is from before regions held zones, when
    /// `regions` named zone blobs directly
    #[serde(default = "format_one")]
    pub format: u16,
    /// `<unix time, 12 digits>-<serial, 8 digits>`: unique, and sorts in the order taken
    pub id: String,
    /// one more than the last snapshot's (two in the same second still differ)
    #[serde(default)]
    pub serial: u64,
    /// when (unix seconds)
    pub taken_at: u64,
    /// the world's edit number at the time (edits after it are in the log)
    pub seq: u64,
    pub palette_version: u16,
    /// every region with blocks, by key (`x_z`): its region blob's hash (format 1: zones and
    /// their zone blobs' hashes)
    pub regions: BTreeMap<String, Hash>,
    /// over every (region, hash): equal roots, equal worlds (as regions' hashes cover their
    /// zones' hashes, which cover their blocks)
    pub root: Hash,
}

impl Manifest {
    fn root_of(regions: &BTreeMap<String, Hash>) -> Hash {
        let mut h = blake3::Hasher::new();
        for (k, v) in regions {
            h.update(k.as_bytes());
            h.update(b"=");
            h.update(&v.0);
            h.update(b";");
        }
        Hash::from_blake3(h.finalize())
    }
}

fn blob_key(h: &Hash) -> String {
    let hex = h.hex();
    format!("blobs/{}/{}", &hex[..2], hex)
}

/// How long snapshots are kept.
pub const KEEP_SECS: u64 = 7 * 24 * 3600;

/// A world's snapshots, in a store.
pub struct Snapshots<S: BlobStore> {
    pub store: S,
}

impl<S: BlobStore> Snapshots<S> {
    pub fn new(store: S) -> Self {
        Snapshots { store }
    }

    /// Take a snapshot: `prev` with the zones in `changed` replaced (a zone with no chunks left
    /// is dropped, and so is a region with no zones left). With no `prev`, `changed` must hold
    /// every zone. Writes the changed zones' blobs and the blobs of the regions they are in.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn take(&self, prev: Option<&Manifest>, changed: &[Zone], taken_at: u64, seq: u64, palette_version: u16) -> Result<Manifest, Error> {
        let mut regions = BTreeMap::new();
        let mut touched: BTreeMap<RegionKey, Region> = BTreeMap::new();
        match prev {
            Some(m) if m.format >= MANIFEST_FORMAT => regions = m.regions.clone(),
            // from before regions: every region is new (its zone blobs are already stored)
            Some(m) => {
                for (z, h) in self.zone_hashes(m)? {
                    let r = region_of_zone(z);
                    touched.entry(r).or_insert_with(|| Region::new(r)).zones.insert(z, h);
                }
            }
            None => {}
        }
        for zone in changed {
            let r = region_of_zone(zone.key);
            let region = match touched.entry(r) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => {
                    let had = match regions.get(&r.to_string()) {
                        Some(h) => self.region(&r.to_string(), h)?,
                        None => Region::new(r),
                    };
                    e.insert(had)
                }
            };
            if zone.chunks.is_empty() {
                region.zones.remove(&zone.key);
                continue;
            }
            let (blob, hash) = zone.encode()?;
            self.put_once(&hash, &blob)?;
            region.zones.insert(zone.key, hash);
        }
        for (r, region) in touched {
            if region.zones.is_empty() {
                regions.remove(&r.to_string());
                continue;
            }
            let (blob, hash) = region.encode()?;
            self.put_once(&hash, &blob)?;
            regions.insert(r.to_string(), hash);
        }
        let serial = self.list()?.last().map_or(1, |m| m.serial + 1);
        let m = Manifest {
            format: MANIFEST_FORMAT,
            id: format!("{taken_at:012}-{serial:08}"),
            serial,
            taken_at,
            seq,
            palette_version,
            root: Manifest::root_of(&regions),
            regions,
        };
        let json = serde_json::to_vec_pretty(&m).map_err(|e| Error(e.to_string()))?;
        self.store.put(&format!("snapshots/{}.json", m.id), &json)?;
        self.store.commit(taken_at, &format!("snapshot {}", m.id))?;
        Ok(m)
    }

    /// Stores a blob under its hash, unless it is there already.
    #[cfg(not(target_arch = "wasm32"))]
    fn put_once(&self, hash: &Hash, blob: &[u8]) -> Result<(), Error> {
        let key = blob_key(hash);
        if self.store.get(&key)?.is_none() {
            self.store.put(&key, blob)?;
        }
        Ok(())
    }

    /// A region blob, read and checked against its hash and its key.
    pub fn region(&self, key: &str, h: &Hash) -> Result<Region, Error> {
        let blob = self.store.get(&blob_key(h))?.ok_or_else(|| Error(format!("region {key}: blob {h:?} is missing")))?;
        let r = Region::decode(&blob)?;
        if r.hash()? != *h || r.key.to_string() != key {
            return err(format!("region {key}: the blob isn't what the snapshot says"));
        }
        Ok(r)
    }

    /// Every zone a snapshot holds, with its blob's hash (reading the region blobs).
    pub fn zone_hashes(&self, m: &Manifest) -> Result<BTreeMap<ZoneKey, Hash>, Error> {
        let mut out = BTreeMap::new();
        if m.format < MANIFEST_FORMAT {
            for (k, h) in &m.regions {
                let z = ZoneKey::parse(k).ok_or_else(|| Error(format!("not a zone: {k}")))?;
                out.insert(z, *h);
            }
            return Ok(out);
        }
        for (k, h) in &m.regions {
            out.extend(self.region(k, h)?.zones);
        }
        Ok(out)
    }

    /// Every snapshot, oldest first.
    pub fn list(&self) -> Result<Vec<Manifest>, Error> {
        let mut out = Vec::new();
        for key in self.store.list("snapshots/")? {
            let Some(bytes) = self.store.get(&key)? else { continue };
            let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| Error(format!("{key}: {e}")))?;
            // a newer format would be read with the wrong rules: refuse it instead
            if m.format > MANIFEST_FORMAT {
                return err(format!("{key}: manifest format {}, this build reads up to {MANIFEST_FORMAT}", m.format));
            }
            if m.root != Manifest::root_of(&m.regions) {
                return err(format!("{key}: its root doesn't match its regions"));
            }
            out.push(m);
        }
        out.sort_by_key(|m| (m.taken_at, m.serial));
        Ok(out)
    }

    pub fn latest(&self) -> Result<Option<Manifest>, Error> {
        Ok(self.list()?.pop())
    }

    /// A snapshot by id.
    pub fn get(&self, id: &str) -> Result<Manifest, Error> {
        self.list()?.into_iter().find(|m| m.id == id).ok_or_else(|| Error(format!("no snapshot {id}")))
    }

    /// The newest snapshot taken at or before `t`.
    pub fn at_or_before(&self, t: u64) -> Result<Option<Manifest>, Error> {
        Ok(self.list()?.into_iter().rfind(|m| m.taken_at <= t))
    }

    /// A snapshot's zones, read and checked against their hashes.
    pub fn zones(&self, m: &Manifest) -> Result<Vec<Zone>, Error> {
        let hashes = self.zone_hashes(m)?;
        let mut out = Vec::with_capacity(hashes.len());
        for (k, h) in &hashes {
            let blob = self.store.get(&blob_key(h))?.ok_or_else(|| Error(format!("zone {k}: blob {h:?} is missing")))?;
            let z = Zone::decode(&blob)?;
            if z.hash()? != *h || z.key != *k {
                return err(format!("zone {k}: the blob isn't what the snapshot says"));
            }
            out.push(z);
        }
        Ok(out)
    }

    /// Drop snapshots taken more than `keep` seconds before `now` (always keeping the newest),
    /// then blobs no kept snapshot names. Returns (snapshots, blobs) removed.
    pub fn prune(&self, now: u64, keep: u64) -> Result<(usize, usize), Error> {
        let all = self.list()?;
        let newest = all.last().map(|m| m.id.clone());
        let (gone, kept): (Vec<Manifest>, Vec<Manifest>) =
            all.into_iter().partition(|m| now.saturating_sub(m.taken_at) > keep && Some(&m.id) != newest.as_ref());
        // what the kept snapshots name: their regions and the zones in them. A region that
        // can't be read stops the pruning, so nothing it names is deleted by mistake.
        let mut named: BTreeSet<String> = BTreeSet::new();
        for m in &kept {
            if m.format >= MANIFEST_FORMAT {
                named.extend(m.regions.values().map(blob_key));
            }
            named.extend(self.zone_hashes(m)?.values().map(blob_key));
        }
        // only once that is known: nothing is deleted if a kept snapshot can't be read
        for m in &gone {
            self.store.delete(&format!("snapshots/{}.json", m.id))?;
        }
        let mut blobs = 0;
        for key in self.store.list("blobs/")? {
            if !named.contains(&key) {
                self.store.delete(&key)?;
                blobs += 1;
            }
        }
        // a store with a history records the pruning, then lets go of history older than what
        // we keep (what it holds of those weeks is only what's gone from the snapshots)
        if !gone.is_empty() || blobs > 0 {
            self.store.commit(now, &format!("pruned {} snapshots, {blobs} blobs", gone.len()))?;
        }
        self.store.squash(now.saturating_sub(keep))?;
        Ok((gone.len(), blobs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::{ChunkKey, World, world::Bounds};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vox-format-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn zone(w: &World, key: ZoneKey) -> Zone {
        let ks: Vec<ChunkKey> = w.keys().into_iter().filter(|k| crate::zone_of(*k) == key).collect();
        Zone::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())))
    }

    fn rebuild(snaps: &Snapshots<DirStore>, m: &Manifest, bounds: Bounds) -> World {
        let mut w = World::new(bounds, 5);
        for z in snaps.zones(m).unwrap() {
            for (k, c) in z.decode_chunks().unwrap() {
                w.put_chunk(k, c);
            }
        }
        w
    }

    #[test]
    fn snapshots_share_unchanged_regions_and_prune_after_a_week() {
        let dir = temp("snap");
        let snaps = Snapshots::new(DirStore::new(&dir));
        let mut w = World::new(Bounds { lo: [-600, -64, -600], hi: [600, 128, 600] }, 5);
        // zones 0 and -1 are in regions 0 and -1
        let (a, b) = (ZoneKey { x: 0, z: 0 }, ZoneKey { x: -1, z: 0 });
        let (ra, rb) = (region_of_zone(a).to_string(), region_of_zone(b).to_string());
        w.set([1, 1, 1], 2);
        w.set([-300, 1, 1], 3);
        let day = 24 * 3600;
        let s1 = snaps.take(None, &[zone(&w, a), zone(&w, b)], 1000, 10, 1).unwrap();
        assert_eq!((s1.format, s1.regions.len()), (MANIFEST_FORMAT, 2));
        assert_eq!(snaps.store.list("blobs/").unwrap().len(), 4, "two zones, two regions");
        // a day later only zone a changed: a new zone and a new region; b's region is shared
        w.set([2, 1, 1], 4);
        let s2 = snaps.take(Some(&s1), &[zone(&w, a)], 1000 + day, 20, 1).unwrap();
        assert_eq!(s2.regions[&rb], s1.regions[&rb]);
        assert_ne!(s2.regions[&ra], s1.regions[&ra]);
        assert_eq!(snaps.store.list("blobs/").unwrap().len(), 6);
        // reading it back gives the world as it was
        let w2 = rebuild(&snaps, &s2, w.bounds);
        assert_eq!(w2.get([2, 1, 1]), 4);
        assert_eq!(w2.get([-300, 1, 1]), 3);
        // eight days after the first: it goes, and so do the zone and region only it named
        let (gone, blobs) = snaps.prune(1000 + 8 * day, KEEP_SECS).unwrap();
        assert_eq!((gone, blobs), (1, 2));
        assert_eq!(snaps.list().unwrap().len(), 1);
        assert!(snaps.zones(&s2).is_ok(), "the kept snapshot is whole");
        // the newest is never pruned, however old
        assert_eq!(snaps.prune(1000 + 100 * day, KEEP_SECS).unwrap(), (0, 0));
        assert_eq!(snaps.at_or_before(1000 + day).unwrap().unwrap().id, s2.id);
        assert!(snaps.at_or_before(10).unwrap().is_none());
        // two in the same second are two snapshots, in order
        let s3 = snaps.take(Some(&s2), &[], 1000 + 100 * day, 20, 1).unwrap();
        let s4 = snaps.take(Some(&s3), &[], 1000 + 100 * day, 20, 1).unwrap();
        assert_ne!(s3.id, s4.id);
        assert_eq!(snaps.latest().unwrap().unwrap().id, s4.id);
        // emptying zone b drops its region from the next snapshot
        w.set([-300, 1, 1], 0);
        let s5 = snaps.take(Some(&s4), &[zone(&w, b)], 1000 + 101 * day, 30, 1).unwrap();
        assert_eq!(s5.regions.keys().collect::<Vec<_>>(), vec![&ra]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_in_one_zone_shares_the_other_zones_of_its_region() {
        let dir = temp("one-region");
        let snaps = Snapshots::new(DirStore::new(&dir));
        let mut w = World::new(Bounds { lo: [-600, -64, -600], hi: [600, 128, 600] }, 5);
        // zones 0 and 1 (zones are 512 blocks across) are both in region 0
        let (a, b) = (ZoneKey { x: 0, z: 0 }, ZoneKey { x: 1, z: 0 });
        w.set([1, 1, 1], 2);
        w.set([520, 1, 1], 3);
        let s1 = snaps.take(None, &[zone(&w, a), zone(&w, b)], 1, 1, 1).unwrap();
        assert_eq!(s1.regions.len(), 1);
        w.set([521, 1, 1], 4);
        let s2 = snaps.take(Some(&s1), &[zone(&w, b)], 2, 2, 1).unwrap();
        let (z1, z2) = (snaps.zone_hashes(&s1).unwrap(), snaps.zone_hashes(&s2).unwrap());
        assert_eq!(z1[&a], z2[&a], "zone a is shared");
        assert_ne!(z1[&b], z2[&b]);
        assert_ne!(s1.root, s2.root);
        // one zone and one region new: 2 + 2 blobs, then 1 + 1
        assert_eq!(snaps.store.list("blobs/").unwrap().len(), 5);
        assert_eq!(rebuild(&snaps, &s2, w.bounds).get([521, 1, 1]), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshots_from_before_regions_still_restore_and_carry_on() {
        let dir = temp("format1");
        let snaps = Snapshots::new(DirStore::new(&dir));
        let mut w = World::new(Bounds { lo: [-600, -64, -600], hi: [600, 128, 600] }, 5);
        let (a, b) = (ZoneKey { x: 0, z: 0 }, ZoneKey { x: -1, z: 0 });
        w.set([1, 1, 1], 2);
        w.set([-300, 1, 1], 3);
        // a format 1 manifest, as written before zones: zone hashes under "regions", no format
        let mut flat = BTreeMap::new();
        for key in [a, b] {
            let (blob, h) = zone(&w, key).encode().unwrap();
            snaps.store.put(&blob_key(&h), &blob).unwrap();
            flat.insert(key.to_string(), h);
        }
        let json = serde_json::json!({
            "id": "000000001000-00000001", "serial": 1, "taken_at": 1000, "seq": 10,
            "palette_version": 1, "regions": flat, "root": Manifest::root_of(&flat),
        });
        snaps.store.put("snapshots/000000001000-00000001.json", json.to_string().as_bytes()).unwrap();
        let old = snaps.latest().unwrap().unwrap();
        assert_eq!(old.format, 1);
        assert_eq!(rebuild(&snaps, &old, w.bounds).get([-300, 1, 1]), 3, "it restores");
        // the next snapshot writes the regions; the zone blobs it already had are kept as they are
        w.set([2, 1, 1], 4);
        let new = snaps.take(Some(&old), &[zone(&w, a)], 2000, 20, 1).unwrap();
        assert_eq!(new.format, MANIFEST_FORMAT);
        assert_eq!(new.regions.len(), 2);
        assert_eq!(snaps.zone_hashes(&new).unwrap()[&b], flat[&b.to_string()]);
        let back = rebuild(&snaps, &new, w.bounds);
        assert_eq!((back.get([2, 1, 1]), back.get([-300, 1, 1])), (4, 3));
        // pruning with both formats kept deletes nothing either names
        assert_eq!(snaps.prune(2000, KEEP_SECS).unwrap(), (0, 0));
        assert!(snaps.zones(&old).is_ok() && snaps.zones(&new).is_ok());
        // and once the old one is gone, only its own zone a goes with it
        assert_eq!(snaps.prune(1000 + KEEP_SECS + 1, KEEP_SECS).unwrap(), (1, 1));
        assert!(snaps.zones(&new).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampered_blobs_and_manifests_are_noticed() {
        let dir = temp("tamper");
        let snaps = Snapshots::new(DirStore::new(&dir));
        let mut w = World::new(Bounds::default(), 5);
        w.set([1, 1, 1], 2);
        let a = ZoneKey { x: 0, z: 0 };
        let s = snaps.take(None, &[zone(&w, a)], 1, 1, 1).unwrap();
        // a zone blob swapped for another world's
        let mut w2 = World::new(Bounds::default(), 5);
        w2.set([3, 1, 1], 4);
        let (other, _) = zone(&w2, a).encode().unwrap();
        let zh = snaps.zone_hashes(&s).unwrap()[&a];
        let good = snaps.store.get(&blob_key(&zh)).unwrap().unwrap();
        snaps.store.put(&blob_key(&zh), &other).unwrap();
        assert!(snaps.zones(&s).is_err());
        snaps.store.put(&blob_key(&zh), &good).unwrap();
        // a region blob swapped for one naming another zone hash
        let rh = s.regions["0_0"];
        let mut forged = Region::new(RegionKey { x: 0, z: 0 });
        forged.zones.insert(a, Hash([1; 16]));
        snaps.store.put(&blob_key(&rh), &forged.encode().unwrap().0).unwrap();
        assert!(snaps.zone_hashes(&s).is_err());
        assert!(snaps.prune(10 * KEEP_SECS, KEEP_SECS).is_err(), "pruning stops rather than guess");
        // a manifest edited by hand
        let key = format!("snapshots/{}.json", s.id);
        let text = String::from_utf8(snaps.store.get(&key).unwrap().unwrap()).unwrap();
        snaps.store.put(&key, text.replace(&rh.hex(), &"0".repeat(32)).as_bytes()).unwrap();
        assert!(snaps.list().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_cannot_leave_the_store() {
        let s = DirStore::new(temp("keys"));
        for bad in ["../x", "a/../../b", "/etc/passwd", "a//b", "", "a/b c"] {
            assert!(s.put(bad, b"x").is_err(), "{bad}");
        }
        assert_eq!(Hash::from_hex(&Hash([7; 16]).hex()).unwrap(), Hash([7; 16]));
        assert!(Hash::from_hex("zz").is_err());
    }
}
