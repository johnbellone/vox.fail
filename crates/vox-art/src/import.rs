//! Imports one model of a `.vox` file into a grid whose colours belong to the project palette.
//!
//! Each colour the model uses is snapped to the nearest palette colour (squared RGB distance,
//! alpha ignored; a tie goes to the lower palette index), and the snaps are reported so the
//! caller can see how far each colour moved. Index 0 is empty and stays empty. A duplicate
//! position keeps its first non-zero colour, as the notes on the `.vox` reader require.

use std::collections::BTreeSet;
use std::fmt;

use crate::grid::VoxelGrid;
use crate::palette::Palette;
use crate::scale::Scale;
use crate::vox::{VoxError, VoxFile, VoxModel};

/// Why a model could not be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The file has no model with this index.
    NoSuchModel { model: usize, models: usize },
    /// The model uses colours but the palette has none to snap them to.
    EmptyPalette,
    /// The model could not be read into a grid at this scale.
    Vox(VoxError),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NoSuchModel { model, models } => {
                write!(f, "model {model} is not in a file of {models} models")
            }
            ImportError::EmptyPalette => write!(f, "the palette has no colours to snap to"),
            ImportError::Vox(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<VoxError> for ImportError {
    fn from(err: VoxError) -> Self {
        ImportError::Vox(err)
    }
}

/// One file colour and the palette colour it was snapped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snap {
    /// The colour as the file has it, RGBA.
    pub from: [u8; 4],
    /// The palette index it became (never 0).
    pub to: u8,
    /// Squared RGB distance between the two colours (alpha ignored).
    pub distance: u32,
}

/// Every snap made by an import: one per distinct colour the model uses, in file index order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapReport {
    pub snaps: Vec<Snap>,
}

impl SnapReport {
    /// The largest squared distance of any snap, or 0 when nothing was snapped.
    pub fn max_distance(&self) -> u32 {
        self.snaps.iter().map(|s| s.distance).max().unwrap_or(0)
    }
}

/// Imports model `model` of `file` at `scale`, snapping its colours to `palette`.
///
/// It never refuses for colour distance: a far snap is reported in the [`SnapReport`] and the
/// grid is made anyway. The caller (the prop import tool) must refuse past its limit, as
/// `docs/design/props.md` (Lane B, "Snaps the palette") requires, naming the colours it refused.
pub fn import(
    file: &VoxFile,
    model: usize,
    palette: &Palette,
    scale: Scale,
) -> Result<(VoxelGrid, SnapReport), ImportError> {
    let source = file.models.get(model).ok_or(ImportError::NoSuchModel {
        model,
        models: file.models.len(),
    })?;

    // Keep only the first non-zero colour at each position, and never a zero colour.
    let mut claimed = BTreeSet::new();
    let voxels: Vec<([u8; 3], u8)> = source
        .voxels
        .iter()
        .filter(|&&(pos, index)| index != 0 && claimed.insert(pos))
        .copied()
        .collect();
    let deduped = VoxModel {
        size: source.size,
        voxels,
    };

    let used: BTreeSet<u8> = deduped.voxels.iter().map(|&(_, index)| index).collect();
    let mut table = [0u8; 256];
    let mut snaps = Vec::with_capacity(used.len());
    for &colour in &used {
        let from = file.palette[usize::from(colour)];
        let (to, distance) = nearest(palette, from).ok_or(ImportError::EmptyPalette)?;
        table[usize::from(colour)] = to;
        snaps.push(Snap { from, to, distance });
    }

    let grid = deduped.to_grid(scale, |index| table[usize::from(index)])?;
    Ok((grid, SnapReport { snaps }))
}

/// The palette index (1-based, flat over every material) of the colour nearest to `rgba`, and
/// the squared RGB distance to it. Ties keep the lower index, since the scan is in order.
/// Returns `None` when the palette has no colour an index can name.
fn nearest(palette: &Palette, rgba: [u8; 4]) -> Option<(u8, u32)> {
    let mut best: Option<(u8, u32)> = None;
    let mut flat = 0usize;
    for material in &palette.materials {
        for colour in &material.colours {
            flat = flat.checked_add(1)?;
            // Grid indices are one byte; colours past 255 cannot be named, so stop there.
            let Ok(index) = u8::try_from(flat) else {
                return best;
            };
            let distance = squared_distance(*colour, rgba);
            if best.is_none_or(|(_, so_far)| distance < so_far) {
                best = Some((index, distance));
            }
        }
    }
    best
}

