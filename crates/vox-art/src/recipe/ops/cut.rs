//! Cut: clears an inclusive box to empty, for doors and openings. It runs in recipe order, so it
//! only removes what the operations before it built.

use serde::{Deserialize, Serialize};

use super::{box_volume, corner_problems};
use crate::palette::Palette;
use crate::recipe::eval::Env;
use crate::rng::Rng;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cut {
    /// One corner of the box to clear, inclusive.
    pub from: [u16; 3],
    /// The opposite corner, inclusive. Each axis must run from `from` upward.
    pub to: [u16; 3],
}

impl Cut {
    pub(crate) fn writes(&self) -> Option<u64> {
        box_volume(self.from, self.to)
    }

    pub(crate) fn check(&self, size: [u16; 3], _palette: &Palette) -> Vec<String> {
        corner_problems("cut", self.from, self.to, size)
    }

    pub(crate) fn apply(&self, env: &mut Env<'_>, _rng: &mut Rng) -> Result<(), String> {
        let [x0, y0, z0] = self.from;
        let [x1, y1, z1] = self.to;
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    env.grid.set([x, y, z], 0).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
}
