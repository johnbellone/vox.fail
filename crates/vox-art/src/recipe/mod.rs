//! Recipes: a short, closed list of operations that builds a voxel grid in the project's style,
//! from JSON that a person or a language model writes.
//!
//! A recipe names its scale, a seed, a size and an ordered list of operations. The types here
//! are the definition; [`eval`] checks a recipe against a palette and then builds it. Later
//! operations overwrite earlier ones. The operations are closed: an unknown one is a parse
//! error, so a recipe can only ask for what this crate knows how to build.
//!
//! The operations come at two scales: prop-scale (lathe, legs, part) and world-scale (box, roof,
//! cut, openings, trim). New variants are appended after `Trim`, never reordered: the JSON names
//! them, and the order is kept for stability.

pub mod eval;
pub mod ops;

use serde::{Deserialize, Serialize};

use crate::scale::Scale;

pub use eval::{Built, RecipeError, eval};
pub use ops::Op;

/// The only recipe version this crate reads.
pub const RECIPE_VERSION: u16 = 1;

/// Most operations in one recipe. Recipes come from outside, so the count is capped.
pub const MAX_OPS: usize = 200;

/// Most voxels a recipe may write, counted before anything is built: the sum over its operations
/// of their box, post and ring volumes. Recipes come from players and are built in the browser
/// and on the server, so one recipe must not cost more than about a megavoxel of work.
pub const MAX_VOXEL_WRITES: u64 = 1_048_576;

/// Most distinct part names in one recipe, the same cap a prop has on its named parts.
pub const MAX_PART_NAMES: usize = 16;

/// Longest recipe text [`Recipe::from_json`] will parse. A recipe is a few kilobytes at most;
/// the cap stops a huge input from being parsed at all.
pub const MAX_RECIPE_BYTES: usize = 16 * 1024;

/// A recipe: a size, a seed and the operations that fill it.
///
/// Input from the network or from a player must be read with [`Recipe::from_json`], not a bare
/// serde_json call, so that [`MAX_RECIPE_BYTES`] applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u16,
    pub scale: RecipeScale,
    pub seed: u64,
    pub size: [u16; 3],
    pub ops: Vec<Op>,
}

impl Recipe {
    /// Parses a recipe from JSON text, refusing text over [`MAX_RECIPE_BYTES`] before the parser
    /// reads it.
    pub fn from_json(text: &str) -> Result<Recipe, String> {
        if text.len() > MAX_RECIPE_BYTES {
            return Err(format!(
                "a recipe is at most {MAX_RECIPE_BYTES} bytes, got {}",
                text.len()
            ));
        }
        serde_json::from_str(text).map_err(|error| error.to_string())
    }
}

/// The scale a recipe is drawn at, as named in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeScale {
    /// 5 cm voxels, the finest scale: held items and small decoration.
    Fine,
    /// 10 cm voxels: furniture and boats.
    Prop,
    /// 0.5 m voxels: structures.
    World,
}

impl RecipeScale {
    /// The grid scale this name stands for. `fine` is the character scale (5 cm voxels), the
    /// only one at that size.
    pub fn scale(self) -> Scale {
        match self {
            RecipeScale::Fine => Scale::Character,
            RecipeScale::Prop => Scale::Prop,
            RecipeScale::World => Scale::World,
        }
    }
}
