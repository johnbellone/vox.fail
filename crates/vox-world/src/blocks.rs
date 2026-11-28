//! Building blocks with real textures, and levels built from them.
//!
//! - **Blocks** (`data/blocks.json`): grass, dirt, stone, cobblestone, bricks, planks, log, sand,
//!   gravel, snow, leaves, glass, concrete, metal, tile and lamp. Each names a texture tile for
//!   its top, sides and bottom, plus how rough or metallic it is, how deep its bumps are, and
//!   whether it's see-through (glass) or glows (lamp).
//! - **Tiles** (`assets/blocks/tiles/`): a 128 x 128 colour PNG per tile, a `.height.png` for its
//!   bumps and, for lamps, a `.glow.png`. `tools/assets/build_blocks.py` draws the built-in ones; any
//!   tile can be repainted (keep it wrapping). At start they're packed into one atlas, with the
//!   bumps turned into a normal map and the roughness into a roughness map.
//! - **Levels** (`data/levels/<name>.json`, or `levels/` in your files folder): boxes of blocks.
//!   A level is a set like the built-in ones: `set:yard`.
//!
//! The mesh draws only faces that can be seen, shades the corners where blocks meet (like light
//! that can't get into a corner), and keeps glass in its own see-through mesh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat},
};
use serde::Deserialize;

/// Tile size in pixels, and the wrapped border around each tile in the atlas (so mipmaps and
/// filtering don't pick up the neighbouring tile).
pub const TILE: usize = 128;
pub const PAD: usize = 8;
const CELL: usize = TILE + 2 * PAD;
const COLS: usize = 8;
/// mip levels: down to 8 x 8 per tile, where the border is still half a pixel
const MIPS: u32 = 5;

#[derive(Deserialize, Clone, Debug)]
pub struct BlockDef {
    pub name: String,
    pub top: String,
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub bottom: Option<String>,
    /// the sides when another block sits on top (grass under grass shows dirt, not a second
    /// grass edge)
    #[serde(default)]
    pub side_covered: Option<String>,
    #[serde(default = "rough")]
    pub roughness: f32,
    #[serde(default)]
    pub metallic: f32,
    /// how deep the bumps look, 1 = normal
    #[serde(default = "one")]
    pub bump: f32,
    #[serde(default)]
    pub glass: bool,
    /// water: see-through, walked through, drawn with the moving water shader
    #[serde(default)]
    pub liquid: bool,
    /// fire: no block, just flames (drawn by the flame shader); walked through
    #[serde(default)]
    pub flame: bool,
    /// light given off where the tile's glow map is white
    #[serde(default)]
    pub glow: f32,
    /// how far its top dips, 0 (flat: built things) to 1 (the deepest dip,
    /// `vox_core::relief::MAX_DIP`), where the world has relief (`docs/design/ground-relief.md`)
    #[serde(default)]
    pub relief: f32,
    /// how much rain it drinks, 0-1: what it doesn't stands in puddles in its dips
    #[serde(default)]
    pub soak: f32,
}

fn rough() -> f32 {
    0.85
}
fn one() -> f32 {
    1.0
}

impl BlockDef {
    pub fn side(&self) -> &str {
        self.side.as_deref().unwrap_or(&self.top)
    }
    pub fn bottom(&self) -> &str {
        self.bottom.as_deref().unwrap_or(&self.top)
    }
}

#[derive(Deserialize)]
struct BlocksFile {
    block_size: f32,
    blocks: Vec<BlockDef>,
}

/// The block types, by index (0 is "air").
pub struct Palette {
    pub size: f32,
    pub defs: Vec<BlockDef>,
}

