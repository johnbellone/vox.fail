//! Snapshots of the world (vox-format): regions that changed since the last one, as blobs in
//! the store, every few minutes and on closing; kept a week, with the edit log trimmed to match.
//! And restoring: to a snapshot, or to any moment (a snapshot plus the edits after it).

#[allow(unused_imports)]
use super::*;
use vox_format::{Region, RegionKey, region_of};

/// Where snapshots are kept: plain files, or an Oxen repository (a commit per snapshot, and
/// history older than a week squashed away).
pub enum SnapStore {
    Dir(vox_format::DirStore),
    Oxen(vox_format::OxenStore),
}

impl SnapStore {
    /// The store in `dir`; with an `oxen` binary, an Oxen repository there (a folder of plain
    /// snapshots from before is taken in as its first commit).
    pub fn open(dir: &std::path::Path, oxen: Option<&std::path::Path>) -> Result<SnapStore, String> {
        match oxen {
            Some(bin) => vox_format::OxenStore::open(dir, bin).map(SnapStore::Oxen).map_err(|e| format!("snapshots: {e}")),
            None => Ok(SnapStore::Dir(vox_format::DirStore::new(dir))),
        }
    }
}

impl vox_format::BlobStore for SnapStore {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), vox_format::Error> {
        match self {
            SnapStore::Dir(s) => s.put(key, bytes),
            SnapStore::Oxen(s) => s.put(key, bytes),
        }
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, vox_format::Error> {
        match self {
            SnapStore::Dir(s) => s.get(key),
            SnapStore::Oxen(s) => s.get(key),
        }
    }
    fn delete(&self, key: &str) -> Result<(), vox_format::Error> {
        match self {
            SnapStore::Dir(s) => s.delete(key),
            SnapStore::Oxen(s) => s.delete(key),
        }
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, vox_format::Error> {
        match self {
            SnapStore::Dir(s) => s.list(prefix),
            SnapStore::Oxen(s) => s.list(prefix),
        }
    }
    fn commit(&self, at: u64, note: &str) -> Result<(), vox_format::Error> {
        match self {
            SnapStore::Dir(s) => s.commit(at, note),
            SnapStore::Oxen(s) => s.commit(at, note),
        }
    }
    fn squash(&self, cutoff: u64) -> Result<usize, vox_format::Error> {
        match self {
            SnapStore::Dir(s) => s.squash(cutoff),
            SnapStore::Oxen(s) => s.squash(cutoff),
        }
    }
}

/// Which state to go back to.
#[derive(Clone, Debug)]
pub enum Restore {
    /// a snapshot by id, or "latest"
    Snapshot(String),
    /// the world as it was at this unix time: the snapshot before it, then the edits up to it
    At(u64),
}

impl WorldState {
    /// The regions to write: all of them, or the ones changed since the last snapshot.
    fn regions_to_write(&self, all: bool) -> Vec<Region> {
        let mut by_region: HashMap<RegionKey, Vec<ChunkKey>> = HashMap::new();
        for k in self.world.keys() {
            let r = region_of(k);
            if all || self.snap_changed.contains(&r) {
                by_region.entry(r).or_default().push(k);
            }
        }
        // a changed region with nothing left in it: written empty, so it's dropped
        for r in &self.snap_changed {
            by_region.entry(*r).or_default();
        }
        let palette = self.cfg.palette.len() as u16;
        by_region
            .into_iter()
            .map(|(r, ks)| Region::from_chunks(r, palette, ks.iter().filter_map(|k| self.world.chunk(*k).map(|c| (*k, c)))))
            .collect()
    }

