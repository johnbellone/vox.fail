//! What each player (or watcher) is sent: streaming by view radius
//! (docs/design/chunk-streaming.md §3, vox-cti.7).
//!
//! A client that sends a [`View`] gets the chunks within its radius instead of the whole world:
//! those nearby on joining, nearest first, then the rest as it moves, paced to a byte budget.
//! Edits go only to players interested in their chunk. A chunk is dropped from what a player
//! holds only past the radius and two chunks more, so walking along the edge doesn't thrash.
//! A client that never sends a view gets everything, as before.

use std::collections::{HashSet, VecDeque};

use vox_core::world::{Bounds, CHUNK, ChunkKey};
use vox_core::wire::View;

/// Chunks past the radius a held chunk may be before it counts as dropped (the client drops it
/// there too).
pub const KEEP_BEYOND: i32 = 2;
/// The budget a view that names none gets, kilobits a second (1 MB/s).
pub const DEFAULT_KBPS: u32 = 8000;
/// The most a view's budget may be, kilobits a second (8 MB/s).
pub const MOST_KBPS: u32 = 64_000;
/// Seconds of budget that may be saved up.
const BURST_SECONDS: f64 = 1.0;

/// What a world allows a view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// the most chunks of full detail round a player
    pub radius: u16,
    pub rings: u8,
}

impl Limits {
    /// A world's limits from its settings: `radius_m` metres of full detail, `rings` rings.
    pub fn new(radius_m: f32, rings: u8) -> Limits {
        let chunks = (vox_core::in_blocks(radius_m.max(0.0)) / CHUNK as f32).ceil();
        Limits { radius: (chunks as u16).clamp(1, vox_core::wire::MAX_VIEW_RADIUS), rings }
    }
}

/// Squared distance between two chunks, in chunks.
fn dist2(a: ChunkKey, b: ChunkKey) -> i64 {
    (0..3).map(|i| (a.0[i] as i64 - b.0[i] as i64).pow(2)).sum()
}

/// One player's interest.
#[derive(Debug)]
pub struct Interest {
    view: View,
    /// chunks within the radius (cut to the world's height), whether they hold blocks or not
    wanted: HashSet<ChunkKey>,
    /// chunks with blocks, wanted and not yet sent, nearest first
    pending: VecDeque<ChunkKey>,
    queued: HashSet<ChunkKey>,
    /// chunks the player holds
    sent: HashSet<ChunkKey>,
    /// bytes it may be sent now
    tokens: f64,
}

impl Interest {
    /// A view, clamped to `limits` and to what any world allows.
    pub fn new(view: View, limits: Limits, bounds: Bounds) -> Interest {
        let mut i = Interest { view, wanted: HashSet::new(), pending: VecDeque::new(), queued: HashSet::new(), sent: HashSet::new(), tokens: 0.0 };
        i.set_view(view, limits, bounds);
        i
    }

    #[cfg(test)]
    pub fn view(&self) -> View {
        self.view
    }

    /// A new view (the player moved or changed their radius): what is wanted is worked out again;
    /// chunks held past the radius and [`KEEP_BEYOND`] are forgotten (the client drops them too).
    pub fn set_view(&mut self, view: View, limits: Limits, bounds: Bounds) {
        let v = view.sane();
        let budget = if v.budget_kbps == 0 { DEFAULT_KBPS } else { v.budget_kbps.min(MOST_KBPS) };
        let r = v.radius.clamp(1, limits.radius);
        // the centre comes from the network: held to the world's chunks and a radius round them,
        // so nothing below can overflow
        let pad = r as i32 + 1;
        let lo = bounds.lo.map(|b| b.div_euclid(CHUNK) - pad);
        let hi = bounds.hi.map(|b| b.div_euclid(CHUNK) + pad);
        let centre = ChunkKey([0, 1, 2].map(|i| v.centre.0[i].clamp(lo[i], hi[i])));
        let v = View { centre, radius: r, rings: v.rings.min(limits.rings), budget_kbps: budget };
        // only the budget changed: nothing to work out again
        let same = self.view.centre == v.centre && self.view.radius == v.radius && self.view.rings == v.rings && !self.wanted.is_empty();
        self.view = v;
        if same {
            return;
        }
        let r = v.radius as i32;
        let c = v.centre.0;
        // the world's height in chunks: nothing is wanted above or below it
        let (ylo, yhi) = (bounds.lo[1].div_euclid(CHUNK), (bounds.hi[1] - 1).div_euclid(CHUNK));
        self.wanted.clear();
        for dx in -r..=r {
            for dz in -r..=r {
                for y in (c[1] - r).max(ylo)..=(c[1] + r).min(yhi) {
                    let k = ChunkKey([c[0] + dx, y, c[2] + dz]);
                    if dist2(k, v.centre) <= (r as i64).pow(2) {
                        self.wanted.insert(k);
                    }
                }
            }
        }
        let keep = ((r + KEEP_BEYOND) as i64).pow(2);
        self.sent.retain(|k| dist2(*k, v.centre) <= keep);
        self.pending.retain(|k| self.wanted.contains(k));
        self.queued.retain(|k| self.wanted.contains(k));
    }