/// Squared distance over the red, green and blue channels. Each difference is at most 255, so
/// three squares fit a `u32` with room to spare.
fn squared_distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    (0..3)
        .map(|channel| {
            let d = i32::from(a[channel]) - i32::from(b[channel]);
            d.unsigned_abs() * d.unsigned_abs()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Material;
    use crate::vox;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn collect_vox(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("props directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                collect_vox(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "vox") {
                out.push(path);
            }
        }
    }

    fn palette_of(colours: &[[u8; 4]]) -> Palette {
        Palette {
            version: 1,
            materials: vec![Material {
                name: "test".to_string(),
                colours: colours.to_vec(),
            }],
        }
    }

    #[test]
    fn every_prop_file_imports_with_identity_snaps_or_fails_with_a_reason() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../source/vox/props");
        let mut paths = Vec::new();
        collect_vox(&root, &mut paths);
        assert!(!paths.is_empty(), "no .vox files under {}", root.display());
        let mut imported = 0usize;
        let mut refused = Vec::new();
        for path in &paths {
            let bytes = fs::read(path).expect("prop file");
            let file = vox::read(&bytes).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            // The palette is the file's own colours, so every used colour has an exact match.
            let colours: Vec<[u8; 4]> = file.palette[1..].to_vec();
            let palette = palette_of(&colours);
            for model in 0..file.models.len() {
                match import(&file, model, &palette, Scale::Prop) {
                    Ok((grid, report)) => {
                        imported += 1;
                        assert_eq!(report.max_distance(), 0, "{}", path.display());
                        let filled = file.models[model]
                            .voxels
                            .iter()
                            .filter(|(_, index)| *index != 0)
                            .map(|(pos, _)| *pos)
                            .collect::<BTreeSet<_>>();
                        assert_eq!(grid.len(), filled.len(), "{}", path.display());
                    }
                    // A model over 64 voxels a side is refused by name, not cut down.
                    Err(err) => {
                        assert!(
                            matches!(err, ImportError::Vox(VoxError::Grid(_))),
                            "{} model {model}: {err}",
                            path.display()
                        );
                        refused.push(format!("{} model {model}: {err}", path.display()));
                    }
                }
            }
        }
        assert!(imported > 0, "no prop model imported: {refused:?}");
    }

    #[test]
    fn a_colour_snaps_to_the_nearest_palette_entry() {
        let mut palette_colours = [[0u8; 4]; 256];
        palette_colours[1] = [118, 118, 126, 255];
        palette_colours[2] = [60, 60, 64, 255];
        let file = VoxFile {
            models: vec![VoxModel {
                size: [2, 1, 1],
                voxels: vec![([0, 0, 0], 1), ([1, 0, 0], 2)],
            }],
            palette: palette_colours,
        };
        let palette = palette_of(&[[60, 60, 64, 255], [120, 120, 128, 255]]);

        let (grid, report) = import(&file, 0, &palette, Scale::Prop).unwrap();
        assert_eq!(grid.get([0, 0, 0]), Some(2));
        assert_eq!(grid.get([1, 0, 0]), Some(1));
        assert_eq!(
            report.snaps,
            vec![
                Snap {
                    from: [118, 118, 126, 255],
                    to: 2,
                    distance: 12,
                },
                Snap {
                    from: [60, 60, 64, 255],
                    to: 1,
                    distance: 0,
                },
            ]
        );
        assert_eq!(report.max_distance(), 12);
    }

    #[test]
    fn a_tie_goes_to_the_lower_index_and_a_duplicate_keeps_its_first_colour() {
        let mut palette_colours = [[0u8; 4]; 256];
        palette_colours[1] = [9, 9, 9, 255];
        let file = VoxFile {
            models: vec![VoxModel {
                size: [1, 1, 1],
                voxels: vec![([0, 0, 0], 0), ([0, 0, 0], 1), ([0, 0, 0], 1)],
            }],
            palette: palette_colours,
        };
        let palette = palette_of(&[[0, 0, 0, 255], [0, 0, 0, 255]]);
        let (grid, report) = import(&file, 0, &palette, Scale::Prop).unwrap();
        assert_eq!(grid.get([0, 0, 0]), Some(1));
        assert_eq!(report.snaps.len(), 1);
        assert_eq!(report.snaps[0].to, 1);
        assert_eq!(report.snaps[0].distance, 243);
    }

    #[test]
    fn a_missing_model_and_an_empty_palette_are_errors() {
        let file = VoxFile {
            models: vec![VoxModel {
                size: [1, 1, 1],
                voxels: vec![([0, 0, 0], 1)],
            }],
            palette: [[1, 2, 3, 255]; 256],
        };
        let palette = palette_of(&[]);
        assert_eq!(
            import(&file, 3, &palette, Scale::Prop).unwrap_err(),
            ImportError::NoSuchModel {
                model: 3,
                models: 1
            }
        );
        assert_eq!(
            import(&file, 0, &palette, Scale::Prop).unwrap_err(),
            ImportError::EmptyPalette
        );
    }
}
