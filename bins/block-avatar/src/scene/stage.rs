//! Output stages and retro sprite mode.
//!
//! The window and the webcam each have a 3D camera and a 2D overlay camera drawing onto the same
//! final picture. The overlay carries the speech bubbles and chat labels (so they show in OBS and
//! in the virtual camera alike). In retro mode the 3D camera renders into a small image instead
//! (the final size divided by the pixel size), and the overlay shows that image through
//! `assets/shaders/retro.wesl`: each big pixel snaps to the character's own palette (in three
//! shades), and the silhouette gets a one-pixel dark outline, like the original 2D sprites.

use bevy::{
    asset::RenderAssetUsages,
    camera::{ClearColorConfig, RenderTarget},
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType, TextureFormat, TextureUsages},
    shader::ShaderRef,
    ui_render::prelude::UiMaterial,
    window::PrimaryWindow,
};

pub const MAX_PALETTE: usize = 256;

#[derive(ShaderType, Clone, Copy, Debug)]
pub struct RetroParams {
    pub palette: [Vec4; MAX_PALETTE],
    pub count: u32,
    pub size: UVec2,
    pub outline: Vec4,
}

#[derive(AsBindGroup, Asset, TypePath, Clone, Debug)]
pub struct RetroMaterial {
    #[uniform(0)]
    pub params: RetroParams,
    #[texture(1)]
    pub image: Handle<Image>,
}

impl UiMaterial for RetroMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/retro.wesl".into()
    }
}

/// One output: the window or the webcam.
pub struct StageView {
    pub window: bool,
    /// size of an image target (the webcam, the portrait clip view); None for the window
    pub image_size: Option<UVec2>,
    /// the portrait recording for shorts
    pub clip: bool,
    pub cam3d: Entity,
    pub overlay: Entity,
    /// root UI node on the overlay; bubbles and labels are spawned under it
    pub root: Entity,
    retro_node: Entity,
    final_target: RenderTarget,
    lowres: Option<(Handle<Image>, UVec2)>,
    material: Option<Handle<RetroMaterial>>,
    /// which target the 3D camera is on now (Some(size) = the retro image of that size)
    on_lowres: Option<UVec2>,
    /// a new retro image waits a couple of frames before the camera moves onto it, so the render
    /// side has it ready (switching in the same frame mismatches the depth buffer size)
    pending: Option<(Handle<Image>, UVec2, u8)>,
}

#[derive(Resource)]
pub struct Stages {
    pub views: Vec<StageView>,
    pub retro: bool,
    /// size of one retro pixel, in screen pixels
    pub scale: u32,
    palette: Vec<[u8; 3]>,
    palette_for: String,
    pub transparent_window: bool,
    /// the viewer's character on stage, if any: its colours join the palette
    pub guest_model: Option<String>,
    /// the stage set, if any (show.rs): its colours join the palette too
    pub set_model: Option<String>,
}

impl Stages {
    pub fn new(retro: bool, scale: u32, transparent_window: bool) -> Self {
        Stages {
            views: vec![],
            retro,
            scale: scale.clamp(2, 12),
            palette: vec![],
            palette_for: String::new(),
            transparent_window,
            guest_model: None,
            set_model: None,
        }
    }
}