impl Palette {
    pub fn load(path: &Path) -> Result<Palette, String> {
        let text = crate::files::read_string(path).ok_or_else(|| format!("{}: can't read it", path.display()))?;
        let f: BlocksFile = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        // every size measured in blocks is worked out for vox_core::BLOCK_M
        let size = vox_core::block_size(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        debug_assert_eq!(size, f.block_size);
        Ok(Palette { size, defs: f.blocks })
    }
    pub fn id(&self, name: &str) -> Option<u16> {
        self.defs.iter().position(|d| d.name == name).map(|i| i as u16 + 1)
    }
    pub fn def(&self, id: u16) -> Option<&BlockDef> {
        self.defs.get((id as usize).checked_sub(1)?)
    }
    /// Every tile name used, in a fixed order.
    pub fn tiles(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for d in &self.defs {
            for t in [d.top.as_str(), d.side(), d.bottom()].into_iter().chain(d.side_covered.as_deref()) {
                if !out.iter().any(|o| o == t) {
                    out.push(t.to_string());
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------
// The atlas
// ------------------------------------------------------------------------------------------

/// One tile's pixels: colour (RGBA 0-255), height (0-1) and glow (0-1).
pub struct TilePixels {
    pub rgba: Vec<[u8; 4]>,
    pub height: Vec<f32>,
    pub glow: Vec<f32>,
}

fn read_png(path: &Path) -> Option<(usize, usize, Vec<[u8; 4]>)> {
    let bytes = crate::files::read(path)?;
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let px: Vec<[u8; 4]> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()].chunks(2).map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        _ => return None,
    };
    Some((info.width as usize, info.height as usize, px))
}

/// A tile from disk; a missing height map is taken from the colour's brightness.
pub fn load_tile(dir: &Path, name: &str) -> Result<TilePixels, String> {
    let (w, h, rgba) = read_png(&dir.join(format!("{name}.png"))).ok_or_else(|| format!("tile {name}: can't read {name}.png"))?;
    if w != TILE || h != TILE {
        return Err(format!("tile {name}: {w} x {h}, should be {TILE} x {TILE}"));
    }
    let grey = |p: &[u8; 4]| (p[0] as f32 * 0.3 + p[1] as f32 * 0.59 + p[2] as f32 * 0.11) / 255.0;
    let height = match read_png(&dir.join(format!("{name}.height.png"))) {
        Some((TILE, TILE, px)) => px.iter().map(grey).collect(),
        _ => rgba.iter().map(grey).collect(),
    };
    let glow = match read_png(&dir.join(format!("{name}.glow.png"))) {
        Some((TILE, TILE, px)) => px.iter().map(grey).collect(),
        _ => vec![0.0; TILE * TILE],
    };
    Ok(TilePixels { rgba, height, glow })
}

/// The packed atlas: four RGBA8 images with all mip levels (colour, normals, roughness and
/// metal, glow), and where each tile sits.
pub struct Atlas {
    pub width: usize,
    pub height: usize,
    pub colour: Vec<u8>,
    pub normal: Vec<u8>,
    pub rough: Vec<u8>,
    pub glow: Vec<u8>,
    /// the tiles' height maps (in red), for the world's parallax shader
    pub heights: Vec<u8>,
    /// tile name -> its rectangle in UV (u0, v0, u1, v1), inset half a pixel
    pub rects: HashMap<String, [f32; 4]>,
}

/// Which block a tile belongs to first (for its roughness, metal and bump depth).
fn owner<'a>(pal: &'a Palette, tile: &str) -> Option<&'a BlockDef> {
    pal.defs.iter().find(|d| d.top == tile || d.side() == tile || d.bottom() == tile)
}

pub fn build_atlas(pal: &Palette, tiles: &[(String, TilePixels)]) -> Atlas {
    let rows = tiles.len().div_ceil(COLS).max(1);
    let (w, h) = (COLS * CELL, rows * CELL);
    let mut colour = vec![[0.0f32; 4]; w * h];
    let mut normal = vec![[0.5f32, 0.5, 1.0, 1.0]; w * h];
    let mut mr = vec![[0.0f32, 0.9, 0.0, 1.0]; w * h];
    let mut glow = vec![[0.0f32; 4]; w * h];
    let mut height = vec![[1.0f32, 0.0, 0.0, 1.0]; w * h];
    let mut rects = HashMap::new();
    for (i, (name, t)) in tiles.iter().enumerate() {
        let def = owner(pal, name);
        let bump = def.map(|d| d.bump).unwrap_or(1.0);
        // relief belongs to tops: the block whose top this tile is
        let top_of = pal.defs.iter().find(|d| d.top == *name);
        let relief = top_of.map_or(0.0, |d| d.relief.clamp(0.0, 1.0));
        // what doesn't drink the rain stands in puddles (built things: all of it)
        let soak = top_of.map_or(0.0, |d| d.soak.clamp(0.0, 1.0));
        let (rough, metal) = def.map(|d| (d.roughness, d.metallic)).unwrap_or((0.85, 0.0));
        let (cx, cy) = ((i % COLS) * CELL, (i / COLS) * CELL);
        let at = |x: isize, y: isize| -> usize { (y.rem_euclid(TILE as isize) as usize) * TILE + x.rem_euclid(TILE as isize) as usize };
        // bump strength: a height step of 1 across 2 pixels tilts the normal this much
        let k = 2.2 * bump;
        for y in -(PAD as isize)..(TILE + PAD) as isize {
            for x in -(PAD as isize)..(TILE + PAD) as isize {
                let s = at(x, y);
                let o = (cy as isize + PAD as isize + y) as usize * w + (cx as isize + PAD as isize + x) as usize;
                let p = t.rgba[s];
                // colour in linear light, so the mips average what the eye sees
                let lin = |c: u8| (c as f32 / 255.0).powf(2.2);
                colour[o] = [lin(p[0]), lin(p[1]), lin(p[2]), p[3] as f32 / 255.0];
                // the normal leans away from rising ground; +x is +u, +y is +v (down the image)
                let dx = (t.height[at(x + 1, y)] - t.height[at(x - 1, y)]) * k;
                let dy = (t.height[at(x, y + 1)] - t.height[at(x, y - 1)]) * k;
                let n = Vec3::new(-dx, -dy, 1.0).normalize();
                normal[o] = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0];
                // crevices are rougher than the tops
                let r = (rough * (0.85 + 0.3 * (1.0 - t.height[s]))).clamp(0.04, 1.0);
                mr[o] = [0.0, r, metal, 1.0];
                let g = t.glow[s];
                glow[o] = [lin(p[0]) * g, lin(p[1]) * g, lin(p[2]) * g, 1.0];
                // green: how deep this tile's parallax goes (its bump, capped); blue: how far the
                // ground dips where this is a block's top (its relief); alpha: how much rain it
                // drinks (its soak)
                height[o] = [t.height[s], (bump / 1.5).clamp(0.0, 1.0), relief, soak];
            }
        }
        let (u0, v0) = ((cx + PAD) as f32 + 0.5, (cy + PAD) as f32 + 0.5);
        rects.insert(name.clone(), [u0 / w as f32, v0 / h as f32, (u0 + TILE as f32 - 1.0) / w as f32, (v0 + TILE as f32 - 1.0) / h as f32]);
    }
    let srgb = |v: f32| ((v.max(0.0).powf(1.0 / 2.2)) * 255.0 + 0.5).min(255.0) as u8;
    let unorm = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Atlas {
        width: w,
        height: h,
        colour: mips(colour, w, h, |p| [srgb(p[0]), srgb(p[1]), srgb(p[2]), unorm(p[3])], false),
        normal: mips(normal, w, h, |p| [unorm(p[0]), unorm(p[1]), unorm(p[2]), 255], true),
        rough: mips(mr, w, h, |p| [0, unorm(p[1]), unorm(p[2]), 255], false),
        glow: mips(glow, w, h, |p| [srgb(p[0]), srgb(p[1]), srgb(p[2]), 255], false),
        heights: mips(height, w, h, |p| [unorm(p[0]), unorm(p[1]), unorm(p[2]), unorm(p[3])], false),
        rects,
    }
}

/// All mip levels, box-filtered; normals are renormalised at each level.
fn mips(mut level: Vec<[f32; 4]>, mut w: usize, mut h: usize, enc: impl Fn([f32; 4]) -> [u8; 4], normal: bool) -> Vec<u8> {
    let mut out = vec![];
    for m in 0..MIPS {
        out.extend(level.iter().flat_map(|p| enc(*p)));
        if m + 1 == MIPS {
            break;
        }
        let (nw, nh) = (w / 2, h / 2);
        let mut next = vec![[0.0f32; 4]; nw * nh];
        for y in 0..nh {
            for x in 0..nw {
                let mut s = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = level[(y * 2 + dy) * w + x * 2 + dx];
                    for c in 0..4 {
                        s[c] += p[c] * 0.25;
                    }
                }
                if normal {
                    let n = Vec3::new(s[0] * 2.0 - 1.0, s[1] * 2.0 - 1.0, s[2] * 2.0 - 1.0).normalize_or(Vec3::Z);
                    s = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0];
                }
                next[y * nw + x] = s;
            }
        }
        level = next;
        w = nw;
        h = nh;
    }
    out
}

