//! Putting the bevels on models as they load, and taking them off again.

#[allow(unused_imports)]
use super::*;

/// The palette image of a material as (width, height, RGBA bytes), when it's readable.
pub(super) fn palette_of(images: &Assets<Image>, mat: &StandardMaterial) -> Option<(usize, usize, Vec<u8>)> {
    let img = images.get(mat.base_color_texture.as_ref()?)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    img.data
        .clone()
        .filter(|d| d.len() >= w * h * 4)
        .map(|d| (w, h, d))
}

/// The model file a mesh or material came from ("models/scene_all.glb").
pub(super) fn model_of(assets: &AssetServer, id: impl Into<bevy::asset::UntypedAssetId>) -> String {
    assets
        .get_path(id)
        .map(|p| p.path().to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// Rebuilds one mesh with this model's beard, hair and cloth colours.
pub(super) fn make_mesh(
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
pub(super) const GLOW: f32 = 3.0;

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