/// Startup, after the webcam camera exists: overlay cameras and their UI roots.
pub fn setup(
    mut commands: Commands,
    mut stages: ResMut<Stages>,
    main: Query<Entity, With<crate::MainCamera>>,
    cam: Res<crate::webcam::Webcam>,
    clips: Res<crate::clips::Clips>,
) {
    let Ok(main) = main.single() else { return };
    let mut make = |cam3d: Entity, target: RenderTarget, order: isize, window: bool| {
        let mut overlay = commands.spawn((
            Camera2d,
            Camera {
                order,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            target.clone(),
        ));
        if window {
            overlay.insert(IsDefaultUiCamera);
        }
        let overlay = overlay.id();
        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                UiTargetCamera(overlay),
                GlobalZIndex(-10),
            ))
            .id();
        let retro_node = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                Visibility::Hidden,
                ChildOf(root),
            ))
            .id();
        StageView {
            window,
            image_size: None,
            clip: false,
            cam3d,
            overlay,
            root,
            retro_node,
            final_target: target,
            lowres: None,
            material: None,
            on_lowres: None,
            pending: None,
        }
    };
    let v = make(
        main,
        RenderTarget::Window(bevy::window::WindowRef::Primary),
        1,
        true,
    );
    stages.views.push(v);
    if let (Some(c), Some(img)) = (cam.camera_entity(), cam.image_handle()) {
        let mut v = make(c, RenderTarget::Image(img.into()), -2, false);
        v.image_size = Some(UVec2::new(cam.width, cam.height));
        stages.views.push(v);
    }
    // the portrait recording for shorts (clips.rs)
    if let (Some(c), Some(img)) = (clips.camera, clips.image.clone()) {
        let mut v = make(c, RenderTarget::Image(img.into()), -4, false);
        v.image_size = Some(UVec2::new(crate::clips::W, crate::clips::H));
        v.clip = true;
        stages.views.push(v);
    }
}

/// Unique colours of a model's palette texture, read straight from the .glb.
pub fn palette_from_glb(path: &std::path::Path) -> Option<Vec<[u8; 3]>> {
    let b = std::fs::read(path).ok()?;
    let n = u32::from_le_bytes(b.get(12..16)?.try_into().ok()?) as usize;
    let j: serde_json_lite::Value =
        serde_json_lite::parse(std::str::from_utf8(b.get(20..20 + n)?).ok()?)?;
    let bin_start = 20 + n + 8;
    let img = j.get("images")?.idx(0)?;
    let bv = j
        .get("bufferViews")?
        .idx(img.get("bufferView")?.num()? as usize)?;
    let off = bv.get("byteOffset").and_then(|v| v.num()).unwrap_or(0.0) as usize;
    let len = bv.get("byteLength")?.num()? as usize;
    let png = b.get(bin_start + off..bin_start + off + len)?;
    let image = Image::from_buffer(
        png,
        ImageType::MimeType("image/png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD,
    )
    .ok()?;
    let data = image.data.as_ref()?;
    let mut out: Vec<[u8; 3]> = vec![];
    for px in data.chunks_exact(4) {
        if px[3] > 0 && !out.contains(&[px[0], px[1], px[2]]) {
            out.push([px[0], px[1], px[2]]);
        }
    }
    Some(out)
}

fn params(palette: &[[u8; 3]], size: UVec2) -> RetroParams {
    // each colour in three shades, so the lighting still reads after snapping to the palette
    let mut pal = [Vec4::ZERO; MAX_PALETTE];
    let mut count = 0;
    'outer: for shade in [1.0f32, 0.78, 0.58] {
        for c in palette {
            if count == MAX_PALETTE {
                break 'outer;
            }
            pal[count] = Vec4::new(
                c[0] as f32 / 255.0 * shade,
                c[1] as f32 / 255.0 * shade,
                c[2] as f32 / 255.0 * shade,
                1.0,
            );
            count += 1;
        }
    }
    RetroParams {
        palette: pal,
        count: count as u32,
        size,
        outline: LinearRgba::from(Color::srgb_u8(26, 22, 30)).to_vec4(),
    }
}

