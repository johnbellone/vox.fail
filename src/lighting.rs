//! Studio lighting: a three-point setup instead of one flat light.
//! - **Key**: the existing sun, casting soft shadows (the cap brim shades the forehead, arms
//!   shade the body, hats shade the head). The background is a flat colour, so a green screen
//!   stays clean.
//! - **Fill**: a weaker light from the other side so shadows aren't black.
//! - **Rim**: a light from behind and above that draws a bright edge on the tops and outline.
//! - Less flat ambient light, so all of that shows.
//! - Corner shading where blocks meet and a slight plastic sheen are baked in by bevel.rs.
//!
//! On top of that:
//! - **Per-set looks**: each set has its own light (warm diner fluorescents, blue moonlight and a
//!   lamp at night, hard stage light at the rally ...), switched with the set.
//! - **Glow**: neon signs, lit windows and sparkles glow (bloom); which palette colours glow is
//!   set in `data/surfaces.json` ("glow").
//! - **Event lights**: a sweeping stage light for a raid, colour-cycling party lights for a sub,
//!   camera flashes during a press conference, a spotlight while you sing, a red flash for a
//!   giant tomato.
//! - **Drop shadow** under the feet in transparent mode, so the character sits on the stream.
//! - **Quality**: Low / Medium / High (shadows, glow, bevels), or Auto, which steps down when the
//!   frame rate can't keep up.
//!
//! `light:on` toggles studio lighting for a before/after; `light:shadows`, `light:glow` and
//! `quality:<low|medium|high|auto>` do the rest.

use bevy::{
    asset::RenderAssetUsages,
    camera::Hdr,
    core_pipeline::tonemapping::Tonemapping,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap},
    post_process::bloom::Bloom,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// Corner shading strength and surface roughness while studio lighting is on.
pub const CORNER_AO: f32 = 0.75;
pub const ROUGHNESS: f32 = 0.5;

pub use vox_world::{FillLight, KeyLight, Level, RimLight};

#[derive(Resource)]
pub struct Studio {
    pub on: bool,
    pub shadows: bool,
    pub glow: bool,
    pub level: Level,
    /// step the level down by itself when the frame rate can't keep up
    pub auto: bool,
    /// a message for the status line when Auto stepped down
    pub note: Option<String>,
    /// colour grading per set (a filmic curve and the set's own colour)
    pub grade: bool,
    /// background blur on sets, 0 (off) to 1: depth of field focused on the character
    pub blur: f32,
    /// the main view's focus distance and f-number now (for the control page)
    pub focus: Option<(f32, f32)>,
    /// in a world: the sun, sky and lamps (`outdoors.rs`) light the scene instead of the
    /// studio rig
    pub outdoors: bool,
}

impl Studio {
    pub fn new(on: bool, shadows: bool, glow: bool, quality: &str) -> Self {
        let (level, auto) = match quality {
            "auto" | "" => (Level::High, true),
            q => (Level::parse(q).unwrap_or(Level::High), false),
        };
        Studio {
            on,
            shadows,
            glow,
            level,
            auto,
            note: None,
            grade: true,
            blur: DEFAULT_BLUR,
            focus: None,
            outdoors: false,
        }
    }

    /// What actually runs, after the quality level has had its say.
    pub fn shadows_live(&self) -> bool {
        self.on && self.shadows && self.level >= Level::Medium
    }
    pub fn glow_live(&self) -> bool {
        self.glow && self.level == Level::High
    }

