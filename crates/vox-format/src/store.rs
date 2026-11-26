//! Where a world's blobs and snapshots live.
//!
//! [`BlobStore`] is the little an object store offers: put, get, delete and list by key. [`DirStore`]
//! keeps them in a folder; an S3 or R2 bucket (or Oxen, see the design) implements the same four
//! calls. On top of it, [`Snapshots`]:
//!
//! - region blobs are stored once, under `blobs/<first two of hash>/<hash>`;
//! - a snapshot is a [`Manifest`] at `snapshots/<id>.json`: every region's hash, the edit number it
//!   was taken at, a serial number (so two in one second never collide), and a root hash;
//! - a new snapshot writes only regions that changed (the rest are shared with the one before);
//! - [`Snapshots::prune`] drops snapshots older than the retention (a week), always keeping the
//!   newest, and deletes blobs no remaining snapshot names.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::{
    Error, err,
    region::{Region, RegionKey},
};

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

/// One snapshot: what every region was at a moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
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
    /// every region with blocks, by key (`x_z`)
    pub regions: BTreeMap<String, Hash>,
    /// over every (region, hash): equal roots, equal worlds
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

    pub fn region_key(s: &str) -> Option<RegionKey> {
        let (x, z) = s.split_once('_')?;
        Some(RegionKey { x: x.parse().ok()?, z: z.parse().ok()? })
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

    /// Take a snapshot: `prev` with the regions in `changed` replaced (a region with no chunks
    /// left is dropped). With no `prev`, `changed` must hold every region.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn take(&self, prev: Option<&Manifest>, changed: &[Region], taken_at: u64, seq: u64, palette_version: u16) -> Result<Manifest, Error> {
        let mut regions = prev.map(|m| m.regions.clone()).unwrap_or_default();
        for r in changed {
            let k = r.key.to_string();
            if r.chunks.is_empty() {
                regions.remove(&k);
                continue;
            }
            let (blob, hash) = r.encode()?;
            let key = blob_key(&hash);
            if self.store.get(&key)?.is_none() {
                self.store.put(&key, &blob)?;
            }
            regions.insert(k, hash);
        }
        let serial = self.list()?.last().map_or(1, |m| m.serial + 1);
        let m = Manifest {
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
        Ok(m)
    }

    /// Every snapshot, oldest first.
    pub fn list(&self) -> Result<Vec<Manifest>, Error> {
        let mut out = Vec::new();
        for key in self.store.list("snapshots/")? {
            let Some(bytes) = self.store.get(&key)? else { continue };
            let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| Error(format!("{key}: {e}")))?;
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

    /// A snapshot's regions, read and checked against their hashes.
    pub fn regions(&self, m: &Manifest) -> Result<Vec<Region>, Error> {
        let mut out = Vec::with_capacity(m.regions.len());
        for (k, h) in &m.regions {
            let blob = self.store.get(&blob_key(h))?.ok_or_else(|| Error(format!("region {k}: blob {h:?} is missing")))?;
            let r = Region::decode(&blob)?;
            if r.hash()? != *h || r.key.to_string() != *k {
                return err(format!("region {k}: the blob isn't what the snapshot says"));
            }
            out.push(r);
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
        for m in &gone {
            self.store.delete(&format!("snapshots/{}.json", m.id))?;
        }
        let named: BTreeSet<String> = kept.iter().flat_map(|m| m.regions.values().map(blob_key)).collect();
        let mut blobs = 0;
        for key in self.store.list("blobs/")? {
            if !named.contains(&key) {
                self.store.delete(&key)?;
                blobs += 1;
            }
        }
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

    fn region(w: &World, key: RegionKey) -> Region {
        let ks: Vec<ChunkKey> = w.keys().into_iter().filter(|k| crate::region_of(*k) == key).collect();
        Region::from_chunks(key, 1, ks.iter().map(|k| (*k, w.chunk(*k).unwrap())))
    }

    #[test]
    fn snapshots_share_unchanged_regions_and_prune_after_a_week() {
        let dir = temp("snap");
        let snaps = Snapshots::new(DirStore::new(&dir));
        let mut w = World::new(Bounds { lo: [-600, -64, -600], hi: [600, 128, 600] }, 5);
        let (a, b) = (RegionKey { x: 0, z: 0 }, RegionKey { x: -1, z: 0 });
        w.set([1, 1, 1], 2);
        w.set([-300, 1, 1], 3);
        let day = 24 * 3600;
        let s1 = snaps.take(None, &[region(&w, a), region(&w, b)], 1000, 10, 1).unwrap();
        assert_eq!(s1.regions.len(), 2);
        // a day later only region a changed: one new blob, b's shared
        w.set([2, 1, 1], 4);
        let s2 = snaps.take(Some(&s1), &[region(&w, a)], 1000 + day, 20, 1).unwrap();
        assert_eq!(s2.regions[&b.to_string()], s1.regions[&b.to_string()]);
        assert_ne!(s2.regions[&a.to_string()], s1.regions[&a.to_string()]);
        assert_eq!(snaps.store.list("blobs/").unwrap().len(), 3);
        // reading it back gives the world as it was
        let back = snaps.regions(&s2).unwrap();
        let mut w2 = World::new(w.bounds, 5);
        for r in back {
            for (k, c) in r.decode_chunks().unwrap() {
                w2.put_chunk(k, c);
            }
        }
        assert_eq!(w2.get([2, 1, 1]), 4);
        assert_eq!(w2.get([-300, 1, 1]), 3);
        // eight days after the first: it goes, and so does the blob only it named
        let (gone, blobs) = snaps.prune(1000 + 8 * day, KEEP_SECS).unwrap();
        assert_eq!((gone, blobs), (1, 1));
        assert_eq!(snaps.list().unwrap().len(), 1);
        assert!(snaps.regions(&s2).is_ok(), "the kept snapshot is whole");
        // the newest is never pruned, however old
        assert_eq!(snaps.prune(1000 + 100 * day, KEEP_SECS).unwrap(), (0, 0));
        assert_eq!(snaps.at_or_before(1000 + day).unwrap().unwrap().id, s2.id);
        assert!(snaps.at_or_before(10).unwrap().is_none());
        // two in the same second are two snapshots, in order
        let s3 = snaps.take(Some(&s2), &[], 1000 + 100 * day, 20, 1).unwrap();
        let s4 = snaps.take(Some(&s3), &[], 1000 + 100 * day, 20, 1).unwrap();
        assert_ne!(s3.id, s4.id);
        assert_eq!(snaps.latest().unwrap().unwrap().id, s4.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampered_blobs_and_manifests_are_noticed() {
        let dir = temp("tamper");
        let snaps = Snapshots::new(DirStore::new(&dir));
        let mut w = World::new(Bounds::default(), 5);
        w.set([1, 1, 1], 2);
        let s = snaps.take(None, &[region(&w, RegionKey { x: 0, z: 0 })], 1, 1, 1).unwrap();
        // a blob swapped for another region's
        let mut w2 = World::new(Bounds::default(), 5);
        w2.set([3, 1, 1], 4);
        let (other, _) = region(&w2, RegionKey { x: 0, z: 0 }).encode().unwrap();
        let h = s.regions["0_0"];
        snaps.store.put(&blob_key(&h), &other).unwrap();
        assert!(snaps.regions(&s).is_err());
        // a manifest edited by hand
        let key = format!("snapshots/{}.json", s.id);
        let text = String::from_utf8(snaps.store.get(&key).unwrap().unwrap()).unwrap();
        snaps.store.put(&key, text.replace(&h.hex(), &"0".repeat(32)).as_bytes()).unwrap();
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
