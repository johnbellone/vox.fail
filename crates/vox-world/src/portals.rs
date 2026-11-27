//! Portals: echoes of other places.
//!
//! A world's settings can name doorways that lead somewhere else (`"portals"` in its level's
//! `"world"`). As you walk towards one, an echo of the place beyond fades into it: the other
//! level, under its own sky and light, seen from where you'd be if the doorway really joined
//! the two places. Far off it's faint and pale; close up it's solid. Walk through and you're
//! there (another world server, or somewhere else in this one).
//!
//! ```json
//! "portals": [{"name": "yard", "at": [0, 0, -39.5], "axis": "z", "size": [8, 16],
//!              "level": "yard", "url": ":7781", "arrive": [0, 0, 11.5], "yaw": 0}]
//! ```
//!
//! - `at`: the middle of the doorway's sill, in blocks; `axis`: which way you walk through it
//!   (`"z"` north and south, `"x"` east and west); `size`: width and height in blocks.
//! - `level`: what the echo shows (`data/levels/<level>.json`, as built: not other players'
//!   edits since).
//! - `url`: the world it leads to (`":7781"` = the same host, that port). Without one it leads
//!   somewhere else in this world.
//! - `arrive`: the matching point on the other side (the other doorway's sill, if it has one),
//!   in blocks; `yaw`: which way you face there after walking through northwards (axis z) or
//!   westwards (axis x), degrees like the camera's (0 north, 90 west). Walk through the other
//!   way and you come out facing the other way.
//!
//! How it's drawn: each destination level is meshed once (in the background) and placed far
//! off on its own render layer, with its own sky dome, light, haze and environment map. Each
//! portal has a camera there that moves as the main camera's mirror through the doorway and
//! renders (HDR, half size) to a texture; the doorway is a quad that shows that texture by
//! screen position (`shaders/portal.wgsl`), shimmering at its edge. Its near plane is pushed
//! out to the doorway, so what stands behind the other side's doorway doesn't block the view.

use std::{
    collections::HashMap,
    f32::consts::{FRAC_PI_2, PI},
    sync::{Arc, Mutex},
};

