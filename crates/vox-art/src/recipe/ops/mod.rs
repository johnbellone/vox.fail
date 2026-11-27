//! The recipe operations. Each one checks its own fields against the grid size and the palette,
//! then writes voxels into the grid being built.
//!
//! New variants go at the end of [`Op`]: the order is part of what a recipe means.

pub mod box_;
pub mod cut;
pub mod lathe;
pub mod legs;
pub mod openings;
pub mod part;
pub mod roof;
pub mod trim;

use serde::{Deserialize, Serialize};

use super::eval::Env;
use crate::palette::Palette;
use crate::rng::Rng;

/// Longest material or part name, in characters (the same cap the palette uses).
const MAX_NAME_CHARS: usize = 32;

/// One operation in a recipe. Externally tagged in JSON, e.g. `{"lathe": {...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Lathe(lathe::Lathe),
    Legs(legs::Legs),
    Part(part::Part),
    Box(box_::Box),
    Roof(roof::Roof),
    Cut(cut::Cut),
    Openings(openings::Openings),
    Trim(trim::Trim),
}

impl Op {
    /// Every problem with this operation for a grid of `size` and `palette`; empty when it is fine.
    pub(crate) fn check(&self, size: [u16; 3], palette: &Palette) -> Vec<String> {
        match self {
            Op::Lathe(op) => op.check(size, palette),
            Op::Legs(op) => op.check(size, palette),
            Op::Part(op) => op.check(size, palette),
            Op::Box(op) => op.check(size, palette),
            Op::Roof(op) => op.check(size, palette),
            Op::Cut(op) => op.check(size, palette),
            Op::Openings(op) => op.check(size, palette),
            Op::Trim(op) => op.check(size, palette),
        }
    }

    /// Most voxel writes this operation makes, or `None` if the count overflows `u64`. Summed
    /// before building to enforce the recipe's write budget.
    pub(crate) fn writes(&self) -> Option<u64> {
        match self {
            Op::Lathe(op) => op.writes(),
            Op::Legs(op) => op.writes(),
            Op::Part(op) => op.writes(),
            Op::Box(op) => op.writes(),
            Op::Roof(op) => op.writes(),
            Op::Cut(op) => op.writes(),
            Op::Openings(op) => op.writes(),
            Op::Trim(op) => op.writes(),
        }
    }

    /// Writes this operation into `env`. Only called on an operation that passed [`Op::check`].
    pub(crate) fn apply(&self, env: &mut Env<'_>, rng: &mut Rng) -> Result<(), String> {
        match self {
            Op::Lathe(op) => op.apply(env, rng),
            Op::Legs(op) => op.apply(env, rng),
            Op::Part(op) => op.apply(env, rng),
            Op::Box(op) => op.apply(env, rng),
            Op::Roof(op) => op.apply(env, rng),
            Op::Cut(op) => op.apply(env, rng),
            Op::Openings(op) => op.apply(env, rng),
            Op::Trim(op) => op.apply(env, rng),
        }
    }
}

/// A message if `name` is not usable as a material or part name: 1 to 32 characters, and no
/// control characters (names end up in messages and in JSON that people read).
pub(crate) fn name_problem(name: &str) -> Option<String> {
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        return Some(format!(
            "name {name:?} must be 1 to {MAX_NAME_CHARS} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Some(format!("name {name:?} contains a control character"));
    }
    None
}

/// The palette index of tone `tone` of material `mat`, or a message saying why there is none.
pub(crate) fn material_index(palette: &Palette, mat: &str, tone: u8) -> Result<u8, String> {
    if !palette.materials.iter().any(|m| m.name == mat) {
        return Err(format!("material {mat:?} is not in the palette"));
    }
    palette
        .index_of(mat, usize::from(tone))
        .ok_or_else(|| format!("material {mat:?} has no tone {tone}"))
}

/// A message if `mat` and `tone` do not name a palette colour, or `None` if they do.
pub(crate) fn material_problem(mat: &str, tone: u8, palette: &Palette) -> Option<String> {
    if let Some(problem) = name_problem(mat) {
        return Some(problem);
    }
    material_index(palette, mat, tone).err()
}

/// Problems with an inclusive box from `from` to `to` in a grid of `size`: each axis of `from`
/// must not pass `to`, and `to` must lie inside the grid. `what` names the operation in messages.
pub(crate) fn corner_problems(
    what: &str,
    from: [u16; 3],
    to: [u16; 3],
    size: [u16; 3],
) -> Vec<String> {
    let mut problems = Vec::new();
    if (0..3).any(|axis| from[axis] > to[axis]) {
        problems.push(format!("{what} has from {from:?} past to {to:?}"));
    }
    if (0..3).any(|axis| to[axis] >= size[axis]) {
        problems.push(format!(
            "{what} reaches {to:?}, outside a grid of size {size:?}"
        ));
    }
    problems
}

/// The number of voxels in the inclusive box from `from` to `to`, or `None` if it overflows
/// `u64`. A box that runs backwards has no voxels.
pub(crate) fn box_volume(from: [u16; 3], to: [u16; 3]) -> Option<u64> {
    let mut volume: u64 = 1;
    for axis in 0..3 {
        if from[axis] > to[axis] {
            return Some(0);
        }
        let span = u64::from(to[axis] - from[axis]) + 1;
        volume = volume.checked_mul(span)?;
    }
    Some(volume)
}