fn image(bytes: Vec<u8>, w: usize, h: usize, format: TextureFormat) -> Image {
    let mut img = Image::new_uninit(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
    img.data = Some(bytes);
    img.texture_descriptor.mip_level_count = MIPS;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}

// ------------------------------------------------------------------------------------------
// Levels
// ------------------------------------------------------------------------------------------

/// A level's blocks: position (in blocks; block (x, y, z) spans x..x+1 times the block size)
/// -> block id. Later boxes overwrite earlier ones; "air" removes. (The rules are
/// vox-core's, so a world server reads levels the same way.)
pub fn load_level(text: &str, pal: &Palette) -> Result<HashMap<IVec3, u16>, String> {
    let names: Vec<String> = pal.defs.iter().map(|d| d.name.clone()).collect();
    Ok(vox_core::level::load(text, &names)?.into_iter().map(|(c, b)| (IVec3::from(c), b)).collect())
}

/// A level's blocks inside `within` (or all of them), held as chunks rather than a map of
/// every block: at quarter-metre blocks a whole level can be tens of millions of blocks
/// (docs/design/chunk-streaming.md). Later boxes overwrite earlier ones; "air" removes.
pub fn load_level_world(text: &str, pal: &Palette, within: Option<vox_core::Bounds>) -> Result<vox_core::World, String> {
    let names: Vec<String> = pal.defs.iter().map(|d| d.name.clone()).collect();
    let region = within.unwrap_or_else(vox_core::level::everywhere);
    let mut world = vox_core::World::new(region, names.len() as u16);
    vox_core::level::apply(text, &names, region, |c, b| {
        world.set(c, b);
    })?;
    Ok(world)
}

/// Where a level's file is: your own levels folder first, then the built-in ones.
pub fn level_path(name: &str) -> Option<PathBuf> {
    // a dungeon's level (what a portal into one shows) is in its own folder; nothing else may
    // name a folder
    let (folder, rest) = match name.strip_prefix("dungeons/") {
        Some(rest) => ("dungeons/", rest),
        None => ("", name),
    };
    let safe: String = rest.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    if safe.is_empty() || safe != rest {
        return None;
    }
    [crate::files::user_dir().join("levels"), crate::files::base().join("data/levels")]
        .into_iter()
        .map(|d| d.join(format!("{folder}{safe}.json")))
        .find(|p| crate::files::exists(p))
}

// ------------------------------------------------------------------------------------------
// Meshing
// ------------------------------------------------------------------------------------------

/// The six faces: outward normal, the direction of +u on the texture, and of +v (down the
/// image). Sides stand upright (texture up = +y).
const FACES: [(IVec3, Vec3, Vec3); 6] = [
    (IVec3::X, Vec3::NEG_Z, Vec3::NEG_Y),
    (IVec3::NEG_X, Vec3::Z, Vec3::NEG_Y),
    (IVec3::Z, Vec3::X, Vec3::NEG_Y),
    (IVec3::NEG_Z, Vec3::NEG_X, Vec3::NEG_Y),
    (IVec3::Y, Vec3::X, Vec3::Z),
    (IVec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
];

/// World chunks' packed vertex (20 bytes instead of 64): the position as usual (shadows and
/// culling use it), the corner of the face's tile in the atlas as two 16-bit fractions, and one
/// number holding the face (which of the six, so the shader can rebuild the normal and
/// tangent), the corner's shading, and where the vertex is across the face in blocks (a merged
/// face spans several; the fragment shader repeats the tile across it). See
/// `shaders/block_vertex.wesl` and `shaders/block.wesl`.
pub const ATTRIBUTE_BLOCK_UV: bevy::mesh::MeshVertexAttribute =
    bevy::mesh::MeshVertexAttribute::new("BlockUv", 0x766f_7801, bevy::mesh::VertexFormat::Unorm16x2);
/// Bits 0-2: the face (the order of `FACES`); bits 8-15: shading, 255 = full light; bits
/// 16-21 and 22-27: the vertex's place across the face along u and v, in blocks.
pub const ATTRIBUTE_BLOCK_FACE: bevy::mesh::MeshVertexAttribute =
    bevy::mesh::MeshVertexAttribute::new("BlockFace", 0x766f_7802, bevy::mesh::VertexFormat::Uint32);

/// Which of `FACES` a face normal is.
fn face_index(n: [f32; 3]) -> u32 {
    match (n[0].round() as i32, n[1].round() as i32, n[2].round() as i32) {
        (1, 0, 0) => 0,
        (-1, 0, 0) => 1,
        (0, 0, 1) => 2,
        (0, 0, -1) => 3,
        (0, 1, 0) => 4,
        _ => 5,
    }
}

/// A face's packed number: its face and its shading.
pub fn pack_face(normal: [f32; 3], shade: f32) -> u32 {
    face_index(normal) | ((shade.clamp(0.0, 1.0) * 255.0).round() as u32) << 8
}

#[derive(Default)]
pub struct MeshData {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub tan: Vec<[f32; 4]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
    /// for the packed form: where the vertex is across the face, in blocks (0 to 32: a merged
    /// face spans several), and the corner of its tile in the atlas
    pub local: Vec<[u8; 2]>,
    pub tile: Vec<[f32; 2]>,
}

impl MeshData {
    pub fn faces(&self) -> usize {
        self.idx.len() / 6
    }
    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tan)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx))
    }

    /// The packed form for the world's solid blocks ([`ATTRIBUTE_BLOCK_UV`],
    /// [`ATTRIBUTE_BLOCK_FACE`]), drawn with [`BlockMaterial`]. Only for axis-aligned block faces.
    pub fn into_packed_mesh(self) -> Mesh {
        let tile: Vec<[u16; 2]> =
            self.tile.iter().map(|[u, v]| [(u.clamp(0.0, 1.0) * 65535.0).round() as u16, (v.clamp(0.0, 1.0) * 65535.0).round() as u16]).collect();
        let face: Vec<u32> = self
            .nrm
            .iter()
            .zip(&self.col)
            .zip(&self.local)
            .map(|((n, c), l)| pack_face(*n, c[0]) | (l[0].min(63) as u32) << 16 | (l[1].min(63) as u32) << 22)
            .collect();
        // the GPU's copy is the only one: nothing reads a chunk's mesh back (the caller sets
        // its bounds, see `bounds`)
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(ATTRIBUTE_BLOCK_UV, bevy::mesh::VertexAttributeValues::Unorm16x2(tile))
            .with_inserted_attribute(ATTRIBUTE_BLOCK_FACE, face)
            .with_inserted_indices(Indices::U32(self.idx))
    }

    /// The box round every vertex (for culling a mesh whose data lives only on the GPU).
    pub fn bounds(&self) -> Option<bevy::camera::primitives::Aabb> {
        let mut it = self.pos.iter().map(|p| Vec3::from(*p));
        let first = it.next()?;
        let (lo, hi) = it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Some(bevy::camera::primitives::Aabb::from_min_max(lo, hi))
    }
}

