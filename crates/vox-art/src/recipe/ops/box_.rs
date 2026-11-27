//! Box: a solid or hollow box of one material, the walls, floors and blocks of a building.
//!
//! The corners are inclusive. A hollow box keeps only its outer shell: a voxel is kept when it
//! lies on any face of the box, so the inside stays empty and a later operation can cut into it.

use serde::{Deserialize, Serialize};

use super::{box_volume, corner_problems, material_index, material_problem};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Box {
    /// One corner of the box, inclusive.
    pub from: [u16; 3],
    /// The opposite corner, inclusive. Each axis must run from `from` upward.
    pub to: [u16; 3],
    pub mat: String,
    pub tone: u8,
    /// Keep only the outer shell.
    #[serde(default)]
    pub hollow: bool,
}

impl Box {
    /// The box's volume: an upper bound when hollow, which writes less than this.
    pub(crate) fn writes(&self) -> Option<u64> {
        box_volume(self.from, self.to)
    }

    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let mut problems = corner_problems("box", self.from, self.to, size);
        if let Some(problem) = material_problem(&self.mat, self.tone, palette) {
            problems.push(problem);
        }
        problems
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = material_index(env.palette, &self.mat, self.tone)?;
        let [x0, y0, z0] = self.from;
        let [x1, y1, z1] = self.to;
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let on_shell = x == x0 || x == x1 || y == y0 || y == y1 || z == z0 || z == z1;
                    if self.hollow && !on_shell {
                        continue;
                    }
                    env.grid.set([x, y, z], index).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
}