/// Keeps every view in the right mode: render target, clear colours, low-res image size, palette.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut stages: ResMut<Stages>,
    av: Res<crate::Avatar>,
    clear: Res<ClearColor>,
    windows: Query<&Window, With<PrimaryWindow>>,
    webcam: Res<crate::webcam::Webcam>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<RetroMaterial>>,
    mut cams: Query<(&mut Camera, &mut RenderTarget)>,
    mut nodes: Query<&mut Visibility>,
    mut commands: Commands,
) {
    // palette follows the character
    let model = av.models[av.model].1.clone();
    let key = format!(
        "{model}|{}|{}",
        stages.guest_model.as_deref().unwrap_or(""),
        stages.set_model.as_deref().unwrap_or("")
    );
    if stages.palette_for != key {
        let path = crate::asset_base().join("assets").join(&model);
        let mut pal = palette_from_glb(&path).unwrap_or_else(|| {
            vec![
                [200, 40, 44],
                [244, 226, 188],
                [52, 80, 160],
                [240, 240, 240],
                [40, 30, 30],
            ]
        });
        for g in [stages.guest_model.clone(), stages.set_model.clone()]
            .into_iter()
            .flatten()
        {
            for c in
                palette_from_glb(&crate::asset_base().join("assets").join(g)).unwrap_or_default()
            {
                if !pal.contains(&c) {
                    pal.push(c);
                }
            }
        }
        stages.palette = pal;
        stages.palette_for = key;
        for v in &stages.views {
            if let Some(mut m) = v.material.as_ref().and_then(|h| mats.get_mut(h)) {
                m.params = params(&stages.palette, m.params.size);
            }
        }
    }
    let retro = stages.retro;
    let scale = stages.scale;
    let transparent = stages.transparent_window;
    let win_size = windows
        .single()
        .ok()
        .map(|w| UVec2::new(w.physical_width(), w.physical_height()));
    let palette = stages.palette.clone();
    for v in stages.views.iter_mut() {
        let full = if v.window {
            win_size.unwrap_or(UVec2::new(900, 900))
        } else {
            v.image_size
                .unwrap_or(UVec2::new(webcam.width, webcam.height))
        };
        let want = UVec2::new(full.x.div_ceil(scale).max(8), full.y.div_ceil(scale).max(8));
        if retro
            && v.lowres.as_ref().is_none_or(|l| l.1 != want)
            && v.pending.as_ref().is_none_or(|p| p.1 != want)
        {
            let mut img =
                Image::new_target_texture(want.x, want.y, TextureFormat::Rgba8UnormSrgb, None);
            img.texture_descriptor.usage |=
                TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
            img.sampler = ImageSampler::nearest();
            v.pending = Some((images.add(img), want, 2));
        }
        if let Some((h, size, wait)) = v.pending.take() {
            if wait > 0 {
                v.pending = Some((h, size, wait - 1));
            } else {
                let m = mats.add(RetroMaterial {
                    params: params(&palette, size),
                    image: h.clone(),
                });
                commands
                    .entity(v.retro_node)
                    .insert(MaterialNode(m.clone()));
                v.lowres = Some((h, size));
                v.material = Some(m);
                v.on_lowres = None; // re-target below
            }
        }
        let retro = retro && v.lowres.is_some();
        // background the overlay paints under the retro picture
        let bg = if v.window {
            if transparent && clear.0.alpha() < 1.0 {
                Color::NONE
            } else {
                clear.0
            }
        } else if clear.0.alpha() < 1.0 {
            Color::srgb_u8(0, 177, 64)
        } else {
            clear.0
        };
        if let Ok((mut c, mut t)) = cams.get_mut(v.cam3d) {
            let want_on = if retro {
                v.lowres.as_ref().map(|l| l.1)
            } else {
                None
            };
            if v.on_lowres != want_on {
                *t = if retro {
                    RenderTarget::Image(v.lowres.as_ref().unwrap().0.clone().into())
                } else {
                    v.final_target.clone()
                };
                v.on_lowres = want_on;
            }
            if retro {
                c.clear_color = ClearColorConfig::Custom(Color::NONE);
            } else if v.window {
                c.clear_color = ClearColorConfig::Default;
            }
            // (the webcam's own system sets its normal background)
        }
        if let Ok((mut c, _)) = cams.get_mut(v.overlay) {
            c.clear_color = if retro {
                ClearColorConfig::Custom(bg)
            } else {
                ClearColorConfig::None
            };
        }
        if let Ok(mut vis) = nodes.get_mut(v.retro_node) {
            let retro = retro && v.on_lowres.is_some();
            *vis = if retro {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// The 3D camera's viewport position of a world point, mapped into the view's final picture
/// (the 3D camera may be drawing into the small retro image).
pub fn to_final(
    view: &StageView,
    cam: &Camera,
    cam_tf: &GlobalTransform,
    world: Vec3,
    final_size: Vec2,
) -> Option<Vec2> {
    let p = cam.world_to_viewport(cam_tf, world).ok()?;
    let src = cam.logical_viewport_size()?;
    let _ = view;
    Some(p * final_size / src)
}

/// Tiny JSON reader, just enough for the glb header (avoids a serde dependency).
mod serde_json_lite {
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub enum Value {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Value>),
        Obj(Vec<(String, Value)>),
    }
    impl Value {
        pub fn get(&self, k: &str) -> Option<&Value> {
            match self {
                Value::Obj(v) => v.iter().find(|(a, _)| a == k).map(|(_, b)| b),
                _ => None,
            }
        }
        pub fn idx(&self, i: usize) -> Option<&Value> {
            match self {
                Value::Arr(v) => v.get(i),
                _ => None,
            }
        }
        pub fn num(&self) -> Option<f64> {
            match self {
                Value::Num(n) => Some(*n),
                _ => None,
            }
        }
    }
    pub fn parse(s: &str) -> Option<Value> {
        let b = s.as_bytes();
        let mut i = 0;
        let v = value(b, &mut i)?;
        Some(v)
    }
    fn ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && (b[*i] as char).is_ascii_whitespace() {
            *i += 1;
        }
    }
    fn value(b: &[u8], i: &mut usize) -> Option<Value> {
        ws(b, i);
        match *b.get(*i)? {
            b'{' => {
                *i += 1;
                let mut v = vec![];
                loop {
                    ws(b, i);
                    if b.get(*i)? == &b'}' {
                        *i += 1;
                        return Some(Value::Obj(v));
                    }
                    let Value::Str(k) = value(b, i)? else {
                        return None;
                    };
                    ws(b, i);
                    *i += 1; // :
                    let val = value(b, i)?;
                    v.push((k, val));
                    ws(b, i);
                    if b.get(*i)? == &b',' {
                        *i += 1;
                    }
                }
            }
            b'[' => {
                *i += 1;
                let mut v = vec![];
                loop {
                    ws(b, i);
                    if b.get(*i)? == &b']' {
                        *i += 1;
                        return Some(Value::Arr(v));
                    }
                    v.push(value(b, i)?);
                    ws(b, i);
                    if b.get(*i)? == &b',' {
                        *i += 1;
                    }
                }
            }
            b'"' => {
                *i += 1;
                let mut s = String::new();
                while *b.get(*i)? != b'"' {
                    if b[*i] == b'\\' {
                        *i += 1;
                        match b.get(*i)? {
                            b'n' => s.push('\n'),
                            b't' => s.push('\t'),
                            b'u' => {
                                let h = std::str::from_utf8(b.get(*i + 1..*i + 5)?).ok()?;
                                s.push(
                                    char::from_u32(u32::from_str_radix(h, 16).ok()?).unwrap_or('?'),
                                );
                                *i += 4;
                            }
                            c => s.push(*c as char),
                        }
                        *i += 1;
                    } else {
                        let start = *i;
                        while *i < b.len() && b[*i] != b'"' && b[*i] != b'\\' {
                            *i += 1;
                        }
                        s.push_str(std::str::from_utf8(&b[start..*i]).ok()?);
                    }
                }
                *i += 1;
                Some(Value::Str(s))
            }
            b't' => {
                *i += 4;
                Some(Value::Bool(true))
            }
            b'f' => {
                *i += 5;
                Some(Value::Bool(false))
            }
            b'n' => {
                *i += 4;
                Some(Value::Null)
            }
            _ => {
                let start = *i;
                while *i < b.len()
                    && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    *i += 1;
                }
                std::str::from_utf8(&b[start..*i])
                    .ok()?
                    .parse()
                    .ok()
                    .map(Value::Num)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_each_models_palette() {
        let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join("assets/models");
        for f in ["scene_all.glb", "dog.glb", "roasted_riot.glb"] {
            let p = super::palette_from_glb(&dir.join(f)).unwrap();
            assert!(p.len() > 20 && p.len() <= 256, "{f}: {}", p.len());
        }
    }
}