    /// light:on, light:shadows, light:glow (toggles); quality:<low|medium|high|auto>
    pub fn command(&mut self, verb: &str, arg: &str) -> Option<String> {
        if verb == "quality" {
            if arg == "auto" {
                self.auto = true;
                return Some(format!("Quality: auto (now {})", self.level.name()));
            }
            self.level = Level::parse(arg)?;
            self.auto = false;
            return Some(format!("Quality: {}", self.level.name()));
        }
        match arg {
            "studio" | "flat" => {
                self.on = arg == "studio";
                Some(format!(
                    "Studio lighting {}",
                    if self.on { "on" } else { "off" }
                ))
            }
            "on" => {
                self.on = !self.on;
                Some(format!(
                    "Studio lighting {}",
                    if self.on { "on" } else { "off" }
                ))
            }
            "shadows" => {
                self.shadows = !self.shadows;
                Some(format!(
                    "Shadows {}",
                    if self.shadows { "on" } else { "off" }
                ))
            }
            "glow" => {
                self.glow = !self.glow;
                Some(format!("Glow {}", if self.glow { "on" } else { "off" }))
            }
            "blur" => {
                self.blur = if self.blur > 0.0 { 0.0 } else { DEFAULT_BLUR };
                Some(blur_msg(self.blur))
            }
            a if a.starts_with("blur:") => {
                let v: f32 = a[5..].parse().ok()?;
                self.blur = (v / 100.0).clamp(0.0, 1.0);
                Some(blur_msg(self.blur))
            }
            "grade" => {
                self.grade = !self.grade;
                Some(format!(
                    "Colour grading {}",
                    if self.grade { "on" } else { "off" }
                ))
            }
            _ => None,
        }
    }

    pub fn quality_setting(&self) -> String {
        if self.auto {
            "auto".into()
        } else {
            self.level.name().into()
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"on": self.on, "shadows": self.shadows, "glow": self.glow, "grade": self.grade, "blur": (self.blur * 100.0).round(), "focus": self.focus.map(|(d, n)| [(d * 100.0).round() / 100.0, (n * 1000.0).round() / 1000.0]), "quality": self.quality_setting(), "level": self.level.name()})
    }
}

pub const DEFAULT_BLUR: f32 = 0.5;

fn blur_msg(b: f32) -> String {
    if b > 0.0 {
        format!("Background blur {:.0}%", b * 100.0)
    } else {
        "Background blur off".into()
    }
}

/// How hard to stop the lens down for a blur strength: the f-number that blurs a point `behind`
/// metres past the focus into a circle `target_px` across, for a camera focused at `focus`
/// metres with this vertical field of view, looking at an image `view_h` pixels tall.
///
/// (Thin lens: circle = f^2 / (N (s - f)) * |d - s| / d on the sensor; Bevy takes the focal
/// length f from the field of view and the sensor height.)
pub fn f_stops_for(focus: f32, behind: f32, target_px: f32, fov: f32, view_h: f32) -> f32 {
    let sensor = bevy::post_process::dof::DepthOfField::default().sensor_height;
    let f = sensor / (2.0 * (fov * 0.5).tan());
    let d = focus + behind;
    let on_sensor_at_n1 = f * f / (focus - f).max(1e-3) * behind / d;
    let px_at_n1 = on_sensor_at_n1 / sensor * view_h;
    (px_at_n1 / target_px.max(0.1)).max(0.01)
}

/// Background blur: depth of field on every 3D camera, focused on the character's head, only
/// with a set behind it (the studio backdrop is a flat colour, and a green screen must stay
/// sharp for the key). The strength is the blur, in pixels per metre of depth behind the
/// character, at the camera's own resolution; the character itself stays sharp.
pub fn blur(
    mut studio: ResMut<Studio>,
    show: Res<crate::show::Show>,
    heads: Query<(&crate::Joint, &GlobalTransform)>,
    mut cams: Query<(Entity, &GlobalTransform, &Projection, &Camera, Option<&mut bevy::post_process::dof::DepthOfField>, Has<crate::MainCamera>), (With<Camera3d>, Without<crate::portals::EchoCamera>)>,
    mut commands: Commands,
) {
    use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
    let on = studio.blur > 0.0 && show.live_set().is_some() && studio.level != Level::Low;
    let head = heads.iter().find(|(j, _)| **j == crate::Joint::Head).map(|(_, g)| g.translation() + Vec3::Y * 0.1);
    if !on {
        studio.focus = None;
    }
    for (e, g, proj, cam, dof, main) in &mut cams {
        let (true, Some(head)) = (on, head) else {
            if dof.is_some() {
                commands.entity(e).remove::<DepthOfField>();
            }
            continue;
        };
        // the distance along the view to the face: the front of the head, not the neck joint
        // (a quarter metre makes the difference between a sharp face and a soft one)
        let to_cam = (g.translation() - head).with_y(0.0).normalize_or_zero();
        let face = head + Vec3::Y * 0.2 + to_cam * 0.28;
        let focus = (face - g.translation()).dot(*g.forward()).max(0.3);
        let fov = match proj {
            Projection::Perspective(p) => p.fov,
            _ => std::f32::consts::FRAC_PI_4,
        };
        let view_h = cam.physical_viewport_size().map(|s| s.y as f32).unwrap_or(900.0);
        // a wall two metres behind the character blurs this many pixels at full strength
        let target = studio.blur * view_h * 0.018;
        let n = f_stops_for(focus, 2.0, target, fov, view_h);
        if main {
            studio.focus = Some((focus, n));
        }
        let want = DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: focus,
            aperture_f_stops: n,
            max_circle_of_confusion_diameter: (target * 2.0).clamp(2.0, 64.0),
            max_depth: 40.0,
            ..default()
        };
        match dof {
            Some(mut d) => {
                if (d.focal_distance - want.focal_distance).abs() > 0.005 || (d.aperture_f_stops - want.aperture_f_stops).abs() > 1e-4 {
                    *d = want;
                }
            }
            None => {
                commands.entity(e).insert(want);
            }
        }
    }
}

