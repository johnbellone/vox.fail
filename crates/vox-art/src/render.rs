//! Flat pictures of a voxel grid for review and catalogue thumbnails: orthographic views, one
//! dimetric view and a contact sheet of four of them. No GPU and no engine.
//!
//! Every pixel is decided with integer arithmetic, so one grid always renders to the same
//! bytes on every target. Only the PNG encoder is native (the `png` crate); the views build
//! on wasm too.
//!
//! Each view is the picture a viewer sees standing on that side of the grid, with +y up, so
//! the top of the picture is the top of the grid. Image rows run down. The project's default
//! camera looks towards -z (`data/levels/yard.json` says "-z is away from the camera"), which
//! is why Front is the view from +z. The right-hand side of a picture is the viewer's forward
//! direction crossed with +y.
//!
//! - Front: from +z, looking towards -z: +x right, +y up.
//! - Back: from -z, looking towards +z: -x right, +y up.
//! - Left: from -x, looking towards +x: +z right, +y up.
//! - Right: from +x, looking towards -x: -z right, +y up.
//! - Top: from +y, looking down: +x right, -z at the top of the picture.
//! - Iso: dimetric, from the (+x, +y, +z) corner: +x runs right and down, +z left and down.

use crate::grid::VoxelGrid;
use crate::palette::Palette;

/// Largest width or height of any picture made here, in pixels.
pub const MAX_SIDE: usize = 4096;

/// A picture: `rgba` holds `width * height` pixels of 4 bytes each, row by row from the top.
/// Pixels nothing covers are `[0, 0, 0, 0]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The direction a picture looks from (see the module doc for the axes of each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// From +z, looking towards -z.
    Front,
    /// From -z, looking towards +z.
    Back,
    /// From -x, looking towards +x.
    Left,
    /// From +x, looking towards -x.
    Right,
    /// From +y, looking down.
    Top,
    /// Dimetric, from the (+x, +y, +z) corner: the top face is drawn at full brightness, the
    /// +x face at 80% and the +z face at 60%.
    Iso,
}

/// Palette index (0 to 255) -> colour. Built once per picture, so each voxel costs a lookup
/// rather than a walk over the palette. Index 0 ("empty") has no colour.
type Colours = [Option<[u8; 4]>; 256];

fn colour_table(palette: &Palette) -> Colours {
    let mut table = [None; 256];
    for index in 1..=u8::MAX {
        table[usize::from(index)] = palette.colour(index);
    }
    table
}

/// Draws `grid` from one view, `px_per_voxel` pixels to a voxel side.
///
/// Fails if `px_per_voxel` is 0, if the picture would be wider or taller than [`MAX_SIDE`],
/// or if the grid uses a palette index the palette does not have.
pub fn view(
    grid: &VoxelGrid,
    palette: &Palette,
    view: View,
    px_per_voxel: u32,
) -> Result<Image, String> {
    render_view(grid, &colour_table(palette), view, px_per_voxel)
}

