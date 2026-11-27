//! Trim: paints the edges of a box in another material: its four vertical corner columns, the
//! rim around its top, or both. A plinth, a parapet or a window surround is one operation over
//! the box it finishes.
//!
//! Trim runs over what the earlier operations built, so it paints a hollow box's corners as well
//! as a solid one's. The box is `of` = `[x0, y0, z0, x1, y1, z1]`, both corners inclusive.

use serde::{Deserialize, Serialize};

use super::{corner_problems, material_index, material_problem};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

/// Which edges of the box to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edges {
    /// The four vertical edges, from floor to top.
    Corners,
    /// The rim around the top face.
    Top,
    /// Both the corners and the top rim.
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trim {
    /// The box to trim: `[x0, y0, z0, x1, y1, z1]`, both corners inclusive.
    pub of: [u16; 6],
    pub edges: Edges,
    pub mat: String,
    pub tone: u8,
}

impl Trim {
    /// An upper bound: all twelve edges of the box, each as long as its span.
    pub(crate) fn writes(&self) -> Option<u64> {
        let [x0, y0, z0, x1, y1, z1] = self.of;
        let span = |a: u16, b: u16| {
            if a > b {
                0
            } else {
                u64::from(b) - u64::from(a) + 1
            }
        };
        let per_group = span(x0, x1)
            .checked_add(span(z0, z1))?
            .checked_add(span(y0, y1))?;
        per_group.checked_mul(4)
    }

    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let [x0, y0, z0, x1, y1, z1] = self.of;
        let mut problems = corner_problems("trim", [x0, y0, z0], [x1, y1, z1], size);
        if let Some(problem) = material_problem(&self.mat, self.tone, palette) {
            problems.push(problem);
        }
        problems
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = material_index(env.palette, &self.mat, self.tone)?;
        let [x0, y0, z0, x1, y1, z1] = self.of;
        if matches!(self.edges, Edges::Corners | Edges::All) {
            for y in y0..=y1 {
                for x in [x0, x1] {
                    for z in [z0, z1] {
                        paint(env, [x, y, z], index)?;
                    }
                }
            }
        }
        if matches!(self.edges, Edges::Top | Edges::All) {
            // The corners already painted above are painted again, which is harmless.
            for x in x0..=x1 {
                paint(env, [x, y1, z0], index)?;
                paint(env, [x, y1, z1], index)?;
            }
            for z in z0..=z1 {
                paint(env, [x0, y1, z], index)?;
                paint(env, [x1, y1, z], index)?;
            }
        }
        Ok(())
    }
}

fn paint(env: &mut Env<'_>, pos: [u16; 3], index: u8) -> Result<(), String> {
    env.grid.set(pos, index).map_err(|e| e.to_string())
}