/// One set's lighting: (colour, strength) for key, fill and rim, and the ambient light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub key: ([f32; 3], f32),
    pub fill: ([f32; 3], f32),
    pub rim: ([f32; 3], f32),
    pub ambient: ([f32; 3], f32),
}

pub fn preset_for(set: Option<&str>) -> Preset {
    let p = |key: ([f32; 3], f32),
             fill: ([f32; 3], f32),
             rim: ([f32; 3], f32),
             ambient: ([f32; 3], f32)| Preset {
        key,
        fill,
        rim,
        ambient,
    };
    match set {
        // afternoon sun: warm key, blue sky fill, a bright sky all round
        Some("yard") => p(
            ([1.0, 0.95, 0.84], 5200.0),
            ([0.7, 0.82, 1.0], 1500.0),
            ([1.0, 0.94, 0.82], 2600.0),
            ([0.78, 0.86, 1.0], 650.0),
        ),
        // warm fluorescent tubes, a cream bounce off the walls
        Some("diner") => p(
            ([1.0, 0.98, 0.9], 3600.0),
            ([1.0, 0.92, 0.8], 1500.0),
            ([1.0, 0.95, 0.85], 2800.0),
            ([1.0, 0.96, 0.88], 470.0),
        ),
        // blue moonlight, a warm street lamp from the side, deep blue ambient
        Some("night") => p(
            ([0.62, 0.72, 1.0], 1700.0),
            ([1.0, 0.72, 0.4], 1900.0),
            ([0.55, 0.7, 1.0], 3600.0),
            ([0.45, 0.55, 0.9], 210.0),
        ),
        // hard stage light, little fill, strong back light
        Some("rally") => p(
            ([1.0, 0.94, 0.82], 5200.0),
            ([0.85, 0.9, 1.0], 600.0),
            ([1.0, 0.92, 0.8], 5200.0),
            ([1.0, 0.97, 0.92], 300.0),
        ),
        // bright, even TV light
        Some("newsroom") => p(
            ([1.0, 0.98, 0.95], 3400.0),
            ([0.92, 0.96, 1.0], 2300.0),
            ([0.8, 0.88, 1.0], 2400.0),
            ([0.96, 0.98, 1.0], 520.0),
        ),
        Some("pressroom") => p(
            ([1.0, 0.97, 0.92], 3600.0),
            ([0.9, 0.94, 1.0], 2000.0),
            ([0.85, 0.9, 1.0], 2600.0),
            ([0.94, 0.96, 1.0], 480.0),
        ),
        // cool, clean, shadow-light
        Some("hospital") => p(
            ([0.95, 1.0, 1.0], 3300.0),
            ([0.92, 1.0, 0.97], 2000.0),
            ([0.9, 1.0, 1.0], 1800.0),
            ([0.95, 1.0, 0.98], 560.0),
        ),
        // plain background: the general studio look
        _ => p(
            ([1.0, 0.96, 0.9], 3900.0),
            ([0.9, 0.94, 1.0], 1500.0),
            ([1.0, 0.97, 0.92], 3500.0),
            ([0.97, 0.98, 1.0], 430.0),
        ),
    }
}

