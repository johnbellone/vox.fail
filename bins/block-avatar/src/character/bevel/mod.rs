//! Hand-made normal mapping for the voxel models.
//!
//! There is no high-poly model to bake from, so the detail is drawn by hand: a small normal-map
//! atlas whose tiles are one voxel face each, painted procedurally with
//! - a rounded **bevel** on every edge that is a real outside edge of the model (the block's
//!   corner catches the light, like a toy block), and
//! - a faint **groove** where two faces sit side by side in the same plane (the seams between
//!   blocks; 0 makes a flat colour area read as one smooth surface again).
//!
//! Each voxel face gets the tile for its own combination of edges (4 edges, 2 kinds: 16 tiles).
//! To know which edges are outside edges, every model mesh is rebuilt at load time from its
//! greedy-merged quads into one quad per voxel face, with
//! - `UV_1` pointing into the atlas tile for that face,
//! - tangents along the face's grid (the normal map's x/y = the face's u/v),
//! - a tiny per-voxel **wobble** of the face normal (hand-placed blocks are never perfectly
//!   flat), so big flat areas get some life in the light.
//!
//! Surfaces can also carry their own fine detail, painted the same way: **stubble** for a beard
//! (lots of tiny bumps), **hair** (short strands running down the head, turned to run downward
//! on side faces too) and **cloth** (a diagonal twill, for denim and caps). Which palette colour
//! of which model is which surface is set in `data/surfaces.json`; each has four variants
//! picked per voxel so the pattern doesn't visibly repeat.
//!
//! The original materials are cloned with the atlas as their normal map (on UV channel 1), so
//! the stock PBR shader does the lighting; nothing else in the app changes. `bevel:on` swaps the
//! originals back for a before/after.

mod atlas;
mod mesh;
mod apply;
#[cfg(test)]
mod tests;
#[allow(unused_imports)]
pub use atlas::*;
#[allow(unused_imports)]
pub use mesh::*;
#[allow(unused_imports)]
pub use apply::*;

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology, UvChannel, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// Pixels per tile edge, and tiles per atlas edge: 16 edge combinations per row, one row per
/// surface variant (plain, 4 stubble, 4 hair down v, 4 hair down u, cloth).
const TILE: usize = 64;
const TILES: usize = 16;
const GROUPS: usize = 14;
const ATLAS: usize = TILE * TILES;
/// Mip levels: down to 1 pixel per tile (the tiles never mix below that).
const MIPS: u32 = 7;

/// Which edges of a voxel face are outside edges: bit 0 = +u side, 1 = -u, 2 = +v, 3 = -v.
pub type EdgeMask = u8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// outside-edge bevel, 0..1
    pub bevel: f32,
    /// seam groove between neighbouring faces, 0..1
    pub grid: f32,
    /// per-voxel wobble of the face normal, 0..1 (1 = about 6 degrees)
    pub wobble: f32,
    /// surface detail (stubble, hair, cloth), 0..1
    pub detail: f32,
    /// corner shading where blocks meet (baked ambient occlusion), 0..1; set by studio lighting
    pub ao: f32,
    /// blocks of one surface blend into each other (the detail runs on across their edges)
    pub blend: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            bevel: 0.6,
            grid: 0.0,
            wobble: 0.1,
            detail: 0.7,
            ao: 0.0,
            blend: true,
        }
    }
}

/// What a voxel face is made of, for its fine detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Plain,
    Stubble,
    Hair,
    Cloth,
    /// glows (neon signs, lit windows); the block itself is drawn plain
    Glow,
}

impl Surface {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "plain" => Surface::Plain,
            "stubble" | "beard" => Surface::Stubble,
            "hair" => Surface::Hair,
            "cloth" | "denim" => Surface::Cloth,
            "glow" => Surface::Glow,
            _ => return None,
        })
    }
}

/// The atlas row for a face: its surface, a variant 0..4, and whether hair should run along u
/// (a side face, where the face's u axis is the model's up).
fn group_of(surface: Surface, variant: usize, along_u: bool) -> usize {
    match surface {
        Surface::Plain | Surface::Glow => 0,
        Surface::Stubble => 1 + variant % 4,
        Surface::Hair => 1 + 4 + (along_u as usize) * 4 + variant % 4,
        Surface::Cloth => 13,
    }
}

/// A tiny deterministic random source for painting.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 & 0xFF_FFFF) as f32 / 16_777_216.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

fn hash3(k: [i32; 3], salt: u32) -> f32 {
    let mut h = (k[0] as u32).wrapping_mul(0x9E37_79B1)
        ^ (k[1] as u32).wrapping_mul(0x85EB_CA77)
        ^ (k[2] as u32).wrapping_mul(0xC2B2_AE3D)
        ^ salt.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0 * 2.0 - 1.0
}

