//! Moving a box (a player) through the blocks: it can't pass through solid blocks, and when
//! walking it steps up onto a block one high. Props add their own cells beside the blocks
//! ([`Occupancy`], [`with_props`]); the caller passes the combined test to [`Body::travel`].

use std::collections::BTreeSet;

use crate::entity::{PROP_CELL_BLOCKS, PROP_UNITS_PER_CELL, PROP_YAWS, prop_cell};
use crate::world::Cell;

use crate::in_blocks;

const EPS: f32 = 1e-3;

/// A player's box: 0.6 m across (half of it either side of the feet) and 1.8 m tall, in blocks.
pub const PLAYER_HALF: f32 = in_blocks(0.3);
pub const PLAYER_HEIGHT: f32 = in_blocks(1.8);
/// Eye height above the feet, in blocks (1.6 m).
pub const EYES: f32 = in_blocks(1.6);
/// Walking climbs onto anything up to this high without a jump, in blocks: half a metre and
/// a little, so a half-metre step (two blocks) is walked up.
pub const STEP_UP: f32 = in_blocks(0.525);
/// Moves are cut into pieces this long so nothing tunnels through a block.
const PIECE: f32 = 0.25;

/// A player's box: `pos` is the middle of the feet; it reaches `half` either side in x and
/// z and `height` up. All in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub pos: [f32; 3],
    pub half: f32,
    pub height: f32,
}

/// What a move ran into.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Contact {
    /// blocked along x, y, z
    pub hit: [bool; 3],
    /// standing on something (blocked while moving down)
    pub ground: bool,
    /// stepped up onto a block
    pub stepped: bool,
}

impl Body {
    fn lo_hi(&self, p: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        (
            [p[0] - self.half, p[1], p[2] - self.half],
            [p[0] + self.half, p[1] + self.height, p[2] + self.half],
        )
    }