    /// Queue the chunks with blocks (`existing`) that are wanted and not yet held, nearest first.
    pub fn queue(&mut self, existing: impl IntoIterator<Item = ChunkKey>) {
        for k in existing {
            if self.wanted.contains(&k) && !self.sent.contains(&k) && self.queued.insert(k) {
                self.pending.push_back(k);
            }
        }
        let centre = self.view.centre;
        self.pending.make_contiguous().sort_by_key(|k| dist2(*k, centre));
    }

    /// Everything queued, nearest first, marked as sent: what a player gets on joining, before
    /// they are told the world is ready.
    pub fn take_all(&mut self) -> Vec<ChunkKey> {
        let out: Vec<ChunkKey> = self.pending.drain(..).collect();
        self.queued.clear();
        self.sent.extend(out.iter().copied());
        out
    }

    /// The queued chunks to send now, `dt` seconds after the last call: nearest first, while the
    /// budget lasts (`size` gives each one's bytes). At least one goes out whenever the budget
    /// isn't in debt, so a chunk bigger than a second's budget still gets through.
    pub fn take_paced(&mut self, dt: f64, size: impl Fn(ChunkKey) -> usize) -> Vec<ChunkKey> {
        let per_second = self.view.budget_kbps as f64 * 1000.0 / 8.0;
        self.tokens = (self.tokens + per_second * dt.max(0.0)).min(per_second * BURST_SECONDS);
        let mut out = Vec::new();
        while self.tokens > 0.0 {
            let Some(k) = self.pending.pop_front() else { break };
            self.queued.remove(&k);
            self.tokens -= size(k) as f64;
            self.sent.insert(k);
            out.push(k);
        }
        out
    }

    /// Whether an edit to chunk `k` goes to this player: they hold it (in their view, or just
    /// past it and not yet forgotten), or it is in their view and not still waiting to be sent
    /// whole (which would carry the edit anyway).
    pub fn wants_edit(&self, k: ChunkKey) -> bool {
        self.sent.contains(&k) || (self.wanted.contains(&k) && !self.queued.contains(&k))
    }

    /// The radius granted, in chunks.
    pub fn radius(&self) -> u16 {
        self.view.radius
    }

    /// Whether anything waits to be sent.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// A chunk sent outside the queue (a whole chunk after a big build): held now.
    pub fn mark_sent(&mut self, k: ChunkKey) {
        if self.queued.remove(&k) {
            self.pending.retain(|q| *q != k);
        }
        self.sent.insert(k);
    }

