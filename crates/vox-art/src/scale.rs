//! The scales voxel art is drawn at, and how big a grid at each one may be.
//!
//! Characters and small props use 5 cm and 10 cm voxels. A world piece (a structure) uses
//! 0.25 m voxels, the size of one world block (`vox_core::BLOCK_M`; vox-art depends on nothing
//! of ours, so the number is repeated here). Every grid is capped at 64 voxels a side, so
//! a prop never outgrows what a single placement and a single edit can handle.

/// The scale a grid is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// 5 cm voxels: the characters' scale, held items and small decoration.
    Character,
    /// 10 cm voxels: furniture, boats and trees.
    Prop,
    /// 0.25 m voxels: structures, one world block per voxel.
    World,
}

/// Most voxels along one side of a grid, at any scale.
const MAX_SIDE: u16 = 64;

impl Scale {
    /// The side of one voxel in metres.
    pub fn voxel_size(&self) -> f32 {
        match self {
            Scale::Character => 0.05,
            Scale::Prop => 0.10,
            Scale::World => 0.25,
        }
    }

    /// The most voxels a grid at this scale may have along one side.
    pub fn max_side(&self) -> u16 {
        MAX_SIDE
    }
}
