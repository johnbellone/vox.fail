//! Reads MagicaVoxel `.vox` files (format 150 and 200) into voxel models.
//!
//! The props are drawn in MagicaVoxel, so their files are the import path into the editor, and
//! the bytes may come from anywhere (a shared folder, a download). Every length is checked
//! against the bytes that really remain before anything is allocated or sliced, every size and
//! index is capped, and nothing panics on any input. Only the chunks the editor needs are read
//! (SIZE, XYZI, RGBA); the rest (scene graph, layers, materials) are skipped by their sizes.
//!
//! Colours are not snapped to the project palette here: [`VoxModel::to_grid`] takes a map from
//! the file's colour indices to palette indices, and the importer decides what it means.

use std::fmt;

use crate::grid::{GridError, VoxelGrid};
use crate::scale::Scale;

/// Largest file accepted, in bytes.
const MAX_FILE: usize = 16 * 1024 * 1024;
/// Most models read from one file.
const MAX_MODELS: usize = 16;
/// Most voxels in one model: a 64 voxel cube.
const MAX_VOXELS: usize = 64 * 64 * 64;
/// Largest side a SIZE chunk may declare. The grid itself accepts less (see [`Scale`]).
const MAX_SIDE: u32 = 256;
/// Deepest chunk nesting followed. Real files nest two or three levels.
const MAX_DEPTH: usize = 8;

/// Why a `.vox` file could not be read or turned into a grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoxError {
    /// The file is bigger than the cap.
    TooLarge(usize),
    /// The file ends inside a header or a chunk, or a count runs past the end of its chunk.
    Truncated,
    /// The file does not start with `VOX `.
    BadMagic,
    /// A format version other than 150 or 200.
    BadVersion(i32),
    /// Chunks nest deeper than [`MAX_DEPTH`].
    TooDeep,
    /// A chunk is malformed in a way the message names.
    BadChunk(&'static str),
    /// A SIZE side is 0 or larger than the cap, or a dimension could not be read.
    BadSize([u32; 3]),
    /// More models than [`MAX_MODELS`].
    TooManyModels,
    /// An XYZI chunk claims more voxels than [`MAX_VOXELS`].
    TooManyVoxels(usize),
    /// A voxel lies outside its model's SIZE.
    VoxelOutside { pos: [u8; 3], size: [u32; 3] },
    /// The file has no model (no SIZE and XYZI pair).
    NoModel,
    /// The file has no RGBA chunk. MagicaVoxel's default palette is not built in, so such files
    /// are refused rather than given invented colours.
    NoPalette,
    /// The model does not fit a grid at the requested scale.
    Grid(GridError),
}

impl fmt::Display for VoxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoxError::TooLarge(len) => write!(f, "vox file of {len} bytes is over the cap"),
            VoxError::Truncated => write!(f, "vox file ends inside a chunk"),
            VoxError::BadMagic => write!(f, "not a MagicaVoxel file (no VOX header)"),
            VoxError::BadVersion(v) => write!(f, "vox format version {v} is not 150 or 200"),
            VoxError::TooDeep => write!(f, "vox chunks nest deeper than {MAX_DEPTH}"),
            VoxError::BadChunk(what) => write!(f, "malformed vox chunk: {what}"),
            VoxError::BadSize(size) => write!(f, "vox model size {size:?} is out of range"),
            VoxError::TooManyModels => write!(f, "vox file has more than {MAX_MODELS} models"),
            VoxError::TooManyVoxels(n) => write!(f, "vox model claims {n} voxels"),
            VoxError::VoxelOutside { pos, size } => {
                write!(f, "vox voxel {pos:?} is outside a model of size {size:?}")
            }
            VoxError::NoModel => write!(f, "vox file has no model"),
            VoxError::NoPalette => write!(f, "vox file has no RGBA palette chunk"),
            VoxError::Grid(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for VoxError {}

impl From<GridError> for VoxError {
    fn from(err: GridError) -> Self {
        VoxError::Grid(err)
    }
}

/// A whole `.vox` file: its models and the palette they were coloured with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxFile {
    pub models: Vec<VoxModel>,
    /// The colour of each colour index: `palette[i]` is index `i`, RGBA. Index 0 is empty and
    /// stays transparent black. The file's RGBA chunk has 256 entries, and entry `k` is colour
    /// index `k + 1`, so the file's last entry (index 256) has no slot and is dropped.
    pub palette: [[u8; 4]; 256],
}