    /// Whether the player holds chunk `k`.
    pub fn holds(&self, k: ChunkKey) -> bool {
        self.sent.contains(&k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(c: [i32; 3], radius: u16) -> View {
        View { centre: ChunkKey(c), radius, rings: 0, budget_kbps: 0 }
    }

    const LIMITS: Limits = Limits { radius: 16, rings: 3 };

    fn bounds() -> Bounds {
        Bounds { lo: [-10_000, -64, -10_000], hi: [10_000, 192, 10_000] }
    }

    #[test]
    fn a_world_limit_in_metres_is_chunks_of_quarter_metre_blocks() {
        // 128 m is 512 blocks, 16 chunks
        assert_eq!(Limits::new(128.0, 3).radius, 16);
        assert_eq!(Limits::new(0.0, 3).radius, 1, "at least the chunk you stand in");
        assert_eq!(Limits::new(1.0e6, 3).radius, vox_core::wire::MAX_VIEW_RADIUS);
    }

    #[test]
    fn a_view_is_clamped_to_the_world_and_wants_a_sphere_cut_to_its_height() {
        let i = Interest::new(View { budget_kbps: 1, ..view([0, 0, 0], 60) }, LIMITS, bounds());
        assert_eq!(i.view().radius, 16, "the world's limit");
        assert_eq!(i.view().budget_kbps, 1);
        assert!(i.wanted.contains(&ChunkKey([16, 0, 0])));
        assert!(!i.wanted.contains(&ChunkKey([16, 0, 1])), "a sphere, not a box");
        // the world is -64..192 blocks: chunks -2..=5 high
        assert!(i.wanted.contains(&ChunkKey([0, 5, 0])) && !i.wanted.contains(&ChunkKey([0, 6, 0])));
        assert!(i.wanted.contains(&ChunkKey([0, -2, 0])) && !i.wanted.contains(&ChunkKey([0, -3, 0])));
        let free = Interest::new(View { budget_kbps: u32::MAX, ..view([0, 0, 0], 2) }, LIMITS, bounds());
        assert_eq!(free.view().budget_kbps, MOST_KBPS);
        assert_eq!(Interest::new(view([0, 0, 0], 2), LIMITS, bounds()).view().budget_kbps, DEFAULT_KBPS);
    }

    #[test]
    fn the_nearest_chunks_come_first_and_only_those_in_view() {
        let mut i = Interest::new(view([0, 0, 0], 4), LIMITS, bounds());
        i.queue([ChunkKey([3, 0, 0]), ChunkKey([100, 0, 0]), ChunkKey([0, 0, 1]), ChunkKey([0, 0, 0]), ChunkKey([-2, 0, 2])]);
        assert_eq!(i.take_all(), vec![ChunkKey([0, 0, 0]), ChunkKey([0, 0, 1]), ChunkKey([-2, 0, 2]), ChunkKey([3, 0, 0])]);
        assert!(i.holds(ChunkKey([3, 0, 0])) && !i.holds(ChunkKey([100, 0, 0])));
        // queued again: what's held isn't sent twice
        i.queue([ChunkKey([3, 0, 0])]);
        assert!(i.take_all().is_empty());
    }

    #[test]
    fn moving_queues_what_comes_into_view_and_drops_what_is_well_behind() {
        let mut i = Interest::new(view([0, 0, 0], 4), LIMITS, bounds());
        let world: Vec<ChunkKey> = (-20..=20).map(|x| ChunkKey([x, 0, 0])).collect();
        i.queue(world.iter().copied());
        assert_eq!(i.take_all().len(), 9, "-4..=4");
        // four chunks east: 5..=8 come into view; -4..=-3 are past the radius but kept (hysteresis)
        i.set_view(view([4, 0, 0], 4), LIMITS, bounds());
        i.queue(world.iter().copied());
        let new: Vec<i32> = i.take_all().iter().map(|k| k.0[0]).collect();
        assert_eq!(new, vec![5, 6, 7, 8]);
        assert!(i.holds(ChunkKey([-2, 0, 0])), "6 chunks behind: within radius + 2");
        assert!(!i.holds(ChunkKey([-3, 0, 0])), "7 behind: dropped");
    }

    #[test]
    fn sending_is_paced_to_the_budget_but_never_stalls() {
        // 80 kbps is 10,000 bytes a second
        let mut i = Interest::new(View { budget_kbps: 80, ..view([0, 0, 0], 4) }, LIMITS, bounds());
        let keys: Vec<ChunkKey> = (0..4).map(|x| ChunkKey([x, 0, 0])).collect();
        i.queue(keys.iter().copied());
        // a tenth of a second: 1,000 bytes, so one 3,000-byte chunk goes (and the budget is in debt)
        assert_eq!(i.take_paced(0.1, |_| 3000).len(), 1);
        assert!(i.take_paced(0.1, |_| 3000).is_empty(), "still paying for the first");
        // a second's worth (the most that saves up): three more
        assert_eq!(i.take_paced(1.0, |_| 3000).len(), 3);
        // one bigger than a second's budget still goes
        let mut big = Interest::new(View { budget_kbps: 8, ..view([0, 0, 0], 4) }, LIMITS, bounds());
        big.queue(keys[..1].iter().copied());
        assert_eq!(big.take_paced(0.05, |_| 60_000).len(), 1);
    }

    #[test]
    fn edits_go_to_those_in_view_and_not_ahead_of_a_queued_chunk() {
        let mut i = Interest::new(view([0, 0, 0], 4), LIMITS, bounds());
        i.queue([ChunkKey([1, 0, 0])]);
        assert!(!i.wants_edit(ChunkKey([1, 0, 0])), "queued: the chunk will carry it");
        assert!(i.wants_edit(ChunkKey([2, 0, 0])), "in view, no blocks yet: the edit is how it arrives");
        assert!(!i.wants_edit(ChunkKey([50, 0, 0])), "out of view");
        i.mark_sent(ChunkKey([1, 0, 0]));
        assert!(i.wants_edit(ChunkKey([1, 0, 0])));
        assert!(i.take_all().is_empty(), "sent outside the queue, so no longer queued");
    }

    #[test]
    fn a_held_chunk_just_past_the_view_still_gets_edits() {
        let mut i = Interest::new(view([0, 0, 0], 4), LIMITS, bounds());
        let world: Vec<ChunkKey> = (-10..=10).map(|x| ChunkKey([x, 0, 0])).collect();
        i.queue(world.iter().copied());
        i.take_all();
        // a step east: chunk -5 is past the radius but still held, so its edits still come
        i.set_view(view([1, 0, 0], 4), LIMITS, bounds());
        assert!(i.holds(ChunkKey([-4, 0, 0])) && !i.wanted.contains(&ChunkKey([-4, 0, 0])));
        assert!(i.wants_edit(ChunkKey([-4, 0, 0])));
    }

    #[test]
    fn a_centre_from_the_network_cant_overflow() {
        for c in [[i32::MAX; 3], [i32::MIN; 3], [i32::MAX, i32::MIN, 0]] {
            let i = Interest::new(view(c, 16), LIMITS, bounds());
            assert!(i.view().centre.0.iter().all(|v| v.abs() < 1000), "{:?}", i.view().centre);
        }
    }
}