/// A set's colour grade: exposure (stops, making up for the filmic curve's roll-off),
/// temperature and tint (CIE x/y shifts: + warmer / + more magenta), saturation and contrast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub contrast: f32,
}

/// The grade for a set, or None where the plain image is better: the filmic curve rolls off
/// bright colours, which dulls the bright sets (diner, rally, newsroom, hospital), while it
/// gives the dark ones depth.
pub fn grade_for(set: Option<&str>) -> Option<Grade> {
    let g = |exposure, temperature, tint, saturation, contrast| Some(Grade { exposure, temperature, tint, saturation, contrast });
    match set {
        // the dark set needs lift: the curve rolls the shadows off
        Some("night") => g(0.75, -0.01, 0.0, 1.1, 1.0),
        Some(_) => None,
        // the dark studio backdrop
        None => g(0.45, 0.0, 0.0, 1.08, 1.0),
    }
}

impl Grade {
    pub fn colour_grading(self) -> bevy::render::view::ColorGrading {
        use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
        let section = ColorGradingSection {
            saturation: self.saturation,
            contrast: self.contrast,
            ..default()
        };
        ColorGrading {
            global: ColorGradingGlobal {
                exposure: self.exposure,
                temperature: self.temperature,
                tint: self.tint,
                ..default()
            },
            shadows: section,
            midtones: section,
            highlights: section,
        }
    }
}

/// Lighting cues from the show (raids, subs, big hits).
#[derive(Resource, Default)]
pub struct Cues(pub Vec<Cue>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cue {
    Raid,
    Sub,
    BigHit,
}

/// The sweeping raid light, the singing spotlight, and the flash (camera flashes, hits).
#[derive(Component)]
pub struct Sweep;

#[derive(Component)]
pub struct SingSpot;

#[derive(Component)]
pub struct Flash;

#[derive(Component)]
pub struct DropShadow;

/// The key light's shadow range: the character and the set behind it.
pub fn key_cascades() -> bevy::light::CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: 2,
        minimum_distance: 0.1,
        first_cascade_far_bound: 3.5,
        maximum_distance: 9.0,
        ..default()
    }
    .build()
}

/// A soft round shadow texture: black, alpha falling off from the middle.
fn blob_image() -> Image {
    let n = 64usize;
    let mut d = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (
                (x as f32 + 0.5) / n as f32 * 2.0 - 1.0,
                (y as f32 + 0.5) / n as f32 * 2.0 - 1.0,
            );
            let r = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - r).clamp(0.0, 1.0).powf(1.6) * 0.55;
            d.extend_from_slice(&[0, 0, 0, (a * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        d,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 0.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(2.6, 1.3, 2.4).looking_at(Vec3::new(0.0, 1.2, 0.0), Vec3::Y),
        FillLight,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 0.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.8, 3.2, -3.6).looking_at(Vec3::new(0.0, 1.2, 0.0), Vec3::Y),
        RimLight,
    ));
    commands.spawn((
        SpotLight {
            intensity: 0.0,
            range: 20.0,
            outer_angle: 0.32,
            inner_angle: 0.22,
            shadow_maps_enabled: false,
            color: Color::srgb(1.0, 0.95, 0.85),
            ..default()
        },
        Transform::from_xyz(0.0, 4.2, 3.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        Sweep,
    ));
    commands.spawn((
        SpotLight {
            intensity: 0.0,
            range: 12.0,
            outer_angle: 0.3,
            inner_angle: 0.18,
            shadow_maps_enabled: false,
            color: Color::srgb(1.0, 0.93, 0.8),
            ..default()
        },
        Transform::from_xyz(0.0, 4.5, 1.2).looking_at(Vec3::new(0.0, 1.4, 0.0), Vec3::Y),
        SingSpot,
    ));
    commands.spawn((
        PointLight {
            intensity: 0.0,
            range: 10.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.3, 1.7, 2.2),
        Flash,
    ));
    let blob = images.add(blob_image());
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(1.0, 0.62))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color_texture: Some(blob),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.004, 0.02),
        Visibility::Hidden,
        DropShadow,
    ));
}

