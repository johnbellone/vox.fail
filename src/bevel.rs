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

/// Height field (in voxel-face units, TILE x TILE, wrapping) of a surface row's detail, or None
/// for plain rows.
pub fn detail_heights(group: usize) -> Option<Vec<f32>> {
    let n = TILE;
    let mut h = vec![0.0f32; n * n];
    // distance on the wrapping tile
    let wrap = |d: f32| d - d.round();
    match group {
        // stubble: lots of tiny round bumps
        1..=4 => {
            let mut r = Rng(0x5eed_0001 + group as u32 * 7919);
            for _ in 0..40 {
                let (cx, cy, rad, ht) = (
                    r.next(),
                    r.next(),
                    r.range(0.05, 0.09),
                    r.range(0.012, 0.025),
                );
                for py in 0..n {
                    for px in 0..n {
                        let (dx, dy) = (
                            wrap((px as f32 + 0.5) / n as f32 - cx),
                            wrap((py as f32 + 0.5) / n as f32 - cy),
                        );
                        let q = (dx * dx + dy * dy) / (rad * rad);
                        if q < 1.0 {
                            let v = ht * (1.0 - q) * (1.0 - q);
                            let c = &mut h[py * n + px];
                            *c = c.max(v);
                        }
                    }
                }
            }
        }
        // hair: short strands running along v (rows 5-8) or u (rows 9-12, the same painted sideways)
        5..=12 => {
            let along_u = group >= 9;
            let mut r = Rng(0x4a1f_0002 + ((group - 5) % 4) as u32 * 104_729);
            for _ in 0..18 {
                let (x0, y0, len, slant, w, ht) = (
                    r.next(),
                    r.next(),
                    r.range(0.5, 1.0),
                    r.range(-0.1, 0.1),
                    r.range(0.035, 0.06),
                    r.range(0.015, 0.03),
                );
                for py in 0..n {
                    for px in 0..n {
                        let (mut u, mut v) =
                            ((px as f32 + 0.5) / n as f32, (py as f32 + 0.5) / n as f32);
                        if along_u {
                            std::mem::swap(&mut u, &mut v);
                        }
                        let t = wrap(v - y0);
                        let along = t + len * 0.5;
                        if !(0.0..len).contains(&along) {
                            continue;
                        }
                        let across = wrap(u - x0 - slant * t);
                        let q = (across / w).powi(2);
                        if q < 1.0 {
                            // tapered at both ends
                            let taper = (std::f32::consts::PI * along / len).sin();
                            let val = ht * taper * (1.0 - q) * (1.0 - q);
                            let c = &mut h[py * n + px];
                            *c = c.max(val);
                        }
                    }
                }
            }
        }
        // cloth: a diagonal twill with a finer cross thread (whole periods, so it tiles)
        13 => {
            let tau = std::f32::consts::TAU;
            for py in 0..n {
                for px in 0..n {
                    let (u, v) = ((px as f32 + 0.5) / n as f32, (py as f32 + 0.5) / n as f32);
                    h[py * n + px] =
                        0.008 * (tau * 6.0 * (u + v)).sin() + 0.003 * (tau * 12.0 * (u - v)).sin();
                }
            }
        }
        _ => return None,
    }
    Some(h)
}

/// The detail's tilt (x, y) at every pixel of a row's tile, from its height field.
fn detail_slopes(group: usize, look: Look) -> Option<Vec<[f32; 2]>> {
    let h = detail_heights(group)?;
    let n = TILE;
    let k = look.detail * n as f32 * 0.5;
    let at = |x: isize, y: isize| {
        h[(y.rem_euclid(n as isize) as usize) * n + x.rem_euclid(n as isize) as usize]
    };
    let mut out = vec![[0.0f32; 2]; n * n];
    for y in 0..n as isize {
        for x in 0..n as isize {
            // the normal leans away from rising ground
            out[y as usize * n + x as usize] = [
                -(at(x + 1, y) - at(x - 1, y)) * k,
                -(at(x, y + 1) - at(x, y - 1)) * k,
            ];
        }
    }
    Some(out)
}

/// The tangent-space normal for a point (x, y in 0..1) on a voxel face with the given outside
/// edges. +x is the +u side, +y the +v side. Returns (x, y, z), unit length.
pub fn tile_normal(x: f32, y: f32, mask: EdgeMask, look: Look) -> [f32; 3] {
    // a rounded bevel on outside edges, a narrow shallow one (the groove) on the rest
    let bevel = |d: f32, outside: bool| -> f32 {
        let (w, max) = if outside {
            (0.24, 0.95 * look.bevel)
        } else {
            (0.09, 0.6 * look.grid)
        };
        if d >= w || max <= 0.0 {
            return 0.0;
        }
        let t = 1.0 - d / w;
        // a rounded edge: the slope eases in from the flat face and is steepest at the edge
        max * t * t * (3.0 - 2.0 * t)
    };
    let a_pu = bevel(1.0 - x, mask & 1 != 0);
    let a_mu = bevel(x, mask & 2 != 0);
    let a_pv = bevel(1.0 - y, mask & 4 != 0);
    let a_mv = bevel(y, mask & 8 != 0);
    // each edge tips the normal towards itself
    let nx = a_pu.sin() - a_mu.sin();
    let ny = a_pv.sin() - a_mv.sin();
    let l2 = nx * nx + ny * ny;
    let (nx, ny) = if l2 > 0.94 {
        let k = (0.94 / l2).sqrt();
        (nx * k, ny * k)
    } else {
        (nx, ny)
    };
    [nx, ny, (1.0 - nx * nx - ny * ny).max(0.0).sqrt()]
}

/// The atlas with all its mip levels (each level's tiles shrunk on their own, so tiles never
/// bleed into each other), as RGBA8 bytes, level after level.
/// How much ambient light reaches each point of a tile (1 = all): the darkness in the seams,
/// between hairs, in the stubble and in the cloth's weave. The scene is lit mostly by ambient
/// light, so this is what makes the detail show; the normal map adds the shine.
pub fn tile_occlusion(
    x: f32,
    y: f32,
    mask: EdgeMask,
    group: usize,
    ao: Option<&[f32]>,
    look: Look,
) -> f32 {
    // seams: a thin dark line on the sides that aren't outside edges
    let mut o = 1.0f32;
    let w = 0.07;
    for (d, outside) in [
        (1.0 - x, mask & 1 != 0),
        (x, mask & 2 != 0),
        (1.0 - y, mask & 4 != 0),
        (y, mask & 8 != 0),
    ] {
        if !outside && d < w {
            o = o.min(1.0 - look.grid * 0.55 * (1.0 - d / w).powi(2));
        }
    }
    if let Some(a) = ao {
        let (px, py) = (
            ((x * TILE as f32) as usize).min(TILE - 1),
            ((y * TILE as f32) as usize).min(TILE - 1),
        );
        o *= 1.0 - 0.5 * look.detail * (1.0 - a[py * TILE + px]);
    }
    let _ = group;
    o.clamp(0.0, 1.0)
}

/// Per-pixel occlusion of a surface row's detail (1 = open), or None for plain rows.
pub fn detail_occlusion(group: usize) -> Option<Vec<f32>> {
    let n = TILE;
    let h = detail_heights(group)?;
    let hmax = h.iter().copied().fold(1e-6f32, f32::max);
    let hmin = h.iter().copied().fold(f32::MAX, f32::min);
    let wrap = |d: f32| d - d.round();
    let mut o = vec![1.0f32; n * n];
    match group {
        // stubble: dark whisker dots in the gaps between the bumps, and the low ground darker
        1..=4 => {
            for (i, v) in h.iter().enumerate() {
                o[i] = 1.0 - 0.35 * (1.0 - (v - hmin) / (hmax - hmin));
            }
            let mut r = Rng(0xd07_0003 + group as u32 * 31337);
            for _ in 0..34 {
                let (cx, cy, rad, dark) =
                    (r.next(), r.next(), r.range(0.03, 0.055), r.range(0.55, 0.8));
                for py in 0..n {
                    for px in 0..n {
                        let (dx, dy) = (
                            wrap((px as f32 + 0.5) / n as f32 - cx),
                            wrap((py as f32 + 0.5) / n as f32 - cy),
                        );
                        let q = (dx * dx + dy * dy) / (rad * rad);
                        if q < 1.0 {
                            let c = &mut o[py * n + px];
                            *c = c.min(1.0 - dark * (1.0 - q));
                        }
                    }
                }
            }
        }
        // hair and cloth: the low ground between strands and threads is in shadow
        _ => {
            let k = if group == 13 { 0.45 } else { 0.7 };
            for (i, v) in h.iter().enumerate() {
                o[i] = 1.0 - k * (1.0 - (v - hmin) / (hmax - hmin)).powf(1.5);
            }
        }
    }
    Some(o)
}

