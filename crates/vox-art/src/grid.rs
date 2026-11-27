//! A sparse grid of voxels, each holding a palette index.
//!
//! Only filled voxels are stored, in a sorted map, so iteration order is the same on every
//! machine. Index 0 means empty and is never stored: setting a voxel to 0 clears it.

use std::collections::BTreeMap;
use std::fmt;

use crate::scale::Scale;

/// Why a grid could not be made or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridError {
    /// A side was 0 or larger than the scale allows.
    BadSize { size: [u16; 3], max_side: u16 },
    /// A position lay outside the grid.
    OutOfBounds { pos: [u16; 3], size: [u16; 3] },
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GridError::BadSize { size, max_side } => {
                write!(
                    f,
                    "grid size {size:?} must have each side in 1..={max_side}"
                )
            }
            GridError::OutOfBounds { pos, size } => {
                write!(f, "voxel {pos:?} is outside a grid of size {size:?}")
            }
        }
    }
}

impl std::error::Error for GridError {}

/// A box of voxels of fixed size. Positions are `[x, y, z]` from `[0, 0, 0]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxelGrid {
    size: [u16; 3],
    cells: BTreeMap<[u16; 3], u8>,
}

impl VoxelGrid {
    /// An empty grid. Each side must be 1 to the scale's largest side.
    pub fn new(scale: Scale, size: [u16; 3]) -> Result<Self, GridError> {
        let max_side = scale.max_side();
        if size.iter().any(|&side| side == 0 || side > max_side) {
            return Err(GridError::BadSize { size, max_side });
        }
        Ok(Self {
            size,
            cells: BTreeMap::new(),
        })
    }

    /// The grid's size in voxels.
    pub fn size(&self) -> [u16; 3] {
        self.size
    }

    /// Sets the voxel at `pos` to palette index `index`; 0 clears it.
    pub fn set(&mut self, pos: [u16; 3], index: u8) -> Result<(), GridError> {
        self.check(pos)?;
        if index == 0 {
            self.cells.remove(&pos);
        } else {
            self.cells.insert(pos, index);
        }
        Ok(())
    }

    /// The palette index at `pos`, or `None` if the voxel is empty or outside the grid.
    pub fn get(&self, pos: [u16; 3]) -> Option<u8> {
        self.cells.get(&pos).copied()
    }

    /// Every filled voxel with its palette index, in sorted position order.
    pub fn iter(&self) -> impl Iterator<Item = ([u16; 3], u8)> + '_ {
        self.cells.iter().map(|(&pos, &index)| (pos, index))
    }

    /// How many voxels are filled.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether no voxel is filled.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The smallest and largest filled positions (both inclusive), or `None` if empty.
    pub fn bounds_of_filled(&self) -> Option<([u16; 3], [u16; 3])> {
        let mut keys = self.cells.keys();
        let first = *keys.next()?;
        Some(keys.fold((first, first), |(min, max), pos| {
            (
                [min[0].min(pos[0]), min[1].min(pos[1]), min[2].min(pos[2])],
                [max[0].max(pos[0]), max[1].max(pos[1]), max[2].max(pos[2])],
            )
        }))
    }

    fn check(&self, pos: [u16; 3]) -> Result<(), GridError> {
        let inside = pos.iter().zip(self.size).all(|(&p, side)| p < side);
        if inside {
            Ok(())
        } else {
            Err(GridError::OutOfBounds {
                pos,
                size: self.size,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_checked_against_the_scale() {
        assert!(VoxelGrid::new(Scale::Prop, [64, 64, 64]).is_ok());
        assert!(VoxelGrid::new(Scale::Character, [1, 1, 1]).is_ok());
        assert_eq!(
            VoxelGrid::new(Scale::Prop, [65, 1, 1]),
            Err(GridError::BadSize {
                size: [65, 1, 1],
                max_side: 64
            })
        );
        assert!(VoxelGrid::new(Scale::World, [0, 4, 4]).is_err());
    }

    #[test]
    fn set_get_and_clear() {
        let mut grid = VoxelGrid::new(Scale::Prop, [4, 4, 4]).unwrap();
        assert!(grid.is_empty());
        grid.set([1, 2, 3], 7).unwrap();
        assert_eq!(grid.get([1, 2, 3]), Some(7));
        assert_eq!(grid.len(), 1);
        grid.set([1, 2, 3], 0).unwrap();
        assert_eq!(grid.get([1, 2, 3]), None);
        assert!(grid.is_empty());
    }

    #[test]
    fn positions_outside_the_grid_are_errors() {
        let mut grid = VoxelGrid::new(Scale::Prop, [4, 4, 4]).unwrap();
        assert!(grid.set([4, 0, 0], 1).is_err());
        assert!(grid.set([0, 0, 4], 1).is_err());
        assert_eq!(grid.get([4, 0, 0]), None);
    }

    #[test]
    fn iteration_is_sorted_and_bounds_cover_filled_voxels() {
        let mut grid = VoxelGrid::new(Scale::Prop, [10, 10, 10]).unwrap();
        assert_eq!(grid.bounds_of_filled(), None);
        grid.set([5, 1, 0], 2).unwrap();
        grid.set([0, 9, 3], 1).unwrap();
        grid.set([2, 2, 8], 3).unwrap();
        let order: Vec<_> = grid.iter().collect();
        assert_eq!(order, vec![([0, 9, 3], 1), ([2, 2, 8], 3), ([5, 1, 0], 2)]);
        assert_eq!(grid.bounds_of_filled(), Some(([0, 1, 0], [5, 9, 8])));
    }
}
