//! A player's view of the world: the blocks the server has confirmed, with the player's own
//! edits on top until the server answers them. Edits show at once; a refused one disappears.

use crate::wire::Applied;
use crate::world::{Cell, ChunkKey, Chunk, Why, World};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    id: u32,
    cell: Cell,
    block: u16,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    /// what the server has said
    pub confirmed: World,
    pending: Vec<Pending>,
    next_id: u32,
    /// the last edit number seen
    pub seq: u64,
}

impl View {
    pub fn new(world: World) -> Self {
        View { confirmed: world, ..Default::default() }
    }

    /// The block as this player sees it.
    pub fn get(&self, c: Cell) -> u16 {
        self.pending.iter().rev().find(|p| p.cell == c).map_or_else(|| self.confirmed.get(c), |p| p.block)
    }

    pub fn solid(&self, c: Cell) -> bool {
        self.get(c) != 0
    }

    /// Every block in the cube from `lo`, `size` blocks a side, as this player sees them, in
    /// y, z, x order (the same as asking [`View::get`] for each, much faster: each chunk is
    /// looked up once, and pending edits are laid on top at the end).
    pub fn copy_region(&self, lo: Cell, size: i32, out: &mut Vec<u16>) {
        let n = size.max(0) as usize;
        out.clear();
        out.resize(n * n * n, 0);
        let hi = [lo[0] + size, lo[1] + size, lo[2] + size];
        let k_lo = ChunkKey::of(lo);
        let k_hi = ChunkKey::of([hi[0] - 1, hi[1] - 1, hi[2] - 1]);
        for ky in k_lo.0[1]..=k_hi.0[1] {
            for kz in k_lo.0[2]..=k_hi.0[2] {
                for kx in k_lo.0[0]..=k_hi.0[0] {
                    let k = ChunkKey([kx, ky, kz]);
                    let Some(ch) = self.confirmed.chunk(k) else { continue };
                    let o = k.origin();
                    let a = [lo[0].max(o[0]), lo[1].max(o[1]), lo[2].max(o[2])];
                    let b = [hi[0].min(o[0] + crate::world::CHUNK), hi[1].min(o[1] + crate::world::CHUNK), hi[2].min(o[2] + crate::world::CHUNK)];
                    for y in a[1]..b[1] {
                        for z in a[2]..b[2] {
                            let row = ((y - lo[1]) as usize * n + (z - lo[2]) as usize) * n;
                            for x in a[0]..b[0] {
                                out[row + (x - lo[0]) as usize] = ch.get([x, y, z]);
                            }
                        }
                    }
                }
            }
        }
        // your own edits, oldest first, so the newest wins as in `get`
        for p in &self.pending {
            let c = p.cell;
            if (0..3).all(|i| c[i] >= lo[i] && c[i] < hi[i]) {
                out[((c[1] - lo[1]) as usize * n + (c[2] - lo[2]) as usize) * n + (c[0] - lo[0]) as usize] = p.block;
            }
        }
    }

