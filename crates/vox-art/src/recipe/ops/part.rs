//! Part: a filled box, recorded under a name so later tools can refer to it (a lid, a seat, a
//! door). The voxels it sets are listed under its name in the built result; a later part with
//! the same name replaces them.

use serde::{Deserialize, Serialize};

use super::{material_index, material_problem, name_problem};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub name: String,
    /// One corner of the box, inclusive.
    pub from: [u16; 3],
    /// The opposite corner, inclusive. Each axis must run from `from` upward.
    pub to: [u16; 3],
    pub mat: String,
    pub tone: u8,
}

impl Part {
    /// The box's volume, or 0 for a box that runs backwards (`check` refuses that anyway).
    pub(crate) fn writes(&self) -> Option<u64> {
        let mut volume: u64 = 1;
        for axis in 0..3 {
            if self.from[axis] > self.to[axis] {
                return Some(0);
            }
            let span = u64::from(self.to[axis] - self.from[axis]) + 1;
            volume = volume.checked_mul(span)?;
        }
        Some(volume)
    }

    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(problem) = name_problem(&self.name) {
            problems.push(problem);
        }
        if let Some(problem) = material_problem(&self.mat, self.tone, palette) {
            problems.push(problem);
        }
        for axis in 0..3 {
            if self.from[axis] > self.to[axis] {
                problems.push(format!(
                    "part {:?} has from {:?} past to {:?}",
                    self.name, self.from, self.to
                ));
                break;
            }
        }
        for axis in 0..3 {
            if self.to[axis] >= size[axis] {
                problems.push(format!(
                    "part {:?} reaches {:?}, outside a grid of size {size:?}",
                    self.name, self.to
                ));
                break;
            }
        }
        problems
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = material_index(env.palette, &self.mat, self.tone)?;
        let mut placed = Vec::new();
        for x in self.from[0]..=self.to[0] {
            for y in self.from[1]..=self.to[1] {
                for z in self.from[2]..=self.to[2] {
                    env.grid.set([x, y, z], index).map_err(|e| e.to_string())?;
                    placed.push([x, y, z]);
                }
            }
        }
        // A later part with the same name replaces this one's voxels, as later voxels overwrite
        // earlier ones, so each name holds at most one box.
        env.parts.insert(self.name.clone(), placed);
        Ok(())
    }
}
