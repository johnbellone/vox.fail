//! The detail atlas: hand-made height, normal, occlusion and colour tiles for each surface, packed with their mip levels.

#[allow(unused_imports)]
use super::*;

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
pub(super) fn detail_slopes(group: usize, look: Look) -> Option<Vec<[f32; 2]>> {
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
pub(super) fn mip_bytes(level: Vec<[f32; 3]>, is_normal: bool) -> Vec<u8> {
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
pub(super) fn f16_bits(v: f32) -> u16 {
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

pub(super) fn mip_encode(
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

pub(super) fn enc(v: f32) -> u8 {
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
pub(super) fn tile_uv(group: usize, mask: EdgeMask) -> (f32, f32, f32, f32) {
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