use bevy::{
    asset::RenderAssetUsages,
    camera::{Hdr, ImageRenderTarget, RenderTarget, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    light::{CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::render_resource::{AsBindGroup, TextureFormat},
    shader::ShaderRef,
    window::PrimaryWindow,
};
use serde_json::Value;

use crate::blocks::{Blocks, Meshes};
use crate::outdoors::{FloorExt, FloorMaterial, SkyLook, SkyMaterial, Style};
use crate::client::WorldClient;

/// The echo fades in from this far (m) and is solid this close.
const FAR: f32 = 34.0;
const NEAR: f32 = 10.0;
/// Where the echoes are put, out of the way (m), and how far apart.
const AWAY: f32 = 6000.0;
const APART: f32 = 3000.0;
/// No going straight back through a portal for this long after coming out (s).
const COOLDOWN: f64 = 1.0;

/// A doorway to somewhere else (see the module's docs).
#[derive(Clone, Debug, PartialEq)]
pub struct PortalDef {
    pub name: String,
    /// the middle of the sill, blocks
    pub at: Vec3,
    /// walked through along x (else along z)
    pub along_x: bool,
    /// width and height, blocks
    pub size: Vec2,
    pub level: String,
    pub url: Option<String>,
    /// the matching point on the other side, blocks
    pub arrive: Vec3,
    /// which way walking through (north for z, west for x) faces there, radians
    pub yaw: f32,
}

/// A world's portals from its settings.
pub fn parse(settings: &Value) -> Vec<PortalDef> {
    let Some(list) = settings.get("portals").and_then(|v| v.as_array()) else { return Vec::new() };
    let v3 = |v: &Value, k: &str| -> Option<Vec3> {
        let a = v.get(k)?.as_array()?;
        Some(Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
    };
    list.iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let size = p.get("size").and_then(|s| s.as_array()).map_or(Vec2::new(4.0, 6.0), |s| {
                Vec2::new(
                    s.first().and_then(|v| v.as_f64()).unwrap_or(4.0) as f32,
                    s.get(1).and_then(|v| v.as_f64()).unwrap_or(6.0) as f32,
                )
            });
            Some(PortalDef {
                name: p.get("name").and_then(|v| v.as_str()).map_or_else(|| format!("portal {}", i + 1), String::from),
                at: v3(p, "at")?,
                along_x: p.get("axis").and_then(|v| v.as_str()) == Some("x"),
                size: size.clamp(Vec2::splat(1.0), Vec2::splat(64.0)),
                level: p.get("level")?.as_str()?.to_string(),
                url: p.get("url").and_then(|v| v.as_str()).map(String::from),
                arrive: v3(p, "arrive")?,
                yaw: (p.get("yaw").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32).to_radians(),
            })
        })
        .collect()
}

impl PortalDef {
    /// Across the doorway (its normal) and along it.
    pub fn normal(&self) -> Vec3 {
        if self.along_x { Vec3::X } else { Vec3::Z }
    }
    fn across(&self) -> Vec3 {
        if self.along_x { Vec3::Z } else { Vec3::X }
    }

    /// How far the world turns going through from `side` (+1: from the +x/+z side, walking
    /// towards -x/-z; -1 the other way).
    pub fn turn(&self, side: f32) -> f32 {
        let base = if self.along_x { FRAC_PI_2 } else { 0.0 };
        self.yaw - base - if side < 0.0 { PI } else { 0.0 }
    }

    /// A point here (blocks) as the matching point on the other side, coming from `side`.
    pub fn carry(&self, p: Vec3, side: f32) -> Vec3 {
        // the world server works out arrivals with vox-core's doorway; this must agree with it
        self.arrive + Quat::from_rotation_y(self.turn(side)) * (p - self.at)
    }

    /// The same doorway as vox-core sees it (what the world server checks).
    pub fn door(&self) -> vox_core::portal::Door {
        vox_core::portal::Door {
            name: self.name.clone(),
            at: self.at.to_array(),
            along_x: self.along_x,
            size: self.size.to_array(),
            level: self.level.clone(),
            url: self.url.clone(),
            arrive: self.arrive.to_array(),
            yaw: self.yaw,
        }
    }

    /// Is `p` (blocks, feet) in the doorway's opening, sideways and up?
    pub fn within(&self, p: Vec3) -> bool {
        let d = p - self.at;
        d.dot(self.across()).abs() <= self.size.x * 0.5 && d.y >= -1.0 && d.y <= self.size.y - 1.0
    }

    /// Which side of the doorway `p` is on (blocks), signed distance.
    pub fn side_of(&self, p: Vec3) -> f32 {
        (p - self.at).dot(self.normal())
    }

    /// Its middle, blocks.
    fn centre(&self) -> Vec3 {
        self.at + Vec3::Y * self.size.y * 0.5
    }

    /// Its four corners, blocks.
    fn corners(&self) -> [Vec3; 4] {
        let a = self.across() * self.size.x * 0.5;
        let h = Vec3::Y * self.size.y;
        [self.at - a, self.at + a, self.at - a + h, self.at + a + h]
    }
}

/// How much of the echo shows from `d` metres away.
pub fn presence(d: f32) -> f32 {
    let t = ((FAR - d) / (FAR - NEAR)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The doorway's surface (`shaders/portal.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PortalMaterial {
    /// x: how present (0-1), y: the echo's ready (0/1), z: width / height, w: a seed
    #[uniform(0)]
    pub params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub echo: Handle<Image>,
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/portal.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// A camera drawing an echo (left alone by the studio's bloom, blur and grading: the main
/// camera does those to the doorway).
#[derive(Component)]
pub struct EchoCamera;

#[derive(Component)]
pub struct PortalSurface;

type Loaded = Result<(Meshes, Value), String>;

/// A destination level, meshed and placed on its own layer.
struct Scene {
    level: String,
    layer: usize,
    offset: Vec3,
    look: Option<SkyLook>,
    loading: Option<Arc<Mutex<Option<Loaded>>>>,
    env: Option<Handle<Image>>,
    failed: bool,
}

/// A portal in this world.
struct Live {
    def: PortalDef,
    scene: usize,
    quad: Entity,
    cam: Entity,
    image: Handle<Image>,
    mat: Handle<PortalMaterial>,
    px: UVec2,
    /// which side you were on last frame (blocks, signed)
    last_side: Option<f32>,
    /// the camera's look is set (fog, sky light)
    dressed: bool,
    /// how much shows now (eased)
    shown: f32,
}

#[derive(Resource, Default)]
pub struct Portals {
    built_for: u32,
    live: Vec<Live>,
    scenes: Vec<Scene>,
    calm_until: f64,
}

/// The doorway as a quad facing both ways, in its own frame: centred, +z its normal.
fn door_mesh(w: f32, h: f32) -> Mesh {
    let (x, y) = (w * 0.5, h * 0.5);
    let quad = [[-x, -y], [x, -y], [x, y], [-x, y]];
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    for n in [1.0f32, -1.0] {
        for q in quad {
            pos.push([q[0], q[1], 0.0]);
            nrm.push([0.0, 0.0, n]);
            uv.push([q[0] / w + 0.5, 0.5 - q[1] / h]);
        }
    }
    let idx = vec![0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

fn target(px: UVec2) -> Image {
    let mut img = Image::new_target_texture(px.x, px.y, TextureFormat::Rgba16Float, None);
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img
}

/// Mesh a level in the background, once its file is here (in the browser it's fetched first:
/// None means ask again next frame).
fn load(level: &str, rects: &HashMap<String, [f32; 4]>) -> Option<Arc<Mutex<Option<Loaded>>>> {
    let slot = Arc::new(Mutex::new(None));
    let Some(path) = crate::blocks::level_path(level) else {
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(format!("no level called {level}")));
        return Some(slot);
    };
    let Some(text) = crate::files::read_string(&path) else {
        crate::files::want(&path);
        return None;
    };
    let (out, level, rects) = (slot.clone(), level.to_string(), rects.clone());
    bevy::tasks::AsyncComputeTaskPool::get()
        .spawn(async move {
            let made = (|| -> Loaded {
                let pal = crate::blocks::Palette::load(&crate::files::base().join("data/blocks.json"))?;
                let cells = crate::blocks::load_level(&text, &pal)?;
                let settings = serde_json::from_str(&vox_core::level::settings(&text)).unwrap_or(Value::Null);
                Ok((crate::blocks::mesh_level(&cells, &pal, &rects), settings))
            })();
            if let Err(e) = &made {
                crate::note!("[portal] echo of {level}: {e}");
            }
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(made);
        })
        .detach();
    Some(slot)
}

/// Every frame in a world: build this world's portals, bring in their echoes, move the echo
/// cameras, fade the doorways in and out, and take you through.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut portals: ResMut<Portals>,
    mut w: ResMut<WorldClient>,
    blocks: Res<Blocks>,
    studio: Res<crate::WorldRender>,
    real: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    main: Query<(&Transform, &Projection, &Camera), (With<crate::WorldCamera>, Without<EchoCamera>)>,
    mut echoes: Query<(&mut Transform, &mut Projection, &mut Camera, &mut RenderTarget), (With<EchoCamera>, Without<crate::WorldCamera>)>,
    mut vis: Query<&mut Visibility, With<PortalSurface>>,
    (mut images, mut meshes, mut mats, mut sky_mats, mut floor_mats): (
        ResMut<Assets<Image>>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<PortalMaterial>>,
        ResMut<Assets<SkyMaterial>>,
        ResMut<Assets<FloorMaterial>>,
    ),
    mut commands: Commands,
) {
    let p = &mut *portals;
    if !w.active() {
        for l in p.live.drain(..) {
            commands.entity(l.quad).try_despawn();
            commands.entity(l.cam).try_despawn();
        }
        return;
    }
    w.travel_check();
    let size = w.block_size();
    let now = real.elapsed_secs_f64();
    let px = windows
        .single()
        .map(|win| UVec2::new(win.physical_width(), win.physical_height()) / 2)
        .unwrap_or(UVec2::new(640, 360))
        .max(UVec2::splat(64));

    // ---- a new world: its portals
    if p.built_for != w.portals_gen {
        p.built_for = w.portals_gen;
        p.calm_until = now + COOLDOWN;
        for l in p.live.drain(..) {
            commands.entity(l.quad).try_despawn();
            commands.entity(l.cam).try_despawn();
        }
        for def in w.portals.clone() {
            let scene = match p.scenes.iter().position(|s| s.level == def.level) {
                Some(i) => i,
                None => {
                    let i = p.scenes.len();
                    p.scenes.push(Scene {
                        level: def.level.clone(),
                        layer: i + 1,
                        offset: Vec3::new(AWAY + APART * i as f32, 0.0, 0.0),
                        look: None,
                        loading: load(&def.level, &blocks.rects),
                        env: None,
                        failed: false,
                    });
                    i
                }
            };
            let image = images.add(target(px));
            let mat = mats.add(PortalMaterial { params: Vec4::new(0.0, 0.0, def.size.x / def.size.y, p.live.len() as f32 * 1.7), echo: image.clone() });
            let rot = if def.along_x { Quat::from_rotation_y(FRAC_PI_2) } else { Quat::IDENTITY };
            let quad = commands
                .spawn((
                    Mesh3d(meshes.add(door_mesh(def.size.x * size, def.size.y * size))),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(def.centre() * size).with_rotation(rot),
                    Visibility::Hidden,
                    NotShadowCaster,
                    NotShadowReceiver,
                    PortalSurface,
                    crate::NoBevel,
                    Name::new(format!("portal {}", def.name)),
                ))
                .id();
            let cam = commands
                .spawn((
                    Camera3d::default(),
                    Camera { order: -10 - p.live.len() as isize, is_active: false, ..default() },
                    RenderTarget::Image(ImageRenderTarget { handle: image.clone(), scale_factor: 1.0 }),
                    Hdr,
                    Tonemapping::None,
                    RenderLayers::layer(p.scenes[scene].layer),
                    Transform::default(),
                    EchoCamera,
                    Name::new(format!("echo {}", def.name)),
                ))
                .id();
            p.live.push(Live { def, scene, quad, cam, image, mat, px, last_side: None, dressed: false, shown: 0.0 });
        }
    }

    // ---- echoes arriving from the background
    for s in &mut p.scenes {
        if s.loading.is_none() && s.look.is_none() && !s.failed {
            s.loading = load(&s.level, &blocks.rects);
        }
        let Some(slot) = &s.loading else { continue };
        let Some(made) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { continue };
        s.loading = None;
        let (m, set) = match made {
            Ok(x) => x,
            Err(_) => {
                s.failed = true;
                continue;
            }
        };
        let layer = RenderLayers::layer(s.layer);
        let style = Style::parse(set.get("sky").and_then(|v| v.as_str()).unwrap_or("earth"));
        let hours = set.get("time").and_then(|v| v.as_f64()).unwrap_or(16.5) as f32;
        let clouds = set.get("clouds").and_then(|v| v.as_f64()).unwrap_or(0.45) as f32;
        let look = crate::outdoors::sky_at(style, hours);
        let Meshes { opaque, glass, water, flame } = m;
        let o = s.offset;
        if opaque.faces() > 0 {
            let aabb = opaque.bounds();
            let mut e = commands.spawn((Mesh3d(meshes.add(opaque.into_packed_mesh())), MeshMaterial3d(blocks.world_solid.clone()), Transform::from_translation(o), layer.clone(), crate::NoBevel));
            if let Some(b) = aabb {
                e.insert(b);
            }
        }
        if glass.faces() > 0 {
            commands.spawn((Mesh3d(meshes.add(glass.into_mesh())), MeshMaterial3d(blocks.glass.clone()), Transform::from_translation(o), layer.clone(), crate::NoBevel));
        }
        if water.faces() > 0 {
            commands.spawn((Mesh3d(meshes.add(water.into_mesh())), MeshMaterial3d(blocks.water.clone()), Transform::from_translation(o), NotShadowCaster, layer.clone(), crate::NoBevel));
        }
        if flame.faces() > 0 {
            commands.spawn((Mesh3d(meshes.add(flame.into_mesh())), MeshMaterial3d(blocks.flame.clone()), Transform::from_translation(o), NotShadowCaster, layer.clone(), crate::NoBevel));
        }
        // its sky, its light and (the void) its endless floor
        commands.spawn((
            Mesh3d(meshes.add(crate::outdoors::dome_mesh())),
            MeshMaterial3d(sky_mats.add(SkyMaterial { sky: crate::outdoors::sky_uniform(&look, clouds, Vec4::ZERO) })),
            Transform::from_translation(o),
            NotShadowCaster,
            NotShadowReceiver,
            layer.clone(),
            crate::NoBevel,
        ));
        let shadows = studio.level >= crate::Level::Medium;
        commands.spawn((
            DirectionalLight {
                illuminance: look.lux,
                color: Color::linear_rgb(look.light.x, look.light.y, look.light.z),
                shadow_maps_enabled: shadows,
                shadow_depth_bias: 0.04,
                shadow_normal_bias: 1.6,
                ..default()
            },
            CascadeShadowConfigBuilder { num_cascades: 2, minimum_distance: 0.1, first_cascade_far_bound: 14.0, maximum_distance: 60.0, overlap_proportion: 0.25 }.build(),
            Transform::from_translation(o + look.key * 50.0).looking_at(o, Vec3::Y),
            layer.clone(),
        ));
        if let Some(f) = set.get("floor").and_then(|v| v.as_f64()) {
            commands.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(1600.0, 1600.0).subdivisions(8))),
                MeshMaterial3d(floor_mats.add(FloorMaterial {
                    base: StandardMaterial { base_color: Color::srgb(0.86, 0.86, 0.89), perceptual_roughness: 0.08, reflectance: 0.55, ..default() },
                    extension: FloorExt { params: Vec4::new(4.0, 0.22, 0.05, 0.0) },
                })),
                Transform::from_translation(o + Vec3::Y * f as f32 * size),
                layer.clone(),
                crate::NoBevel,
            ));
        }
        s.env = Some(images.add(crate::outdoors::sky_cube(&look, 64)));
        s.look = Some(look);
        crate::note!("[portal] echo of {} ready", s.level);
    }

    // ---- going through, by command
    if let Some(name) = w.portal_go.take().filter(|_| !w.travelling()) {
        match w.portals.iter().find(|d| d.name == name).cloned() {
            Some(def) => {
                let here = Vec3::from(w.body.pos);
                let side = if def.side_of(here) < 0.0 { -1.0 } else { 1.0 };
                go(&mut w, &def, def.at + def.normal() * side * 0.05, side);
                p.calm_until = now + COOLDOWN;
            }
            None => w.notice(format!("No portal called {name}")),
        }
    }

    // ---- each doorway: fade, mirror the camera, and walking through
    let Ok((cam_t, cam_proj, cam)) = main.single() else { return };
    let cam_g = GlobalTransform::from(*cam_t);
    let body = Vec3::from(w.body.pos);
    let me = (body + Vec3::Y * 2.0) * size;
    let travelling = w.travelling();
    let mut through: Option<(PortalDef, Vec3, f32)> = None;
    for l in &mut p.live {
        let s = &p.scenes[l.scene];
        let d = me.distance(l.def.centre() * size);
        let ready = s.look.is_some();
        // seen from behind you (the camera on the far side of the doorway from you, as just
        // after coming through) it's an empty doorway: the echo is for what's ahead of you
        let cam_side = l.def.side_of(cam_t.translation / size);
        let same_side = cam_side * l.def.side_of(body) >= 0.0;
        let want = if s.failed || !same_side { 0.0 } else { presence(d) };
        l.shown += (want - l.shown) * (1.0 - (-real.delta_secs().min(0.25) * 5.0).exp());
        let pres = l.shown;
        // walking through: you were on one side last frame and are on the other now, inside
        // the opening
        let side_now = l.def.side_of(body);
        if let Some(before) = l.last_side {
            if before * side_now < 0.0 && l.def.within(body) && now >= p.calm_until && !travelling && through.is_none() {
                let side = before.signum();
                through = Some((l.def.clone(), body, side));
            }
        }
        l.last_side = Some(side_now);
        // the doorway's look
        if let Some(mut m) = mats.get_mut(&l.mat) {
            let want = Vec4::new(pres, if ready { 1.0 } else { 0.0 }, m.params.z, m.params.w);
            if m.params != want {
                m.params = want;
            }
        }
        if let Ok(mut v) = vis.get_mut(l.quad) {
            let want = if pres > 0.002 { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
        let Ok((mut et, mut ep, mut ec, mut rt)) = echoes.get_mut(l.cam) else { continue };
        // drawn only while the doorway is on screen
        let fwd = cam_t.forward();
        let seen = {
            let (mut lo, mut hi, mut behind) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY), false);
            for c in l.def.corners() {
                let p = c * size;
                match cam.world_to_ndc(&cam_g, p) {
                    Some(n) if (p - cam_t.translation).dot(*fwd) > 0.05 => {
                        lo = lo.min(n.truncate());
                        hi = hi.max(n.truncate());
                    }
                    _ => behind = true,
                }
            }
            behind || (hi.x >= -1.0 && lo.x <= 1.0 && hi.y >= -1.0 && lo.y <= 1.0)
        };
        let on = pres > 0.002 && ready && seen;
        if ec.is_active != on {
            ec.is_active = on;
        }
        if !on {
            continue;
        }
        // the window changed size: a new texture to match
        if l.px != px {
            l.px = px;
            l.image = images.add(target(px));
            *rt = RenderTarget::Image(ImageRenderTarget { handle: l.image.clone(), scale_factor: 1.0 });
            if let Some(mut m) = mats.get_mut(&l.mat) {
                m.echo = l.image.clone();
            }
        }
        if !l.dressed {
            l.dressed = true;
            if let (Some(look), Some(env)) = (s.look, s.env.clone()) {
                let h = look.horizon;
                ec.clear_color = ClearColorConfig::Custom(Color::linear_rgb(h.x, h.y, h.z));
                if studio.env_maps {
                    commands
                        .entity(l.cam)
                        .insert(bevy::light::GeneratedEnvironmentMapLight { environment_map: env, intensity: look.ambient, ..default() });
                }
                commands.entity(l.cam).insert((
                    DistanceFog {
                        color: Color::linear_rgb(h.x, h.y, h.z),
                        falloff: if look.style == Style::Earth {
                            FogFalloff::from_visibility(look.visibility)
                        } else {
                            FogFalloff::from_visibility_squared(look.visibility)
                        },
                        ..default()
                    },
                ));
            }
        }
        // the camera's mirror on the other side of the doorway
        let cam_blocks = cam_t.translation / size;
        let side = if l.def.side_of(cam_blocks) < 0.0 { -1.0 } else { 1.0 };
        let turn = Quat::from_rotation_y(l.def.turn(side));
        et.translation = s.offset + l.def.carry(cam_blocks, side) * size;
        et.rotation = turn * cam_t.rotation;
        // nothing nearer than the doorway: what's behind the other side's doorway would block it
        let nearest = l
            .def
            .corners()
            .iter()
            .map(|c| (*c * size - cam_t.translation).dot(*fwd))
            .fold(f32::INFINITY, f32::min);
        let near = (nearest * 0.97).max(0.05);
        if let Projection::Perspective(mp) = cam_proj {
            let want = PerspectiveProjection { near, ..mp.clone() };
            match &mut *ep {
                Projection::Perspective(e) if e.fov == want.fov && (e.near - near).abs() < 1e-3 => {}
                _ => *ep = Projection::Perspective(want),
            }
        }
    }
    if let Some((def, body, side)) = through {
        go(&mut w, &def, body, side);
        p.calm_until = now + COOLDOWN;
        for l in &mut p.live {
            l.last_side = None;
        }
    }
}

