//! Openings: a row of windows or doors cut into one outer wall of a box, so a house front gets
//! its windows from a few numbers rather than a cut per window.
//!
//! The face is the plane of the box's outer wall that `face` names: `+x` is the wall at the box's
//! largest x, and so on. Along the face the first opening starts `every / 2` voxels in from the
//! face's edge, and one more starts every `every` voxels after it. Each is `size[0]` wide along the
//! face and `size[1]` high, with its bottom `sill` voxels above the box's floor. An opening that
//! does not fit wholly inside the face is left out, so the result is the same on every target.
//! Its voxels take `mat` (glass, say) when one is given, and are emptied when not. Only the face's
//! outer layer is replaced, so a hole is one voxel deep; thicker walls need a `cut`.

use serde::{Deserialize, Serialize};

use super::{corner_problems, material_index, material_problem};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

/// The outer wall an opening row is cut into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Face {
    #[serde(rename = "+x")]
    PosX,
    #[serde(rename = "-x")]
    NegX,
    #[serde(rename = "+z")]
    PosZ,
    #[serde(rename = "-z")]
    NegZ,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Openings {
    pub face: Face,
    /// The box the face belongs to: `[x0, y0, z0, x1, y1, z1]`, both corners inclusive.
    pub of: [u16; 6],
    /// Voxels from one opening to the next along the face. At least 2.
    pub every: u16,
    /// `[width, height]` of each opening: width along the face, height up.
    pub size: [u16; 2],
    /// Height of each opening's bottom above the box's floor.
    pub sill: u16,
    /// The material of the opening's voxels; emptied when absent.
    #[serde(default)]
    pub mat: Option<String>,
    #[serde(default)]
    pub tone: u8,
}

impl Openings {
    /// The inclusive box of the face: one plane of the box's outer wall.
    fn face_box(&self) -> ([u16; 3], [u16; 3]) {
        let [x0, y0, z0, x1, y1, z1] = self.of;
        match self.face {
            Face::PosX => ([x1, y0, z0], [x1, y1, z1]),
            Face::NegX => ([x0, y0, z0], [x0, y1, z1]),
            Face::PosZ => ([x0, y0, z1], [x1, y1, z1]),
            Face::NegZ => ([x0, y0, z0], [x1, y1, z0]),
        }
    }

    /// The voxels this row writes: the openings that fit along the face, times each one's
    /// `width * height`. Openings may overlap, and every overlapping write counts, so this is an
    /// upper bound on the writes `apply` makes (equal to them when the openings fit under the
    /// box's top), not the face's area. `None` when the count overflows `u64`, as the other
    /// operations report it.
    pub(crate) fn writes(&self) -> Option<u64> {
        let (from, to) = self.face_box();
        // The same axis `apply` runs the openings along.
        let across = match self.face {
            Face::PosX | Face::NegX => 2,
            Face::PosZ | Face::NegZ => 0,
        };
        let start = u64::from(from[across]);
        let end = u64::from(to[across]);
        let [width, height] = self.size.map(u64::from);
        // `every` is at least 1 here: a zero step is refused by `check`, and must not divide by zero.
        let every = u64::from(self.every).max(1);
        let first = start + u64::from(self.every) / 2;
        // An opening fits while its last voxel is still on the face: its start is at most
        // `end + 1 - width`. A face narrower than one opening has none.
        let Some(last_start) = (end + 1).checked_sub(width).filter(|&s| s >= first) else {
            return Some(0);
        };
        let openings = (last_start - first) / every + 1;
        openings.checked_mul(width)?.checked_mul(height)
    }

    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let [x0, y0, z0, x1, y1, z1] = self.of;
        let mut problems = corner_problems("openings", [x0, y0, z0], [x1, y1, z1], size);
        if self.every < 2 {
            problems.push(format!(
                "openings every must be at least 2, got {}",
                self.every
            ));
        }
        if self.size.contains(&0) {
            problems.push(format!(
                "openings size must be at least 1 by 1, got {:?}",
                self.size
            ));
        }
        // The top voxel of the highest opening is `sill + size[1] - 1` above the floor; past the
        // box's top none of the openings could fit, so that is a problem, not a silent no-op.
        let top =
            (u32::from(y0) + u32::from(self.sill) + u32::from(self.size[1])).saturating_sub(1);
        if top > u32::from(y1) {
            problems.push(format!(
                "openings sill {} and height {} reach up to {top}, past the box's top at {y1}",
                self.sill, self.size[1]
            ));
        }
        if let Some(mat) = &self.mat
            && let Some(problem) = material_problem(mat, self.tone, palette)
        {
            problems.push(problem);
        }
        problems
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = match &self.mat {
            Some(mat) => material_index(env.palette, mat, self.tone)?,
            None => 0,
        };
        let (from, to) = self.face_box();
        // `across` runs along the face, `normal` is the axis the face is perpendicular to, and
        // the height is always y (axis 1).
        let (across, normal) = match self.face {
            Face::PosX | Face::NegX => (2, 0),
            Face::PosZ | Face::NegZ => (0, 2),
        };
        let plane = from[normal];
        let start = u32::from(from[across]);
        let end = u32::from(to[across]);
        let floor = u32::from(from[1]);
        let ceiling = u32::from(to[1]);
        let [width, height] = self.size.map(u32::from);
        let bottom = floor + u32::from(self.sill);
        let top = bottom + height.saturating_sub(1);
        if top > ceiling {
            // Every opening in this face would stick out of the top, so none is cut.
            return Ok(());
        }
        let mut at = start + u32::from(self.every) / 2;
        while at + width.saturating_sub(1) <= end {
            for u in at..at + width {
                for v in bottom..=top {
                    let mut pos = [0u16; 3];
                    pos[normal] = plane;
                    pos[across] = u16::try_from(u).map_err(|e| e.to_string())?;
                    pos[1] = u16::try_from(v).map_err(|e| e.to_string())?;
                    env.grid.set(pos, index).map_err(|e| e.to_string())?;
                }
            }
            at += u32::from(self.every);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(face: Face, of: [u16; 6], every: u16, size: [u16; 2]) -> Openings {
        Openings {
            face,
            of,
            every,
            size,
            sill: 0,
            mat: None,
            tone: 0,
        }
    }

    #[test]
    fn overlapping_openings_count_every_write() {
        // A 64-long face with 32-wide openings every 2: the starts are 1, 3, ..., 31, which is
        // sixteen openings of 32 by 64 each. The face's own area (64 by 64) is not the count.
        let op = row(Face::PosZ, [0, 0, 0, 63, 63, 63], 2, [32, 64]);
        assert_eq!(op.writes(), Some(16 * 32 * 64));
    }

    #[test]
    fn an_opening_wider_than_the_face_writes_nothing() {
        let op = row(Face::NegX, [0, 0, 0, 0, 9, 9], 2, [32, 4]);
        assert_eq!(op.writes(), Some(0));
    }

    #[test]
    fn a_zero_step_does_not_divide_by_zero() {
        // `check` refuses a step under 2; `writes` must still return for such a recipe.
        let op = row(Face::PosX, [0, 0, 0, 63, 63, 63], 0, [4, 4]);
        assert!(op.writes().is_some());
    }
}