/// The level as two meshes: solid blocks, and glass. Only faces that can be seen are drawn; a
/// corner where blocks meet is shaded darker.
/// Two crossed quads through a block, each both ways round, a block and a half tall; uv runs
/// 0-1 across and 0 (bottom) to 1 (top); the colour's red is a random seed per fire.
fn flame_quads(out: &mut MeshData, p: IVec3, s: f32) {
    let seed = {
        let h = (p.x as u32).wrapping_mul(73856093) ^ (p.y as u32).wrapping_mul(19349663) ^ (p.z as u32).wrapping_mul(83492791);
        (h % 1000) as f32 / 1000.0
    };
    let o = p.as_vec3() * s;
    let (y0, y1) = (o.y, o.y + 1.45 * s);
    for (a, b) in [(Vec2::new(0.1, 0.1), Vec2::new(0.9, 0.9)), (Vec2::new(0.1, 0.9), Vec2::new(0.9, 0.1))] {
        let (a, b) = (Vec3::new(o.x + a.x * s, 0.0, o.z + a.y * s), Vec3::new(o.x + b.x * s, 0.0, o.z + b.y * s));
        let along = (b - a).normalize();
        let nrm = along.cross(Vec3::Y);
        for flip in [false, true] {
            let base = out.pos.len() as u32;
            for (x, y, u, v) in [(a, y0, 0.0, 0.0), (b, y0, 1.0, 0.0), (b, y1, 1.0, 1.0), (a, y1, 0.0, 1.0)] {
                out.pos.push([x.x, y, x.z]);
                out.nrm.push(if flip { (-nrm).to_array() } else { nrm.to_array() });
                out.uv.push([u, v]);
                out.tan.push([along.x, along.y, along.z, 1.0]);
                out.col.push([seed, 1.0, 1.0, 1.0]);
            }
            if flip {
                out.idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            } else {
                out.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
}

/// A block mesh in four parts: solid blocks, glass, water and flames.
#[derive(Default)]
pub struct Meshes {
    pub opaque: MeshData,
    pub glass: MeshData,
    pub water: MeshData,
    /// crossed quads for flames
    pub flame: MeshData,
}

pub fn mesh_level(cells: &HashMap<IVec3, u16>, pal: &Palette, rects: &HashMap<String, [f32; 4]>) -> Meshes {
    let mut keys: Vec<(IVec3, u16)> = cells.iter().map(|(p, b)| (*p, *b)).collect();
    keys.sort_by_key(|(p, _)| (p.y, p.z, p.x));
    mesh_cells(keys, |p| cells.get(&p).copied().unwrap_or(0), pal, rects, false)
}

/// [`mesh_level`] for a level held as chunks ([`load_level_world`]).
pub fn mesh_world(world: &vox_core::World, pal: &Palette, rects: &HashMap<String, [f32; 4]>) -> Meshes {
    let mut keys = world.keys();
    keys.sort_by_key(|k| (k.0[1], k.0[2], k.0[0]));
    let n = vox_core::CHUNK;
    let cells = keys.into_iter().filter_map(|k| world.chunk(k).map(|c| (k.origin(), c))).flat_map(move |(o, c)| {
        (0..n).flat_map(move |y| (0..n).flat_map(move |z| (0..n).map(move |x| [o[0] + x, o[1] + y, o[2] + z])))
            .filter_map(move |p| match c.get(p) {
                0 => None,
                id => Some((IVec3::from(p), id)),
            })
    });
    mesh_cells(cells, |p| world.get(p.to_array()), pal, rects, false)
}

/// [`mesh_level`] for some of a bigger world's blocks (a chunk): `cells` are the blocks to
/// draw, `get` reads any block (0 = air), so faces and corner shading at the edges see the
/// neighbours. With `merge`, neighbouring solid faces with the same tile and even light
/// become one bigger face (for [`MeshData::into_packed_mesh`] and [`BlockMaterial`], whose
/// shader repeats the tile across it); without, every face is its own (for the standard
/// material).
pub fn mesh_cells(
    cells: impl IntoIterator<Item = (IVec3, u16)>,
    get: impl Fn(IVec3) -> u16,
    pal: &Palette,
    rects: &HashMap<String, [f32; 4]>,
    merge: bool,
) -> Meshes {
    // see-through: glass and water
    let clear = |id: u16| pal.def(id).is_some_and(|d| d.glass || d.liquid || d.flame);
    let liquid = |id: u16| pal.def(id).is_some_and(|d| d.liquid);
    let solid_at = |p: IVec3| {
        let b = get(p);
        b != 0 && !clear(b)
    };
    let mut m = Meshes::default();
    let s = pal.size;
    // faces waiting to be merged: (face, slice) -> (u, v) across it -> (tile, shade)
    let mut flat: HashMap<(usize, i32), HashMap<(i32, i32), ([f32; 4], f32)>> = HashMap::new();
    for (p, id) in cells {
        let Some(def) = pal.def(id) else { continue };
        if def.flame {
            flame_quads(&mut m.flame, p, s);
            continue;
        }
        for (fi, (dir, udir, vdir)) in FACES.into_iter().enumerate() {
            let n = p + dir;
            // hidden behind a solid block, or glass next to the same glass
            match get(n) {
                0 => {}
                nb if !clear(nb) => continue,
                nb if nb == id => continue,
                _ => {}
            }
            let tile = match dir {
                IVec3::Y => def.top.as_str(),
                IVec3::NEG_Y => def.bottom(),
                _ => match &def.side_covered {
                    Some(t) if solid_at(p + IVec3::Y) => t.as_str(),
                    _ => def.side(),
                },
            };
            let Some(r) = rects.get(tile).copied() else { continue };
            // the top of a pool sits a little below the block's top
            let surface = def.liquid && !liquid(get(p + IVec3::Y));
            let nrm = dir.as_vec3();
            // corner shading: the blocks in front of the face around each corner
            let mut ao = [0.0f32; 4];
            for (k, (cu, cv)) in CORNERS.into_iter().enumerate() {
                let side1 = n + (udir * cu).round().as_ivec3();
                let side2 = n + (vdir * cv).round().as_ivec3();
                let diag = n + (udir * cu + vdir * cv).round().as_ivec3();
                let (a, b, c) = (solid_at(side1), solid_at(side2), solid_at(diag));
                let occ = if a && b { 3 } else { a as u8 + b as u8 + c as u8 };
                ao[k] = [1.0, 0.82, 0.68, 0.55][occ as usize];
            }
            let plain = !def.liquid && !def.glass;
            if merge && plain && ao.iter().all(|a| *a == ao[0]) {
                // a candidate for merging: by its slice, and where it is across the face
                let (u, v) = (p.as_vec3().dot(udir).round() as i32, p.as_vec3().dot(vdir).round() as i32);
                flat.entry((fi, p.as_vec3().dot(nrm).round() as i32)).or_default().insert((u, v), (r, ao[0]));
                continue;
            }
            let out = if def.liquid {
                &mut m.water
            } else if def.glass {
                &mut m.glass
            } else {
                &mut m.opaque
            };
            let top = (p.y as f32 + 1.0) * s;
            let centre = (p.as_vec3() + Vec3::splat(0.5) + nrm * 0.5) * s;
            let corners = CORNERS.map(|(cu, cv)| {
                let mut c = centre + (udir * cu + vdir * cv) * (s * 0.5);
                if surface && (c.y - top).abs() < 1e-4 {
                    c.y -= 0.12 * s;
                }
                c
            });
            quad(out, corners, (dir, udir, vdir), r, [1, 1], ao);
        }
    }
    // merge: the greedy way, row by row, each face as wide and then as tall as it can go
    for ((fi, slice), mut faces) in flat {
        let (dir, udir, vdir) = FACES[fi];
        let nrm = dir.as_vec3();
        let mut keys: Vec<(i32, i32)> = faces.keys().copied().collect();
        keys.sort_by_key(|(u, v)| (*v, *u));
        for (u0, v0) in keys {
            let Some(&(r, shade)) = faces.get(&(u0, v0)) else { continue };
            let same = |f: Option<&([f32; 4], f32)>| f.is_some_and(|(r2, s2)| *r2 == r && *s2 == shade);
            let mut w = 1;
            while w < 32 && same(faces.get(&(u0 + w, v0))) {
                w += 1;
            }
            let mut h = 1;
            'grow: while h < 32 {
                for du in 0..w {
                    if !same(faces.get(&(u0 + du, v0 + h))) {
                        break 'grow;
                    }
                }
                h += 1;
            }
            for dv in 0..h {
                for du in 0..w {
                    faces.remove(&(u0 + du, v0 + dv));
                }
            }
            // the block at (u, v) in this slice, and a corner of it
            let block = |u: i32, v: i32| nrm * slice as f32 + udir * u as f32 + vdir * v as f32;
            let corner = |b: Vec3, cu: f32, cv: f32| (b + Vec3::splat(0.5) + nrm * 0.5 + (udir * cu + vdir * cv) * 0.5) * s;
            let (u1, v1) = (u0 + w - 1, v0 + h - 1);
            let corners = [
                corner(block(u0, v0), -1.0, -1.0),
                corner(block(u1, v0), 1.0, -1.0),
                corner(block(u1, v1), 1.0, 1.0),
                corner(block(u0, v1), -1.0, 1.0),
            ];
            quad(&mut m.opaque, corners, (dir, udir, vdir), r, [w as u8, h as u8], [shade; 4]);
        }
    }
    m
}

/// A face's corners, as (u, v) signs: they go round the face in this order.
const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];

/// One face (`size` blocks across along u and v) into `out`: its corners in [`CORNERS`] order,
/// counter-clockwise seen from outside, split along the brighter diagonal.
fn quad(out: &mut MeshData, corners: [Vec3; 4], (dir, udir, vdir): (IVec3, Vec3, Vec3), r: [f32; 4], size: [u8; 2], ao: [f32; 4]) {
    let nrm = dir.as_vec3();
    let base = out.pos.len() as u32;
    let w = if nrm.cross(udir).dot(vdir) >= 0.0 { 1.0 } else { -1.0 };
    for (k, (cu, cv)) in CORNERS.into_iter().enumerate() {
        out.pos.push(corners[k].to_array());
        out.nrm.push(nrm.to_array());
        out.tan.push([udir.x, udir.y, udir.z, w]);
        out.uv.push([if cu < 0.0 { r[0] } else { r[2] }, if cv < 0.0 { r[1] } else { r[3] }]);
        out.col.push([ao[k], ao[k], ao[k], 1.0]);
        out.local.push([if cu < 0.0 { 0 } else { size[0] }, if cv < 0.0 { 0 } else { size[1] }]);
        out.tile.push([r[0], r[1]]);
    }
    // split the square along the brighter diagonal, so the shading doesn't zigzag
    if ao[0] + ao[2] >= ao[1] + ao[3] {
        out.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        out.idx.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
    }
    // counter-clockwise seen from outside
    let (a, b, c) = (corners[0], corners[1], corners[2]);
    if (b - a).cross(c - a).dot(nrm) < 0.0 {
        let l = out.idx.len();
        out.idx[l - 6..].chunks_mut(3).for_each(|t| t.swap(1, 2));
    }
}

// ------------------------------------------------------------------------------------------
// In the app
// ------------------------------------------------------------------------------------------

/// The blocks, their atlas and materials (built once at start).
#[derive(Resource)]
pub struct Blocks {
    pub palette: Option<Palette>,
    pub rects: HashMap<String, [f32; 4]>,
    pub solid: Handle<StandardMaterial>,
    pub glass: Handle<StandardMaterial>,
    /// the world's blocks: the same textures with parallax (crevices that look deep)
    pub world_solid: Handle<BlockMaterial>,
    pub water: Handle<WaterMaterial>,
    pub flame: Handle<FlameMaterial>,
    pub status: String,
}

/// A level standing in as the set.
#[derive(Component)]
pub struct LevelRoot(pub String);

pub fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut block_mats: ResMut<Assets<BlockMaterial>>,
    mut water_mats: ResMut<Assets<WaterMaterial>>,
    mut flame_mats: ResMut<Assets<FlameMaterial>>,
) {
    let base = crate::files::base();
    let made = (|| -> Result<(Palette, Atlas), String> {
        let pal = Palette::load(&base.join("data/blocks.json"))?;
        let dir = base.join("assets/blocks/tiles");
        let tiles = pal.tiles().into_iter().map(|t| load_tile(&dir, &t).map(|px| (t, px))).collect::<Result<Vec<_>, _>>()?;
        let atlas = build_atlas(&pal, &tiles);
        Ok((pal, atlas))
    })();
    let (palette, rects, solid, glass, world_solid, water, status) = match made {
        Ok((pal, a)) => {
            let (w, h) = (a.width, a.height);
            let colour = images.add(image(a.colour, w, h, TextureFormat::Rgba8UnormSrgb));
            let normal = images.add(image(a.normal, w, h, TextureFormat::Rgba8Unorm));
            let rough = images.add(image(a.rough, w, h, TextureFormat::Rgba8Unorm));
            let glow = images.add(image(a.glow, w, h, TextureFormat::Rgba8UnormSrgb));
            let height = images.add(image(a.heights, w, h, TextureFormat::Rgba8Unorm));
            let glow_k = pal.defs.iter().map(|d| d.glow).fold(0.0, f32::max).max(1.0);
            let solid = mats.add(StandardMaterial {
                base_color_texture: Some(colour.clone()),
                normal_map_texture: Some(normal.clone()),
                metallic_roughness_texture: Some(rough.clone()),
                perceptual_roughness: 1.0,
                metallic: 1.0,
                emissive_texture: Some(glow),
                emissive: LinearRgba::rgb(glow_k, glow_k, glow_k),
                ..default()
            });
            let world_solid = block_mats.add(BlockMaterial {
                base: mats.get(&solid).cloned().unwrap_or_default(),
                extension: BlockExt {
                    // crevices up to 7% of a block deep, 12 steps
                    params: Vec4::new(0.09, 14.0, TILE as f32, 0.0),
                    atlas: Vec4::new(w as f32, h as f32, CELL as f32, PAD as f32),
                    height,
                    // none until a world asks for it (`outdoors::relief_to_blocks`)
                    relief: Vec4::new(0.0, pal.size, 0.0, 0.0),
                    relief_at: UVec4::ZERO,
                    water: Vec4::ZERO,
                },
            });
            let water = water_mats.add(WaterMaterial {
                base: StandardMaterial {
                    base_color_texture: Some(colour.clone()),
                    base_color: Color::srgba(WATER_TINT[0], WATER_TINT[1], WATER_TINT[2], 1.0),
                    perceptual_roughness: 0.04,
                    reflectance: 0.5,
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                },
                extension: WaterExt { waves: Vec4::new(1.0, 1.0, 0.0, 0.0), swell: Vec4::new(0.0, 1.0, WATER_OPACITY, pal.size) },
            });
            let glass = mats.add(StandardMaterial {
                base_color_texture: Some(colour),
                normal_map_texture: Some(normal),
                metallic_roughness_texture: Some(rough),
                perceptual_roughness: 1.0,
                metallic: 1.0,
                alpha_mode: AlphaMode::Blend,
                reflectance: 0.6,
                ..default()
            });
            let n = pal.defs.len();
            (Some(pal), a.rects, solid, glass, world_solid, water, format!("{n} blocks"))
        }
        Err(e) => {
            crate::note!("[blocks] {e}");
            (None, HashMap::new(), Handle::default(), Handle::default(), Handle::default(), Handle::default(), format!("Blocks unavailable: {e}"))
        }
    };
    let flame = flame_mats.add(FlameMaterial { params: Vec4::new(3.2, 0.0, 0.0, 0.0) });
    commands.insert_resource(Blocks { palette, rects, solid, glass, world_solid, water, flame, status });
}

