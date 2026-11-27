//! Level cells: a built world-scale recipe as block cells placed in a world.
//!
//! A world-scale recipe names its materials after blocks in `data/blocks.json`, so each filled
//! voxel becomes one block at the origin plus its position. Tones play no part in that, since a
//! block has one look.

use crate::palette::Palette;
use crate::recipe::Built;

/// Most filled voxels one build may export: the Build cap of the world editor spec.
pub const MAX_BUILD_CELLS: usize = 32_768;

/// Why a build cannot be exported as level cells. The callers match on the variant (the server
/// answers `TooBig` as "too big", anything else as invalid); the text is for people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelError {
    /// The build has more filled voxels than [`MAX_BUILD_CELLS`].
    TooBig { cells: usize },
    /// A palette index names no material.
    UnknownIndex { index: u8 },
    /// The origin plus a voxel position runs past the `i32` range.
    Overflow { origin: [i32; 3] },
}

impl std::fmt::Display for LevelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LevelError::TooBig { cells } => write!(
                f,
                "a build may have at most {MAX_BUILD_CELLS} cells, this one has {cells}"
            ),
            LevelError::UnknownIndex { index } => write!(f, "palette index {index} names no material"),
            LevelError::Overflow { origin } => write!(f, "a build at {origin:?} runs past the i32 range"),
        }
    }
}

/// Every filled voxel of `built` as `(position, block name)`, sorted by position, with the
/// origin added to each position.
///
/// Expects a world-scale build whose materials are named after blocks. [`Built`] carries no
/// scale, so the caller checks the recipe's scale is `world` before calling this.
///
/// Refused when the build has more than [`MAX_BUILD_CELLS`] voxels, or when the origin plus a
/// position leaves the `i32` range. A palette index that names no material is refused too,
/// which a palette checked by [`Palette::from_json`] never produces.
pub fn level_cells(
    built: &Built,
    palette: &Palette,
    origin: [i32; 3],
) -> Result<Vec<([i32; 3], String)>, LevelError> {
    let count = built.grid.len();
    if count > MAX_BUILD_CELLS {
        return Err(LevelError::TooBig { cells: count });
    }
    let mut cells = Vec::with_capacity(count);
    for (pos, index) in built.grid.iter() {
        let at = place(origin, pos)?;
        let name = material_name(palette, index).ok_or(LevelError::UnknownIndex { index })?;
        cells.push((at, name.to_string()));
    }
    cells.sort_by_key(|(at, _)| *at);
    Ok(cells)
}

/// The origin plus a voxel position, checked against the `i32` range on every axis.
fn place(origin: [i32; 3], pos: [u16; 3]) -> Result<[i32; 3], LevelError> {
    let mut at = [0; 3];
    for (out, (base, offset)) in at.iter_mut().zip(origin.iter().zip(pos)) {
        *out = base
            .checked_add(i32::from(offset))
            .ok_or(LevelError::Overflow { origin })?;
    }
    Ok(at)
}

/// The name of the material a palette index belongs to, or `None` for 0 and an index past the
/// last colour. Indices run material by material, as [`Palette::index_of`] numbers them.
fn material_name(palette: &Palette, index: u8) -> Option<&str> {
    let mut first = 1usize;
    for material in &palette.materials {
        let end = first + material.colours.len();
        if (first..end).contains(&usize::from(index)) {
            return Some(&material.name);
        }
        first = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Recipe, eval};

    const PALETTE: &str = r#"{
        "version": 1,
        "materials": [
            { "name": "brick", "colours": [[150, 70, 50, 255]] },
            { "name": "wood", "colours": [[90, 50, 20, 255], [140, 90, 40, 255]] }
        ]
    }"#;

    fn build(json: &str) -> (Built, Palette) {
        let palette = Palette::from_json(PALETTE).unwrap();
        let recipe = Recipe::from_json(json).unwrap();
        (eval(&recipe, &palette).unwrap(), palette)
    }

    #[test]
    fn two_cells_are_named_and_placed_at_the_origin() {
        let (built, palette) = build(
            r#"{"version":1,"scale":"world","seed":1,"size":[2,1,1],
            "ops":[{"box":{"from":[0,0,0],"to":[0,0,0],"mat":"brick","tone":0}},
                   {"box":{"from":[1,0,0],"to":[1,0,0],"mat":"wood","tone":1}}]}"#,
        );
        assert_eq!(
            level_cells(&built, &palette, [10, 5, -3]),
            Ok(vec![
                ([10, 5, -3], "brick".to_string()),
                ([11, 5, -3], "wood".to_string()),
            ])
        );
    }

    #[test]
    fn a_build_at_the_cap_is_allowed() {
        // 64 by 64 by 8 is exactly 32,768 voxels.
        let (built, palette) = build(
            r#"{"version":1,"scale":"world","seed":1,"size":[64,64,8],
            "ops":[{"box":{"from":[0,0,0],"to":[63,63,7],"mat":"brick","tone":0}}]}"#,
        );
        assert_eq!(
            level_cells(&built, &palette, [0, 0, 0]).unwrap().len(),
            MAX_BUILD_CELLS
        );
    }

    #[test]
    fn a_build_over_the_cap_is_refused() {
        // 64 by 64 by 9 is 36,864 voxels, over the 32,768 cap.
        let (built, palette) = build(
            r#"{"version":1,"scale":"world","seed":1,"size":[64,64,9],
            "ops":[{"box":{"from":[0,0,0],"to":[63,63,8],"mat":"brick","tone":0}}]}"#,
        );
        let error = level_cells(&built, &palette, [0, 0, 0]).unwrap_err();
        assert_eq!(error, LevelError::TooBig { cells: 36_864 });
        assert!(error.to_string().contains("32768"));
        assert!(error.to_string().contains("36864"));
    }

    #[test]
    fn an_origin_that_overflows_is_refused() {
        let (built, palette) = build(
            r#"{"version":1,"scale":"world","seed":1,"size":[2,1,1],
            "ops":[{"box":{"from":[0,0,0],"to":[1,0,0],"mat":"brick","tone":0}}]}"#,
        );
        let error = level_cells(&built, &palette, [i32::MAX, 0, 0]).unwrap_err();
        assert_eq!(error, LevelError::Overflow { origin: [i32::MAX, 0, 0] });
        assert!(error.to_string().contains("i32"));
    }
}
