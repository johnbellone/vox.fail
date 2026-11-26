//! Snapshots of the world (vox-format): regions that changed since the last one, as blobs in
//! the store, every few minutes and on closing; kept a week, with the edit log trimmed to match.
//! And restoring: to a snapshot, or to any moment (a snapshot plus the edits after it).

#[allow(unused_imports)]
use super::*;
use vox_format::{Region, RegionKey, region_of};

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
pub fn list_snapshots(dir: &std::path::Path) -> Result<Vec<(String, u64, usize)>, String> {
    let snaps = vox_format::Snapshots::new(vox_format::DirStore::new(dir));
    Ok(snaps.list().map_err(|e| e.to_string())?.into_iter().map(|m| (m.id, m.taken_at, m.regions.len())).collect())
}