/// One model: its size and its filled voxels, each with a colour index from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxModel {
    pub size: [u32; 3],
    pub voxels: Vec<([u8; 3], u8)>,
}

impl VoxModel {
    /// Puts the model into a grid at `scale`. `map` turns each file colour index into a palette
    /// index; 0 drops the voxel. Fails if the model's sides do not fit the scale's grid.
    pub fn to_grid(&self, scale: Scale, map: impl Fn(u8) -> u8) -> Result<VoxelGrid, VoxError> {
        let mut dims = [0u16; 3];
        for (dst, &side) in dims.iter_mut().zip(&self.size) {
            *dst = u16::try_from(side).map_err(|_| VoxError::BadSize(self.size))?;
        }
        let mut grid = VoxelGrid::new(scale, dims)?;
        for &(pos, index) in &self.voxels {
            let mapped = map(index);
            if mapped == 0 {
                continue;
            }
            let at = [u16::from(pos[0]), u16::from(pos[1]), u16::from(pos[2])];
            grid.set(at, mapped)?;
        }
        Ok(grid)
    }
}

/// Reads a `.vox` file from its bytes.
pub fn read(bytes: &[u8]) -> Result<VoxFile, VoxError> {
    if bytes.len() > MAX_FILE {
        return Err(VoxError::TooLarge(bytes.len()));
    }
    let (magic, rest) = split(bytes, 4)?;
    if magic != b"VOX " {
        return Err(VoxError::BadMagic);
    }
    let (version, rest) = take_i32(rest)?;
    if version != 150 && version != 200 {
        return Err(VoxError::BadVersion(version));
    }
    let mut parser = Parser::default();
    parser.walk(rest, 0)?;
    let palette = parser.palette.ok_or(VoxError::NoPalette)?;
    if parser.models.is_empty() {
        return Err(VoxError::NoModel);
    }
    Ok(VoxFile {
        models: parser.models,
        palette,
    })
}

/// Splits `n` bytes off the front of `data`, or reports that the file ends early.
fn split(data: &[u8], n: usize) -> Result<(&[u8], &[u8]), VoxError> {
    data.split_at_checked(n).ok_or(VoxError::Truncated)
}

/// Reads one little-endian i32 from the front of `data`.
fn take_i32(data: &[u8]) -> Result<(i32, &[u8]), VoxError> {
    let (head, rest) = split(data, 4)?;
    let bytes: [u8; 4] = head.try_into().map_err(|_| VoxError::Truncated)?;
    Ok((i32::from_le_bytes(bytes), rest))
}

/// Reads a chunk size, which the format stores as a signed i32 and which must not be negative.
fn take_size(data: &[u8]) -> Result<(usize, &[u8]), VoxError> {
    let (value, rest) = take_i32(data)?;
    let size = usize::try_from(value).map_err(|_| VoxError::BadChunk("negative chunk size"))?;
    Ok((size, rest))
}

/// What has been read so far: the models, the SIZE waiting for its XYZI, and the palette.
#[derive(Default)]
struct Parser {
    pending_size: Option<[u32; 3]>,
    models: Vec<VoxModel>,
    palette: Option<[[u8; 4]; 256]>,
}

impl Parser {
    /// Reads the chunks in `data` (and, recursively, their children). `depth` is the nesting
    /// level of these chunks: 0 at the top of the file.
    fn walk(&mut self, mut data: &[u8], depth: usize) -> Result<(), VoxError> {
        while !data.is_empty() {
            let (id, rest) = split(data, 4)?;
            let (content_len, rest) = take_size(rest)?;
            let (children_len, rest) = take_size(rest)?;
            let (content, rest) = split(rest, content_len)?;
            let (children, rest) = split(rest, children_len)?;
            match id {
                b"SIZE" => self.size(content)?,
                b"XYZI" => self.voxels(content)?,
                b"RGBA" => self.rgba(content)?,
                _ => {}
            }
            if !children.is_empty() {
                let child_depth = depth + 1;
                if child_depth > MAX_DEPTH {
                    return Err(VoxError::TooDeep);
                }
                self.walk(children, child_depth)?;
            }
            data = rest;
        }
        Ok(())
    }

