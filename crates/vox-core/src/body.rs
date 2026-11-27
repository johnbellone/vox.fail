//! Moving a box (a player) through the blocks: it can't pass through solid blocks, and when
//! walking it steps up onto a block one high. Props add their own cells beside the blocks
//! ([`Occupancy`], [`with_props`]); the caller passes the combined test to [`Body::travel`].

use std::collections::BTreeSet;

use crate::entity::{PROP_YAWS, prop_cell};
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

/// The cells one prop blocks: its footprint (0.5 m cells, relative to the pivot's floor cell
/// [`prop_cell`] of `at`), turned by `yaw` to the nearest quarter turn, and `height` blocks up
/// from the floor. A quarter turn maps the cell (x, z) to (z, -x), the same turn as a rotation
/// of +90 degrees about +y. Cells that would overflow are left out (positions come from the
/// network).
pub fn prop_footprint_cells(at: [i32; 3], yaw: u8, footprint: &[[i16; 2]], height: u8) -> Vec<Cell> {
    let floor = prop_cell(at);
    // round to the nearest quarter turn (6 steps of 15 degrees); a half turn rounds up
    let turns = (((yaw % PROP_YAWS) as i32 + 3) / 6) % 4;
    let mut out = Vec::with_capacity(footprint.len() * height as usize);
    for &[dx, dz] in footprint {
        let (mut x, mut z) = (i32::from(dx), i32::from(dz));
        for _ in 0..turns {
            (x, z) = (z, -x);
        }
        let (Some(cx), Some(cz)) = (floor[0].checked_add(x), floor[2].checked_add(z)) else { continue };
        for k in 0..i32::from(height) {
            let Some(cy) = floor[1].checked_add(k) else { break };
            out.push([cx, cy, cz]);
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

    #[test]
    fn prop_cells_turn_with_yaw() {
        let fp = [[0, 0], [1, 0]];
        let cells = |yaw| {
            let mut o = Occupancy::default();
            o.add_prop([0, 0, 0], yaw, &fp, 2);
            o
        };
        let flat = cells(0);
        assert_eq!(flat.len(), 4);
        for c in [[0, 0, 0], [0, 1, 0], [1, 0, 0], [1, 1, 0]] {
            assert!(flat.contains(c), "{c:?}");
        }
        // 6 steps of 15 degrees is a quarter turn: the cell at (1, 0) moves to (0, -1)
        let quarter = cells(6);
        assert_eq!(quarter.len(), 4);
        for c in [[0, 0, 0], [0, 1, 0], [0, 0, -1], [0, 1, -1]] {
            assert!(quarter.contains(c), "{c:?}");
        }
        assert!(!quarter.contains([1, 0, 0]));
    }

    #[test]
    fn body_stops_against_a_prop_cell() {
        let mut props = Occupancy::default();
        // a prop whose footprint is the cell at [1, 0, 0]: at 5 cm units, x = 10 is block 1
        props.add_prop([10, 0, 0], 0, &[[0, 0]], 2);
        let none = |_: Cell| false;
        let solid = with_props(none, &props);
        let mut b = Body { pos: [0.5, 0.0, 0.5], half: 0.3, height: 3.6 };
        let c = b.travel(&solid, [3.0, 0.0, 0.0], 0.0);
        assert!(c.hit[0], "{c:?}");
        assert!(b.pos[0] > 0.65 && b.pos[0] < 0.7, "{:?}", b.pos);
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