/// Through `def` from `side`, standing at `body` (blocks).
fn go(w: &mut WorldClient, def: &PortalDef, body: Vec3, side: f32) {
    let to = def.carry(body, side);
    let a = w.arrival(to.to_array(), def.turn(side));
    match &def.url {
        Some(url) => w.through(&def.name, side, url, a),
        None => {
            w.arrive(a);
            w.notice(format!("Through to {}", def.name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> PortalDef {
        parse(&serde_json::json!({"portals": [
            {"name": "yard", "at": [0, 0, -39.5], "axis": "z", "size": [8, 16], "level": "yard", "url": ":7781", "arrive": [0, 0, 11.5], "yaw": 0},
            {"name": "bad"}
        ]}))
        .remove(0)
    }

    #[test]
    fn reads_portals() {
        let g = gate();
        assert_eq!(g.name, "yard");
        assert!(!g.along_x);
        assert_eq!(g.size, Vec2::new(8.0, 16.0));
        assert_eq!(g.url.as_deref(), Some(":7781"));
        // the one without a level or place is left out
        let all = parse(&serde_json::json!({"portals": [{"name": "bad"}]}));
        assert!(all.is_empty());
        assert!(parse(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn walking_through_carries_on() {
        let g = gate();
        // walking north through the gate: you come out north of the yard's doorway, facing the
        // same way, the same distance past it
        let to = g.carry(Vec3::new(1.0, 0.0, -40.0), 1.0);
        assert!((to - Vec3::new(1.0, 0.0, 11.0)).length() < 1e-4, "{to}");
        assert!(g.turn(1.0).abs() < 1e-6);
        // walking south through it: turned round, so south of it here is north of there
        let to = g.carry(Vec3::new(1.0, 0.0, -39.0), -1.0);
        assert!((to - Vec3::new(-1.0, 0.0, 11.0)).length() < 1e-4, "{to}");
        assert!((g.turn(-1.0) + PI).abs() < 1e-6);
    }

    #[test]
    fn turning_doorways() {
        // walking west through an x doorway into a place where you then face west too: no turn
        let d = PortalDef { along_x: true, yaw: FRAC_PI_2, ..gate() };
        assert!(d.turn(1.0).abs() < 1e-6);
        // ... where you then face north: a quarter turn right
        let d = PortalDef { along_x: true, yaw: 0.0, at: Vec3::ZERO, arrive: Vec3::ZERO, ..gate() };
        let to = d.carry(Vec3::new(-1.0, 0.0, 0.0), 1.0);
        assert!((to - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4, "{to}");
    }

    #[test]
    fn in_the_opening() {
        let g = gate();
        assert!(g.within(Vec3::new(3.9, 0.0, -39.5)));
        assert!(!g.within(Vec3::new(4.5, 0.0, -39.5)));
        assert!(!g.within(Vec3::new(0.0, 16.0, -39.5)));
        assert!(g.side_of(Vec3::new(0.0, 0.0, -30.0)) > 0.0);
    }

    #[test]
    fn fades_in_as_you_come() {
        assert_eq!(presence(60.0), 0.0);
        assert_eq!(presence(5.0), 1.0);
        assert!(presence(20.0) > 0.3 && presence(20.0) < 0.9);
        assert!(presence(15.0) > presence(25.0));
    }

    #[test]
    fn arrivals_agree_with_the_server() {
        let s: Value = serde_json::from_str(
            r#"{"portals": [{"name": "a", "at": [3, 1, -39.5], "axis": "z", "size": [8, 16], "level": "yard", "url": ":7781", "arrive": [0, 0, 11.5], "yaw": 30},
                            {"name": "b", "at": [-7, 0, 2], "axis": "x", "size": [4, 6], "level": "sea", "url": ":7783", "arrive": [10, 2, -4], "yaw": -120}]}"#,
        )
        .unwrap();
        let (here, core) = (parse(&s), vox_core::portal::parse(&s));
        assert_eq!(here.len(), core.len());
        for (d, c) in here.iter().zip(&core) {
            assert_eq!(d.door(), *c);
            for p in [Vec3::new(1.0, 0.5, -39.0), Vec3::new(-7.5, 0.0, 3.0), Vec3::new(4.0, 2.0, 0.0)] {
                for side in [1.0, -1.0] {
                    let (a, b) = (d.carry(p, side), Vec3::from(c.carry(p.to_array(), side)));
                    assert!((a - b).length() < 1e-4, "{a} {b}");
                }
            }
        }
    }

    #[test]
    fn the_portal_url() {
        use crate::client::portal_url;
        assert_eq!(portal_url(Some("ws://127.0.0.1:7780/ws"), ":7781"), "ws://127.0.0.1:7781/ws");
        assert_eq!(portal_url(Some("wss://vox.example:443/ws"), ":8443"), "wss://vox.example:8443/ws");
        assert_eq!(portal_url(Some("ws://[::1]:7780/ws"), ":7781"), "ws://[::1]:7781/ws");
        assert_eq!(portal_url(None, "ws://other:9/ws"), "ws://other:9/ws");
        // vox.fail: worlds behind one host, by port in the path
        assert_eq!(portal_url(Some("wss://vox.fail/w/7780/ws"), ":7781"), "wss://vox.fail/w/7781/ws");
        assert_eq!(portal_url(Some("wss://vox.fail/w/7780/ws"), "/yard/ws"), "wss://vox.fail/yard/ws");
    }
}