/// One voxel face cell found in the mesh.
struct Cell {
    /// the solid voxel it belongs to, in voxel units
    key: [i32; 3],
    /// normal axis 0..3 and sign
    axis: usize,
    pos: bool,
    /// the cell's corner with the lowest u and v, in mesh units
    corner: Vec3,
    /// its size along u and v (one voxel, or the whole quad when it isn't on the grid)
    size: (f32, f32),
    /// on the voxel grid; off-grid pieces (thin plates, half-voxel details) take part in
    /// nothing and stay flat
    regular: bool,
    uv0: [[f32; 2]; 4],
}

#[derive(Resource)]
pub struct Bevel {
    pub on: bool,
    pub look: Look,
    /// (normal map, occlusion, colour detail)
    atlas: Option<(Handle<Image>, Handle<Image>, Handle<Image>)>,
    meshes: HashMap<AssetId<Mesh>, Option<Handle<Mesh>>>,
    /// (original material, glossy eyes) -> bevelled material
    mats: HashMap<(AssetId<StandardMaterial>, bool), Handle<StandardMaterial>>,
    /// the atlas needs repainting (strength or grid changed)
    repaint: bool,
    /// the meshes need rebuilding (wobble changed)
    rebuild: bool,
    /// model path -> palette colour -> surface (data/surfaces.json)
    surfaces: HashMap<String, HashMap<[u8; 3], Surface>>,
    /// the studio-lighting state the meshes and materials were last made for
    studio: Option<bool>,
    /// set materials stay matte (a shiny floor turns white under the studio lights)
    matte: HashSet<AssetId<StandardMaterial>>,
}

/// Reads `data/surfaces.json`: `{"models/x.glb": {"b4805e": "stubble", ...}, ...}`.
pub fn parse_surfaces(text: &str) -> HashMap<String, HashMap<[u8; 3], Surface>> {
    let mut out = HashMap::new();
    let Ok(serde_json::Value::Object(models)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        return out;
    };
    for (model, cols) in models {
        let Some(cols) = cols.as_object() else {
            continue;
        };
        let mut m = HashMap::new();
        for (hex, kind) in cols {
            let hex = hex.trim_start_matches('#');
            let rgb = (hex.len() == 6)
                .then(|| {
                    (0..3)
                        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
                        .collect::<Option<Vec<u8>>>()
                })
                .flatten();
            if let (Some(rgb), Some(k)) = (rgb, kind.as_str().and_then(Surface::parse)) {
                m.insert([rgb[0], rgb[1], rgb[2]], k);
            }
        }
        out.insert(model, m);
    }
    out
}

impl Bevel {
    pub fn new(on: bool, look: Look) -> Self {
        let surfaces = parse_surfaces(
            &std::fs::read_to_string(crate::asset_base().join("data/surfaces.json"))
                .unwrap_or_default(),
        );
        Bevel {
            on,
            look,
            atlas: None,
            meshes: HashMap::new(),
            mats: HashMap::new(),
            repaint: false,
            rebuild: false,
            surfaces,
            studio: None,
            matte: HashSet::new(),
        }
    }

    /// bevel:on (toggle), bevel:<0-100>, bevel:grid:<0-100>, bevel:wobble:<0-100>, bevel:detail:<0-100>
    pub fn command(&mut self, arg: &str) -> Option<String> {
        let pct = |s: &str| {
            s.trim()
                .parse::<f32>()
                .ok()
                .map(|v| (v / 100.0).clamp(0.0, 1.0))
        };
        if arg == "on" {
            self.on = !self.on;
            return Some(format!(
                "Block bevels {}",
                if self.on { "on" } else { "off" }
            ));
        }
        if let Some(v) = arg.strip_prefix("grid:") {
            self.look.grid = pct(v)?;
            self.repaint = true;
            return Some(format!("Block seams {:.0}%", self.look.grid * 100.0));
        }
        if let Some(v) = arg.strip_prefix("detail:") {
            self.look.detail = pct(v)?;
            self.repaint = true;
            self.rebuild = true;
            return Some(format!(
                "Beard, hair and cloth detail {:.0}%",
                self.look.detail * 100.0
            ));
        }
        if arg == "blend" {
            self.look.blend = !self.look.blend;
            self.rebuild = true;
            return Some(format!(
                "Blend blocks {}",
                if self.look.blend { "on" } else { "off" }
            ));
        }
        if let Some(v) = arg.strip_prefix("wobble:") {
            self.look.wobble = pct(v)?;
            self.rebuild = true;
            return Some(format!("Block wobble {:.0}%", self.look.wobble * 100.0));
        }
        self.look.bevel = pct(arg)?;
        self.repaint = true;
        Some(format!("Block bevels {:.0}%", self.look.bevel * 100.0))
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"on": self.on, "bevel": (self.look.bevel * 100.0).round(), "grid": (self.look.grid * 100.0).round(), "wobble": (self.look.wobble * 100.0).round(), "detail": (self.look.detail * 100.0).round(), "blend": self.look.blend})
    }
}

/// On meshes that aren't voxel models (block levels, the sky): left alone.
pub use vox_world::NoBevel;

/// On an entity whose mesh has been looked at: the originals, to swap back.
#[derive(Component)]
pub struct Bevelled {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    swapped: bool,
}
