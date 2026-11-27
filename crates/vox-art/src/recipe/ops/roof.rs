//! Roof: layers stacked over a rectangle, so a flat slab, a pitched gable or a shallower stepped
//! roof comes out of a few numbers.
//!
//! Layer `k` sits at `at_y + k`. A gable layer is the rectangle shrunk by `k` on each side across
//! the ridge, so the roof rises one voxel per voxel in (a 45 degree pitch); a stepped layer is
//! shrunk by `2k`, rising one voxel per two in. The layers stop once the shrunk rectangle is
//! empty. The ridge runs along x when `along_x` is set, otherwise along z.

use serde::{Deserialize, Serialize};

use super::{material_index, material_problem};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoofStyle {
    /// One layer at `at_y`.
    Flat,
    /// One voxel up per voxel in, until the rectangle closes.
    Gable,
    /// Two voxels in per voxel up: a shallower pitch.
    Stepped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roof {
    /// The rectangle the roof covers, `[x0, z0, x1, z1]`, inclusive.
    pub over: [u16; 4],
    /// The height of the bottom layer.
    pub at_y: u16,
    pub style: RoofStyle,
    /// Whether the ridge runs along x (the sides across the ridge are the z sides).
    #[serde(default)]
    pub along_x: bool,
    pub mat: String,
    pub tone: u8,
}

impl Roof {
    /// The rectangle `[x0, z0, x1, z1]` of layer `k`, or `None` once it is empty. The ridge
    /// direction and style decide how far each layer is shrunk in.
    fn layer(&self, k: u32) -> Option<[u16; 4]> {
        let shrink = match self.style {
            RoofStyle::Flat if k > 0 => return None,
            RoofStyle::Flat => 0,
            RoofStyle::Gable => k,
            RoofStyle::Stepped => k.checked_mul(2)?,
        };
        let [x0, z0, x1, z1] = self.over.map(i64::from);
        let s = i64::from(shrink);
        let (lo, hi) = if self.along_x {
            (z0 + s, z1 - s)
        } else {
            (x0 + s, x1 - s)
        };
        if lo > hi || x0 > x1 || z0 > z1 {
            return None;
        }
        let [a, b, c, d] = if self.along_x {
            [x0, lo, x1, hi]
        } else {
            [lo, z0, hi, z1]
        };
        // Shrinking only narrows `over`, which fits in u16, so these never fail.
        Some([
            u16::try_from(a).ok()?,
            u16::try_from(b).ok()?,
            u16::try_from(c).ok()?,
            u16::try_from(d).ok()?,
        ])
    }

    /// Number of layers, counting from 0; a flat roof is one, an invalid rectangle is none.
    fn layer_count(&self) -> u32 {
        let mut count = 0;
        while self.layer(count).is_some() {
            count += 1;
        }
        count
    }

    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(problem) = material_problem(&self.mat, self.tone, palette) {
            problems.push(problem);
        }
        let [x0, z0, x1, z1] = self.over;
        if x0 > x1 || z0 > z1 {
            problems.push(format!("roof over {:?} runs backwards", self.over));
        }
        if x1 >= size[0] || z1 >= size[2] {
            problems.push(format!(
                "roof over {:?} reaches outside a grid of size {size:?}",
                self.over
            ));
        }
        let layers = self.layer_count();
        if layers > 0 {
            // The top layer is `layers - 1` above `at_y`; it must stay inside the grid.
            let top = u32::from(self.at_y) + layers - 1;
            if top >= u32::from(size[1]) {
                problems.push(format!(
                    "roof at_y {} with {layers} layers reaches height {top}, above the grid (height {})",
                    self.at_y, size[1]
                ));
            }
        }
        problems
    }

    /// Upper bound on the writes: the sum of the layers' areas.
    pub(crate) fn writes(&self) -> Option<u64> {
        let mut total: u64 = 0;
        let mut k = 0;
        while let Some([x0, z0, x1, z1]) = self.layer(k) {
            let width = u64::from(x1) - u64::from(x0) + 1;
            let depth = u64::from(z1) - u64::from(z0) + 1;
            total = total.checked_add(width.checked_mul(depth)?)?;
            k += 1;
        }
        Some(total)
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = material_index(env.palette, &self.mat, self.tone)?;
        let mut k = 0;
        while let Some([x0, z0, x1, z1]) = self.layer(k) {
            let y = u16::try_from(u32::from(self.at_y) + k).map_err(|e| e.to_string())?;
            for x in x0..=x1 {
                for z in z0..=z1 {
                    env.grid.set([x, y, z], index).map_err(|e| e.to_string())?;
                }
            }
            k += 1;
        }
        Ok(())
    }
}
