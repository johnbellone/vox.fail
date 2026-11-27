//! A node: one thing in the world graph, with the footprint it may build on, how far it has been
//! refined, and (once it is detailed enough) the recipe that builds it.
//!
//! Nodes come from players and from a language model, so their sizes are capped here and checked
//! by [`crate::constraints`]; nothing in this file trusts a value it is given.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Longest side of a footprint, in blocks. A node builds at most the structure cap, so a side
/// past this could not be built anyway.
pub const MAX_SIDE: u16 = 64;

/// Most keys in a node's `params`.
pub const MAX_PARAMS: usize = 32;

/// Longest a node's `params` may be once serialized to JSON.
pub const MAX_PARAM_BYTES: usize = 4 * 1024;

/// What a node is. A closed, versioned vocabulary: a new kind is appended, never reordered, so
/// stored graphs keep their meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    House,
    Tower,
    Wall,
    Road,
    Bridge,
    Well,
    Tree,
    Garden,
    Plaza,
    Village,
    Island,
    Other,
}

/// How far a node has been refined. Ordered: a node only moves up the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// A kind and a description, nothing placed.
    Intent,
    /// A footprint and a position.
    Layout,
    /// An outline and a height.
    Massing,
    /// A recipe with walls, roof and openings.
    Structure,
    /// Trim, props and weathering on top of the structure.
    Dressing,
}

/// A box of blocks a node may build in: its minimum corner in the world and its size.
///
/// Sides are inclusive: a footprint at `[4, 0, 4]` of size `[3, 2, 3]` covers x 4 to 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    /// The world block of the footprint's minimum corner.
    pub at: [i32; 3],
    pub size: [u16; 3],
}

impl Footprint {
    pub fn new(at: [i32; 3], size: [u16; 3]) -> Footprint {
        Footprint { at, size }
    }

    /// Minimum and maximum corners, inclusive. Done in `i64` so that a footprint near the edge of
    /// the `i32` range cannot overflow while it is compared.
    pub(crate) fn bounds(&self) -> ([i64; 3], [i64; 3]) {
        let min = self.at.map(i64::from);
        let max = [0, 1, 2].map(|axis| min[axis] + i64::from(self.size[axis]) - 1);
        (min, max)
    }

    /// True when `inner` lies wholly inside this footprint.
    pub fn contains(&self, inner: &Footprint) -> bool {
        let (outer_min, outer_max) = self.bounds();
        let (inner_min, inner_max) = inner.bounds();
        (0..3).all(|axis| outer_min[axis] <= inner_min[axis] && inner_max[axis] <= outer_max[axis])
    }

    /// True when the two footprints share at least one block.
    pub fn overlaps(&self, other: &Footprint) -> bool {
        let (a_min, a_max) = self.bounds();
        let (b_min, b_max) = other.bounds();
        (0..3).all(|axis| a_min[axis] <= b_max[axis] && b_min[axis] <= a_max[axis])
    }
}

/// One node of the world graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: u32,
    pub kind: Kind,
    pub detail: Detail,
    pub footprint: Footprint,
    /// The node this one sits inside. Its footprint must lie inside the parent's.
    pub parent: Option<u32>,
    /// What must be true of the node: size, floors, doors facing the path. At most
    /// [`MAX_PARAMS`] keys and [`MAX_PARAM_BYTES`] of JSON.
    pub params: Map<String, Value>,
    /// The recipe, as JSON text, once the node is detailed enough. At most
    /// `vox_art::recipe::MAX_RECIPE_BYTES`.
    pub recipe: Option<String>,
    /// Seeds the recipe, so expansion is reproducible.
    pub seed: u64,
}

impl Node {
    /// A node at the `intent` level with nothing else set: no parent, no params, no recipe, seed 0.
    pub fn new(id: u32, kind: Kind, footprint: Footprint) -> Node {
        Node {
            id,
            kind,
            detail: Detail::Intent,
            footprint,
            parent: None,
            params: Map::new(),
            recipe: None,
            seed: 0,
        }
    }
}