// ------------------------------------------------------------------------------------------
// Shaders for the world's blocks and water
// ------------------------------------------------------------------------------------------

/// Block faces with parallax occlusion mapping: the texture is shifted along the view by its
/// height map, so mortar, cracks and planks' gaps look sunken (`shaders/block.wesl`).
pub type BlockMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, BlockExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct BlockExt {
    /// depth (fraction of a block face), steps, tile size in pixels
    #[uniform(100)]
    pub params: Vec4,
    /// atlas width and height, cell and border size, in pixels
    #[uniform(101)]
    pub atlas: Vec4,
    #[texture(102)]
    #[sampler(103)]
    pub height: Handle<Image>,
    /// ground relief (`vox_core::relief`): x the deepest dip in blocks (0: none in this world),
    /// y the block size in metres
    #[uniform(104)]
    pub relief: Vec4,
    /// ground relief: the world's seed, its origin on the map (x, z blocks, as two's-complement
    /// bits) and the function's version
    #[uniform(105)]
    pub relief_at: UVec4,
    /// water on the ground where you are: how full the dips are, how wet the ground is, 1 when
    /// frozen, the rain falling (mm/h)
    #[uniform(106)]
    pub water: Vec4,
}

impl bevy::pbr::MaterialExtension for BlockExt {
    fn vertex_shader() -> bevy::shader::ShaderRef {
        "shaders/block_vertex.wesl".into()
    }

    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/block.wesl".into()
    }

    /// The main pass reads the packed vertex and tells the fragment shader it has normals,
    /// tangents, coordinates and colours (rebuilt by the vertex shader). Shadows and other
    /// prepasses keep the standard layout: they only need the position.
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        let prepass = descriptor.vertex.shader_defs.iter().any(|d| *d == "PREPASS_PIPELINE".into());
        if prepass || !layout.0.contains(ATTRIBUTE_BLOCK_FACE) {
            return Ok(());
        }
        let vertex = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_BLOCK_UV.at_shader_location(1),
            ATTRIBUTE_BLOCK_FACE.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex];
        for def in ["VERTEX_NORMALS", "VERTEX_UVS", "VERTEX_UVS_A", "VERTEX_UVS_B", "VERTEX_TANGENTS", "VERTEX_COLORS"] {
            let d: bevy::shader::ShaderDefVal = def.into();
            if !descriptor.vertex.shader_defs.contains(&d) {
                descriptor.vertex.shader_defs.push(d.clone());
            }
            if let Some(f) = descriptor.fragment.as_mut() {
                if !f.shader_defs.contains(&d) {
                    f.shader_defs.push(d);
                }
            }
        }
        Ok(())
    }
}