    /// Take a snapshot now if anything changed (or none has been taken), then keep a week's.
    pub(super) fn snapshot(&mut self) -> Result<(), String> {
        self.last_snapshot = Instant::now();
        let Some(snaps) = &self.snaps else { return Ok(()) };
        if !self.snap_full && self.snap_changed.is_empty() {
            return Ok(());
        }
        let regions = self.regions_to_write(self.snap_full);
        let prev = if self.snap_full { None } else { self.snap_prev.as_ref() };
        let now = unix_now();
        let m = snaps.take(prev, &regions, now, self.seq, self.cfg.palette.len() as u16).map_err(|e| e.to_string())?;
        eprintln!("snapshot {}: {} regions ({} written)", m.id, m.regions.len(), regions.len());
        let (gone, blobs) = snaps.prune(now, self.cfg.keep_snapshots).map_err(|e| e.to_string())?;
        if gone > 0 {
            eprintln!("dropped {gone} snapshots older than a week ({blobs} blobs)");
        }
        if let SnapStore::Oxen(o) = &snaps.store {
            let commits = o.log().map(|l| l.len()).unwrap_or(0);
            eprintln!("oxen history: {commits} commits");
        }
        // the edit log only needs to reach back to the oldest snapshot kept
        if let Some(oldest) = snaps.list().map_err(|e| e.to_string())?.first() {
            let _ = self.db.execute("DELETE FROM edits WHERE seq <= ?1", params![oldest.seq as i64]);
        }
        self.snap_prev = Some(m);
        self.snap_changed.clear();
        self.snap_full = false;
        Ok(())
    }

    /// Go back: first a snapshot of how things are (so the restore can be undone), then the
    /// chosen state written over the world, then a snapshot of that. Run with the world closed.
    pub(super) fn restore(&mut self, which: &Restore) -> Result<String, String> {
        if self.snaps.is_none() {
            return Err("no snapshots for this world (--snapshots)".into());
        }
        // what we're leaving
        self.snap_full = self.snap_full || self.snap_prev.is_none();
        self.snapshot()?;
        let Some(snaps) = &self.snaps else { unreachable!() };
        let (m, until) = match which {
            Restore::Snapshot(id) if id == "latest" => (snaps.latest().map_err(|e| e.to_string())?.ok_or("no snapshots yet")?, None),
            Restore::Snapshot(id) => (snaps.get(id).map_err(|e| e.to_string())?, None),
            Restore::At(t) => (snaps.at_or_before(*t).map_err(|e| e.to_string())?.ok_or("no snapshot that early (they're kept a week)")?, Some(*t)),
        };
        let mut world = World::new(self.cfg.bounds, self.cfg.palette.len() as u16);
        for r in snaps.regions(&m).map_err(|e| e.to_string())? {
            for (k, c) in r.decode_chunks().map_err(|e| e.to_string())? {
                world.put_chunk(k, c);
            }
        }
        // then the edits after the snapshot, up to the moment
        let mut replayed = 0;
        if let Some(t) = until {
            let mut q = self
                .db
                .prepare("SELECT x, y, z, new FROM edits WHERE seq > ?1 AND at <= ?2 ORDER BY seq")
                .map_err(|e| e.to_string())?;
            let rows = q
                .query_map(params![m.seq as i64, t as i64], |r| Ok(([r.get::<_, i32>(0)?, r.get(1)?, r.get(2)?], r.get::<_, u16>(3)?)))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (c, b) = row.map_err(|e| e.to_string())?;
                world.set(c, b);
                replayed += 1;
            }
        }
        // every chunk that was or is now: rewritten (or removed) on the next flush
        let mut touched: HashSet<ChunkKey> = self.world.keys().into_iter().collect();
        touched.extend(world.keys());
        self.world = world;
        for k in &touched {
            self.dirty.insert(*k);
            self.snap_changed.insert(region_of(*k));
        }
        self.flush()?;
        self.snapshot()?;
        let what = match until {
            Some(t) => format!("restored to {t} (snapshot {} and {replayed} edits after it)", m.id),
            None => format!("restored snapshot {}", m.id),
        };
        eprintln!("{what}");
        Ok(what)
    }
}

/// Restore a world (closed) from its snapshots.
pub fn restore(cfg: Config, which: Restore) -> Result<String, String> {
    let mut s = WorldState::open(cfg)?;
    s.restore(&which)
}

/// A world's snapshots, newest last: (id, taken at, regions).
pub fn list_snapshots(dir: &std::path::Path, oxen: Option<&std::path::Path>) -> Result<Vec<(String, u64, usize)>, String> {
    let snaps = vox_format::Snapshots::new(SnapStore::open(dir, oxen)?);
    Ok(snaps.list().map_err(|e| e.to_string())?.into_iter().map(|m| (m.id, m.taken_at, m.regions.len())).collect())
}