    fn size(&mut self, content: &[u8]) -> Result<(), VoxError> {
        if content.len() != 12 {
            return Err(VoxError::BadChunk("SIZE is not 12 bytes"));
        }
        let (x, rest) = take_i32(content)?;
        let (y, rest) = take_i32(rest)?;
        let (z, _) = take_i32(rest)?;
        // A negative side is reported as 0, which is already out of range.
        let dims = [x, y, z].map(|side| u32::try_from(side).unwrap_or(0));
        if dims.iter().any(|side| !(1..=MAX_SIDE).contains(side)) {
            return Err(VoxError::BadSize(dims));
        }
        self.pending_size = Some(dims);
        Ok(())
    }

    fn voxels(&mut self, content: &[u8]) -> Result<(), VoxError> {
        let size = self
            .pending_size
            .take()
            .ok_or(VoxError::BadChunk("XYZI without a SIZE before it"))?;
        let (count, body) = take_i32(content)?;
        let count =
            usize::try_from(count).map_err(|_| VoxError::BadChunk("negative voxel count"))?;
        if count > MAX_VOXELS {
            return Err(VoxError::TooManyVoxels(count));
        }
        if self.models.len() >= MAX_MODELS {
            return Err(VoxError::TooManyModels);
        }
        let needed = count.checked_mul(4).ok_or(VoxError::Truncated)?;
        let (entries, _) = split(body, needed)?;
        let mut voxels = Vec::with_capacity(count);
        for &[x, y, z, index] in entries.as_chunks::<4>().0 {
            if u32::from(x) >= size[0] || u32::from(y) >= size[1] || u32::from(z) >= size[2] {
                return Err(VoxError::VoxelOutside {
                    pos: [x, y, z],
                    size,
                });
            }
            voxels.push(([x, y, z], index));
        }
        self.models.push(VoxModel { size, voxels });
        Ok(())
    }

    fn rgba(&mut self, content: &[u8]) -> Result<(), VoxError> {
        let (entries, _) =
            split(content, 256 * 4).map_err(|_| VoxError::BadChunk("RGBA too short"))?;
        let mut palette = [[0u8; 4]; 256];
        // Entry i of the file is colour index i + 1; index 0 stays empty.
        for (slot, &rgba) in palette.iter_mut().skip(1).zip(entries.as_chunks::<4>().0) {
            *slot = rgba;
        }
        self.palette = Some(palette);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn chunk(id: &[u8; 4], content: &[u8], children: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((content.len() as i32).to_le_bytes());
        out.extend((children.len() as i32).to_le_bytes());
        out.extend(content);
        out.extend(children);
        out
    }

    /// A whole file: the header and a MAIN chunk holding `children`.
    fn file(children: &[u8]) -> Vec<u8> {
        let mut out = b"VOX ".to_vec();
        out.extend(150i32.to_le_bytes());
        out.extend(chunk(b"MAIN", &[], children));
        out
    }

    fn size_chunk(x: i32, y: i32, z: i32) -> Vec<u8> {
        let mut content = Vec::new();
        for side in [x, y, z] {
            content.extend(side.to_le_bytes());
        }
        chunk(b"SIZE", &content, &[])
    }

    fn xyzi_chunk(voxels: &[[u8; 4]]) -> Vec<u8> {
        let mut content = (voxels.len() as i32).to_le_bytes().to_vec();
        for voxel in voxels {
            content.extend(voxel);
        }
        chunk(b"XYZI", &content, &[])
    }

    /// A palette where entry `k` is `[k, 128, 64, 255]`.
    fn rgba_chunk() -> Vec<u8> {
        let mut content = Vec::new();
        for k in 0..=255u8 {
            content.extend([k, 128, 64, 255]);
        }
        chunk(b"RGBA", &content, &[])
    }

    fn small_fixture() -> Vec<u8> {
        file(
            &[
                size_chunk(2, 2, 2),
                xyzi_chunk(&[[0, 0, 0, 1], [1, 1, 1, 2]]),
                rgba_chunk(),
            ]
            .concat(),
        )
    }

    fn collect_props(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("props directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                collect_props(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "vox") {
                out.push(path);
            }
        }
    }

    #[test]
    fn small_fixture_reads_and_maps_to_a_grid() {
        let vox = read(&small_fixture()).unwrap();
        assert_eq!(vox.models.len(), 1);
        assert_eq!(vox.models[0].size, [2, 2, 2]);
        assert_eq!(vox.models[0].voxels.len(), 2);
        assert_eq!(vox.palette[0], [0; 4]);
        assert_eq!(vox.palette[1], [0, 128, 64, 255]);
        assert_eq!(vox.palette[255], [254, 128, 64, 255]);

        let grid = vox.models[0]
            .to_grid(Scale::Prop, |index| if index == 2 { 5 } else { index })
            .unwrap();
        assert_eq!(grid.get([0, 0, 0]), Some(1));
        assert_eq!(grid.get([1, 1, 1]), Some(5));
        let dropped = vox.models[0]
            .to_grid(Scale::Prop, |index| if index == 1 { 0 } else { index })
            .unwrap();
        assert_eq!(dropped.len(), 1);
    }