    /// Make an edit locally: checked like the server would, shown at once. Returns the id
    /// to send it with.
    pub fn edit(&mut self, c: Cell, b: u16) -> Result<u32, Why> {
        if !self.confirmed.bounds.contains(c) {
            return Err(Why::Outside);
        }
        if b > self.confirmed.kinds {
            return Err(Why::NoSuchBlock);
        }
        if self.get(c) == b {
            return Err(Why::NoChange);
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.pending.push(Pending { id: self.next_id, cell: c, block: b });
        Ok(self.next_id)
    }

    /// The server made an edit (anyone's). Returns the cell if what this player sees there
    /// changed.
    pub fn applied(&mut self, a: &Applied, me: u32) -> Option<Cell> {
        let before = self.get(a.cell);
        self.seq = self.seq.max(a.seq);
        self.confirmed.set(a.cell, a.block);
        if a.player == me {
            self.pending.retain(|p| p.id != a.id);
        }
        (self.get(a.cell) != before).then_some(a.cell)
    }

    /// The server refused one of this player's edits. Returns the cell if what this player
    /// sees there changed.
    pub fn refused(&mut self, id: u32) -> Option<Cell> {
        let i = self.pending.iter().position(|p| p.id == id)?;
        let cell = self.pending[i].cell;
        let before = self.get(cell);
        self.pending.remove(i);
        (self.get(cell) != before).then_some(cell)
    }

    /// A whole chunk from the server.
    pub fn chunk(&mut self, k: ChunkKey, c: Chunk) {
        self.confirmed.put_chunk(k, c);
    }

    /// Edits not answered yet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copying_a_region_matches_asking_block_by_block() {
        use crate::world::Bounds;
        let mut w = World::new(Bounds::default(), 9);
        let mut s = 12345u32;
        for _ in 0..4000 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let c = [(s % 120) as i32 - 60, ((s >> 8) % 40) as i32 - 10, ((s >> 16) % 120) as i32 - 60];
            w.set(c, (s % 9) as u16 + 1);
        }
        let mut v = View::new(w);
        let _ = v.edit([1, 1, 1], 0);
        let _ = v.edit([31, 0, 32], 3);
        let _ = v.edit([31, 0, 32], 5);
        let lo = [-1, -1, -1];
        let mut out = Vec::new();
        v.copy_region(lo, 34, &mut out);
        let mut i = 0;
        for y in 0..34 {
            for z in 0..34 {
                for x in 0..34 {
                    let c = [lo[0] + x, lo[1] + y, lo[2] + z];
                    assert_eq!(out[i], v.get(c), "{c:?}");
                    i += 1;
                }
            }
        }
        assert_eq!(out[((1 + 0) * 34 + (32 + 1)) * 34 + (31 + 1)], 5, "the newest pending edit wins");
    }
    use crate::world::Bounds;

    fn applied(seq: u64, player: u32, id: u32, cell: Cell, block: u16) -> Applied {
        Applied { seq, player, id, cell, block }
    }

    #[test]
    fn own_edit_shows_then_is_confirmed() {
        let mut v = View::new(World::new(Bounds::default(), 4));
        let id = v.edit([1, 2, 3], 2).unwrap();
        assert_eq!(v.get([1, 2, 3]), 2);
        assert_eq!(v.confirmed.get([1, 2, 3]), 0);
        assert_eq!(v.applied(&applied(1, 9, id, [1, 2, 3], 2), 9), None, "already shown");
        assert_eq!(v.pending(), 0);
        assert_eq!(v.confirmed.get([1, 2, 3]), 2);
    }

    #[test]
    fn refused_edit_disappears() {
        let mut v = View::new(World::new(Bounds::default(), 4));
        let id = v.edit([0, 0, 0], 1).unwrap();
        assert_eq!(v.refused(id), Some([0, 0, 0]));
        assert_eq!(v.get([0, 0, 0]), 0);
        assert_eq!(v.refused(id), None);
    }

    #[test]
    fn others_edits_show_unless_ours_is_pending() {
        let mut v = View::new(World::new(Bounds::default(), 4));
        assert_eq!(v.applied(&applied(1, 2, 1, [5, 0, 0], 3), 9), Some([5, 0, 0]));
        assert_eq!(v.get([5, 0, 0]), 3);
        // ours goes on top of theirs until it's answered
        let id = v.edit([6, 0, 0], 1).unwrap();
        assert_eq!(v.applied(&applied(2, 2, 2, [6, 0, 0], 4), 9), None);
        assert_eq!(v.get([6, 0, 0]), 1);
        // ours lands after theirs: the same as what's shown
        assert_eq!(v.applied(&applied(3, 9, id, [6, 0, 0], 1), 9), None);
        assert_eq!(v.get([6, 0, 0]), 1);
        assert_eq!(v.seq, 3);
    }

    #[test]
    fn local_checks() {
        let mut v = View::new(World::new(Bounds::default(), 4));
        assert_eq!(v.edit([0, 0, 0], 0), Err(Why::NoChange));
        assert_eq!(v.edit([0, 0, 0], 5), Err(Why::NoSuchBlock));
        assert_eq!(v.edit([999, 0, 0], 1), Err(Why::Outside));
        // two edits to one block: the later one shows; refusing the earlier leaves it
        let a = v.edit([0, 0, 0], 1).unwrap();
        let _b = v.edit([0, 0, 0], 2).unwrap();
        assert_eq!(v.refused(a), None);
        assert_eq!(v.get([0, 0, 0]), 2);
    }
}
