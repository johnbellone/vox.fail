//! Lathe: a profile turned about a vertical axis, for things that are round in plan (barrels,
//! vases, lamps, mugs, bells).
//!
//! The axis runs through a grid corner, `axis_at`, so a layer of radius `r` is `2r` voxels across
//! and symmetric. Layer `i` is at height `from_y + i`. A voxel is in a layer when its centre lies
//! within `r` of the axis; a radius of 0 is an empty layer, so a profile can start and stop
//! anywhere.

use serde::{Deserialize, Serialize};

use super::{material_index, material_problem};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

/// Most layers in one profile.
const MAX_PROFILE: usize = 64;
/// Largest radius of one layer, in voxels.
const MAX_RADIUS: u16 = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lathe {
    /// The `[x, z]` grid corner the axis passes through.
    pub axis_at: [u16; 2],
    /// The height of the bottom layer.
    pub from_y: u16,
    /// The radius of each layer, bottom up.
    pub profile: Vec<u16>,
    pub mat: String,
    pub tone: u8,
}

impl Lathe {
    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        let mut problems = Vec::new();
        if self.profile.is_empty() || self.profile.len() > MAX_PROFILE {
            problems.push(format!(
                "lathe profile needs 1 to {MAX_PROFILE} layers, got {}",
                self.profile.len()
            ));
        }
        if let Some(problem) = material_problem(&self.mat, self.tone, palette) {
            problems.push(problem);
        }
        let [sx, sy, _] = size;
        let [ax, az] = self.axis_at;
        if ax > size[0] || az > size[2] {
            problems.push(format!("lathe axis {:?} is outside the grid", self.axis_at));
        }
        match usize::from(self.from_y).checked_add(self.profile.len()) {
            Some(top) if top <= usize::from(sy) => {}
            _ => problems.push(format!(
                "lathe from_y {} with {} layers goes above the grid (height {sy})",
                self.from_y,
                self.profile.len()
            )),
        }
        for (layer, &radius) in self.profile.iter().enumerate() {
            if radius > MAX_RADIUS {
                problems.push(format!(
                    "lathe radius {radius} at layer {layer} is over {MAX_RADIUS}"
                ));
                continue;
            }
            if radius == 0 {
                continue;
            }
            // The layer reaches `radius` voxels either side of the axis; it must stay in the grid.
            let reach = u32::from(radius);
            if u32::from(ax) < reach || u32::from(ax) + reach > u32::from(sx) {
                problems.push(format!(
                    "lathe layer {layer} of radius {radius} runs outside the grid in x"
                ));
            }
            if u32::from(az) < reach || u32::from(az) + reach > u32::from(size[2]) {
                problems.push(format!(
                    "lathe layer {layer} of radius {radius} runs outside the grid in z"
                ));
            }
        }
        problems
    }

    /// Upper bound on the writes: each layer's square of `2r` voxels, which holds its disc.
    pub(crate) fn writes(&self) -> Option<u64> {
        self.profile.iter().try_fold(0u64, |total, &radius| {
            let side = u64::from(radius).checked_mul(2)?;
            total.checked_add(side.checked_mul(side)?)
        })
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let index = material_index(env.palette, &self.mat, self.tone)?;
        let ax = i64::from(self.axis_at[0]);
        let az = i64::from(self.axis_at[1]);
        for (layer, &radius) in self.profile.iter().enumerate() {
            if radius == 0 {
                continue;
            }
            let r = i64::from(radius);
            let y = u16::try_from(usize::from(self.from_y) + layer).map_err(|e| e.to_string())?;
            // Offsets run from -r to r-1: the voxel at ax + dx has its centre at dx + 0.5 from the
            // axis. Doubling both sides keeps the test in whole numbers: (2dx+1)^2 + (2dz+1)^2
            // must not exceed (2r)^2.
            for dx in -r..r {
                for dz in -r..r {
                    let (ox, oz) = (2 * dx + 1, 2 * dz + 1);
                    if ox * ox + oz * oz > 4 * r * r {
                        continue;
                    }
                    let x = u16::try_from(ax + dx).map_err(|e| e.to_string())?;
                    let z = u16::try_from(az + dz).map_err(|e| e.to_string())?;
                    env.grid.set([x, y, z], index).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
}
