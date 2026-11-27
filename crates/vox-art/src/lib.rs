//! vox-art: voxels in the project's style, built by the editor and the game, with no engine in them.
//!
//! - [`scale`]: the three scales (character, prop, world) and their voxel sizes and grid limits.
//! - [`grid`]: a sparse grid of palette indices with checked bounds.
//! - [`palette`]: the project palette, its named materials and their colour ramps.
//! - [`rng`]: one small seeded generator, the same on every target.
//!
//! Runs natively and in wasm: no threads, no clocks, no file system.

pub mod check;
pub mod grid;
pub mod import;
pub mod palette;
pub mod prop;
pub mod recipe;
pub mod rng;
pub mod scale;
pub mod vox;

pub use grid::{GridError, VoxelGrid};
pub use palette::{Material, Palette};
pub use prop::PropDef;
pub use rng::Rng;
pub use scale::Scale;
pub use vox::{VoxError, VoxFile, VoxModel};