    #[test]
    fn every_prop_file_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../source/vox/props");
        let mut paths = Vec::new();
        collect_props(&root, &mut paths);
        assert!(!paths.is_empty(), "no .vox files under {}", root.display());
        for path in &paths {
            let bytes = fs::read(path).expect("prop file");
            let vox = read(&bytes).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            assert!(!vox.models.is_empty(), "{} has no model", path.display());
            assert!(
                vox.models.iter().any(|m| !m.voxels.is_empty()),
                "{} has no voxel",
                path.display()
            );
        }
    }

    #[test]
    fn header_alone_is_refused() {
        assert_eq!(read(b"VOX "), Err(VoxError::Truncated));
        assert!(read(b"VOX \x96\0\0\0").is_err());
        assert_eq!(read(b"NOPE\x96\0\0\0"), Err(VoxError::BadMagic));
        assert_eq!(read(b"VOX \x63\0\0\0"), Err(VoxError::BadVersion(99)));
    }

    #[test]
    fn xyzi_claiming_a_million_voxels_is_refused() {
        let mut content = 1_000_000i32.to_le_bytes().to_vec();
        content.extend([0u8; 8]);
        let children = [size_chunk(4, 4, 4), chunk(b"XYZI", &content, &[])].concat();
        assert!(matches!(
            read(&file(&children)),
            Err(VoxError::TooManyVoxels(1_000_000))
        ));
    }

    #[test]
    fn xyzi_count_past_the_chunk_is_refused() {
        let mut content = 100i32.to_le_bytes().to_vec();
        content.extend([0u8; 8]);
        let children = [size_chunk(4, 4, 4), chunk(b"XYZI", &content, &[])].concat();
        assert_eq!(read(&file(&children)), Err(VoxError::Truncated));
    }

    #[test]
    fn zero_size_is_refused() {
        let children = [size_chunk(0, 1, 1), xyzi_chunk(&[]), rgba_chunk()].concat();
        assert_eq!(read(&file(&children)), Err(VoxError::BadSize([0, 1, 1])));
    }

    #[test]
    fn voxel_outside_its_size_is_refused() {
        let children = [
            size_chunk(2, 2, 2),
            xyzi_chunk(&[[2, 0, 0, 1]]),
            rgba_chunk(),
        ]
        .concat();
        assert_eq!(
            read(&file(&children)),
            Err(VoxError::VoxelOutside {
                pos: [2, 0, 0],
                size: [2, 2, 2]
            })
        );
    }

    #[test]
    fn chunk_larger_than_the_file_is_refused() {
        let mut bytes = b"VOX ".to_vec();
        bytes.extend(150i32.to_le_bytes());
        bytes.extend(b"MAIN");
        bytes.extend(0i32.to_le_bytes());
        bytes.extend(1000i32.to_le_bytes());
        assert_eq!(read(&bytes), Err(VoxError::Truncated));
    }

    #[test]
    fn deep_nesting_is_refused() {
        let mut inner = chunk(b"nGRP", &[], &[]);
        for _ in 0..10 {
            inner = chunk(b"nGRP", &[], &inner);
        }
        assert_eq!(read(&file(&inner)), Err(VoxError::TooDeep));
    }

    #[test]
    fn file_without_a_palette_is_refused() {
        let children = [size_chunk(2, 2, 2), xyzi_chunk(&[[0, 0, 0, 1]])].concat();
        assert_eq!(read(&file(&children)), Err(VoxError::NoPalette));
    }

    #[test]
    fn every_prefix_of_a_real_file_is_refused_or_read() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../source/vox/props");
        let mut paths = Vec::new();
        collect_props(&root, &mut paths);
        let smallest = paths
            .iter()
            .min_by_key(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX))
            .expect("a prop file");
        let bytes = fs::read(smallest).expect("prop file");
        assert!(read(&bytes).is_ok());
        for len in 0..bytes.len() {
            let prefix = bytes.get(..len).unwrap_or_default();
            assert!(read(prefix).is_err(), "prefix of {len} bytes was accepted");
        }
    }
}