/// Water: moving waves on the surface, more mirror-like the flatter you look across it, and
/// the sky and sun in it (`shaders/water.wesl`); and, where a world asks for it, a swell that
/// lifts and drops the surface itself, with pale crests.
pub type WaterMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, WaterExt>;

/// Flames: rising, flickering noise shaped into tongues of fire, white-yellow at the root
/// to red at the tips, added onto what's behind (`shaders/flame.wesl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct FlameMaterial {
    /// x: brightness
    #[uniform(0)]
    pub params: Vec4,
}

impl Material for FlameMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/flame.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterExt {
    /// wave height, wave speed, the wind's direction (x, z)
    #[uniform(100)]
    pub waves: Vec4,
    /// the swell's height in metres (0: none), how pale its crests go, how opaque the water is
    /// looking straight down, the block size (m)
    #[uniform(101)]
    pub swell: Vec4,
}

/// The tint water has unless a world says otherwise (multiplied with the water tile).
pub const WATER_TINT: [f32; 3] = [0.75, 0.9, 1.0];
/// How much water hides what's under it, looking straight down, unless a world says otherwise.
pub const WATER_OPACITY: f32 = 0.55;

impl bevy::pbr::MaterialExtension for WaterExt {
    fn vertex_shader() -> bevy::shader::ShaderRef {
        "shaders/water.wesl".into()
    }

    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/water.wesl".into()
    }
}

