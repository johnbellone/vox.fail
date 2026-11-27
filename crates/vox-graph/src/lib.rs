//! vox-graph: the world graph. Nodes are things in a level or a zone (a house, a road, a well), each
//! refined in detail levels from a bare intent to a recipe of voxels, and linked to other nodes.
//!
//! - [`node`]: a node, its kind, detail level, footprint and recipe.
//! - [`link`]: a relation between two nodes (`contains`, `next_to`, and so on).
//! - [`graph`]: the graph itself, with its caps, adding, removing and spatial queries.
//! - [`constraints`]: the placement rules, reported as problems rather than enforced silently.
//! - [`entity`]: a world's node entity and block names as a graph node and a palette (shared by
//!   the server's build and the editor's preview).
//! - [`expand`]: a node's recipe built through `vox-art` into block cells in the world.
//!
//! No Bevy, no threads, no clocks, no file system: native and wasm alike. Only `contains` links
//! are enforced by [`constraints`] for now; the other relations are stored and shown.

pub mod constraints;
pub mod entity;
pub mod expand;
pub mod graph;
pub mod link;
pub mod node;

pub use constraints::{Problem, check};
pub use expand::{ExpandError, expand};
pub use graph::Graph;
pub use link::{Link, Rel};
pub use vox_art::export::level::MAX_BUILD_CELLS;
pub use node::{Detail, Footprint, Kind, Node};