/// The high-resolution colour detail of a surface row, a multiplier around 1 (the block's own
/// colour on average; above 1 lighter, so it shows on dark hair too), drawn from the same height
/// field as its normal map so the shading and the pattern line up: whiskers, strands and
/// threads. None for plain rows.
pub fn detail_albedo(group: usize, look: Look) -> Option<Vec<f32>> {
    let n = TILE;
    let h = detail_heights(group)?;
    let hmax = h.iter().copied().fold(1e-6f32, f32::max);
    let hmin = h.iter().copied().fold(f32::MAX, f32::min);
    let k = (look.detail / 0.7).min(1.4);
    let wrap = |d: f32| d - d.round();
    let mut a = vec![1.0f32; n * n];
    match group {
        // stubble: a slightly mottled base with dark whisker dots
        1..=4 => {
            for (i, v) in h.iter().enumerate() {
                a[i] = 1.0 + 0.25 * k * ((v - hmin) / (hmax - hmin) - 0.3);
            }
            let mut r = Rng(0xd07_0003 + group as u32 * 31337);
            for _ in 0..90 {
                let (cx, cy, rad, dark) =
                    (r.next(), r.next(), r.range(0.018, 0.035), r.range(0.3, 0.5));
                for py in 0..n {
                    for px in 0..n {
                        let (dx, dy) = (
                            wrap((px as f32 + 0.5) / n as f32 - cx),
                            wrap((py as f32 + 0.5) / n as f32 - cy),
                        );
                        let q = (dx * dx + dy * dy) / (rad * rad);
                        if q < 1.0 {
                            let c = &mut a[py * n + px];
                            *c = c.min(1.0 - dark * k * (1.0 - q * q) * 1.2);
                        }
                    }
                }
            }
        }
        // hair: strands catch the light on top, the gaps between them are dark
        5..=12 => {
            for (i, v) in h.iter().enumerate() {
                let t = (v - hmin) / (hmax - hmin);
                // gaps dark, strand tops catching the light
                a[i] = 1.0 + 0.45 * k * (t.powf(0.8) - 0.4);
            }
        }
        // cloth: the twill's valleys a little darker
        _ => {
            for (i, v) in h.iter().enumerate() {
                a[i] = 1.0 + 0.3 * k * ((v - hmin) / (hmax - hmin) - 0.5);
            }
        }
    }
    for v in &mut a {
        *v = v.clamp(0.2, 1.6);
    }
    Some(a)
}

/// The colour-detail atlas, same layout as the normal map, all mip levels, RGBA16F (linear,
/// so it can go above 1).
pub fn atlas_albedo(look: Look) -> Vec<u8> {
    let mut px: Vec<[f32; 3]> = vec![[1.0; 3]; ATLAS * ATLAS];
    for group in 0..GROUPS {
        let Some(a) = detail_albedo(group, look) else {
            continue;
        };
        for mask in 0..16usize {
            let (tx, ty) = (mask * TILE, group * TILE);
            for y in 0..TILE {
                for x in 0..TILE {
                    let v = a[y * TILE + x];
                    px[(ty + y) * ATLAS + tx + x] = [v, v, v];
                }
            }
        }
    }
    mip_encode(px, false, &|n, out| {
        for v in [n[0], n[1], n[2], 1.0] {
            out.extend_from_slice(&f16_bits(v).to_le_bytes());
        }
    })
}

/// Both atlases with all their mip levels, as RGBA8 bytes level after level: the normal map,
/// and the occlusion map (grey).
pub fn atlas_pair(look: Look) -> (Vec<u8>, Vec<u8>) {
    // blended blocks have no seams between them
    let look = Look {
        grid: if look.blend { 0.0 } else { look.grid },
        ..look
    };
    let mut normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 1.0]; ATLAS * ATLAS];
    let mut occl: Vec<[f32; 3]> = vec![[1.0, 1.0, 1.0]; ATLAS * ATLAS];
    for group in 0..GROUPS {
        let slopes = detail_slopes(group, look);
        let ao = detail_occlusion(group);
        for mask in 0..16u8 {
            let (tx, ty) = (mask as usize * TILE, group * TILE);
            for py in 0..TILE {
                for px in 0..TILE {
                    let (x, y) = (
                        (px as f32 + 0.5) / TILE as f32,
                        (py as f32 + 0.5) / TILE as f32,
                    );
                    let mut n = tile_normal(x, y, mask, look);
                    if let Some(s) = &slopes {
                        let d = s[py * TILE + px];
                        let v = Vec3::new(
                            n[0] / n[2].max(0.2) + d[0],
                            n[1] / n[2].max(0.2) + d[1],
                            1.0,
                        )
                        .normalize();
                        n = v.to_array();
                    }
                    normals[(ty + py) * ATLAS + tx + px] = n;
                    let o = tile_occlusion(x, y, mask, group, ao.as_deref(), look);
                    occl[(ty + py) * ATLAS + tx + px] = [o, o, o];
                }
            }
        }
    }
    (mip_bytes(normals, true), mip_bytes(occl, false))
}

/// Box-filters a level-0 atlas down tile by tile (tile edges line up with even pixels at every
/// level used) and packs all levels as RGBA8.
fn mip_bytes(level: Vec<[f32; 3]>, is_normal: bool) -> Vec<u8> {
    mip_encode(level, is_normal, &|n, out| {
        if is_normal {
            out.extend_from_slice(&[enc(n[0]), enc(n[1]), enc(n[2]), 255]);
        } else {
            let g = (n[0] * 255.0).round().clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[g, g, g, 255]);
        }
    })
}

/// IEEE half float, for the colour detail (it goes above 1: strands are lighter than the block).
fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if exp <= 0 {
        return sign;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    sign | ((exp as u16) << 10) | ((b >> 13) & 0x3ff) as u16
}

fn mip_encode(
    mut level: Vec<[f32; 3]>,
    is_normal: bool,
    put: &dyn Fn([f32; 3], &mut Vec<u8>),
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut size = ATLAS;
    for m in 0..MIPS {
        for n in &level {
            put(*n, &mut out);
        }
        if m + 1 == MIPS {
            break;
        }
        let half = size / 2;
        let mut next = vec![[0.0f32; 3]; half * half];
        for y in 0..half {
            for x in 0..half {
                let mut s = [0.0f32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let v = level[(2 * y + dy) * size + 2 * x + dx];
                    for k in 0..3 {
                        s[k] += v[k] * 0.25;
                    }
                }
                next[y * half + x] = if is_normal {
                    let l = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt().max(1e-6);
                    [s[0] / l, s[1] / l, s[2] / l]
                } else {
                    s
                };
            }
        }
        level = next;
        size = half;
    }
    out
}

fn enc(v: f32) -> u8 {
    ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8
}