pub fn sky_mesh() -> Mesh {
    let (w, h) = (90.0f32, 40.0f32);
    let top = [0.32, 0.56, 0.92, 1.0];
    let low = [0.78, 0.88, 0.98, 1.0];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-w, -h * 0.3, 0.0], [w, -h * 0.3, 0.0], [w, h, 0.0], [-w, h, 0.0]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![low, low, top, top])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette::load(&crate::files::base().join("data/blocks.json")).expect("data/blocks.json")
    }

    #[test]
    fn every_block_has_its_tiles() {
        let pal = palette();
        assert!(pal.defs.len() >= 16);
        let dir = crate::files::base().join("assets/blocks/tiles");
        for t in pal.tiles() {
            let px = load_tile(&dir, &t).unwrap();
            assert_eq!(px.rgba.len(), TILE * TILE);
            let hmin = px.height.iter().copied().fold(1.0, f32::min);
            let hmax = px.height.iter().copied().fold(0.0, f32::max);
            assert!(hmax - hmin > 0.05, "{t} has no bumps");
        }
        assert!(pal.def(pal.id("glass").unwrap()).unwrap().glass);
        assert!(pal.def(pal.id("lamp").unwrap()).unwrap().glow > 0.0);
        assert_eq!(pal.def(pal.id("grass").unwrap()).unwrap().bottom(), "dirt");
        assert_eq!(pal.def(pal.id("grass").unwrap()).unwrap().side_covered.as_deref(), Some("dirt"));
    }

    #[test]
    fn atlas_layout() {
        let pal = palette();
        let dir = crate::files::base().join("assets/blocks/tiles");
        let tiles: Vec<_> = pal.tiles().into_iter().map(|t| (t.clone(), load_tile(&dir, &t).unwrap())).collect();
        let a = build_atlas(&pal, &tiles);
        let bytes: usize = (0..MIPS).map(|m| (a.width >> m) * (a.height >> m) * 4).sum();
        assert_eq!(a.colour.len(), bytes);
        assert_eq!(a.normal.len(), bytes);
        assert!(a.width % (1 << (MIPS - 1)) == 0 && a.height % (1 << (MIPS - 1)) == 0, "mips divide evenly");
        for r in a.rects.values() {
            assert!(r[0] > 0.0 && r[2] < 1.0 && r[0] < r[2] && r[1] < r[3]);
        }
        // the border repeats the tile (wraps), so filtering at the edge sees the other side
        let (x0, y0) = (PAD, PAD);
        let px = |x: usize, y: usize| &a.colour[(y * a.width + x) * 4..(y * a.width + x) * 4 + 4];
        assert_eq!(px(x0 - 1, y0 + 5), px(x0 + TILE - 1, y0 + 5));
        // normals point out of the surface everywhere
        assert!(a.normal.chunks(4).take(a.width * a.height).all(|p| p[2] > 128));
    }

    #[test]
    fn levels_and_meshes() {
        let pal = palette();
        let lvl = r#"{"boxes":[{"block":"stone","from":[0,0,0],"to":[2,0,0]},{"block":"air","from":[1,0,0],"to":[1,0,0]}],
                      "blocks":[[5,0,0,"glass"],[6,0,0,"glass"],[7,0,0,"stone"]]}"#;
        let cells = load_level(lvl, &pal).unwrap();
        assert_eq!(cells.len(), 5, "air took one out");
        assert!(load_level(r#"{"boxes":[{"block":"unobtainium","from":[0,0,0],"to":[0,0,0]}]}"#, &pal).is_err());
        let dir = crate::files::base().join("assets/blocks/tiles");
        let tiles: Vec<_> = pal.tiles().into_iter().map(|t| (t.clone(), load_tile(&dir, &t).unwrap())).collect();
        let rects = build_atlas(&pal, &tiles).rects;
        // a lone block: six faces; two touching: ten
        let one: HashMap<_, _> = [(IVec3::ZERO, pal.id("bricks").unwrap())].into();
        assert_eq!(mesh_level(&one, &pal, &rects).opaque.faces(), 6);
        let two: HashMap<_, _> = [(IVec3::ZERO, 2), (IVec3::X, 2)].into();
        assert_eq!(mesh_level(&two, &pal, &rects).opaque.faces(), 10);
        // glass, glass, stone in a row: the glass pane shows no inner face, stone shows the face behind the glass
        let Meshes { opaque: o, glass: g, .. } = mesh_level(&cells, &pal, &rects);
        assert_eq!(g.faces(), 9, "two panes: 12 faces, less the 2 between them and the 1 against the stone");
        assert!(o.faces() >= 6 + 6 + 6);
        // every face: tangent square to the normal, counter-clockwise outward, uv inside its tile
        for m in [&o, &g] {
            for t in m.idx.chunks(3) {
                let (a, b, c) = (Vec3::from(m.pos[t[0] as usize]), Vec3::from(m.pos[t[1] as usize]), Vec3::from(m.pos[t[2] as usize]));
                assert!((b - a).cross(c - a).dot(Vec3::from(m.nrm[t[0] as usize])) > 0.0);
            }
            for (n, tg) in m.nrm.iter().zip(&m.tan) {
                assert!(Vec3::from(*n).dot(Vec3::new(tg[0], tg[1], tg[2])).abs() < 1e-6);
            }
        }
        // grass under grass: the lower block's sides are dirt, the top one's keep the grass edge
        let g = pal.id("grass").unwrap();
        let stack: HashMap<_, _> = [(IVec3::ZERO, g), (IVec3::Y, g)].into();
        let m = mesh_level(&stack, &pal, &rects).opaque;
        let in_tile = |uv: [f32; 2], t: &str| {
            let r = rects[t];
            uv[0] >= r[0] - 1e-6 && uv[0] <= r[2] + 1e-6 && uv[1] >= r[1] - 1e-6 && uv[1] <= r[3] + 1e-6
        };
        // positions are in metres: look near the foot of the lower block and of the upper one
        let b = vox_core::BLOCK_M;
        let side_tiles = |y: f32, t: &str| m.pos.iter().zip(&m.uv).zip(&m.nrm).filter(|((p, uv), n)| n[1] == 0.0 && (p[1] - y).abs() < 0.6 * b && in_tile(**uv, t)).count();
        assert!(side_tiles(0.25 * b, "dirt") > 0 && side_tiles(0.25 * b, "grass_side") == 0, "lower block: dirt sides");
        assert!(side_tiles(1.25 * b, "grass_side") > 0, "upper block: grass sides");
        // corner shading: a block on a floor is darker where it meets the floor
        let mut floor: HashMap<IVec3, u16> = HashMap::new();
        for x in -1..=1 {
            for z in -1..=1 {
                floor.insert(IVec3::new(x, 0, z), 2);
            }
        }
        floor.insert(IVec3::new(0, 1, 0), 2);
        let m = mesh_level(&floor, &pal, &rects).opaque;
        let dark = m.col.iter().filter(|c| c[0] < 1.0).count();
        assert!(dark >= 8, "the floor darkens round the block's foot");
    }

    #[test]
    fn fire_is_flames() {
        let pal = palette();
        let dir = crate::files::base().join("assets/blocks/tiles");
        let tiles: Vec<_> = pal.tiles().into_iter().map(|t| (t.clone(), load_tile(&dir, &t).unwrap())).collect();
        let rects = build_atlas(&pal, &tiles).rects;
        let fire = pal.id("fire").unwrap();
        assert!(pal.def(fire).unwrap().flame);
        // a fire beside a stone: the fire is four quads (two crossed, each both ways round) and
        // hides none of the stone's faces
        let cells: HashMap<_, _> = [(IVec3::ZERO, fire), (IVec3::X, pal.id("stone").unwrap())].into();
        let m = mesh_level(&cells, &pal, &rects);
        assert_eq!(m.flame.faces(), 4);
        assert_eq!(m.opaque.faces(), 6);
        assert!(m.flame.uv.iter().all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])));
        // water: its surface sits below the block's top, and the stone beside it shows its face
        let water = pal.id("water").unwrap();
        let cells: HashMap<_, _> = [(IVec3::ZERO, water), (IVec3::X, pal.id("stone").unwrap())].into();
        let m = mesh_level(&cells, &pal, &rects);
        assert_eq!(m.opaque.faces(), 6);
        assert_eq!(m.water.faces(), 5, "no face against the stone");
        let top = m.water.pos.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(top < pal.size * 0.95 && top > pal.size * 0.8);
    }

    #[test]
    fn a_level_held_as_chunks_meshes_as_the_map_does() {
        let pal = palette();
        let rects = HashMap::from([("stone".to_string(), [0.0, 0.0, 1.0, 1.0]), ("glass".to_string(), [0.0, 0.0, 1.0, 1.0])]);
        let level = r#"{"boxes": [
            {"block": "stone", "from": [-40, -1, -3], "to": [40, -1, 3]},
            {"block": "stone", "from": [28, 0, -2], "to": [36, 6, 2], "hollow": true},
            {"block": "glass", "from": [30, 2, -1], "to": [31, 3, -1]}
        ]}"#;
        let map = load_level(level, &pal).unwrap();
        let world = load_level_world(level, &pal, None).unwrap();
        assert_eq!(world.count(), map.len());
        let (a, b) = (mesh_level(&map, &pal, &rects), mesh_world(&world, &pal, &rects));
        assert_eq!((a.opaque.faces(), a.glass.faces()), (b.opaque.faces(), b.glass.faces()));
        assert!(a.opaque.faces() > 0);
        // only a region: the floor's middle, nothing of the hut at x = 28 and beyond
        let part = load_level_world(level, &pal, Some(vox_core::Bounds { lo: [-8, -64, -8], hi: [8, 64, 8] })).unwrap();
        assert_eq!(part.count(), 16 * 7);
    }

    #[test]
    fn built_in_levels_load() {
        let pal = palette();
        // every level file, in subfolders too (dungeon templates live in levels/dungeons/)
        let mut files = Vec::new();
        let mut dirs = vec![crate::files::base().join("data/levels")];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "json") {
                    files.push(p);
                }
            }
        }
        assert!(files.len() > 5, "{files:?}");
        for path in files {
            let t = std::fs::read_to_string(&path).unwrap();
            let cells = load_level(&t, &pal).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            assert!(cells.len() > 100);
            // the stage is clear: nothing where the character stands
            for y in 0..4 {
                assert!(!cells.contains_key(&IVec3::new(0, y, 0)), "{}", path.display());
            }
        }
        assert!(level_path("yard").is_some());
        assert!(level_path("dungeons/crypt").is_some());
        for bad in ["../etc", "dungeons/../yard", "dungeons/", "levels/yard", "dungeons/dungeons/crypt"] {
            assert!(level_path(bad).is_none(), "{bad}");
        }
    }
}

