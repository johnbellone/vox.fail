//! Which block a ray hits: a voxel walk (Amanatides & Woo) through the blocks.

use crate::world::{Cell, World};

/// A ray's hit: the block, and the face it came in through (as a unit step). The block
/// to place against it is `cell + normal`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub cell: Cell,
    pub normal: [i32; 3],
    /// how far along the ray, in blocks
    pub distance: f32,
}

/// The first solid block along `dir` from `origin` (both in blocks) within `max` blocks.
/// A ray starting inside a block hits it, with a zero normal.
pub fn raycast(world: &World, origin: [f32; 3], dir: [f32; 3], max: f32) -> Option<Hit> {
    raycast_with(|c| world.solid(c), origin, dir, max)
}

/// [`raycast`] against any test for "is this block solid".
pub fn raycast_with(solid: impl Fn(Cell) -> bool, origin: [f32; 3], dir: [f32; 3], max: f32) -> Option<Hit> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if !(len > 1e-9) || !origin.iter().all(|v| v.is_finite()) {
        return None;
    }
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];
    let mut cell = [origin[0].floor() as i32, origin[1].floor() as i32, origin[2].floor() as i32];
    let mut step = [0i32; 3];
    let mut t_max = [f32::INFINITY; 3];
    let mut t_delta = [f32::INFINITY; 3];
    for i in 0..3 {
        if d[i] > 0.0 {
            step[i] = 1;
            t_max[i] = (cell[i] as f32 + 1.0 - origin[i]) / d[i];
            t_delta[i] = 1.0 / d[i];
        } else if d[i] < 0.0 {
            step[i] = -1;
            t_max[i] = (origin[i] - cell[i] as f32) / -d[i];
            t_delta[i] = -1.0 / d[i];
        }
    }
    let mut normal = [0i32; 3];
    let mut t = 0.0;
    loop {
        if solid(cell) {
            return Some(Hit { cell, normal, distance: t });
        }
        let i = if t_max[0] < t_max[1] {
            if t_max[0] < t_max[2] { 0 } else { 2 }
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        t = t_max[i];
        if t > max {
            return None;
        }
        cell[i] += step[i];
        t_max[i] += t_delta[i];
        normal = [0; 3];
        normal[i] = -step[i];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Bounds;

    fn floor_world() -> World {
        let mut w = World::new(Bounds::default(), 4);
        for x in -10..10 {
            for z in -10..10 {
                w.set([x, -1, z], 1);
            }
        }
        w.set([3, 0, 0], 2);
        w
    }

    #[test]
    fn looks_down_at_the_floor() {
        let w = floor_world();
        let h = raycast(&w, [0.5, 3.6, 0.5], [0.0, -1.0, 0.0], 10.0).unwrap();
        assert_eq!(h.cell, [0, -1, 0]);
        assert_eq!(h.normal, [0, 1, 0]);
        assert!((h.distance - 3.6).abs() < 1e-4);
    }

    #[test]
    fn hits_the_side_of_a_block() {
        let w = floor_world();
        let h = raycast(&w, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0).unwrap();
        assert_eq!(h.cell, [3, 0, 0]);
        assert_eq!(h.normal, [-1, 0, 0]);
        assert!((h.distance - 2.5).abs() < 1e-4);
    }

    #[test]
    fn diagonal_and_negative_directions() {
        let w = floor_world();
        let h = raycast(&w, [-0.5, 2.5, -0.5], [-1.0, -1.0, -1.0], 20.0).unwrap();
        assert_eq!(h.cell[1], -1);
        assert_eq!(h.normal, [0, 1, 0]);
        // the cell before the hit is air: where a block would be placed
        let p = [h.cell[0] + h.normal[0], h.cell[1] + h.normal[1], h.cell[2] + h.normal[2]];
        assert!(!w.solid(p));
    }

    #[test]
    fn misses_beyond_reach_and_into_the_sky() {
        let w = floor_world();
        assert!(raycast(&w, [0.5, 3.0, 0.5], [0.0, -1.0, 0.0], 2.0).is_none());
        assert!(raycast(&w, [0.5, 3.0, 0.5], [0.0, 1.0, 0.0], 50.0).is_none());
        assert!(raycast(&w, [0.5, 3.0, 0.5], [0.0, 0.0, 0.0], 50.0).is_none());
        assert!(raycast(&w, [f32::NAN, 3.0, 0.5], [0.0, -1.0, 0.0], 50.0).is_none());
    }

    #[test]
    fn starting_inside_a_block() {
        let w = floor_world();
        let h = raycast(&w, [3.5, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0).unwrap();
        assert_eq!(h.cell, [3, 0, 0]);
        assert_eq!(h.normal, [0, 0, 0]);
    }
}