/// The four views Front, Right, Top and Iso side by side, with one voxel's width between them.
///
/// Fails under the same rules as [`view`], and if the sheet itself would be over [`MAX_SIDE`]
/// wide or tall. The sheet's size is worked out from the four parts before any voxel is drawn.
pub fn contact_sheet(
    grid: &VoxelGrid,
    palette: &Palette,
    px_per_voxel: u32,
) -> Result<Image, String> {
    const PARTS: [View; 4] = [View::Front, View::Right, View::Top, View::Iso];
    let mut sizes = [(0usize, 0usize); 4];
    for (slot, part) in sizes.iter_mut().zip(PARTS) {
        *slot = view_size(grid, part, px_per_voxel)?;
    }
    // Each part is at most MAX_SIDE, and the gaps are one voxel (at most MAX_SIDE) each, so
    // these sums cannot overflow a usize.
    let gap = px_per_voxel as usize;
    let mut width = gap * (PARTS.len() - 1);
    let mut height = 0usize;
    for &(w, h) in &sizes {
        width += w;
        height = height.max(h);
    }
    let width = checked_side(width)?;
    let height = checked_side(height)?;

    let colours = colour_table(palette);
    let mut rgba = vec![0u8; width * height * 4];
    let mut x0 = 0usize;
    for part in PARTS {
        let img = render_view(grid, &colours, part, px_per_voxel)?;
        let row_bytes = img.width as usize * 4;
        for y in 0..img.height as usize {
            let src = y * row_bytes;
            let dst = (y * width + x0) * 4;
            rgba[dst..dst + row_bytes].copy_from_slice(&img.rgba[src..src + row_bytes]);
        }
        x0 += img.width as usize + gap;
    }
    Ok(Image {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// Encodes a picture as a PNG file (8-bit RGBA). Native only.
#[cfg(not(target_arch = "wasm32"))]
pub fn encode_png(img: &Image) -> Result<Vec<u8>, String> {
    let expected = img.width as usize * img.height as usize * 4;
    if img.rgba.len() != expected {
        return Err(format!(
            "picture holds {} bytes, expected {expected}",
            img.rgba.len()
        ));
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, img.width, img.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("PNG header: {e}"))?;
    writer
        .write_image_data(&img.rgba)
        .map_err(|e| format!("PNG data: {e}"))?;
    writer.finish().map_err(|e| format!("PNG end: {e}"))?;
    Ok(out)
}

/// Depths are `i32`, smaller being nearer the viewer. Orthographic depths are voxel counts
/// along the view, at most 64 because grid sides are at most 64 (`Scale::max_side`).
///
/// A dimetric depth is `-exit`, where `exit` is a quarter-voxel position along the view line
/// over `s` pixels a voxel, with `s <= MAX_SIDE`. The frame (`iso_frame`) keeps the picture at
/// most `MAX_SIDE` pixels, so `s * (sx + sz) <= MAX_SIDE` and `s * sy <= MAX_SIDE`. A cube
/// bound is then `4s` times a grid coordinate (at most 4 * MAX_SIDE) or a half-pixel offset
/// `pu` or `pv` (each within about 2 * MAX_SIDE of the frame), so every term is under 2^16 in
/// magnitude. `i32` holds it with room to spare.
struct Canvas {
    width: usize,
    height: usize,
    depth: Vec<i32>,
    rgba: Vec<u8>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        let pixels = width * height;
        Self {
            width,
            height,
            depth: vec![i32::MAX; pixels],
            rgba: vec![0; pixels * 4],
        }
    }

    /// Keeps the colour nearest the viewer: a smaller depth wins, and an equal depth keeps the
    /// colour drawn first.
    fn put(&mut self, x: usize, y: usize, depth: i32, colour: [u8; 4]) {
        let pixel = y * self.width + x;
        if depth < self.depth[pixel] {
            self.depth[pixel] = depth;
            self.rgba[pixel * 4..pixel * 4 + 4].copy_from_slice(&colour);
        }
    }

    fn finish(self) -> Image {
        Image {
            width: self.width as u32,
            height: self.height as u32,
            rgba: self.rgba,
        }
    }
}

/// Fails if `px_per_voxel` is 0: a picture needs at least one pixel a voxel.
fn check_px(px: u32) -> Result<(), String> {
    if px == 0 {
        Err("pixels per voxel must be at least 1".to_string())
    } else {
        Ok(())
    }
}

/// A side of `cells` voxels at `px` pixels each, refused if it is over [`MAX_SIDE`].
fn ortho_side(cells: usize, px: u32) -> Result<usize, String> {
    cells
        .checked_mul(px as usize)
        .ok_or_else(|| format!("picture is over {MAX_SIDE} pixels"))
        .and_then(checked_side)
}

/// `side` if it is 1 to [`MAX_SIDE`] pixels.
fn checked_side(side: usize) -> Result<usize, String> {
    if (1..=MAX_SIDE).contains(&side) {
        Ok(side)
    } else {
        Err(format!("picture side of {side} pixels is over {MAX_SIDE}"))
    }
}

/// Width or height of the dimetric picture for a span of `span` half-pixels.
fn iso_side(span: i64) -> Result<usize, String> {
    let pixels = ((span + 1) / 2).max(1);
    let pixels =
        usize::try_from(pixels).map_err(|_| format!("picture is over {MAX_SIDE} pixels"))?;
    checked_side(pixels)
}

/// The picture's columns and rows for an orthographic view of a grid of `size`, or `None`
/// for the dimetric view.
fn ortho_dims(view: View, [sx, sy, sz]: [usize; 3]) -> Option<(usize, usize)> {
    match view {
        View::Front | View::Back => Some((sx, sy)),
        View::Left | View::Right => Some((sz, sy)),
        View::Top => Some((sx, sz)),
        View::Iso => None,
    }
}

/// Where the voxel at `[x, y, z]` lands in an orthographic view: its column, its row and its
/// depth (smaller is nearer the viewer). `None` for the dimetric view.
fn ortho_cell(
    view: View,
    [sx, sy, sz]: [usize; 3],
    [x, y, z]: [usize; 3],
) -> Option<(usize, usize, i32)> {
    // Grid sides are at most 64, so no depth here comes near i32::MAX.
    let depth = |d: usize| i32::try_from(d).unwrap_or(i32::MAX);
    match view {
        View::Front => Some((x, sy - 1 - y, depth(sz - 1 - z))),
        View::Back => Some((sx - 1 - x, sy - 1 - y, depth(z))),
        View::Left => Some((z, sy - 1 - y, depth(x))),
        View::Right => Some((sz - 1 - z, sy - 1 - y, depth(sx - 1 - x))),
        View::Top => Some((x, z, depth(sy - 1 - y))),
        View::Iso => None,
    }
}

fn colour_of(colours: &Colours, index: u8) -> Result<[u8; 4], String> {
    colours[usize::from(index)].ok_or_else(|| {
        format!("a voxel uses palette index {index}, which the palette does not have")
    })
}

/// The size in pixels of `view` at `px` pixels a voxel, checked against [`MAX_SIDE`], without
/// drawing anything.
fn view_size(grid: &VoxelGrid, view: View, px: u32) -> Result<(usize, usize), String> {
    check_px(px)?;
    match view {
        View::Iso => {
            let frame = iso_frame(grid.size(), px)?;
            Ok((frame.width, frame.height))
        }
        _ => {
            let (cols, rows) = ortho_dims(view, grid.size().map(usize::from))
                .ok_or("not an orthographic view")?;
            Ok((ortho_side(cols, px)?, ortho_side(rows, px)?))
        }
    }
}

fn render_view(
    grid: &VoxelGrid,
    colours: &Colours,
    view: View,
    px: u32,
) -> Result<Image, String> {
    check_px(px)?;
    match view {
        View::Iso => iso(grid, colours, px),
        _ => ortho(grid, colours, view, px),
    }
}

/// Draws an orthographic view. One pass over the voxels keeps the nearest voxel in each
/// picture cell, then each cell is filled with its `px * px` pixels, so the cost is the
/// voxels plus the pixels rather than voxels times pixels.
fn ortho(grid: &VoxelGrid, colours: &Colours, view: View, px: u32) -> Result<Image, String> {
    let (width, height) = view_size(grid, view, px)?;
    let size = grid.size().map(usize::from);
    let (cols, rows) = ortho_dims(view, size).ok_or("not an orthographic view")?;
    let mut nearest: Vec<Option<(i32, [u8; 4])>> = vec![None; cols * rows];
    for (pos, index) in grid.iter() {
        let colour = colour_of(colours, index)?;
        let Some((col, row, depth)) = ortho_cell(view, size, pos.map(usize::from)) else {
            continue;
        };
        let cell = &mut nearest[row * cols + col];
        if cell.is_none_or(|(held, _)| depth < held) {
            *cell = Some((depth, colour));
        }
    }
    let p = px as usize;
    let mut rgba = vec![0u8; width * height * 4];
    for row in 0..rows {
        for col in 0..cols {
            let Some((_, colour)) = nearest[row * cols + col] else {
                continue;
            };
            for dy in 0..p {
                let y = row * p + dy;
                for dx in 0..p {
                    let i = (y * width + col * p + dx) * 4;
                    rgba[i..i + 4].copy_from_slice(&colour);
                }
            }
        }
    }
    Ok(Image {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// A grid corner in the dimetric projection, in half-pixel units. The projection is
/// `screen x = s (x - z)` and `screen y = s ((x + z) / 2 - y)`, with `s` pixels per voxel;
/// doubling both keeps the pixel centres on whole numbers.
fn project(x: i64, y: i64, z: i64, s: i64) -> (i64, i64) {
    (2 * s * (x - z), s * (x + z) - 2 * s * y)
}

/// Where the dimetric picture sits and how big it is: the lowest corner of the grid box in
/// half-pixel units, and the picture's size in pixels.
struct IsoFrame {
    u0: i64,
    v0: i64,
    width: usize,
    height: usize,
}

/// The dimetric frame for a grid of `size`. It covers the whole grid box, so its size depends
/// only on the grid's size and `px`.
fn iso_frame(size: [u16; 3], px: u32) -> Result<IsoFrame, String> {
    let s = i64::from(px);
    let [sx, sy, sz] = size.map(i64::from);
    let (mut u0, mut u1, mut v0, mut v1) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    for x in [0, sx] {
        for y in [0, sy] {
            for z in [0, sz] {
                let (u, v) = project(x, y, z, s);
                u0 = u0.min(u);
                u1 = u1.max(u);
                v0 = v0.min(v);
                v1 = v1.max(v);
            }
        }
    }
    Ok(IsoFrame {
        u0,
        v0,
        width: iso_side(u1 - u0)?,
        height: iso_side(v1 - v0)?,
    })
}

/// Which face of a voxel a view line leaves through, and the shade it is drawn at.
#[derive(Clone, Copy)]
enum Face {
    X,
    Y,
    Z,
}

impl Face {
    fn brightness(self) -> u32 {
        match self {
            Face::Y => 100,
            Face::X => 80,
            Face::Z => 60,
        }
    }
}

/// Whether the dimetric view line through the pixel centre `(pu, pv)` (half-pixel units, as
/// [`project`]) passes through the unit cube at `[x, y, z]`. If it does, returns where it
/// leaves the cube, in quarters of a voxel over `s` (larger is nearer the viewer), and the
/// face it leaves through.
///
/// The view line runs along (1, 1, 1), so it is `X = t + a`, `Y = t + c`, `Z = t` for
/// `a = pu / 2s` and `c = (pu - 2pv) / 4s`. Multiplying `t` by `4s` makes each bound whole.
fn cube_exit([x, y, z]: [i64; 3], pu: i64, pv: i64, s: i64) -> Option<(i64, Face)> {
    let k = 4 * s;
    let shift_y = pu - 2 * pv;
    let lo_x = k * x - 2 * pu;
    let hi_x = k * (x + 1) - 2 * pu;
    let lo_y = k * y - shift_y;
    let hi_y = k * (y + 1) - shift_y;
    let lo_z = k * z;
    let hi_z = k * (z + 1);
    let lo = lo_x.max(lo_y).max(lo_z);
    let (mut hi, mut face) = (hi_x, Face::X);
    if hi_y < hi {
        hi = hi_y;
        face = Face::Y;
    }
    if hi_z < hi {
        hi = hi_z;
        face = Face::Z;
    }
    (lo < hi).then_some((hi, face))
}

/// Scales a colour's RGB by `percent` percent; alpha is kept.
fn shade(colour: [u8; 4], percent: u32) -> [u8; 4] {
    let scale = |c: u8| (u32::from(c) * percent / 100) as u8;
    [
        scale(colour[0]),
        scale(colour[1]),
        scale(colour[2]),
        colour[3],
    ]
}

fn iso(grid: &VoxelGrid, colours: &Colours, px: u32) -> Result<Image, String> {
    let s = i64::from(px);
    let frame = iso_frame(grid.size(), px)?;
    let (u0, v0) = (frame.u0, frame.v0);
    let (width, height) = (frame.width, frame.height);
    let mut canvas = Canvas::new(width, height);
    for (pos, index) in grid.iter() {
        let colour = colour_of(colours, index)?;
        let [x, y, z] = pos.map(i64::from);
        let (mut a0, mut a1, mut b0, mut b1) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
        for dx in 0..=1 {
            for dy in 0..=1 {
                for dz in 0..=1 {
                    let (u, v) = project(x + dx, y + dy, z + dz, s);
                    a0 = a0.min(u);
                    a1 = a1.max(u);
                    b0 = b0.min(v);
                    b1 = b1.max(v);
                }
            }
        }
        // Only the pixel columns and rows under the cube's outline need testing; the margin
        // of one pixel is harmless because the exact test below rejects misses.
        let i_lo = ((a0 - u0 - 1).div_euclid(2) - 1).max(0);
        let i_hi = ((a1 - u0).div_euclid(2) + 1).min(width as i64 - 1);
        let j_lo = ((b0 - v0 - 1).div_euclid(2) - 1).max(0);
        let j_hi = ((b1 - v0).div_euclid(2) + 1).min(height as i64 - 1);
        for j in j_lo..=j_hi {
            for i in i_lo..=i_hi {
                let pu = u0 + 2 * i + 1;
                let pv = v0 + 2 * j + 1;
                if let Some((exit, face)) = cube_exit([x, y, z], pu, pv, s) {
                    let shaded = shade(colour, face.brightness());
                    // Bounded by the proof on `Canvas`, so the conversion cannot clamp.
                    let depth = i32::try_from(-exit).unwrap_or(i32::MIN);
                    canvas.put(i as usize, j as usize, depth, shaded);
                }
            }
        }
    }
    Ok(canvas.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Recipe, eval::eval};
    use crate::scale::Scale;

    /// Wood has three tones: palette indices 1 to 3.
    const PALETTE: &str = r#"{
        "version": 1,
        "materials": [
            { "name": "wood", "colours": [[90, 50, 20, 255], [140, 90, 40, 255], [190, 140, 80, 255]] }
        ]
    }"#;

    const STOOL: &str = r#"{
        "version": 1, "scale": "prop", "seed": 2, "size": [8, 8, 8],
        "ops": [
            { "lathe": { "axis_at": [4, 4], "from_y": 6, "profile": [3], "mat": "wood", "tone": 2 } },
            { "legs": { "at": [[1, 1], [5, 1], [1, 5], [5, 5]], "from_y": 0, "height": 6, "thick": 1, "mat": "wood", "tone": 0 } }
        ]
    }"#;

    const WOOD_1: [u8; 4] = [90, 50, 20, 255];
    const WOOD_2: [u8; 4] = [140, 90, 40, 255];
    const WOOD_3: [u8; 4] = [190, 140, 80, 255];

    fn palette() -> Palette {
        Palette::from_json(PALETTE).unwrap()
    }

    fn pixel(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        img.rgba[i..i + 4].try_into().unwrap()
    }

    fn hash8(img: &Image) -> String {
        blake3::hash(&img.rgba).as_bytes()[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Four voxels: A at the origin (WOOD_1), B one step along +x (WOOD_2), C one step along
    /// +z (WOOD_3) and D one step up from A (WOOD_3).
    fn small_grid() -> VoxelGrid {
        let mut grid = VoxelGrid::new(Scale::Prop, [2, 2, 2]).unwrap();
        grid.set([0, 0, 0], 1).unwrap();
        grid.set([1, 0, 0], 2).unwrap();
        grid.set([0, 0, 1], 3).unwrap();
        grid.set([0, 1, 0], 3).unwrap();
        grid
    }

    #[test]
    fn front_view_is_from_plus_z_so_the_plus_z_voxel_is_nearest() {
        let img = view(&small_grid(), &palette(), View::Front, 1).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        // C (z = 1) hides A (z = 0) in the bottom-left cell; +x is to the right.
        assert_eq!(pixel(&img, 0, 1), WOOD_3);
        assert_eq!(pixel(&img, 1, 1), WOOD_2);
        assert_eq!(pixel(&img, 0, 0), WOOD_3);
        assert_eq!(pixel(&img, 1, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn back_view_is_from_minus_z_so_the_minus_z_voxel_is_nearest() {
        let img = view(&small_grid(), &palette(), View::Back, 1).unwrap();
        // -x is to the right: A (x = 0) is in the right column and C (z = 1) is hidden behind it.
        assert_eq!(pixel(&img, 1, 1), WOOD_1);
        assert_eq!(pixel(&img, 0, 1), WOOD_2);
        assert_eq!(pixel(&img, 1, 0), WOOD_3);
        assert_eq!(pixel(&img, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn left_view_is_from_minus_x_with_plus_z_to_the_right() {
        let img = view(&small_grid(), &palette(), View::Left, 1).unwrap();
        // A (x = 0) hides B (x = 1). C sits at z = 1, the right column.
        assert_eq!(pixel(&img, 0, 1), WOOD_1);
        assert_eq!(pixel(&img, 1, 1), WOOD_3);
        assert_eq!(pixel(&img, 0, 0), WOOD_3);
        assert_eq!(pixel(&img, 1, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn right_view_is_from_plus_x_with_minus_z_to_the_right() {
        let img = view(&small_grid(), &palette(), View::Right, 1).unwrap();
        // B (x = 1) is nearest in the cell it shares with A; C (z = 1) is in the left column.
        assert_eq!(pixel(&img, 1, 1), WOOD_2);
        assert_eq!(pixel(&img, 0, 1), WOOD_3);
        assert_eq!(pixel(&img, 1, 0), WOOD_3);
        assert_eq!(pixel(&img, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn top_view_is_from_above_with_minus_z_at_the_top() {
        let img = view(&small_grid(), &palette(), View::Top, 1).unwrap();
        // D (y = 1) hides A. +x is to the right, and C (z = 1) is in the bottom row.
        assert_eq!(pixel(&img, 0, 0), WOOD_3);
        assert_eq!(pixel(&img, 1, 0), WOOD_2);
        assert_eq!(pixel(&img, 0, 1), WOOD_3);
        assert_eq!(pixel(&img, 1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn one_voxel_iso_is_a_hexagon_with_three_shades() {
        let mut grid = VoxelGrid::new(Scale::Prop, [1, 1, 1]).unwrap();
        grid.set([0, 0, 0], 1).unwrap();
        let img = view(&grid, &palette(), View::Iso, 2).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        let covered = (0..16).filter(|i| img.rgba[i * 4 + 3] == 255).count();
        assert_eq!(covered, 12);
        // Left-lower pixel is on the +z face (60%), upper-right on the top face (100%).
        assert_eq!(pixel(&img, 0, 1), [54, 30, 12, 255]);
        assert_eq!(pixel(&img, 1, 1), WOOD_1);
    }

    #[test]
    fn contact_sheet_of_the_stool_is_deterministic() {
        let recipe = Recipe::from_json(STOOL).unwrap();
        let grid = eval(&recipe, &palette()).unwrap().grid;
        let first = contact_sheet(&grid, &palette(), 4).unwrap();
        let again = contact_sheet(&grid, &palette(), 4).unwrap();
        assert_eq!(first, again);
        // The dimetric view is the tallest part: its span is 2s(sy) + s(sx + sz) half-pixels.
        assert_eq!(first.height, 64);
        assert_eq!(hash8(&first), "30fd0bf8b7af544d");
    }

    #[test]
    fn oversize_and_zero_pixel_requests_are_refused() {
        let grid = VoxelGrid::new(Scale::Prop, [64, 1, 1]).unwrap();
        assert!(view(&grid, &palette(), View::Front, 65).is_err());
        assert!(view(&grid, &palette(), View::Front, 0).is_err());
        assert!(contact_sheet(&grid, &palette(), 65).is_err());
        assert!(contact_sheet(&grid, &palette(), 0).is_err());
    }

    #[test]
    fn a_sheet_too_wide_is_refused_though_each_part_fits() {
        // Front, Top and Iso are each under MAX_SIDE wide at 40 pixels a voxel, but the four
        // side by side are not.
        let grid = VoxelGrid::new(Scale::Prop, [64, 1, 1]).unwrap();
        assert!(view(&grid, &palette(), View::Front, 40).is_ok());
        assert!(view(&grid, &palette(), View::Iso, 40).is_ok());
        assert!(contact_sheet(&grid, &palette(), 40).is_err());
    }

    #[test]
    fn png_output_starts_with_the_signature() {
        let img = view(&small_grid(), &palette(), View::Front, 1).unwrap();
        let bytes = encode_png(&img).unwrap();
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }
}