pub fn atlas_image(bytes: Vec<u8>) -> Image {
    // 8 bytes a pixel is the colour detail (half floats), 4 the normal and occlusion maps
    let total8: usize = (0..MIPS).map(|m| (ATLAS >> m) * (ATLAS >> m) * 8).sum();
    let format = if bytes.len() == total8 {
        TextureFormat::Rgba16Float
    } else {
        TextureFormat::Rgba8Unorm
    };
    let mut img = Image::new_uninit(
        Extent3d {
            width: ATLAS as u32,
            height: ATLAS as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::default(),
    );
    img.texture_descriptor.mip_level_count = MIPS;
    img.data = Some(bytes);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("bevel atlas".into()),
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        lod_max_clamp: (MIPS - 1) as f32,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}

/// Where a face samples the atlas: (u, v) for its four corners (u0, v0) .. (u1, v1), kept half
/// a texel inside the tile for its surface row and edge mask.
fn tile_uv(group: usize, mask: EdgeMask) -> (f32, f32, f32, f32) {
    let (tx, ty) = (mask as f32, group as f32);
    let inset = 0.5 / TILE as f32;
    let s = 1.0 / TILES as f32;
    (
        s * (tx + inset),
        s * (ty + inset),
        s * (tx + 1.0 - inset),
        s * (ty + 1.0 - inset),
    )
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

/// The models' voxel size in mesh units.
pub const VOXEL: f32 = 0.05;

/// The grid a mesh is built on: 0.05 when half or more of its quad sides are whole multiples of it
/// (true of every character, hat and set), else the smallest side that most sides are
/// multiples of. A mesh that is one big box can't tell, so the usual size wins.
fn grid_of(sides: &[f32]) -> Option<f32> {
    let score = |c: f32| {
        sides
            .iter()
            .filter(|s| (*s / c - (*s / c).round()).abs() < 0.02)
            .count() as f32
            / sides.len().max(1) as f32
    };
    if score(VOXEL) >= 0.5 {
        return Some(VOXEL);
    }
    let mut cands: Vec<f32> = sides.iter().copied().filter(|s| *s > 0.002).collect();
    cands.sort_by(f32::total_cmp);
    cands.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    cands.into_iter().find(|c| score(*c) >= 0.75)
}

/// Rebuilds a voxel mesh (axis-aligned, greedy-merged quads) as one quad per voxel face, with
/// atlas UVs, tangents and a wobbled normal. None if the mesh isn't a voxel mesh.
pub fn rebuild(
    mesh: &Mesh,
    look: Look,
    surface_of: &dyn Fn([f32; 2]) -> (Surface, [f32; 3]),
) -> Option<Mesh> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let Ok(VertexAttributeValues::Float32x3(pos)) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let Ok(VertexAttributeValues::Float32x3(nrm)) = mesh.try_attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        return None;
    };
    let uv0 = match mesh.try_attribute(Mesh::ATTRIBUTE_UV_0) {
        Ok(VertexAttributeValues::Float32x2(u)) => u.clone(),
        _ => vec![[0.0, 0.0]; pos.len()],
    };
    let idx: Vec<u32> = match mesh.indices()? {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    };
    if idx.len() % 6 != 0 || idx.is_empty() {
        return None;
    }
    // quads: two triangles sharing 4 corners, axis-aligned
    struct Quad {
        axis: usize,
        pos: bool,
        lo: Vec3,
        hi: Vec3,
        corners: [usize; 4],
    }
    let mut quads = vec![];
    for t in idx.chunks(6) {
        let mut vs: Vec<usize> = t.iter().map(|&i| i as usize).collect();
        vs.sort_unstable();
        vs.dedup();
        if vs.len() != 4 || vs.iter().any(|&v| v >= pos.len()) {
            return None;
        }
        let n = Vec3::from(nrm[vs[0]]);
        let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs()))?;
        if n[axis].abs() < 0.99 {
            return None;
        }
        let ps: Vec<Vec3> = vs.iter().map(|&v| Vec3::from(pos[v])).collect();
        let lo = ps.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(*p));
        let hi = ps.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(*p));
        if (hi[axis] - lo[axis]).abs() > 1e-4 {
            return None;
        }
        quads.push(Quad {
            axis,
            pos: n[axis] > 0.0,
            lo,
            hi,
            corners: [vs[0], vs[1], vs[2], vs[3]],
        });
    }
    let sides: Vec<f32> = quads
        .iter()
        .flat_map(|q| {
            let (u, v) = ((q.axis + 1) % 3, (q.axis + 2) % 3);
            [q.hi[u] - q.lo[u], q.hi[v] - q.lo[v]]
        })
        .collect();
    let vox = grid_of(&sides)?;
    let on_grid = |q: &Quad| -> Option<(usize, usize)> {
        let (ua, va) = ((q.axis + 1) % 3, (q.axis + 2) % 3);
        let (nu, nv) = ((q.hi[ua] - q.lo[ua]) / vox, (q.hi[va] - q.lo[va]) / vox);
        let (iu, iv) = (nu.round(), nv.round());
        ((nu - iu).abs() < 0.02
            && (nv - iv).abs() < 0.02
            && iu >= 1.0
            && iv >= 1.0
            && iu * iv <= 1.0e5)
            .then_some((iu as usize, iv as usize))
    };
    // the grid's offset, from the first on-grid quad's first voxel
    let q0 = quads.iter().find(|q| on_grid(q).is_some())?;
    let (u0a, v0a) = ((q0.axis + 1) % 3, (q0.axis + 2) % 3);
    let mut first = q0.lo - axis_vec(q0.axis, q0.pos) * vox * 0.5;
    first[u0a] += vox * 0.5;
    first[v0a] += vox * 0.5;
    let off = Vec3::new(
        frac_off(first.x, vox),
        frac_off(first.y, vox),
        frac_off(first.z, vox),
    );
    let key_of = |c: Vec3| -> [i32; 3] {
        let k = (c - off) / vox;
        [k.x.round() as i32, k.y.round() as i32, k.z.round() as i32]
    };
    // where block corners sit: half a block from the centres
    let lattice = off + Vec3::splat(vox * 0.5);
    let mut weld: std::collections::HashMap<[i64; 3], Vec3> = std::collections::HashMap::new();
    // cut every quad into voxel cells
    let mut cells = vec![];
    for q in &quads {
        let (ua, va) = ((q.axis + 1) % 3, (q.axis + 2) % 3);
        let grid = on_grid(q);
        let (iu, iv) = grid.unwrap_or((1, 1));
        let (su, sv) = (
            (q.hi[ua] - q.lo[ua]) / iu as f32,
            (q.hi[va] - q.lo[va]) / iv as f32,
        );
        // the quad's UV_0 at its four corners, for spreading over the cells
        let corner_uv = |cu: bool, cv: bool| -> [f32; 2] {
            let want_u = if cu { q.hi[ua] } else { q.lo[ua] };
            let want_v = if cv { q.hi[va] } else { q.lo[va] };
            q.corners
                .iter()
                .find(|&&c| {
                    (pos[c][ua] - want_u).abs() < 1e-4 && (pos[c][va] - want_v).abs() < 1e-4
                })
                .map(|&c| uv0[c])
                .unwrap_or(uv0[q.corners[0]])
        };
        let (c00, c10, c01, c11) = (
            corner_uv(false, false),
            corner_uv(true, false),
            corner_uv(false, true),
            corner_uv(true, true),
        );
        let lerp = |a: f32, b: f32| -> [f32; 2] {
            let top = [
                c00[0] + (c10[0] - c00[0]) * a,
                c00[1] + (c10[1] - c00[1]) * a,
            ];
            let bot = [
                c01[0] + (c11[0] - c01[0]) * a,
                c01[1] + (c11[1] - c01[1]) * a,
            ];
            [
                top[0] + (bot[0] - top[0]) * b,
                top[1] + (bot[1] - top[1]) * b,
            ]
        };
        for j in 0..iv {
            for i in 0..iu {
                let mut corner = q.lo;
                corner[ua] += i as f32 * su;
                corner[va] += j as f32 * sv;
                let mut centre = corner;
                centre[ua] += su * 0.5;
                centre[va] += sv * 0.5;
                let key = key_of(centre - axis_vec(q.axis, q.pos) * vox * 0.5);
                let (a0, a1, b0, b1) = (
                    i as f32 / iu as f32,
                    (i + 1) as f32 / iu as f32,
                    j as f32 / iv as f32,
                    (j + 1) as f32 / iv as f32,
                );
                cells.push(Cell {
                    key,
                    axis: q.axis,
                    pos: q.pos,
                    corner,
                    size: (su, sv),
                    regular: grid.is_some(),
                    uv0: [lerp(a0, b0), lerp(a1, b0), lerp(a1, b1), lerp(a0, b1)],
                });
            }
        }
    }
    let faces: HashSet<([i32; 3], usize, bool)> = cells
        .iter()
        .filter(|c| c.regular)
        .map(|c| (c.key, c.axis, c.pos))
        .collect();
    let add = |k: [i32; 3], axis: usize, s: i32| -> [i32; 3] {
        let mut k = k;
        k[axis] += s;
        k
    };
    // build the new mesh
    let n = cells.len();
    let (mut p, mut nn, mut u0, mut u1, mut tan) = (
        Vec::with_capacity(n * 4),
        Vec::with_capacity(n * 4),
        Vec::with_capacity(n * 4),
        Vec::with_capacity(n * 4),
        Vec::with_capacity(n * 4),
    );
    let mut col: Vec<[f32; 4]> = Vec::with_capacity(n * 4);
    let mut ind: Vec<u32> = Vec::with_capacity(n * 6);
    // about 6 degrees at full; blended blocks barely tilt, so they read as one surface
    let wobble = look.wobble * 0.105 * if look.blend { 0.3 } else { 1.0 };
    // every voxel that shows a face is solid; the voxels next to a face's air are what shade
    // its corners
    let solid: HashSet<[i32; 3]> = cells.iter().filter(|c| c.regular).map(|c| c.key).collect();
    for c in &cells {
        let (ua, va) = ((c.axis + 1) % 3, (c.axis + 2) % 3);
        let s = if c.pos { 1 } else { -1 };
        // each in-plane side: outside edge unless the face carries on flat, or turns inward
        let mut mask: EdgeMask = 0;
        for (bit, dir_axis, dir) in [(1u8, ua, 1i32), (2, ua, -1), (4, va, 1), (8, va, -1)] {
            if !c.regular {
                break;
            }
            let beside = add(c.key, dir_axis, dir);
            let flat = faces.contains(&(beside, c.axis, c.pos));
            let inward = faces.contains(&(add(beside, c.axis, s), dir_axis, dir < 0));
            if !flat && !inward {
                mask |= bit;
            }
        }
        // off-grid pieces sample one flat texel in the middle of tile 0
        let mid = [
            (c.uv0[0][0] + c.uv0[2][0]) * 0.5,
            (c.uv0[0][1] + c.uv0[2][1]) * 0.5,
        ];
        // one pattern per surface: it wraps, so strands and threads run on across block edges
        // and neighbouring blocks blend into one surface (variants would start a new pattern
        // at every edge)
        let variant = if look.blend {
            0
        } else {
            ((hash3(c.key, 99 + c.axis as u32) * 0.5 + 0.5) * 3.999) as usize
        };
        let (surface, colour) = surface_of(mid);
        let surface = if c.regular { surface } else { Surface::Plain };
        let group = group_of(surface, variant, ua == 1);
        let (tu0, tv0, tu1, tv1) = if c.regular {
            tile_uv(group, mask)
        } else {
            (
                0.5 / TILES as f32,
                0.5 / TILES as f32,
                0.5 / TILES as f32,
                0.5 / TILES as f32,
            )
        };
        let nrm = axis_vec(c.axis, c.pos);
        let t_dir = axis_vec(ua, true);
        let v_dir = axis_vec(va, true);
        let w = if nrm.cross(t_dir).dot(v_dir) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let wob = (nrm
            + (t_dir * hash3(c.key, c.axis as u32 * 2 + c.pos as u32)
                + v_dir * hash3(c.key, 7 + c.axis as u32 * 2 + c.pos as u32))
                * wobble)
            .normalize();
        // the tangent square to the tilted normal (the shader builds its frame from both; a
        // frame that isn't square can blow up into a NaN pixel)
        let tan_w = (t_dir - wob * wob.dot(t_dir)).normalize();
        // one square per face: the fine detail is in the colour atlas now
        let pixels = [(0.0f32, 0.0f32, 1.0f32, 1.0f32, 1.0f32)];
        let ccw = t_dir.cross(v_dir).dot(nrm) > 0.0;
        let lerp_uv0 = |a: f32, b: f32| -> [f32; 2] {
            let top = [
                c.uv0[0][0] + (c.uv0[1][0] - c.uv0[0][0]) * a,
                c.uv0[0][1] + (c.uv0[1][1] - c.uv0[0][1]) * a,
            ];
            let bot = [
                c.uv0[3][0] + (c.uv0[2][0] - c.uv0[3][0]) * a,
                c.uv0[3][1] + (c.uv0[2][1] - c.uv0[3][1]) * a,
            ];
            [
                top[0] + (bot[0] - top[0]) * b,
                top[1] + (bot[1] - top[1]) * b,
            ]
        };
        // corner shading: the classic block-game ambient occlusion, from the three voxels that
        // sit against each corner on the open side
        let corner_ao = |hu: bool, hv: bool| -> f32 {
            if look.ao <= 0.0 || !c.regular {
                return 1.0;
            }
            let front = add(c.key, c.axis, s);
            let side_u = add(front, ua, if hu { 1 } else { -1 });
            let side_v = add(front, va, if hv { 1 } else { -1 });
            let diag = add(side_u, va, if hv { 1 } else { -1 });
            let (a, b, d) = (
                solid.contains(&side_u),
                solid.contains(&side_v),
                solid.contains(&diag),
            );
            let level = if a && b {
                3
            } else {
                a as usize + b as usize + d as usize
            };
            1.0 - look.ao * [0.0, 0.3, 0.5, 0.65][level]
        };
        let ao4 = [
            corner_ao(false, false),
            corner_ao(true, false),
            corner_ao(true, true),
            corner_ao(false, true),
        ];
        let ao_at = |u: f32, v: f32| -> f32 {
            let top = ao4[0] + (ao4[1] - ao4[0]) * u;
            let bot = ao4[3] + (ao4[2] - ao4[3]) * u;
            top + (bot - top) * v
        };
        for (a0, b0, a1, b1, shade) in pixels {
            let base = p.len() as u32;
            let sq = [(a0, b0), (a1, b0), (a1, b1), (a0, b1)];
            let lit: Vec<f32> = sq.iter().map(|(u, v)| ao_at(*u, *v)).collect();
            for (k, (cu, cv)) in sq.into_iter().enumerate() {
                let mut q = c.corner;
                q[ua] += cu * c.size.0;
                q[va] += cv * c.size.1;
                // every block corner on one exact lattice value, so neighbouring faces share
                // their corners bit for bit (otherwise rounding leaves pinhole cracks between
                // them, and the depth of field spreads a crack into a black square)
                if c.regular {
                    // (only a rounding-size nudge: parts of a mesh on their own offset grid
                    // stay where they are)
                    let snapped = lattice + ((q - lattice) / vox).round() * vox;
                    if (snapped - q).abs().max_element() < vox * 0.01 {
                        q = snapped;
                    }
                }
                // and any corner met twice (off-grid faces too) takes the first copy's exact value
                let wk = [(q.x * 1e5).round() as i64, (q.y * 1e5).round() as i64, (q.z * 1e5).round() as i64];
                let q = *weld.entry(wk).or_insert(q);
                p.push(q.to_array());
                nn.push(wob.to_array());
                u0.push(lerp_uv0(cu, cv));
                u1.push([tu0 + (tu1 - tu0) * cu, tv0 + (tv1 - tv0) * cv]);
                tan.push([tan_w.x, tan_w.y, tan_w.z, w]);
                // the block's palette colour (the material's texture is the detail atlas)
                let g = shade * lit[k];
                col.push([colour[0] * g, colour[1] * g, colour[2] * g, 1.0]);
            }
            // counter-clockwise seen from outside; split along the diagonal that keeps the
            // shading smooth (the usual fix for corner-shading seams)
            let flip = lit[0] + lit[2] < lit[1] + lit[3];
            let tris: [u32; 6] = match (ccw, flip) {
                (true, false) => [0, 1, 2, 0, 2, 3],
                (false, false) => [0, 2, 1, 0, 3, 2],
                (true, true) => [0, 1, 3, 1, 2, 3],
                (false, true) => [0, 3, 1, 1, 3, 2],
            };
            ind.extend(tris.iter().map(|t| base + t));
        }
    }
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nn)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, u0)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, u1)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tan)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(ind)),
    )
}

