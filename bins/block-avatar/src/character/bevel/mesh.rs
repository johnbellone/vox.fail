//! Rebuilding a model's mesh with bevelled edges, voxel by voxel.

#[allow(unused_imports)]
use super::*;

/// The models' voxel size in mesh units.
pub const VOXEL: f32 = 0.05;

/// The grid a mesh is built on: 0.05 when half or more of its quad sides are whole multiples of it
/// (true of every character, hat and set), else the smallest side that most sides are
/// multiples of. A mesh that is one big box can't tell, so the usual size wins.
pub(super) fn grid_of(sides: &[f32]) -> Option<f32> {
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

pub(super) fn axis_vec(axis: usize, pos: bool) -> Vec3 {
    let mut v = Vec3::ZERO;
    v[axis] = if pos { 1.0 } else { -1.0 };
    v
}

/// The grid offset along one axis: where voxel centres sit, as a fraction of a voxel.
pub(super) fn frac_off(centre: f32, vox: f32) -> f32 {
    let k = centre / vox;
    (k - k.round()) * vox
}
