//! Legs: square posts standing on the ground, for tables, stools, chairs and beds.
//!
//! Each post is `thick` voxels square, its low corner at `at`, rising `height` voxels from
//! `from_y`.

use serde::{Deserialize, Serialize};

use super::material_index;
use super::material_problem;
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

/// Most posts in one operation.
const MAX_LEGS: usize = 8;
/// Thickest post, in voxels.
const MAX_THICK: u16 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Legs {
    /// The `[x, z]` low corner of each post.
    pub at: Vec<[u16; 2]>,
    pub from_y: u16,
    pub height: u16,
    /// Side of each post in voxels, 1 to 4.
    pub thick: u16,
    pub mat: String,
    pub tone: u8,
}

impl Legs {
    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let mut problems = Vec::new();
        if self.at.is_empty() {
            problems.push("legs needs at least one post".to_string());
        }
        if self.at.len() > MAX_LEGS {
            problems.push(format!(
                "legs: at most {MAX_LEGS} posts, got {}",
                self.at.len()
            ));
        }
        if self.thick == 0 || self.thick > MAX_THICK {
            problems.push(format!(
                "legs thick must be 1 to {MAX_THICK}, got {}",
                self.thick
            ));
        }
        if self.height == 0 {
            problems.push("legs height must be at least 1".to_string());
        }
        match usize::from(self.from_y).checked_add(usize::from(self.height)) {
            Some(top) if top <= usize::from(size[1]) => {}
            _ => problems.push(format!(
                "legs from_y {} with height {} goes above the grid (height {})",
                self.from_y, self.height, size[1]
            )),
        }
        let thick = u32::from(self.thick);
        for [x, z] in &self.at {
            if u32::from(*x) + thick > u32::from(size[0])
                || u32::from(*z) + thick > u32::from(size[2])
            {
                problems.push(format!("leg at {:?} runs outside the grid", [x, z]));
            }
        }
        if let Some(problem) = material_problem(&self.mat, self.tone, palette) {
            problems.push(problem);
        }
        problems
    }

    /// Upper bound on the writes: each post's `thick` by `thick` column over `height` layers.
    pub(crate) fn writes(&self) -> Option<u64> {
        let posts = u64::try_from(self.at.len()).ok()?;
        let thick = u64::from(self.thick);
        posts
            .checked_mul(thick.checked_mul(thick)?)?
            .checked_mul(u64::from(self.height))
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = material_index(env.palette, &self.mat, self.tone)?;
        let top = u32::from(self.from_y) + u32::from(self.height);
        for [x, z] in &self.at {
            for y in u32::from(self.from_y)..top {
                for dx in 0..self.thick {
                    for dz in 0..self.thick {
                        let pos = [
                            x.checked_add(dx).ok_or("leg x overflows")?,
                            u16::try_from(y).map_err(|e| e.to_string())?,
                            z.checked_add(dz).ok_or("leg z overflows")?,
                        ];
                        env.grid.set(pos, index).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        Ok(())
    }
}