fn axis_vec(axis: usize, pos: bool) -> Vec3 {
    let mut v = Vec3::ZERO;
    v[axis] = if pos { 1.0 } else { -1.0 };
    v
}

/// The grid offset along one axis: where voxel centres sit, as a fraction of a voxel.
fn frac_off(centre: f32, vox: f32) -> f32 {
    let k = centre / vox;
    (k - k.round()) * vox
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
#[derive(Component)]
pub struct NoBevel;

/// On an entity whose mesh has been looked at: the originals, to swap back.
#[derive(Component)]
pub struct Bevelled {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    swapped: bool,
}

/// The palette image of a material as (width, height, RGBA bytes), when it's readable.
fn palette_of(images: &Assets<Image>, mat: &StandardMaterial) -> Option<(usize, usize, Vec<u8>)> {
    let img = images.get(mat.base_color_texture.as_ref()?)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    img.data
        .clone()
        .filter(|d| d.len() >= w * h * 4)
        .map(|d| (w, h, d))
}

/// The model file a mesh or material came from ("models/scene_all.glb").
fn model_of(assets: &AssetServer, id: impl Into<bevy::asset::UntypedAssetId>) -> String {
    assets
        .get_path(id)
        .map(|p| p.path().to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// Rebuilds one mesh with this model's beard, hair and cloth colours.
fn make_mesh(
    bevel: &Bevel,
    meshes: &mut Assets<Mesh>,
    images: &Assets<Image>,
    assets: &AssetServer,
    mesh: &Handle<Mesh>,
    mat: &StandardMaterial,
) -> Option<Option<Handle<Mesh>>> {
    let m = meshes.get(mesh)?;
    let table = bevel.surfaces.get(&model_of(assets, mesh.id()));
    // the palette colours move into the mesh, so it has to be readable
    let palette = Some(palette_of(images, mat)?);
    let surface_of = |uv: [f32; 2]| -> (Surface, [f32; 3]) {
        let Some((w, h, d)) = &palette else {
            return (Surface::Plain, [1.0; 3]);
        };
        let (x, y) = (
            ((uv[0] * *w as f32) as usize).min(w - 1),
            ((uv[1] * *h as f32) as usize).min(h - 1),
        );
        let i = (y * w + x) * 4;
        let rgb = [d[i], d[i + 1], d[i + 2]];
        let lin = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
        (
            table
                .and_then(|t| t.get(&rgb).copied())
                .unwrap_or(Surface::Plain),
            [lin.red, lin.green, lin.blue],
        )
    };
    let r = rebuild(m, bevel.look, &surface_of);
    Some(r.map(|m| meshes.add(m)))
}

/// Brightness of glowing colours (above 1, so the bloom picks them up).
const GLOW: f32 = 3.0;

/// Swaps every voxel mesh in the scene for its bevelled rebuild (PostUpdate, so scenes spawned
/// this frame are done before they are drawn).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn apply(
    mut bevel: ResMut<Bevel>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
    fresh: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Option<&Name>,
        ),
        (Without<Bevelled>, Without<NoBevel>),
    >,
    mut done: Query<(
        Entity,
        &mut Bevelled,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut commands: Commands,
    mut was_on: Local<Option<bool>>,
    studio: Res<crate::lighting::Studio>,
) {
    let bevel = &mut *bevel;
    // studio lighting: corner shading in the meshes, a little plastic sheen on the materials
    if bevel.studio != Some(studio.on) {
        let first = bevel.studio.is_none();
        bevel.studio = Some(studio.on);
        bevel.look.ao = if studio.on {
            crate::lighting::CORNER_AO
        } else {
            0.0
        };
        for ((orig, glossy), h) in bevel.mats.iter() {
            if *glossy {
                continue;
            }
            let rough = if studio.on && !bevel.matte.contains(orig) {
                crate::lighting::ROUGHNESS
            } else {
                mats.get(*orig)
                    .map(|m| m.perceptual_roughness)
                    .unwrap_or(0.85)
            };
            if let Some(mut m) = mats.get_mut(h) {
                m.perceptual_roughness = rough;
            }
        }
        if !first {
            bevel.rebuild = true;
        }
    }
    // a repaint is a new pair of images, handed to every bevelled material (a material doesn't
    // pick up an image changed in place)
    if bevel.atlas.is_none() || bevel.repaint {
        bevel.repaint = false;
        let (nb, ob) = atlas_pair(bevel.look);
        let set = (
            images.add(atlas_image(nb)),
            images.add(atlas_image(ob)),
            images.add(atlas_image(atlas_albedo(bevel.look))),
        );
        for h in bevel.mats.values() {
            if let Some(mut m) = mats.get_mut(h) {
                m.normal_map_texture = Some(set.0.clone());
                m.occlusion_texture = Some(set.1.clone());
                m.base_color_texture = Some(set.2.clone());
            }
        }
        if let Some((a, b, c)) = bevel.atlas.replace(set) {
            images.remove(&a);
            images.remove(&b);
            images.remove(&c);
        }
    }
    let (atlas, occlusion, albedo) = bevel.atlas.clone().expect("made above");
    // bevels are off at Low quality
    let on = bevel.on && studio.level != crate::lighting::Level::Low;
    if bevel.rebuild {
        // rebuild in place: the new mesh goes straight in, with no plain-looking frame between
        bevel.rebuild = false;
        bevel.meshes.clear();
        for (_, mut b, mut m, _) in &mut done {
            let Some(orig_mat) = mats.get(&b.mat).cloned() else {
                continue;
            };
            if orig_mat.base_color_texture.is_none() {
                continue;
            }
            let id = b.mesh.id();
            let r = match bevel.meshes.get(&id) {
                Some(r) => r.clone(),
                None => {
                    let Some(r) =
                        make_mesh(bevel, &mut meshes, &images, &assets, &b.mesh, &orig_mat)
                    else {
                        continue;
                    };
                    bevel.meshes.insert(id, r.clone());
                    r
                }
            };
            if let (Some(nm), true) = (r, b.swapped) {
                m.0 = nm;
                b.swapped = true;
            }
        }
    }
    // on/off: swap back and forth without rebuilding
    if *was_on != Some(on) {
        if was_on.is_some() {
            for (_, mut b, mut m, mut mt) in &mut done {
                if !b.swapped && on {
                    let nmat = bevel
                        .mats
                        .get(&(b.mat.id(), false))
                        .or_else(|| bevel.mats.get(&(b.mat.id(), true)));
                    if let (Some(Some(nm)), Some(nmat)) = (bevel.meshes.get(&b.mesh.id()), nmat) {
                        m.0 = nm.clone();
                        mt.0 = nmat.clone();
                        b.swapped = true;
                    }
                } else if b.swapped && !on {
                    m.0 = b.mesh.clone();
                    mt.0 = b.mat.clone();
                    b.swapped = false;
                }
            }
        }
        *was_on = Some(on);
    }
    for (e, mesh, mat, name) in &fresh {
        let Some(original) = mats.get(&mat.0).cloned() else {
            continue;
        };
        // only the palette-textured voxel materials
        if original.base_color_texture.is_none() {
            commands.entity(e).insert(Bevelled {
                mesh: mesh.0.clone(),
                mat: mat.0.clone(),
                swapped: false,
            });
            continue;
        }
        let rebuilt = match bevel.meshes.get(&mesh.0.id()) {
            Some(r) => r.clone(),
            None => {
                let Some(r) = make_mesh(bevel, &mut meshes, &images, &assets, &mesh.0, &original)
                else {
                    continue;
                };
                bevel.meshes.insert(mesh.0.id(), r.clone());
                r
            }
        };
        let mut tag = Bevelled {
            mesh: mesh.0.clone(),
            mat: mat.0.clone(),
            swapped: false,
        };
        if let Some(nm) = rebuilt {
            // eyes get a glossy, wet look: they catch a highlight from the lights
            let glossy = name.is_some_and(|n| n.as_str().starts_with("eyes"));
            let key = (mat.0.id(), glossy);
            let nmat = match bevel.mats.get(&key) {
                Some(h) => h.clone(),
                None => {
                    let mut m = original.clone();
                    // the colour comes from the mesh (palette colour per face) times the
                    // high-resolution detail atlas, which shares its UVs with the normal map
                    m.base_color_texture = Some(albedo.clone());
                    m.base_color_channel = UvChannel::Uv1;
                    m.normal_map_texture = Some(atlas.clone());
                    m.normal_map_channel = UvChannel::Uv1;
                    let model = model_of(&assets, mat.0.id());
                    let is_set = model.starts_with("sets");
                    if glossy {
                        m.perceptual_roughness = 0.12;
                        m.reflectance = 0.9;
                    } else if is_set {
                        bevel.matte.insert(mat.0.id());
                    } else if studio.on {
                        m.perceptual_roughness = crate::lighting::ROUGHNESS;
                    }
                    m.occlusion_texture = Some(occlusion.clone());
                    m.occlusion_channel = UvChannel::Uv1;
                    // glowing colours: an emissive copy of the palette with only those colours lit
                    if let (Some(table), Some((w, h, d))) =
                        (bevel.surfaces.get(&model), palette_of(&images, &original))
                    {
                        let glow: Vec<[u8; 3]> = table
                            .iter()
                            .filter(|(_, s)| **s == Surface::Glow)
                            .map(|(c, _)| *c)
                            .collect();
                        if !glow.is_empty() {
                            let mut mask = d.clone();
                            for px in mask.chunks_mut(4) {
                                if !glow.contains(&[px[0], px[1], px[2]]) {
                                    px[0] = 0;
                                    px[1] = 0;
                                    px[2] = 0;
                                }
                            }
                            let mut img = Image::new(
                                Extent3d {
                                    width: w as u32,
                                    height: h as u32,
                                    depth_or_array_layers: 1,
                                },
                                TextureDimension::D2,
                                mask,
                                TextureFormat::Rgba8UnormSrgb,
                                RenderAssetUsages::default(),
                            );
                            img.sampler = ImageSampler::nearest();
                            m.emissive_texture = Some(images.add(img));
                            m.emissive = LinearRgba::rgb(GLOW, GLOW, GLOW);
                        }
                    }
                    let h = mats.add(m);
                    bevel.mats.insert(key, h.clone());
                    h
                }
            };
            if on {
                commands
                    .entity(e)
                    .insert((Mesh3d(nm), MeshMaterial3d(nmat)));
                tag.swapped = true;
            }
        }
        commands.entity(e).insert(tag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x1x1 bar of voxels made of greedy quads, like the models' meshes.
    fn bar() -> Mesh {
        let v = 0.05;
        let (x1, y1, z1) = (2.0 * v, v, v);
        let mut p: Vec<[f32; 3]> = vec![];
        let mut n: Vec<[f32; 3]> = vec![];
        let mut idx: Vec<u32> = vec![];
        let mut quad = |c: [[f32; 3]; 4], nrm: [f32; 3]| {
            let b = p.len() as u32;
            p.extend_from_slice(&c);
            n.extend_from_slice(&[nrm; 4]);
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        };
        quad(
            [[0.0, 0.0, z1], [x1, 0.0, z1], [x1, y1, z1], [0.0, y1, z1]],
            [0.0, 0.0, 1.0],
        );
        quad(
            [
                [0.0, 0.0, 0.0],
                [0.0, y1, 0.0],
                [x1, y1, 0.0],
                [x1, 0.0, 0.0],
            ],
            [0.0, 0.0, -1.0],
        );
        quad(
            [[0.0, y1, 0.0], [0.0, y1, z1], [x1, y1, z1], [x1, y1, 0.0]],
            [0.0, 1.0, 0.0],
        );
        quad(
            [
                [0.0, 0.0, 0.0],
                [x1, 0.0, 0.0],
                [x1, 0.0, z1],
                [0.0, 0.0, z1],
            ],
            [0.0, -1.0, 0.0],
        );
        quad(
            [[x1, 0.0, 0.0], [x1, y1, 0.0], [x1, y1, z1], [x1, 0.0, z1]],
            [1.0, 0.0, 0.0],
        );
        quad(
            [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, z1],
                [0.0, y1, z1],
                [0.0, y1, 0.0],
            ],
            [-1.0, 0.0, 0.0],
        );
        let uv = vec![[0.25f32, 0.5]; p.len()];
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
    }

    #[test]
    fn tiles_lean_towards_outside_edges() {
        let look = Look {
            grid: 0.15,
            blend: false,
            ..Look::default()
        };
        let flat = tile_normal(0.5, 0.5, 0b1111, look);
        assert!(flat[2] > 0.999, "the middle of a face is flat");
        let e = tile_normal(0.99, 0.5, 0b0001, look);
        assert!(e[0] > 0.4 && e[2] > 0.3, "+u outside edge leans +x: {e:?}");
        let g = tile_normal(0.99, 0.5, 0b0000, look);
        assert!(
            g[0] > 0.0 && g[0] < e[0] * 0.6,
            "a seam is much softer than a bevel: {g:?}"
        );
        let c = tile_normal(0.02, 0.02, 0b1010, look);
        assert!(
            c[0] < -0.3 && c[1] < -0.3,
            "a corner leans both ways: {c:?}"
        );
        let off = tile_normal(
            0.99,
            0.5,
            0b0001,
            Look {
                bevel: 0.0,
                grid: 0.0,
                wobble: 0.0,
                detail: 0.0,
                ao: 0.0,
                blend: true,
            },
        );
        assert!(off[2] > 0.9999, "zero strength is flat");
        for (x, y) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let n = tile_normal(
                x,
                y,
                0b1111,
                Look {
                    bevel: 1.0,
                    grid: 1.0,
                    wobble: 1.0,
                    detail: 1.0,
                    ao: 1.0,
                    blend: true,
                },
            );
            assert!(
                (n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-4 && n[2] > 0.2,
                "{n:?}"
            );
        }
        // atlas: every mip level present, alpha opaque
        let bytes = atlas_pair(look).0;
        let total: usize = (0..MIPS).map(|m| (ATLAS >> m) * (ATLAS >> m) * 4).sum();
        assert_eq!(bytes.len(), total);
        // the centre of tile 0 (no outside edges) is the flat normal (128, 128, 255)
        let c = (TILE / 2 * ATLAS + TILE / 2) * 4;
        assert_eq!(&bytes[c..c + 4], &[128, 128, 255, 255]);
    }

    #[test]
    fn beard_hair_and_cloth_detail() {
        let look = Look {
            grid: 0.15,
            blend: false,
            ..Look::default()
        };
        // plain rows have none
        assert!(detail_slopes(0, look).is_none());
        let spread = |g: usize| {
            let s = detail_slopes(g, look).unwrap();
            let (mut sx, mut sy) = (0.0f32, 0.0f32);
            for d in &s {
                sx += d[0].abs();
                sy += d[1].abs();
            }
            (sx / s.len() as f32, sy / s.len() as f32)
        };
        let (bx, by) = spread(group_of(Surface::Stubble, 0, false));
        assert!(
            bx > 0.05 && by > 0.05 && (bx / by - 1.0).abs() < 0.5,
            "stubble is bumpy both ways: {bx} {by}"
        );
        let (vx, vy) = spread(group_of(Surface::Hair, 1, false));
        assert!(vx > 2.0 * vy, "hair down v ripples across u: {vx} {vy}");
        let (ux, uy) = spread(group_of(Surface::Hair, 1, true));
        assert!(uy > 2.0 * ux, "hair down u ripples across v: {ux} {uy}");
        // cloth tiles seamlessly: the left and right columns match the wrap
        let h = detail_heights(group_of(Surface::Cloth, 0, false)).unwrap();
        let step = (h[1] - h[0]).abs().max(1e-6);
        assert!((h[TILE - 1] - h[0]).abs() <= step * 2.5);
        // variants differ
        assert_ne!(detail_heights(1), detail_heights(2));
        // detail 0 is flat again
        assert!(
            detail_slopes(
                1,
                Look {
                    detail: 0.0,
                    ..look
                }
            )
            .unwrap()
            .iter()
            .all(|d| d[0] == 0.0 && d[1] == 0.0)
        );
        // occlusion: stubble has dark whisker dots, plain tiles are open in the middle
        let ao = detail_occlusion(1).unwrap();
        let dark = ao.iter().filter(|v| **v < 0.6).count();
        assert!(dark > 200 && dark < TILE * TILE / 2, "whisker dots: {dark}");
        assert_eq!(tile_occlusion(0.5, 0.5, 0, 0, None, look), 1.0);
        assert!(
            tile_occlusion(0.99, 0.5, 0, 0, None, look) < 0.95,
            "seams are a little darker"
        );
        assert_eq!(
            tile_occlusion(0.99, 0.5, 1, 0, None, look),
            1.0,
            "outside edges aren't"
        );
        let (nb, ob) = atlas_pair(look);
        assert_eq!(nb.len(), ob.len());
        // colour detail: plain rows have none; stubble has dark whiskers on a light base; hair
        // has dark gaps where the height field is low (pattern and bumps line up)
        assert!(detail_albedo(0, look).is_none());
        let st = detail_albedo(1, look).unwrap();
        let dark = st.iter().filter(|v| **v < 0.75).count();
        assert!(
            dark > 100 && dark < TILE * TILE / 3 && st.iter().any(|v| *v > 1.0),
            "whiskers: {dark}"
        );
        let mean_st = st.iter().sum::<f32>() / st.len() as f32;
        assert!(
            (mean_st - 1.0).abs() < 0.15,
            "keeps the block's colour on average: {mean_st}"
        );
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        let g = group_of(Surface::Hair, 0, false);
        let (hh, ha) = (detail_heights(g).unwrap(), detail_albedo(g, look).unwrap());
        let (lo, hi) = (
            hh.iter().copied().fold(f32::MAX, f32::min),
            hh.iter().copied().fold(f32::MIN, f32::max),
        );
        let mean = |f: &dyn Fn(f32) -> bool| {
            let v: Vec<f32> = hh
                .iter()
                .zip(&ha)
                .filter(|(h, _)| f(**h))
                .map(|(_, a)| *a)
                .collect();
            v.iter().sum::<f32>() / v.len().max(1) as f32
        };
        assert!(
            mean(&|h| h > lo + 0.7 * (hi - lo)) > mean(&|h| h < lo + 0.1 * (hi - lo)) + 0.2,
            "strand tops are lighter than the gaps"
        );
        assert_eq!(atlas_albedo(look).len(), nb.len() * 2);
        // the surfaces file
        let s = parse_surfaces(
            r##"{"models/a.glb": {"#b4805e": "stubble", "704626": "hair", "zz": "hair", "304e96": "silk"}}"##,
        );
        let a = &s["models/a.glb"];
        assert_eq!(a.get(&[0xb4, 0x80, 0x5e]), Some(&Surface::Stubble));
        assert_eq!(a.get(&[0x70, 0x46, 0x26]), Some(&Surface::Hair));
        assert_eq!(a.len(), 2, "bad colours and unknown surfaces are skipped");
        // the real file parses and names the lead's beard
        let real = parse_surfaces(
            &std::fs::read_to_string(crate::asset_base().join("data/surfaces.json")).unwrap(),
        );
        assert_eq!(
            real["models/scene_all.glb"].get(&[0xb4, 0x80, 0x5e]),
            Some(&Surface::Stubble)
        );
    }

    #[test]
    fn rebuild_cuts_quads_into_voxel_faces_and_finds_edges() {
        let m = rebuild(
            &bar(),
            Look {
                wobble: 0.0,
                ..Look::default()
            },
            &|_| (Surface::Plain, [0.5, 0.25, 1.0]),
        )
        .expect("a voxel mesh");
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        // 2 cells on each long side (4 sides) + 1 on each end = 10 faces
        assert_eq!(p.len(), 10 * 4);
        let Some(VertexAttributeValues::Float32x2(u1)) = m.attribute(Mesh::ATTRIBUTE_UV_1) else {
            panic!()
        };
        let Some(VertexAttributeValues::Float32x4(t)) = m.attribute(Mesh::ATTRIBUTE_TANGENT) else {
            panic!()
        };
        let Some(VertexAttributeValues::Float32x3(n)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else {
            panic!()
        };
        // which tile each face uses: the front's two cells each have 3 outside edges, the shared
        // side between them is flat
        let mut masks = vec![];
        for f in 0..10 {
            let uv = u1[f * 4];
            let (tx, ty) = (
                (uv[0] * TILES as f32) as usize,
                (uv[1] * TILES as f32) as usize,
            );
            assert_eq!(ty, 0, "plain faces use the plain row");
            masks.push(tx as u8);
            // tangent frame: perpendicular to the normal
            let (tt, nn) = (
                Vec3::new(t[f * 4][0], t[f * 4][1], t[f * 4][2]),
                Vec3::from(n[f * 4]),
            );
            assert!(tt.dot(nn).abs() < 1e-5);
        }
        let threes = masks.iter().filter(|m| m.count_ones() == 3).count();
        let fours = masks.iter().filter(|m| m.count_ones() == 4).count();
        assert_eq!((threes, fours), (8, 2), "{masks:?}");
        // the triangles face outward
        let Some(Indices::U32(ix)) = m.indices() else {
            panic!()
        };
        for tri in ix.chunks(3) {
            let (a, b, c) = (
                Vec3::from(p[tri[0] as usize]),
                Vec3::from(p[tri[1] as usize]),
                Vec3::from(p[tri[2] as usize]),
            );
            assert!((b - a).cross(c - a).dot(Vec3::from(n[tri[0] as usize])) > 0.0);
        }
        // the grid: the usual voxel size even for a one-box arm, a finer one for fine props
        assert_eq!(grid_of(&[0.15, 0.3, 0.15, 0.3]), Some(VOXEL));
        assert_eq!(grid_of(&[0.015, 0.03, 0.045, 0.015]), Some(0.015));
        // corner shading: an L of three voxels darkens the inside corner, an outside corner stays lit
        let lit = rebuild(
            &bar(),
            Look {
                wobble: 0.0,
                ao: 1.0,
                ..Look::default()
            },
            &|_| (Surface::Plain, [0.5, 0.25, 1.0]),
        )
        .unwrap();
        let Some(VertexAttributeValues::Float32x4(cs)) = lit.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!()
        };
        assert!(
            cs.iter().all(|c| c[..3] == [0.5, 0.25, 1.0]),
            "the palette colour, unshaded: a lone bar has nothing to shade it"
        );
        // a sphere-ish mesh isn't a voxel mesh
        assert!(
            rebuild(&Sphere::new(1.0).mesh().build(), Look::default(), &|_| (
                Surface::Plain,
                [1.0; 3]
            ))
            .is_none()
        );
    }
}

/// Every set and character, rebuilt from its real file: nothing the GPU would turn into a NaN
/// (non-finite numbers, zero-area triangles, normals or tangents that vanish or lie in the face).
#[cfg(test)]
mod real_models {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    /// The meshes of a .glb (first primitive of each), as Bevy meshes with what rebuild reads.
    fn load(path: &std::path::Path) -> Vec<(String, Mesh)> {
        let b = std::fs::read(path).unwrap();
        let jl = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&b[20..20 + jl]).unwrap();
        let bin = &b[20 + jl + 8..];
        let acc = |i: usize| -> (Vec<f32>, Vec<u32>, usize) {
            let a = &j["accessors"][i];
            let v = &j["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let off = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
            let n = a["count"].as_u64().unwrap() as usize;
            let comps = match a["type"].as_str().unwrap() {
                "SCALAR" => 1,
                "VEC2" => 2,
                "VEC3" => 3,
                _ => 4,
            };
            let ct = a["componentType"].as_u64().unwrap();
            let stride = v["byteStride"].as_u64().unwrap_or(0) as usize;
            let size = match ct {
                5126 | 5125 => 4,
                5123 => 2,
                _ => 1,
            };
            let stride = if stride == 0 { size * comps } else { stride };
            let (mut f, mut u) = (vec![], vec![]);
            for k in 0..n {
                for c in 0..comps {
                    let at = off + k * stride + c * size;
                    match ct {
                        5126 => f.push(f32::from_le_bytes(bin[at..at + 4].try_into().unwrap())),
                        5125 => u.push(u32::from_le_bytes(bin[at..at + 4].try_into().unwrap())),
                        5123 => u.push(u16::from_le_bytes(bin[at..at + 2].try_into().unwrap()) as u32),
                        _ => u.push(bin[at] as u32),
                    }
                }
            }
            (f, u, comps)
        };
        let mut out = vec![];
        for m in j["meshes"].as_array().unwrap() {
            for (pi, pr) in m["primitives"].as_array().unwrap().iter().enumerate() {
                let at = &pr["attributes"];
                let (Some(pi_), Some(ni), Some(ui), Some(ii)) = (at["POSITION"].as_u64(), at["NORMAL"].as_u64(), at["TEXCOORD_0"].as_u64(), pr["indices"].as_u64()) else { continue };
                let p = acc(pi_ as usize).0;
                let n = acc(ni as usize).0;
                let uv = acc(ui as usize).0;
                let idx = acc(ii as usize).1;
                let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p.chunks(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n.chunks(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv.chunks(2).map(|c| [c[0], c[1]]).collect::<Vec<_>>())
                    .with_inserted_indices(Indices::U32(idx));
                out.push((format!("{}#{pi}", m["name"].as_str().unwrap_or("?")), mesh));
            }
        }
        out
    }

    fn check(name: &str, m: &Mesh) -> Result<(), String> {
        let f3 = |a| match m.attribute(a) {
            Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
            _ => vec![],
        };
        let p = f3(Mesh::ATTRIBUTE_POSITION);
        let n = f3(Mesh::ATTRIBUTE_NORMAL);
        let Some(VertexAttributeValues::Float32x4(t)) = m.attribute(Mesh::ATTRIBUTE_TANGENT) else { return Err(format!("{name}: no tangents")) };
        let Some(VertexAttributeValues::Float32x4(c)) = m.attribute(Mesh::ATTRIBUTE_COLOR) else { return Err(format!("{name}: no colours")) };
        let Some(VertexAttributeValues::Float32x2(u1)) = m.attribute(Mesh::ATTRIBUTE_UV_1) else { return Err(format!("{name}: no uv1")) };
        for k in 0..p.len() {
            let all = p[k].iter().chain(&n[k]).chain(&t[k]).chain(&c[k]).chain(&u1[k]);
            if all.clone().any(|v| !v.is_finite()) {
                return Err(format!("{name}: vertex {k} not finite"));
            }
            let (nv, tv) = (Vec3::from(n[k]), Vec3::new(t[k][0], t[k][1], t[k][2]));
            if nv.length() < 0.5 || tv.length() < 0.5 {
                return Err(format!("{name}: vertex {k} normal {nv} tangent {tv}"));
            }
            if nv.normalize().dot(tv.normalize()).abs() > 1e-4 {
                return Err(format!("{name}: vertex {k} tangent {tv} not square to the normal {nv}"));
            }
            if nv.normalize().cross(tv.normalize()).length() < 0.5 {
                return Err(format!("{name}: vertex {k} tangent {tv} along the normal {nv}"));
            }
            if !(0.0..=1.0).contains(&u1[k][0]) || !(0.0..=1.0).contains(&u1[k][1]) {
                return Err(format!("{name}: vertex {k} uv1 {:?} outside the atlas", u1[k]));
            }
        }
        if let Some(Indices::U32(ix)) = m.indices() {
            for (ti, tri) in ix.chunks(3).enumerate() {
                let (a, b, cc) = (Vec3::from(p[tri[0] as usize]), Vec3::from(p[tri[1] as usize]), Vec3::from(p[tri[2] as usize]));
                if (b - a).cross(cc - a).length() < 1e-9 {
                    return Err(format!("{name}: triangle {ti} has no area at {a}"));
                }
                // slivers: the shortest side far shorter than the longest
                let sides = [(b - a).length(), (cc - b).length(), (a - cc).length()];
                let (lo, hi) = (sides.iter().copied().fold(f32::MAX, f32::min), sides.iter().copied().fold(0.0, f32::max));
                if lo < hi * 1e-3 {
                    return Err(format!("{name}: triangle {ti} is a sliver ({lo} vs {hi}) at {a}"));
                }
            }
        }
        Ok(())
    }

    /// Neighbouring faces meet exactly: every corner position that appears comes out as one
    /// bit pattern (no near-duplicates that would open pinhole cracks).
    fn watertight(name: &str, m: &Mesh) -> Result<(), String> {
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { return Ok(()) };
        let mut seen: std::collections::HashMap<[i64; 3], [u32; 3]> = std::collections::HashMap::new();
        for v in p {
            let k = [(v[0] * 1e5).round() as i64, (v[1] * 1e5).round() as i64, (v[2] * 1e5).round() as i64];
            let bits = [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
            if let Some(b) = seen.insert(k, bits) {
                if b != bits {
                    return Err(format!("{name}: two slightly different copies of corner {v:?}"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn rebuilt_models_are_clean() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<_> = ["assets/sets", "assets/models"].iter().flat_map(|d| std::fs::read_dir(root.join(d)).unwrap().flatten().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "glb")).collect();
        files.sort();
        let mut bad = vec![];
        let mut count = 0;
        for f in &files {
            for (name, mesh) in load(f) {
                if let Some(r) = rebuild(&mesh, Look::default(), &|_| (Surface::Plain, [0.5; 3])) {
                    count += 1;
                    let label = format!("{}:{name}", f.file_name().unwrap().to_string_lossy());
                    if let Err(e) = check(&label, &r).and_then(|_| watertight(&label, &r)) {
                        bad.push(e);
                    }
                }
            }
        }
        assert!(count > 50, "rebuilt {count} meshes");
        assert!(bad.is_empty(), "{} bad meshes:\n{}", bad.len(), bad.join("\n"));
    }
}

#[cfg(test)]
mod atlas_checks {
    use super::*;
    #[test]
    fn atlas_normals_never_vanish() {
        let look = Look::default();
        let (nb, ob) = atlas_pair(look);
        let mut worst = f32::MAX;
        for px in nb.chunks(4) {
            let d = |b: u8| b as f32 / 255.0 * 2.0 - 1.0;
            let l = (d(px[0]).powi(2) + d(px[1]).powi(2) + d(px[2]).powi(2)).sqrt();
            worst = worst.min(l);
        }
        assert!(worst > 0.5, "shortest normal {worst}");
        assert!(ob.chunks(4).all(|p| p[0] > 0));
        let a = atlas_albedo(look);
        let bad = a.chunks(2).map(|h| u16::from_le_bytes([h[0], h[1]])).filter(|h| (h & 0x7c00) == 0x7c00).count();
        assert_eq!(bad, 0, "inf/nan halves in the colour atlas");
    }
}
