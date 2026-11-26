//! Tests: the tiles, real models, and the atlas.

#[allow(unused_imports)]
use super::*;

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
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
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