/// What the event lights are doing.
pub struct EventState {
    /// the colour grade the cameras were last given
    graded: Option<Option<Grade>>,
    raid_until: f32,
    raid_start: f32,
    party_until: f32,
    hit_at: f32,
    next_flash: f32,
    flash_at: f32,
}

impl Default for EventState {
    /// No hit and no flash yet: long ago, so the app doesn't open on a red pop or a flash.
    fn default() -> Self {
        EventState {
            graded: None,
            raid_until: 0.0,
            raid_start: 0.0,
            party_until: 0.0,
            hit_at: f32::NEG_INFINITY,
            next_flash: 0.0,
            flash_at: f32::NEG_INFINITY,
        }
    }
}

/// Puts the lights in the state asked for, plays the event lights, and manages glow, shadows,
/// the drop shadow and Auto quality.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update(
    mut studio: ResMut<Studio>,
    mut cues: ResMut<Cues>,
    mut av: ResMut<crate::Avatar>,
    show: Res<crate::show::Show>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut dir: Query<(
        &mut DirectionalLight,
        Has<KeyLight>,
        Has<FillLight>,
        Has<RimLight>,
    )>,
    mut spots: Query<(&mut SpotLight, &mut Transform, Has<Sweep>), Without<DropShadow>>,
    mut flash: Query<
        (&mut PointLight, &mut Transform),
        (With<Flash>, Without<SpotLight>, Without<DropShadow>),
    >,
    mut blob: Query<
        (&mut Visibility, &mut Transform),
        (With<DropShadow>, Without<SpotLight>, Without<Flash>),
    >,
    (cams, overlays): (
        Query<(Entity, Has<Bloom>), (With<Camera3d>, Without<crate::portals::EchoCamera>)>,
        Query<(Entity, Has<Hdr>), With<Camera2d>>,
    ),
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut ev: Local<EventState>,
    mut fps: Local<(f32, f32, f32)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    // Auto quality: after a settling period, watch 5 s windows; step down when slow
    let rt = real.elapsed_secs();
    let dt = real.delta_secs();
    if fps.0 == 0.0 {
        *fps = (rt, 0.0, 0.0);
    }
    fps.1 += dt;
    fps.2 += 1.0;
    if rt - fps.0 >= 5.0 {
        let rate = fps.2 / fps.1.max(1e-3);
        if studio.auto && rt > 12.0 && rate < 45.0 && studio.level > Level::Low {
            studio.level = if studio.level == Level::High {
                Level::Medium
            } else {
                Level::Low
            };
            let msg = format!(
                "Quality stepped down to {} ({rate:.0} fps)",
                studio.level.name()
            );
            crate::note!("[light] {msg}");
            studio.note = Some(msg);
        }
        *fps = (rt, 0.0, 0.0);
    }
    if let Some(n) = studio.note.take() {
        av.status = n;
    }
    // cues
    for c in std::mem::take(&mut cues.0) {
        match c {
            Cue::Raid => {
                ev.raid_start = now;
                ev.raid_until = now + 6.0;
            }
            Cue::Sub => ev.party_until = now + 4.0,
            Cue::BigHit => ev.hit_at = now,
        }
    }
    let on = studio.on;
    let preset = preset_for(show.live_set());
    let party = now < ev.party_until;
    let hue = |t: f32| Color::hsl((t * 220.0) % 360.0, 0.9, 0.6);
    let col = |c: [f32; 3]| Color::srgb(c[0], c[1], c[2]);
    for (mut l, key, fill, rim) in &mut dir {
        if studio.outdoors {
            break;
        }
        if key {
            if on {
                l.illuminance = preset.key.1;
                l.color = col(preset.key.0);
                l.shadow_maps_enabled = studio.shadows_live();
                l.shadow_depth_bias = 0.03;
                l.shadow_normal_bias = 1.2;
            } else {
                l.illuminance = 3500.0;
                l.color = Color::WHITE;
                l.shadow_maps_enabled = false;
            }
        } else if fill {
            l.illuminance = if on { preset.fill.1 } else { 0.0 } + if party { 1800.0 } else { 0.0 };
            l.color = if party { hue(now) } else { col(preset.fill.0) };
        } else if rim {
            l.illuminance = if on { preset.rim.1 } else { 0.0 } + if party { 2500.0 } else { 0.0 };
            l.color = if party {
                hue(now + 0.8)
            } else {
                col(preset.rim.0)
            };
        }
    }
    let (amb_c, amb_b) = if on {
        (col(preset.ambient.0), preset.ambient.1)
    } else {
        (Color::WHITE, 600.0)
    };
    if !studio.outdoors && (ambient.brightness != amb_b || ambient.color != amb_c) {
        ambient.brightness = amb_b;
        ambient.color = amb_c;
    }
    let want_map = if studio.level == Level::High {
        2048
    } else {
        1024
    };
    if shadow_map.size != want_map {
        shadow_map.size = want_map;
    }
    // spots: the raid sweep and the singing spotlight
    for (mut s, mut t, sweep) in &mut spots {
        if sweep {
            let k = ((now - ev.raid_start) / 6.0).clamp(0.0, 1.0);
            s.intensity = if now < ev.raid_until {
                6.0e5 * (std::f32::consts::PI * k).sin().max(0.2)
            } else {
                0.0
            };
            let target = Vec3::new((now * 1.6).sin() * 1.4, 1.0, (now * 0.9).cos() * 0.4);
            *t = Transform::from_xyz(0.0, 4.2, 3.0).looking_at(target, Vec3::Y);
        } else {
            let want = if av.singing { 4.0e5 } else { 0.0 };
            s.intensity += (want - s.intensity) * 0.15;
        }
    }
    // the flash: press-conference cameras, a red pop for a giant tomato
    if show.press && now >= ev.next_flash {
        ev.flash_at = now;
        let r = av.rand01();
        ev.next_flash = now + 0.4 + 1.8 * r;
    }
    for (mut l, mut t) in &mut flash {
        let since_flash = now - ev.flash_at;
        let since_hit = now - ev.hit_at;
        if since_hit < 0.25 {
            l.color = Color::srgb(1.0, 0.25, 0.2);
            l.intensity = 8.0e5 * (1.0 - since_hit / 0.25);
            t.translation = Vec3::new(0.2, 1.6, 1.0);
        } else if since_flash < 0.09 {
            l.color = Color::WHITE;
            l.intensity = 2.5e6 * (1.0 - since_flash / 0.09);
            let side = if (ev.flash_at * 7.0).sin() > 0.0 {
                1.0
            } else {
                -1.0
            };
            t.translation = Vec3::new(side * 1.2, 1.5, 2.2);
        } else if l.intensity != 0.0 {
            l.intensity = 0.0;
        }
    }
    // drop shadow in transparent mode
    let transparent = av.background == crate::Background::Transparent;
    for (mut v, mut t) in &mut blob {
        let want = if transparent {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
        t.translation.x = 0.0;
    }
    // glow: bloom on every 3D camera. Bloom needs HDR, and the 2D overlay cameras that draw the
    // HUD and bubbles on the same targets must match it, or the window stops updating.
    // (only on a set or the studio backdrop: on a green screen the haze would spill into the key)
    let glow = studio.glow_live()
        && (show.live_set().is_some() || av.background == crate::Background::Studio || studio.outdoors);
    for (e, has) in &cams {
        if glow && !has {
            // additive, and only above full white: lit walls stay as they are, emissive props glow
            commands.entity(e).insert(Bloom {
                intensity: 0.22,
                low_frequency_boost: 0.15,
                high_pass_frequency: 0.6,
                prefilter: bevy::post_process::bloom::BloomPrefilter {
                    threshold: 1.2,
                    threshold_softness: 0.3,
                },
                ..Bloom::OLD_SCHOOL
            });
        } else if !glow && has {
            commands.entity(e).remove::<(Bloom, Hdr)>();
        }
    }
    for (e, has) in &overlays {
        if glow && !has {
            commands.entity(e).insert(Hdr);
        } else if !glow && has {
            commands.entity(e).remove::<Hdr>();
        }
    }
    // colour grading: a filmic curve and the set's own colour, again never over a green screen
    // (the key colour has to come out exactly as it went in)
    let backdrop = show.live_set().is_some() || av.background == crate::Background::Studio;
    let want = if studio.outdoors {
        // outdoors: a filmic curve for the sun and sky, a touch of lift and colour
        Some(Grade { exposure: 0.25, temperature: 0.0, tint: 0.0, saturation: 1.08, contrast: 1.04 })
    } else {
        (studio.on && studio.grade && backdrop).then(|| grade_for(show.live_set())).flatten()
    };
    if ev.graded != Some(want) {
        ev.graded = Some(want);
        for (e, _) in &cams {
            match want {
                Some(g) => {
                    commands
                        .entity(e)
                        .insert((Tonemapping::TonyMcMapface, g.colour_grading()));
                }
                None => {
                    commands.entity(e).insert((
                        Tonemapping::None,
                        bevy::render::view::ColorGrading::default(),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_quality() {
        let mut s = Studio::new(true, true, true, "auto");
        assert!(s.auto && s.level == Level::High && s.shadows_live() && s.glow_live());
        assert_eq!(
            s.command("light", "on").as_deref(),
            Some("Studio lighting off")
        );
        assert!(!s.shadows_live(), "no studio, no shadows");
        s.command("light", "on");
        assert_eq!(
            s.command("quality", "medium").as_deref(),
            Some("Quality: medium")
        );
        assert!(
            !s.auto && s.shadows_live() && !s.glow_live(),
            "medium: shadows, no glow"
        );
        s.command("quality", "low");
        assert!(!s.shadows_live());
        assert!(s.command("quality", "ultra").is_none());
        assert_eq!(s.quality_setting(), "low");
        assert!(s.command("light", "disco").is_none());
        assert_eq!(key_cascades().bounds.len(), 2);
    }

    #[test]
    fn blur_strength_and_commands() {
        let mut s = Studio::new(true, true, true, "high");
        assert_eq!(s.blur, DEFAULT_BLUR, "on by default");
        s.command("light", "blur").unwrap();
        assert_eq!(s.blur, 0.0);
        s.command("light", "blur").unwrap();
        assert_eq!(s.blur, DEFAULT_BLUR);
        s.command("light", "blur:20").unwrap();
        assert!((s.blur - 0.2).abs() < 1e-6);
        s.command("light", "blur").unwrap();
        assert_eq!(s.blur, 0.0);
        assert!(s.command("light", "blur:lots").is_none());
        // more blur wanted -> a wider aperture (smaller f-number); further from the camera ->
        // a narrower one for the same blur
        let fov = std::f32::consts::FRAC_PI_4;
        let a = f_stops_for(3.0, 2.0, 10.0, fov, 900.0);
        assert!(f_stops_for(3.0, 2.0, 20.0, fov, 900.0) < a);
        assert!(f_stops_for(5.0, 2.0, 10.0, fov, 900.0) < a, "further away needs more aperture for the same blur");
        // check against the thin-lens formula directly
        let sensor = bevy::post_process::dof::DepthOfField::default().sensor_height;
        let f = sensor / (2.0 * (fov * 0.5).tan());
        let px = f * f / (a * (3.0 - f)) * 2.0 / 5.0 / sensor * 900.0;
        assert!((px - 10.0).abs() < 0.01, "{px}");
    }

    #[test]
    fn grades_only_the_dark_scenes() {
        assert!(grade_for(Some("night")).is_some());
        assert!(grade_for(None).is_some(), "the dark studio backdrop");
        for bright in ["diner", "rally", "newsroom", "hospital", "pressroom"] {
            assert!(grade_for(Some(bright)).is_none(), "{bright}");
        }
        let g = grade_for(Some("night")).unwrap().colour_grading();
        assert!(g.global.exposure > 0.0 && g.midtones.saturation > 1.0);
    }

    #[test]
        fn every_set_has_its_own_light() {
        let base = preset_for(None);
        for set in crate::show::SETS {
            let p = preset_for(Some(set));
            assert_ne!(p, base, "{set}");
            assert!(p.key.1 > 0.0 && p.ambient.1 > 100.0);
        }
        assert!(
            preset_for(Some("night")).ambient.1 < base.ambient.1,
            "night is darker"
        );
    }
}