    /// Is the box at `p` inside any solid block?
    pub fn blocked(&self, solid: &impl Fn(Cell) -> bool, p: [f32; 3]) -> bool {
        let (lo, hi) = self.lo_hi(p);
        let a = lo.map(|v| v.floor() as i32);
        let b = hi.map(|v| v.floor() as i32);
        for x in a[0]..=b[0] {
            for y in a[1]..=b[1] {
                for z in a[2]..=b[2] {
                    if solid([x, y, z]) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Move along one axis as far as possible; true if something was in the way.
    fn slide(&mut self, solid: &impl Fn(Cell) -> bool, axis: usize, d: f32) -> bool {
        let mut left = d;
        while left.abs() > 0.0 {
            let piece = left.clamp(-PIECE, PIECE);
            left -= piece;
            let mut p = self.pos;
            p[axis] += piece;
            if !self.blocked(solid, p) {
                self.pos = p;
                continue;
            }
            // put the box against the face of the block it ran into
            let (lo, hi) = self.lo_hi(p);
            let (lo0, hi0) = self.lo_hi(self.pos);
            if piece > 0.0 {
                let face = hi[axis].floor();
                p[axis] = self.pos[axis] + (face - EPS - hi0[axis]).max(0.0).min(piece);
            } else {
                let face = lo[axis].floor() + 1.0;
                p[axis] = self.pos[axis] + (face + EPS - lo0[axis]).min(0.0).max(piece);
            }
            if !self.blocked(solid, p) {
                self.pos = p;
            }
            return true;
        }
        false
    }

    /// Move by `delta`, x then z then y. With `step_up`, a walk blocked by something up to
    /// `step_up` high climbs onto it instead.
    pub fn travel(&mut self, solid: impl Fn(Cell) -> bool, delta: [f32; 3], step_up: f32) -> Contact {
        let mut c = Contact::default();
        let start = self.pos;
        let flat = [delta[0], delta[2]];
        let hit_x = self.slide(&solid, 0, flat[0]);
        let hit_z = self.slide(&solid, 2, flat[1]);
        if (hit_x || hit_z) && step_up > 0.0 && delta[1] <= 0.0 {
            // try again from higher up and keep it if it gets further
            let walked = self.pos;
            let mut up = *self;
            up.pos = start;
            if !up.slide(&solid, 1, step_up) {
                let hx = up.slide(&solid, 0, flat[0]);
                let hz = up.slide(&solid, 2, flat[1]);
                let far = |p: [f32; 3]| (p[0] - start[0]).powi(2) + (p[2] - start[2]).powi(2);
                if far(up.pos) > far(walked) + 1e-6 {
                    up.slide(&solid, 1, -step_up);
                    self.pos = up.pos;
                    c.stepped = true;
                    c.hit = [hx, false, hz];
                }
            }
            if !c.stepped {
                c.hit = [hit_x, false, hit_z];
            }
        } else {
            c.hit = [hit_x, false, hit_z];
        }
        c.hit[1] = self.slide(&solid, 1, delta[1]);
        c.ground = c.hit[1] && delta[1] < 0.0;
        c
    }

    /// Is there ground just under the feet?
    pub fn on_ground(&self, solid: impl Fn(Cell) -> bool) -> bool {
        let p = [self.pos[0], self.pos[1] - 2.0 * EPS, self.pos[2]];
        self.blocked(&solid, p)
    }
}

/// The cells props block, beside the blocks. Built from the props in a zone, so the server and
/// the client (prediction) derive the same set from the same instances.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Occupancy {
    cells: BTreeSet<Cell>,
}

impl Occupancy {
    pub fn contains(&self, c: Cell) -> bool {
        self.cells.contains(&c)
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Blocks the cells of a prop at `at` (5 cm units), turned by `yaw` (15 degree steps).
    pub fn add_prop(&mut self, at: [i32; 3], yaw: u8, footprint: &[[i16; 2]], height: u8) {
        self.cells.extend(prop_footprint_cells(at, yaw, footprint, height));
    }
}

/// A prop's yaw (15 degree steps) to the nearest quarter turn; a half turn rounds up.
fn quarter_turns(yaw: u8) -> i32 {
    (((yaw % PROP_YAWS) as i32 + 3) / 6) % 4
}

/// `(x, z)` turned by `turns` quarter turns: each maps (x, z) to (z, -x), the same turn as a
/// rotation of +90 degrees about +y.
fn turned(x: i32, z: i32, turns: i32) -> (i32, i32) {
    (0..turns).fold((x, z), |(x, z), _| (z, -x))
}

/// The cells one prop blocks: its footprint, turned by `yaw` to the nearest quarter turn, and
/// `height` up from the block the pivot `at` is in ([`prop_cell`]).
///
/// The footprint and height are in a prop's half-metre cells (`vox-art` draws props on that
/// grid; see [`PROP_CELL_BLOCKS`]). The footprint is relative to the half-metre cell the pivot
/// is in, so a prop covers the same ground at any block size; each of its cells covers
/// [`PROP_CELL_BLOCKS`] × [`PROP_CELL_BLOCKS`] blocks, and its height that many blocks for each
/// half-metre. Cells that would overflow are left out (positions come from the network).
pub fn prop_footprint_cells(at: [i32; 3], yaw: u8, footprint: &[[i16; 2]], height: u8) -> Vec<Cell> {
    prop_layers(at, yaw, footprint, i32::from(height) * PROP_CELL_BLOCKS)
}

/// The blocks a prop's footprint covers in the layer its pivot stands in: what it rests on is
/// the layer below these, what it hangs from the layer above its top.
pub fn prop_base_cells(at: [i32; 3], yaw: u8, footprint: &[[i16; 2]]) -> Vec<Cell> {
    prop_layers(at, yaw, footprint, 1)
}

/// The block just behind the pivot's block, turned with the prop: a wall prop's pivot is its
/// back, so this is the wall.
pub fn prop_behind_cell(at: [i32; 3], yaw: u8) -> Option<Cell> {
    let pivot = prop_cell(at);
    let (x, z) = turned(0, 1, quarter_turns(yaw));
    Some([pivot[0].checked_add(x)?, pivot[1], pivot[2].checked_add(z)?])
}

/// The footprint's blocks in `layers` layers up from the pivot's block.
fn prop_layers(at: [i32; 3], yaw: u8, footprint: &[[i16; 2]], layers: i32) -> Vec<Cell> {
    let floor = prop_cell(at)[1];
    // the half-metre cell the pivot is in, along x and z
    let (hx, hz) = (at[0].div_euclid(PROP_UNITS_PER_CELL), at[2].div_euclid(PROP_UNITS_PER_CELL));
    let turns = quarter_turns(yaw);
    let n = PROP_CELL_BLOCKS;
    let mut out = Vec::with_capacity(footprint.len() * (n * n * layers.max(0)) as usize);
    for &[dx, dz] in footprint {
        let (x, z) = turned(i32::from(dx), i32::from(dz), turns);
        // the cell's first block along x and z
        let corner = |h: i32, d: i32| h.checked_add(d)?.checked_mul(n);
        let (Some(bx), Some(bz)) = (corner(hx, x), corner(hz, z)) else { continue };
        for k in 0..layers {
            let Some(cy) = floor.checked_add(k) else { break };
            for i in 0..n {
                for j in 0..n {
                    let (Some(cx), Some(cz)) = (bx.checked_add(i), bz.checked_add(j)) else { continue };
                    out.push([cx, cy, cz]);
                }
            }
        }
    }
    out
}

/// `blocks` with the props' cells added: a test for what a body is stopped by.
pub fn with_props<'a>(blocks: impl Fn(Cell) -> bool + 'a, props: &'a Occupancy) -> impl Fn(Cell) -> bool + 'a {
    move |c| blocks(c) || props.contains(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Bounds, World};

    /// The blocks of the half-metre cell (`cx`, `cz`) for `height` half-metres up from layer 0.
    fn cell_blocks(cx: i32, cz: i32, height: i32) -> Vec<Cell> {
        let n = PROP_CELL_BLOCKS;
        let mut out = Vec::new();
        for y in 0..height * n {
            for x in cx * n..(cx + 1) * n {
                for z in cz * n..(cz + 1) * n {
                    out.push([x, y, z]);
                }
            }
        }
        out
    }

    #[test]
    fn prop_cells_turn_with_yaw() {
        let fp = [[0, 0], [1, 0]];
        let cells = |yaw| {
            let mut o = Occupancy::default();
            o.add_prop([0, 0, 0], yaw, &fp, 2);
            o
        };
        // each half-metre cell is PROP_CELL_BLOCKS blocks square and, per half-metre of height,
        // PROP_CELL_BLOCKS blocks tall
        let per_cell = (PROP_CELL_BLOCKS * PROP_CELL_BLOCKS * 2 * PROP_CELL_BLOCKS) as usize;
        let flat = cells(0);
        assert_eq!(flat.len(), 2 * per_cell);
        for c in cell_blocks(0, 0, 2).into_iter().chain(cell_blocks(1, 0, 2)) {
            assert!(flat.contains(c), "{c:?}");
        }
        // 6 steps of 15 degrees is a quarter turn: the cell at (1, 0) moves to (0, -1)
        let quarter = cells(6);
        assert_eq!(quarter.len(), 2 * per_cell);
        for c in cell_blocks(0, 0, 2).into_iter().chain(cell_blocks(0, -1, 2)) {
            assert!(quarter.contains(c), "{c:?}");
        }
        for c in cell_blocks(1, 0, 2) {
            assert!(!quarter.contains(c), "{c:?}");
        }
    }

    #[test]
    fn body_stops_against_a_prop_cell() {
        let mut props = Occupancy::default();
        // a prop whose footprint is the half-metre cell one over from the origin's: its blocks
        // start PROP_CELL_BLOCKS blocks along x
        props.add_prop([PROP_UNITS_PER_CELL, 0, 0], 0, &[[0, 0]], 2);
        let face = PROP_CELL_BLOCKS as f32;
        let none = |_: Cell| false;
        let solid = with_props(none, &props);
        let mut b = Body { pos: [0.5, 0.0, 0.5], half: PLAYER_HALF, height: PLAYER_HEIGHT };
        let c = b.travel(&solid, [3.0, 0.0, 0.0], 0.0);
        assert!(c.hit[0], "{c:?}");
        let stop = face - PLAYER_HALF;
        assert!(b.pos[0] > stop - 0.05 && b.pos[0] < stop + 0.01, "{:?}", b.pos);
    }

    fn world() -> World {
        let mut w = World::new(Bounds::default(), 4);
        for x in -10..10 {
            for z in -10..10 {
                w.set([x, -1, z], 1);
            }
        }
        // a one-high platform from x = 3, a three-high wall at z = 3
        for x in 3..10 {
            for z in -10..3 {
                w.set([x, 0, z], 1);
            }
        }
        for x in -10..10 {
            for y in 0..3 {
                w.set([x, y, 3], 1);
            }
        }
        w
    }

    fn player() -> Body {
        Body { pos: [0.5, 0.0, 0.5], half: 0.3, height: 3.6 }
    }

    #[test]
    fn falls_onto_the_floor() {
        let w = world();
        let mut b = player();
        b.pos[1] = 5.0;
        let c = b.travel(|p| w.solid(p), [0.0, -10.0, 0.0], 0.0);
        assert!(c.ground);
        assert!(b.pos[1].abs() < 0.01, "{:?}", b.pos);
        assert!(b.on_ground(|p| w.solid(p)));
    }

    #[test]
    fn walls_stop_and_slide() {
        let w = world();
        let mut b = player();
        let c = b.travel(|p| w.solid(p), [0.0, 0.0, 5.0], 1.0);
        assert!(c.hit[2]);
        assert!(b.pos[2] < 3.0 - 0.3 + 0.01 && b.pos[2] > 2.6, "{:?}", b.pos);
        // walking diagonally into the wall still moves along it
        let c = b.travel(|p| w.solid(p), [-1.0, 0.0, 1.0], 1.0);
        assert!(c.hit[2] && !c.hit[0]);
        assert!((b.pos[0] - -0.5).abs() < 1e-4);
    }

    #[test]
    fn steps_up_one_block() {
        let w = world();
        let mut b = player();
        let c = b.travel(|p| w.solid(p), [4.0, 0.0, 0.0], 1.05);
        assert!(c.stepped, "{c:?}");
        assert!(b.pos[0] > 4.0, "{:?}", b.pos);
        assert!((b.pos[1] - 1.0).abs() < 0.01, "{:?}", b.pos);
        // but not without step_up
        let mut b = player();
        b.travel(|p| w.solid(p), [4.0, 0.0, 0.0], 0.0);
        assert!(b.pos[0] < 3.0);
    }

    #[test]
    fn does_not_climb_walls() {
        let w = world();
        let mut b = player();
        b.travel(|p| w.solid(p), [0.0, 0.0, 5.0], 1.05);
        assert!(b.pos[1].abs() < 0.01);
        assert!(b.pos[2] < 3.0);
    }

    #[test]
    fn head_bumps_ceiling() {
        let mut w = world();
        w.set([0, 4, 0], 1);
        let mut b = player();
        let c = b.travel(|p| w.solid(p), [0.0, 2.0, 0.0], 0.0);
        assert!(c.hit[1] && !c.ground);
        assert!(b.pos[1] + 3.6 <= 4.0 && b.pos[1] > 0.3, "{:?}", b.pos);
    }
}