#[cfg(test)]
mod packed_tests {
    use super::*;

    #[test]
    fn faces_and_shading_pack_into_one_number() {
        // the order the shader's tables follow: +X, -X, +Z, -Z, +Y, -Y
        for (i, (dir, udir, vdir)) in FACES.iter().enumerate() {
            let n = dir.as_vec3().to_array();
            assert_eq!(pack_face(n, 1.0) & 7, i as u32);
            // and every face's tangent is +u with handedness -1, as block_vertex.wesl has it
            assert!(dir.as_vec3().cross(*udir).dot(*vdir) < 0.0, "face {i}");
        }
        assert_eq!(pack_face([0.0, 1.0, 0.0], 0.55) >> 8, 140);
        assert_eq!(pack_face([0.0, -1.0, 0.0], 2.0) >> 8, 255, "clamped");
    }

    #[test]
    fn the_packed_mesh_has_the_three_attributes_and_nothing_else() {
        let mut m = MeshData::default();
        m.pos = vec![[0.0; 3]; 4];
        m.nrm = vec![[1.0, 0.0, 0.0]; 4];
        m.uv = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.25, 0.5]];
        m.tan = vec![[0.0, 0.0, -1.0, -1.0]; 4];
        m.col = vec![[0.82, 0.82, 0.82, 1.0]; 4];
        m.idx = vec![0, 1, 2, 0, 2, 3];
        // a face three blocks wide and two high, its tile's corner at (0.25, 0.5)
        m.local = vec![[0, 0], [3, 0], [3, 2], [0, 2]];
        m.tile = vec![[0.25, 0.5]; 4];
        let mesh = m.into_packed_mesh();
        let ids: Vec<_> = mesh.attributes().map(|(a, _)| a.id).collect();
        assert_eq!(ids.len(), 3);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
        let Some(bevy::mesh::VertexAttributeValues::Unorm16x2(tile)) = mesh.attribute(ATTRIBUTE_BLOCK_UV) else { panic!("tile") };
        assert_eq!(tile[2], [16384, 32768]);
        let Some(bevy::mesh::VertexAttributeValues::Uint32(f)) = mesh.attribute(ATTRIBUTE_BLOCK_FACE) else { panic!("face") };
        // +X face, shading 209, 3 blocks along u and 2 along v at the far corner
        assert_eq!(f[2], 209 << 8 | 3 << 16 | 2 << 22);
        // 12 + 4 + 4 bytes a vertex
        assert_eq!(mesh.get_vertex_size(), 20);
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn level(name: &str) -> (Palette, HashMap<String, [f32; 4]>, HashMap<IVec3, u16>) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pal = Palette::load(&root.join("data/blocks.json")).unwrap();
        let tiles: Vec<_> = pal.tiles().into_iter().map(|t| load_tile(&root.join("assets/blocks/tiles"), &t).map(|px| (t, px)).unwrap()).collect();
        let atlas = build_atlas(&pal, &tiles);
        let text = std::fs::read_to_string(root.join(format!("data/levels/{name}.json"))).unwrap();
        let cells = load_level(&text, &pal).unwrap();
        (pal, atlas.rects, cells)
    }

    #[test]
    fn merging_draws_the_same_surface_with_far_fewer_faces() {
        // the yard is flat and built (most faces merge); the islands are crags, where shaded
        // corners keep faces apart so the shading stays right
        for (name, at_least) in [("yard", 0.6), ("islands", 0.2)] {
            let (pal, rects, cells) = level(name);
            let mut keys: Vec<(IVec3, u16)> = cells.iter().map(|(p, b)| (*p, *b)).collect();
            keys.sort_by_key(|(p, _)| (p.y, p.z, p.x));
            let get = |p: IVec3| cells.get(&p).copied().unwrap_or(0);
            let each = mesh_cells(keys.clone(), get, &pal, &rects, false);
            let merged = mesh_cells(keys, get, &pal, &rects, true);
            // the same area of solid surface, in blocks
            let area = |m: &MeshData| -> f32 {
                m.idx.chunks(3).map(|t| {
                    let p = |i: u32| Vec3::from(m.pos[i as usize]);
                    (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
                }).sum::<f32>() / (pal.size * pal.size)
            };
            let (a, b) = (area(&each.opaque), area(&merged.opaque));
            assert!((a - b).abs() < 0.5, "{name}: {a} vs {b}");
            // glass and water are untouched
            assert_eq!(each.glass.faces(), merged.glass.faces());
            assert_eq!(each.water.faces(), merged.water.faces());
            println!("{name}: {} faces each, {} merged ({:.0}% fewer)", each.opaque.faces(), merged.opaque.faces(),
                100.0 * (1.0 - merged.opaque.faces() as f32 / each.opaque.faces() as f32));
            assert!((merged.opaque.faces() as f32) < each.opaque.faces() as f32 * (1.0 - at_least), "{name}");
            // every merged face's place across it stays within what the vertex can hold
            assert!(merged.opaque.local.iter().all(|l| l[0] <= 32 && l[1] <= 32));
        }
    }
}
