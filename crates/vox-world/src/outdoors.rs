//! A world's sky and light, in one of three styles the world chooses (its level's `"world":
//! {"sky": …}`):
//!
//! - **earth:** a sun that moves with the time of day, blue overhead, a warm horizon at dusk,
//!   clouds drifting on the wind, stars and moonlight at night.
//! - **void:** the luminous void: bright, even, pale light from everywhere, an endless polished
//!   floor (`shaders/floor.wgsl`) going into white fog, three moons faint in the light.
//! - **islands:** fog without end in every direction, dark, with three moons (a great pale one,
//!   a red one, a small cold one) veiled behind it; lit by the pale moon and by fire.
//! - **sea:** a storm at night over open water: a sky black overhead and slate at the horizon,
//!   one low moon breaking through and laying a cold path across the waves.
//! - **orbit:** 250 km up: black space full of stars and a hard white sun; the planet below,
//!   its edge 15.8° under eye level (where the horizon really is from that height), turning
//!   slowly beneath with clouds, oceans that catch the sun, and cities on the night side; a
//!   thin blue line of atmosphere along its rim. As the clock runs you pass into its shadow.
//!
//! A world can also set how its water looks (`"water": {"colour": [r, g, b], "swell": metres,
//! "opacity": 0-1}`): the colour it's tinted, how high its swell rolls, and how much it hides
//! what's under it (see `shaders/water.wgsl`).
//!
//! Either way the same sky (without its small detail) is drawn into an environment map, so the
//! whole sky lights the world softly and shows in glass, metal and water; haze takes the
//! horizon's colour and reaches as far as the weather lets you see; the nearest lamps and fires
//! light up (lamps after dark, fires always, flickering). The weather is made visible too
//! ([`weather_look`]): a volume of fog baked from the simulation round the camera (High
//! quality), rain or snow, and flames and water moving with the wind.
//!
//! The time: `world:time:16.5` (hours), `world:time:auto` (a day every 24 minutes; in the void
//! the moons drift round instead). `world:clouds:0.6`.

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{CascadeShadowConfigBuilder, FogVolume, NotShadowCaster, NotShadowReceiver, VolumetricFog, VolumetricLight},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
    shader::ShaderRef,
};

use crate::{FillLight, KeyLight, RimLight, WorldRender as Studio};
use crate::client::{LightKind, WorldClient};
use vox_core::weather::Weather;

/// Faces of the sky cube used for lighting, in pixels.
const ENV_SIZE: u32 = 64;
/// Lamps and fires lit at once: the nearest ones.
const LIGHTS: usize = 12;
/// A day in real seconds when the clock runs.
const DAY: f32 = 24.0 * 60.0;
const SKY_RADIUS: f32 = 450.0;

/// The sky shader's inputs (`shaders/sky.wgsl`).
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct SkyUniform {
    /// towards the sun
    pub sun: Vec4,
    pub zenith: Vec4,
    pub horizon: Vec4,
    pub ground: Vec4,
    pub sun_colour: Vec4,
    /// x: night (0-1), y: cloud cover (0-1), z: cloud brightness, w: style (0 earth, 1 void)
    pub misc: Vec4,
    /// three moons: direction and angular radius (radians); colour and glow
    pub moon_a: Vec4,
    pub moon_b: Vec4,
    pub moon_c: Vec4,
    pub moon_a_colour: Vec4,
    pub moon_b_colour: Vec4,
    pub moon_c_colour: Vec4,
    /// x, z: the wind (m/s) the clouds drift on; w: how grey the clouds are (rain)
    pub wind: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub sky: SkyUniform,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/sky.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Component)]
pub struct SkyDome;

/// The void's endless floor (`shaders/floor.wgsl`).
pub type FloorMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, FloorExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FloorExt {
    /// seam spacing (m), seam strength, ripple strength
    #[uniform(100)]
    pub params: Vec4,
}

impl bevy::pbr::MaterialExtension for FloorExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/floor.wgsl".into()
    }
}

/// Rain or snow round the camera (`shaders/rain.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct RainMaterial {
    /// how hard (0-1), snow (0/1), wind x, wind z
    #[uniform(0)]
    pub params: Vec4,
}

impl Material for RainMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/rain.wgsl".into()
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

#[derive(Component)]
pub struct EndlessFloor;

#[derive(Component)]
pub struct RainRig;

#[derive(Component)]
pub struct FogBox;

/// Three open cylinders round the camera (radius 2.5, 5 and 9 m, 16 m tall), seen from inside
/// and out; uv in metres round and up; the colour's red says which (0, 0.5, 1).
fn rain_mesh() -> Mesh {
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut col = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for (layer, r) in [2.5f32, 5.0, 9.0].into_iter().enumerate() {
        let segs = 28;
        let base = pos.len() as u32;
        for i in 0..=segs {
            let a = i as f32 / segs as f32 * TAU;
            let (x, z) = (a.cos() * r, a.sin() * r);
            for y in [-8.0f32, 8.0] {
                pos.push([x, y, z]);
                nrm.push([a.cos(), 0.0, a.sin()]);
                uv.push([a * r, y + 8.0]);
                col.push([layer as f32 / 2.0, 1.0, 1.0, 1.0]);
            }
        }
        for i in 0..segs as u32 {
            let (a, b, c, d) = (base + i * 2, base + i * 2 + 1, base + i * 2 + 2, base + i * 2 + 3);
            idx.extend_from_slice(&[a, c, b, b, c, d, a, b, c, b, d, c]);
        }
    }
    Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(bevy::mesh::Indices::U32(idx))
}

#[derive(Component)]
pub struct PoolLight;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Earth,
    /// the luminous void: an endless pale floor in bright fog
    Void,
    /// floating islands in dark fog
    Islands,
    /// a storm at night over the open sea
    Sea,
    /// in orbit, 250 km above the planet
    Orbit,
}

impl Style {
    pub fn parse(s: &str) -> Style {
        match s {
            "void" => Style::Void,
            "islands" => Style::Islands,
            "sea" => Style::Sea,
            "orbit" => Style::Orbit,
            _ => Style::Earth,
        }
    }
}

#[derive(Resource)]
pub struct Outdoors {
    /// time of day in hours
    pub hours: f32,
    /// the clock runs
    pub running: bool,
    pub clouds: f32,
    pub style: Style,
    dome: Option<(Entity, Handle<SkyMaterial>)>,
    env: Option<Handle<Image>>,
    /// what the environment map was last drawn for
    env_for: Option<(Style, f32)>,
    pool: Vec<Entity>,
    last_pick: f32,
    set_up: bool,
    /// the volume of fog (at High quality), its density texture (baked from the weather) and
    /// fine detail to go with it
    volume: Option<Entity>,
    density: Option<Handle<Image>>,
    detail: Vec<f32>,
    last_bake: f32,
    baked_at: Vec3,
    /// the endless floor (the void) and the rain round the camera
    floor: Option<Entity>,
    rain: Option<(Entity, Handle<RainMaterial>)>,
    /// clouds set by hand (`world:clouds:`) instead of by the weather
    clouds_set: bool,
    /// the world's water: its tint, its swell (m; 0: none), how opaque it is
    pub water: WaterLook,
}

impl Default for Outdoors {
    fn default() -> Self {
        Outdoors {
            hours: 16.5,
            running: false,
            clouds: 0.45,
            style: Style::Earth,
            dome: None,
            env: None,
            env_for: None,
            pool: Vec::new(),
            last_pick: f32::NEG_INFINITY,
            set_up: false,
            volume: None,
            density: None,
            detail: Vec::new(),
            last_bake: f32::NEG_INFINITY,
            baked_at: Vec3::ZERO,
            floor: None,
            rain: None,
            clouds_set: false,
            water: WaterLook::default(),
        }
    }
}

impl Outdoors {
    /// `time:<hours>` or `time:auto`; `clouds:<0-1>`.
    pub fn command(&mut self, verb: &str, arg: &str) -> Option<String> {
        match verb {
            "time" if arg == "auto" => {
                self.running = true;
                Some("Time goes by".into())
            }
            "time" => {
                let h: f32 = arg.parse().ok()?;
                self.hours = h.rem_euclid(24.0);
                self.running = false;
                Some(format!("Time: {:02}:{:02}", self.hours as u32, (self.hours.fract() * 60.0) as u32))
            }
            "clouds" => {
                self.clouds = arg.parse::<f32>().ok()?.clamp(0.0, 1.0);
                self.clouds_set = true;
                Some(format!("Clouds: {:.0}%", self.clouds * 100.0))
            }
            _ => None,
        }
    }
}

/// A moon: towards it, its angular radius (radians), colour and glow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moon {
    pub dir: Vec3,
    pub radius: f32,
    pub colour: Vec3,
    pub glow: f32,
}

/// The light and sky for a style and time of day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyLook {
    pub style: Style,
    /// towards the sun
    pub sun: Vec3,
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    /// the key light's colour and strength in lux (the sun, or a moon at night)
    pub light: Vec3,
    pub lux: f32,
    /// towards the key light
    pub key: Vec3,
    /// 0 by day, 1 at night
    pub night: f32,
    /// the environment map's strength, cd/m²
    pub ambient: f32,
    /// how far you can see through the haze, metres
    pub visibility: f32,
    pub moons: [Option<Moon>; 3],
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn sky_at(style: Style, hours: f32) -> SkyLook {
    match style {
        Style::Earth => earth_at(hours),
        Style::Void => void_at(hours),
        Style::Islands => islands_at(hours),
        Style::Sea => sea_at(hours),
        Style::Orbit => orbit_at(hours),
    }
}

/// The sun rises in the east (+x) at 6, is highest at noon, sets in the west at 18, and
/// stands a little to the south (+z, behind the camera's starting view).
fn earth_at(hours: f32) -> SkyLook {
    let phase = (hours - 6.0) / 24.0 * TAU;
    let sun = Vec3::new(phase.cos(), phase.sin() * 0.88, 0.42).normalize();
    let alt = sun.y;
    let day = smooth(-0.08, 0.22, alt);
    let low = 1.0 - smooth(0.0, 0.32, alt);
    let dusk = low * smooth(-0.22, 0.02, alt);
    let night = 1.0 - smooth(-0.24, -0.02, alt);
    let day_zen = Vec3::new(0.16, 0.36, 0.88);
    let day_hor = Vec3::new(0.6, 0.76, 0.95);
    let dusk_zen = Vec3::new(0.18, 0.22, 0.5);
    let dusk_hor = Vec3::new(1.1, 0.48, 0.22);
    let night_zen = Vec3::new(0.005, 0.009, 0.028);
    let night_hor = Vec3::new(0.022, 0.032, 0.07);
    let zenith = day_zen.lerp(dusk_zen, dusk).lerp(night_zen, night);
    let horizon = day_hor.lerp(dusk_hor, dusk).lerp(night_hor, night);
    let ground = horizon * Vec3::new(0.32, 0.31, 0.29);
    let warm = Vec3::new(1.0, 0.5, 0.26);
    let white = Vec3::new(1.0, 0.95, 0.87);
    let sun_light = warm.lerp(white, smooth(0.02, 0.4, alt));
    let sun_lux = 6500.0 * smooth(-0.03, 0.2, alt);
    // the moon: opposite the sun, dim and blue
    let moon_lux = 160.0 * night;
    let moon_dir = Vec3::new(-sun.x, (-sun.y).max(0.25), -sun.z).normalize();
    let (light, lux, key) =
        if sun_lux >= moon_lux { (sun_light, sun_lux, sun) } else { (Vec3::new(0.55, 0.66, 1.0), moon_lux, moon_dir) };
    let moon = (night > 0.05).then_some(Moon { dir: moon_dir, radius: 0.03, colour: Vec3::new(0.9, 0.92, 1.0), glow: night });
    SkyLook {
        style: Style::Earth,
        sun,
        zenith,
        horizon,
        ground,
        light,
        lux,
        key,
        night,
        ambient: 450.0 + 350.0 * day,
        visibility: 520.0 - 260.0 * night,
        moons: [moon, None, None],
    }
}

/// The void: an endless pale floor in luminous fog. No sun: the light is soft and comes from
/// everywhere, a little more from high up; three moons hang faint in the brightness and drift
/// slowly round with the clock.
fn void_at(hours: f32) -> SkyLook {
    let turn = hours / 24.0 * TAU * 0.25;
    let at = |yaw: f32, pitch: f32| {
        let y = yaw + turn;
        Vec3::new(y.sin() * pitch.cos(), pitch.sin(), -y.cos() * pitch.cos())
    };
    let pale = Moon { dir: at(0.3, 0.62), radius: 0.15, colour: Vec3::new(1.0, 0.98, 0.95), glow: 0.4 };
    let rose = Moon { dir: at(-1.0, 0.34), radius: 0.07, colour: Vec3::new(1.0, 0.84, 0.82), glow: 0.4 };
    let blue = Moon { dir: at(2.5, 0.9), radius: 0.04, colour: Vec3::new(0.86, 0.94, 1.0), glow: 0.4 };
    SkyLook {
        style: Style::Void,
        sun: Vec3::NEG_Y,
        // white light everywhere: a touch cooler overhead, warm at eye level where the fog
        // glows brightest, pale below
        zenith: Vec3::new(0.74, 0.78, 0.86),
        horizon: Vec3::new(1.0, 0.97, 0.93),
        ground: Vec3::new(0.9, 0.9, 0.93),
        light: Vec3::new(1.0, 0.98, 0.95),
        lux: 2400.0,
        key: Vec3::new(0.25, 0.92, 0.3).normalize(),
        night: 0.0,
        ambient: 1400.0,
        visibility: 130.0,
        moons: [Some(pale), Some(rose), Some(blue)],
    }
}

/// How high orbit is: 250 km above a planet the Earth's size.
pub const ORBIT_KM: f32 = 250.0;
const PLANET_KM: f32 = 6371.0;

/// The height of the planet's rim below eye level, as `dir.y` (sine of the dip): -0.272, the
/// planet's edge 15.8° down. Everything below it is the planet.
pub fn orbit_limb() -> f32 {
    let h = 1.0 + ORBIT_KM / PLANET_KM;
    -(1.0 - 1.0 / (h * h)).sqrt()
}

/// Where a look in direction `dir` meets the planet, as the surface's up there (a unit
/// vector from the planet's centre), or None if it goes past into space.
pub fn orbit_ground(dir: Vec3) -> Option<Vec3> {
    let h = 1.0 + ORBIT_KM / PLANET_KM;
    let b = h * dir.y;
    let disc = b * b - (h * h - 1.0);
    if disc < 0.0 || b >= 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    Some((Vec3::new(0.0, h, 0.0) + dir * t).normalize())
}

/// Orbit: no air, so the sky is black and the sun white and hard; the day goes round with the
/// clock as on the ground, but it's night only while the planet is between you and the sun.
fn orbit_at(hours: f32) -> SkyLook {
    let phase = (hours - 6.0) / 24.0 * TAU;
    let sun = Vec3::new(phase.cos(), phase.sin() * 0.88, 0.42).normalize();
    // the planet covers everything below its rim: the sun sets behind it there
    let limb = orbit_limb();
    let night = 1.0 - smooth(limb - 0.03, limb + 0.01, sun.y);
    // how much of the planet below is in daylight
    let below = smooth(-0.6, 0.5, sun.y);
    let moon_dir = Vec3::new(-0.62, 0.38, -0.69).normalize();
    let moon = Moon { dir: moon_dir, radius: 0.016, colour: Vec3::new(0.95, 0.94, 0.9), glow: 0.2 };
    SkyLook {
        style: Style::Orbit,
        sun,
        zenith: Vec3::new(0.0004, 0.0005, 0.0009),
        // the rim of the atmosphere, lit blue by day
        horizon: Vec3::new(0.06, 0.14, 0.34) * (0.08 + 0.92 * (1.0 - night)),
        // the planet, seen as a whole: blue, cloud-flecked, dark on its night side
        ground: Vec3::new(0.09, 0.14, 0.24) * (0.03 + below),
        light: Vec3::new(1.0, 0.98, 0.95),
        lux: 14000.0 * (1.0 - night),
        // on the night side the planet's glow lights from below, faintly
        key: if night < 0.5 { sun } else { Vec3::new(0.2, -0.9, 0.3).normalize() },
        night,
        ambient: 260.0 + 340.0 * below,
        visibility: 20000.0,
        moons: [Some(moon), None, None],
    }
}

/// The dark sea: a storm at night. Black cloud overhead, slate at the horizon where the
/// spray hangs, and one moon low in a break in the cloud, its light lying across the water in
/// a cold path (it's the key light, so the waves catch it).
fn sea_at(hours: f32) -> SkyLook {
    let turn = hours / 24.0 * TAU * 0.25;
    let yaw = 0.9 + turn;
    let pitch = 0.17f32;
    let dir = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos());
    let moon = Moon { dir, radius: 0.11, colour: Vec3::new(0.86, 0.9, 1.0), glow: 1.6 };
    SkyLook {
        style: Style::Sea,
        sun: Vec3::NEG_Y,
        zenith: Vec3::new(0.012, 0.015, 0.021),
        horizon: Vec3::new(0.052, 0.064, 0.074),
        ground: Vec3::new(0.02, 0.027, 0.031),
        light: Vec3::new(0.72, 0.8, 0.95),
        lux: 520.0,
        key: dir,
        night: 1.0,
        ambient: 900.0,
        visibility: 120.0,
        moons: [Some(moon), None, None],
    }
}

/// Floating islands in dark fog: no sun, three moons that drift slowly round with the clock.
fn islands_at(hours: f32) -> SkyLook {
    let turn = hours / 24.0 * TAU * 0.25;
    let at = |yaw: f32, pitch: f32| {
        let y = yaw + turn;
        Vec3::new(y.sin() * pitch.cos(), pitch.sin(), -y.cos() * pitch.cos())
    };
    // the great pale moon ahead and up, the red one low to the left, the cold one behind
    let pale = Moon { dir: at(0.35, 0.52), radius: 0.16, colour: Vec3::new(0.92, 0.88, 0.8), glow: 1.0 };
    let red = Moon { dir: at(-1.15, 0.2), radius: 0.075, colour: Vec3::new(1.0, 0.32, 0.18), glow: 1.2 };
    let cold = Moon { dir: at(2.6, 0.75), radius: 0.04, colour: Vec3::new(0.45, 0.9, 1.0), glow: 1.4 };
    SkyLook {
        style: Style::Islands,
        sun: Vec3::NEG_Y,
        // fog in every direction, going on for ever: slate blue, darker overhead, brightest in
        // the band at eye level where the moonlight lies in it, and a sea of fog below
        zenith: Vec3::new(0.026, 0.03, 0.044),
        horizon: Vec3::new(0.07, 0.076, 0.094),
        ground: Vec3::new(0.05, 0.056, 0.07),
        light: Vec3::new(0.78, 0.84, 1.0),
        lux: 420.0,
        key: pale.dir,
        night: 1.0,
        ambient: 1600.0,
        visibility: 95.0,
        moons: [Some(pale), Some(red), Some(cold)],
    }
}

fn angle(a: Vec3, b: Vec3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos()
}

/// Value noise that repeats every `period` cells across x and z.
fn tiled_noise(x: f32, y: f32, z: f32, period: i32, seed: u32) -> f32 {
    let hash = |i: i32, j: i32, k: i32| {
        let (i, k) = (i.rem_euclid(period), k.rem_euclid(period));
        let mut h = (i as u32).wrapping_mul(0x8da6b343) ^ (j as u32).wrapping_mul(0xd8163841) ^ (k as u32).wrapping_mul(0xcb1ab31f) ^ seed;
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 65535.0
    };
    let (i, j, k) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let f = |v: f32| {
        let t = v - v.floor();
        t * t * (3.0 - 2.0 * t)
    };
    let (u, v, w) = (f(x), f(y), f(z));
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dj: i32, dk: i32| l(hash(i, j + dj, k + dk), hash(i + 1, j + dj, k + dk), u);
    l(l(c(0, 0), c(1, 0), v), l(c(0, 1), c(1, 1), v), w)
}

/// The fog volume's texture: texels across x, up, across z.
pub const VOL: [usize; 3] = [64, 16, 64];
/// Its densest (what 255 means), per metre.
const VOL_MAX: f32 = 0.12;
/// The fog's absorption and scattering (their sum turns extinction into density).
const FOG_ABSORB: f32 = 0.06;
const FOG_SCATTER: f32 = 0.6;

/// Fine detail for the fog: billowing noise, flattened so it lies in layers, tiling across x
/// and z, VOL in size, 0-1.
pub fn fog_detail() -> Vec<f32> {
    let [nx, ny, nz] = VOL;
    let mut d = Vec::with_capacity(nx * ny * nz);
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                let (fx, fy, fz) = (x as f32 / nx as f32, y as f32 / ny as f32, z as f32 / nz as f32);
                let mut n = 0.0;
                let mut amp = 0.5;
                for (o, period) in [4, 8, 16].into_iter().enumerate() {
                    let p = period as f32;
                    n += amp * tiled_noise(fx * p, fy * p * 0.5 + o as f32 * 7.0, fz * p, period, 17 + o as u32);
                    amp *= 0.5;
                }
                d.push(smooth(0.3, 0.8, n / 0.875));
            }
        }
    }
    d
}

fn detail_at(detail: &[f32], x: f32, y: f32, z: f32) -> f32 {
    let [nx, ny, nz] = VOL;
    let (i0, j0, k0) = (x.floor() as i32, y.floor().clamp(0.0, (ny - 1) as f32) as i32, z.floor() as i32);
    let (fx, fz) = (x - x.floor(), z - z.floor());
    let at = |i: i32, k: i32| detail[(k.rem_euclid(nz as i32) as usize * ny + j0 as usize) * nx + i.rem_euclid(nx as i32) as usize];
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    l(l(at(i0, k0), at(i0 + 1, k0), fx), l(at(i0, k0 + 1), at(i0 + 1, k0 + 1), fx), fz)
}

/// The fog volume: a box this big round the camera, m. (Bevy's volumetric fog lights a
/// volume as if light crossed all of it and only inside the sun's shadow cascades, so it is
/// kept near; the distance fog carries on beyond.)
pub const FOG_BOX: Vec3 = Vec3::new(64.0, 24.0, 64.0);

/// How thick the fog is (per metre) at each texel of the box round `centre`, from the
/// weather: cloud and fog water (Kunkel: extinction 0.11 LWC^0.88 per metre), haze as the air
/// nears saturation, and for the luminous void a standing mist, thicker low down; all broken
/// up by detail that stays put in the world and drifts with the wind, and thinning to nothing
/// at the box's sides and top so it never shows an edge. Bytes, 255 = VOL_MAX.
pub fn bake_fog(wx: &Weather, style: Style, detail: &[f32], t: f32, centre: Vec3) -> Vec<u8> {
    let [nx, ny, nz] = VOL;
    let period = [wx.nx as f32 * wx.cell[0], wx.nz as f32 * wx.cell[2]];
    let wind = wx.climate.wind();
    let mut out = Vec::with_capacity(nx * ny * nz);
    for k in 0..nz {
        let fz = (k as f32 + 0.5) / nz as f32;
        for j in 0..ny {
            let up = (j as f32 + 0.5) / ny as f32;
            for i in 0..nx {
                let fx = (i as f32 + 0.5) / nx as f32;
                let p = centre + (Vec3::new(fx, up, fz) - 0.5) * FOG_BOX;
                let lwc = wx.cloud_at(p.to_array());
                let rh = wx.humidity_at(p.to_array());
                let height = (p.y - wx.climate.ground).max(0.0);
                let mut beta = 0.11 * lwc.max(0.0).powf(0.88) + 0.02 * smooth(0.82, 1.0, rh);
                if style == Style::Void {
                    beta += 0.008 + 0.02 * (-height / 6.0).exp();
                }
                // the detail, in its own repeating texels, carried along by the wind
                let dx = ((p.x - wind[0] * t - wx.origin[0]) / period[0]).rem_euclid(1.0) * VOL[0] as f32;
                let dz = ((p.z - wind[1] * t - wx.origin[1]) / period[1]).rem_euclid(1.0) * VOL[2] as f32;
                let d = detail_at(detail, dx, (height / 2.0).min((VOL[1] - 1) as f32), dz);
                let edge = smooth(0.0, 0.22, fx.min(1.0 - fx)) * smooth(0.0, 0.22, fz.min(1.0 - fz)) * smooth(0.0, 0.3, 1.0 - up);
                let beta = beta * (0.3 + 1.4 * d) * edge;
                let dens = beta / (FOG_ABSORB + FOG_SCATTER);
                out.push(((dens / VOL_MAX).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    out
}

fn volume_image(data: Vec<u8>) -> Image {
    let [nx, ny, nz] = VOL;
    let mut img = Image::new(
        Extent3d { width: nx as u32, height: ny as u32, depth_or_array_layers: nz as u32 },
        TextureDimension::D3,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

/// The sky's colour looking along `dir`, without clouds, stars or drifting fog banks: the
/// shader's sky, for the environment map.
pub fn sky_colour(look: &SkyLook, dir: Vec3) -> Vec3 {
    let e = dir.y;
    let mut c = if e >= 0.0 {
        look.horizon.lerp(look.zenith, e.max(0.0).powf(0.45))
    } else {
        look.horizon.lerp(look.ground, (-e * 5.0).clamp(0.0, 1.0))
    };
    if look.style == Style::Void {
        // the luminous void: brightest at eye level, softly cooler overhead, pale below
        c = if e >= 0.0 {
            look.horizon.lerp(look.zenith, smooth(0.0, 1.0, e))
        } else {
            look.horizon.lerp(look.ground, smooth(0.0, 0.5, -e))
        };
    } else if look.style == Style::Earth {
        let towards = dir.dot(look.sun).max(0.0);
        c += look.light * (1.0 - look.night) * (towards.powf(10.0) * 0.35 + towards.powf(300.0) * 2.0);
    } else if look.style == Style::Orbit {
        let towards = dir.dot(look.sun).max(0.0);
        match orbit_ground(dir) {
            Some(n) => {
                // the ground: lit where the sun is up over it, bluer and brighter as the
                // look grazes the atmosphere towards the rim
                let day = smooth(-0.1, 0.25, n.dot(look.sun));
                let graze = (1.0 - (-dir.dot(n)).clamp(0.0, 1.0)).powi(3);
                c = Vec3::new(0.09, 0.14, 0.24) * (0.02 + 1.3 * day);
                c = c.lerp(Vec3::new(0.25, 0.45, 0.9) * (0.1 + 0.9 * day), graze * 0.7);
            }
            None => {
                let above = e - orbit_limb();
                c = look.zenith + look.horizon * (-above / 0.015).exp();
                // the sun, unsoftened by any air
                c += look.light * (1.0 - look.night) * (towards.powf(800.0) * 2.0 + towards.powf(60.0) * 0.04);
            }
        }
    } else {
        // the void's fog has no edge: darker overhead, a sea of fog below, brightest at eye level
        c = if e >= 0.0 {
            look.horizon.lerp(look.zenith, smooth(0.0, 0.9, e))
        } else {
            look.horizon.lerp(look.ground, smooth(0.0, 0.6, -e))
        };
    }
    // in the void the moons are veiled by the fog
    let veil = match look.style {
        Style::Earth => 1.0,
        Style::Islands => 0.12,
        Style::Sea => 0.5,
        Style::Orbit => 1.0,
        Style::Void => 0.18,
    };
    for m in look.moons.iter().flatten() {
        // the planet hides what's behind it
        if look.style == Style::Orbit && orbit_ground(dir).is_some() {
            break;
        }
        let a = angle(dir, m.dir);
        c += m.colour * m.glow * 0.08 * (-a / (m.radius * 2.5)).exp();
        if a < m.radius {
            c += m.colour * 1.2 * veil;
        }
    }
    c
}

/// A cube of the sky, faces in wgpu's order (+x, -x, +y, -y, +z, -z), half floats.
pub fn sky_cube(look: &SkyLook, size: u32) -> Image {
    let n = size as usize;
    let mut data = Vec::with_capacity(n * n * 6 * 8);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let dir = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let c = sky_colour(look, dir);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend_from_slice(&f16_bits(ch).to_le_bytes());
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img
}

/// f32 to IEEE half-float bits (round to nearest; enough for colours).
fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if v.is_nan() {
        return 0x7e00;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    let h = sign as u32 | ((exp as u32) << 10) | (man >> 13);
    (h + ((man >> 12) & 1)) as u16
}

/// The sky dome: a big sphere seen from inside.
pub fn dome_mesh() -> Mesh {
    let mut mesh = Mesh::from(Sphere::new(SKY_RADIUS).mesh().uv(48, 24));
    // seen from inside: turn every triangle round
    if let Some(bevy::mesh::Indices::U32(idx)) = mesh.indices_mut() {
        idx.chunks_mut(3).for_each(|t| t.swap(1, 2));
    }
    mesh
}

/// How a world's water looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterLook {
    /// multiplied with the water tile, each 0-1
    pub tint: [f32; 3],
    /// metres from the calm surface to a crest (0: none, at most 3)
    pub swell: f32,
    /// how much it hides what's under it, looking straight down (0-1)
    pub opacity: f32,
}

impl Default for WaterLook {
    fn default() -> Self {
        WaterLook { tint: crate::blocks::WATER_TINT, swell: 0.0, opacity: crate::blocks::WATER_OPACITY }
    }
}

/// A world's `"water"` setting, or the usual clear water when it says nothing; nonsense is
/// clamped or left out.
pub fn water_look(v: Option<&serde_json::Value>) -> WaterLook {
    let mut look = WaterLook::default();
    let num = |k: &str| v.and_then(|v| v.get(k)).and_then(|h| h.as_f64()).map(|h| h as f32).filter(|h| h.is_finite());
    if let Some(c) = v.and_then(|v| v.get("colour")).and_then(|c| c.as_array()).filter(|c| c.len() == 3) {
        let rgb: Vec<f32> = c.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).filter(|x| x.is_finite()).collect();
        if rgb.len() == 3 {
            look.tint = [rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0)];
        }
    }
    if let Some(h) = num("swell") {
        look.swell = h.clamp(0.0, 3.0);
    }
    if let Some(o) = num("opacity") {
        look.opacity = o.clamp(0.0, 1.0);
    }
    look
}

pub fn sky_uniform(look: &SkyLook, clouds: f32, wind: Vec4) -> SkyUniform {
    let v = |c: Vec3| c.extend(1.0);
    let moon = |m: &Option<Moon>| m.map_or((Vec4::ZERO, Vec4::ZERO), |m| (m.dir.extend(m.radius), m.colour.extend(m.glow)));
    let (a, ac) = moon(&look.moons[0]);
    let (b, bc) = moon(&look.moons[1]);
    let (c, cc) = moon(&look.moons[2]);
    let style = match look.style {
        Style::Earth => 0.0,
        Style::Islands | Style::Sea => 1.0,
        Style::Void => 2.0,
        Style::Orbit => 3.0,
    };
    SkyUniform {
        sun: look.sun.extend(0.0),
        zenith: v(look.zenith),
        horizon: v(look.horizon),
        ground: v(look.ground),
        sun_colour: v(look.light),
        misc: Vec4::new(look.night, if style > 0.0 { 0.0 } else { clouds }, 1.0 - look.night * 0.85, style),
        moon_a: a,
        moon_b: b,
        moon_c: c,
        moon_a_colour: ac,
        moon_b_colour: bc,
        moon_c_colour: cc,
        wind,
    }
}

/// Every frame in a world: the clock, the key light, the sky dome, the environment map, the
/// haze, and the lamps and fires.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut out: ResMut<Outdoors>,
    mut w: ResMut<WorldClient>,
    mut studio: ResMut<Studio>,
    real: Res<Time<Real>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut images: ResMut<Assets<Image>>,
    mut sky_mats: ResMut<Assets<SkyMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut lights: Query<(Entity, &mut DirectionalLight, &mut Transform, Has<KeyLight>, Has<FillLight>, Has<RimLight>)>,
    cams: Query<(Entity, &Transform), (With<crate::WorldCamera>, Without<DirectionalLight>, Without<SkyDome>, Without<PoolLight>)>,
    mut domes: Query<&mut Transform, (With<SkyDome>, Without<DirectionalLight>, Without<PoolLight>)>,
    mut pool: Query<(&mut PointLight, &mut Transform, &mut Visibility), (With<PoolLight>, Without<DirectionalLight>, Without<SkyDome>)>,
    mut commands: Commands,
) {
    if !w.active() {
        return;
    }
    studio.outdoors = true;
    for (verb, arg) in std::mem::take(&mut w.sky_cmds) {
        match out.command(&verb, &arg) {
            Some(msg) => w.notice(msg),
            None => w.notice(format!("world:{verb}:{arg}?")),
        }
    }
    // the world's own settings, when they arrive
    if let Some(s) = w.take_settings() {
        out.style = Style::parse(s.get("sky").and_then(|v| v.as_str()).unwrap_or("earth"));
        if let Some(h) = s.get("time").and_then(|v| v.as_f64()) {
            out.hours = (h as f32).rem_euclid(24.0);
        }
        if let Some(c) = s.get("clouds").and_then(|v| v.as_f64()) {
            out.clouds = (c as f32).clamp(0.0, 1.0);
        }
        out.water = water_look(s.get("water"));
        // the water's look is pushed with the waves (below): do it now
        out.last_bake = f32::NEG_INFINITY;
    }
    if out.running {
        out.hours = (out.hours + real.delta_secs() / DAY * 24.0).rem_euclid(24.0);
    }
    let look = sky_at(out.style, out.hours);
    w.hours = out.hours;
    let Ok((cam, cam_t)) = cams.single() else { return };
    // ---- once: the dome, the light pool, the key light's shadow reach
    if !out.set_up {
        out.set_up = true;
        let mesh = dome_mesh();
        let mat = sky_mats.add(SkyMaterial { sky: sky_uniform(&look, out.clouds, Vec4::ZERO) });
        let dome = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(cam_t.translation),
                NotShadowCaster,
                NotShadowReceiver,
                SkyDome,
                crate::NoBevel,
                Name::new("sky"),
            ))
            .id();
        out.dome = Some((dome, mat));
        for _ in 0..LIGHTS {
            let e = commands
                .spawn((
                    PointLight { intensity: 0.0, range: 10.0, radius: 0.15, shadow_maps_enabled: false, ..default() },
                    Transform::default(),
                    Visibility::Hidden,
                    PoolLight,
                ))
                .id();
            out.pool.push(e);
        }
        for (e, _, _, key, _, _) in &lights {
            if key {
                commands.entity(e).insert(
                    CascadeShadowConfigBuilder {
                        num_cascades: 3,
                        minimum_distance: 0.1,
                        first_cascade_far_bound: 7.0,
                        maximum_distance: 70.0,
                        overlap_proportion: 0.25,
                    }
                    .build(),
                );
            }
        }
    }
    // ---- the key light (the sun or a moon); the studio's fill and rim off
    let shadows = studio.shadows && studio.level >= crate::Level::Medium;
    for (_, mut l, mut t, key, fill, rim) in &mut lights {
        if key {
            l.illuminance = look.lux;
            l.color = Color::linear_rgb(look.light.x, look.light.y, look.light.z);
            l.shadow_maps_enabled = shadows;
            l.shadow_depth_bias = 0.04;
            l.shadow_normal_bias = 1.6;
            *t = Transform::from_translation(look.key * 50.0).looking_at(Vec3::ZERO, Vec3::Y);
        } else if (fill || rim) && l.illuminance != 0.0 {
            l.illuminance = 0.0;
        }
    }
    let amb = if !studio.env_maps {
        // no light from the sky's environment map: flat ambient light in its place
        look.ambient * 0.6
    } else if look.style == Style::Void {
        55.0
    } else {
        30.0 + 50.0 * (1.0 - look.night)
    };
    if ambient.brightness != amb {
        ambient.brightness = amb;
        ambient.color = Color::linear_rgb(look.horizon.x, look.horizon.y, look.horizon.z).mix(&Color::WHITE, 0.5);
    }
    // ---- the dome follows the camera; its colours follow the time
    if let Some((dome, mat)) = &out.dome {
        if let Ok(mut t) = domes.get_mut(*dome) {
            t.translation = cam_t.translation;
        }
        // clouds from the weather (unless set by hand): more as the air nears saturation, grey
        // with rain; they drift on the wind
        let (cover, wind) = match w.weather.as_ref() {
            Some(wx) => {
                let c = wx.climate;
                let cover = if out.clouds_set { out.clouds } else { ((c.humidity - 0.4) * 1.6).clamp(0.05, 1.0) };
                let wv = c.wind();
                let grey = smooth(0.95, 1.15, c.humidity);
                (cover, Vec4::new(wv[0], wv[1], 0.0, grey))
            }
            None => (out.clouds, Vec4::ZERO),
        };
        let u = sky_uniform(&look, cover, wind);
        if sky_mats.get(mat).is_some_and(|m| m.sky != u) {
            if let Some(mut m) = sky_mats.get_mut(mat) {
                m.sky = u;
            }
        }
    }
    // ---- the environment map, redrawn when the sky has changed enough (every 6 game minutes)
    let stale = match out.env_for {
        None => true,
        Some((st, h)) => st != out.style || (out.hours - h).abs() > 0.1,
    };
    if stale && studio.env_maps {
        let cube = sky_cube(&look, ENV_SIZE);
        let handle = match &out.env {
            Some(h) => {
                if let Some(mut img) = images.get_mut(h) {
                    *img = cube;
                }
                h.clone()
            }
            None => images.add(cube),
        };
        commands.entity(cam).insert(bevy::light::GeneratedEnvironmentMapLight {
            environment_map: handle.clone(),
            intensity: look.ambient,
            ..default()
        });
        out.env = Some(handle);
        out.env_for = Some((out.style, out.hours));
    }
    let volumetric = out.volume.is_some();
    // ---- haze: the horizon's colour, warmed by the sun when looking towards it; as far as
    // the weather lets you see (the fog volume, when there is one, does the near fog itself)
    let h = look.horizon;
    let sun_haze = match look.style {
        Style::Islands | Style::Sea => look.light * 0.06,
        Style::Void | Style::Orbit => look.light * 0.0,
        Style::Earth => look.light * (1.0 - look.night) * 0.4,
    };
    let air_vis = w.air().map_or(f32::INFINITY, |a| a.visibility);
    let vis = if volumetric { look.visibility.max(220.0) } else { look.visibility.min(air_vis.max(25.0)) };
    commands.entity(cam).insert(DistanceFog {
        color: Color::linear_rgb(h.x, h.y, h.z),
        directional_light_color: Color::linear_rgba(sun_haze.x, sun_haze.y, sun_haze.z, 0.5),
        directional_light_exponent: if look.style == Style::Earth { 24.0 } else { 6.0 },
        // the void's fog closes in like a wall; earth's haze thickens gently with distance
        falloff: if look.style == Style::Earth {
            FogFalloff::from_visibility(vis)
        } else {
            FogFalloff::from_visibility_squared(vis)
        },
    });
    // ---- the nearest lamps (after dark) and fires (always), fires flickering
    let now = real.elapsed_secs();
    let lamps_lit = smooth(0.25, 0.8, look.night.max(1.0 - smooth(-0.02, 0.12, look.sun.y)));
    let size = w.block_size();
    if now - out.last_pick > 0.4 {
        out.last_pick = now;
        let me = Vec3::from(w.body.pos);
        let mut near: Vec<(f32, [i32; 3], LightKind)> = w
            .lights
            .iter()
            .filter(|(_, k)| **k == LightKind::Fire || lamps_lit > 0.01)
            .map(|(c, k)| ((IVec3::from(*c).as_vec3() + 0.5 - me).length_squared(), *c, *k))
            .filter(|(d, _, _)| *d < 80.0 * 80.0)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (i, e) in out.pool.iter().enumerate() {
            let Ok((mut l, mut t, mut v)) = pool.get_mut(*e) else { continue };
            match near.get(i) {
                Some((_, c, kind)) => {
                    let lift = if *kind == LightKind::Fire { 0.9 } else { 0.5 };
                    t.translation = (IVec3::from(*c).as_vec3() + Vec3::new(0.5, lift, 0.5)) * size;
                    l.color = match kind {
                        LightKind::Fire => Color::srgb(1.0, 0.52, 0.18),
                        LightKind::Lamp => Color::srgb(1.0, 0.78, 0.5),
                    };
                    l.range = if *kind == LightKind::Fire { 10.0 } else { 9.0 };
                    if *v != Visibility::Inherited {
                        *v = Visibility::Inherited;
                    }
                }
                None => {
                    if *v != Visibility::Hidden {
                        *v = Visibility::Hidden;
                    }
                    l.intensity = 0.0;
                }
            }
        }
    }
    // brightness every frame, so fires flicker
    for (i, e) in out.pool.iter().enumerate() {
        let Ok((mut l, t, v)) = pool.get_mut(*e) else { continue };
        if *v == Visibility::Hidden {
            continue;
        }
        let fire = l.color.to_srgba().blue < 0.3;
        l.intensity = if fire {
            let p = t.translation;
            let f = 0.78 + 0.14 * (now * 9.0 + p.x * 3.1).sin() + 0.08 * (now * 23.0 + p.z * 1.7 + i as f32).sin();
            32_000.0 * f
        } else {
            90_000.0 * lamps_lit
        };
    }
}

/// The weather made visible: a volume of fog baked from it (High quality), rain or snow round
/// the camera, flames leaning and water running with the wind, and the void's endless floor.
#[allow(clippy::too_many_arguments)]
pub fn weather_look(
    mut out: ResMut<Outdoors>,
    w: Res<WorldClient>,
    studio: Res<Studio>,
    real: Res<Time<Real>>,
    blocks: Res<crate::blocks::Blocks>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut flames: ResMut<Assets<crate::blocks::FlameMaterial>>,
    mut waters: ResMut<Assets<crate::blocks::WaterMaterial>>,
    mut floors: ResMut<Assets<FloorMaterial>>,
    mut rains: ResMut<Assets<RainMaterial>>,
    cams: Query<(Entity, &Transform), (With<crate::WorldCamera>, Without<EndlessFloor>, Without<RainRig>, Without<FogBox>)>,
    keys: Query<(Entity, Has<KeyLight>), With<DirectionalLight>>,
    mut placed: Query<(&mut Transform, Has<EndlessFloor>, Has<RainRig>), Without<crate::WorldCamera>>,
    mut commands: Commands,
) {
    if !w.active() || !w.ready {
        return;
    }
    let Ok((cam, cam_t)) = cams.single() else { return };
    let look = sky_at(out.style, out.hours);
    let now = real.elapsed_secs();
    let size = w.block_size();
    let air = w.air();
    let wind = air.map_or(Vec2::ZERO, |a| Vec2::new(a.wind[0], a.wind[2]));
    // ---- the endless floor
    match (w.floor, out.floor) {
        (Some(f), None) => {
            let mat = floors.add(FloorMaterial {
                base: StandardMaterial {
                    base_color: Color::srgb(0.86, 0.86, 0.89),
                    perceptual_roughness: 0.08,
                    reflectance: 0.55,
                    ..default()
                },
                extension: FloorExt { params: Vec4::new(4.0, 0.22, 0.05, 0.0) },
            });
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(Plane3d::default().mesh().size(1600.0, 1600.0).subdivisions(8))),
                    MeshMaterial3d(mat),
                    Transform::from_xyz(cam_t.translation.x, f as f32 * size, cam_t.translation.z),
                    EndlessFloor,
                    crate::NoBevel,
                    Name::new("endless floor"),
                ))
                .id();
            out.floor = Some(e);
        }
        (None, Some(e)) => {
            commands.entity(e).try_despawn();
            out.floor = None;
        }
        _ => {}
    }
    // ---- rain or snow round the camera
    let rain = air.map_or(0.0, |a| a.rain);
    let snow = air.is_some_and(|a| a.temperature < 0.5);
    if rain > 0.15 && out.rain.is_none() {
        let mat = rains.add(RainMaterial { params: Vec4::ZERO });
        let e = commands
            .spawn((
                Mesh3d(meshes.add(rain_mesh())),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(cam_t.translation),
                NotShadowCaster,
                NotShadowReceiver,
                RainRig,
                crate::NoBevel,
                Name::new("rain"),
            ))
            .id();
        out.rain = Some((e, mat));
    }
    if let Some((_, mat)) = &out.rain {
        let p = Vec4::new((rain / 8.0).clamp(0.0, 1.0) * (rain > 0.1) as i32 as f32, snow as i32 as f32, wind.x, wind.y);
        if rains.get(mat).is_some_and(|m| m.params != p) {
            if let Some(mut m) = rains.get_mut(mat) {
                m.params = p;
            }
        }
    }
    for (mut t, floor, rig) in &mut placed {
        if floor {
            if let Some(f) = w.floor {
                t.translation = Vec3::new(cam_t.translation.x, f as f32 * size, cam_t.translation.z);
            }
        } else if rig {
            t.translation = cam_t.translation;
        }
    }
    // ---- flames lean and water runs with the wind (four times a second)
    if now - out.last_bake.max(0.0) > 0.25 || out.last_bake < 0.0 {
        let fp = Vec4::new(3.2, wind.x, wind.y, 0.0);
        if flames.get(&blocks.flame).is_some_and(|m| m.params != fp) {
            if let Some(mut m) = flames.get_mut(&blocks.flame) {
                m.params = fp;
            }
        }
        let speed = wind.length();
        let dir = if speed > 0.05 { wind / speed } else { Vec2::new(0.8, 0.6) };
        let wp = Vec4::new((0.5 + speed / 5.0).clamp(0.4, 2.5), 1.0, dir.x, dir.y);
        // the swell rolls higher in a gale, never less than two thirds of the world's own
        let lift = out.water.swell * (0.67 + (speed / 12.0).min(1.0) * 0.33);
        let [r, g, b] = out.water.tint;
        let tint = Color::srgba(r, g, b, 1.0);
        let opacity = out.water.opacity;
        let changed = waters.get(&blocks.water).is_some_and(|m| {
            m.extension.waves != wp || m.extension.swell.x != lift || m.extension.swell.z != opacity || m.base.base_color != tint
        });
        if changed && let Some(mut m) = waters.get_mut(&blocks.water) {
            m.extension.waves = wp;
            m.extension.swell.x = lift;
            m.extension.swell.z = opacity;
            m.base.base_color = tint;
        }
    }
    // ---- the fog volume, baked from the weather twice a second (High quality)
    let Some(wx) = w.weather.as_ref() else { return };
    let foggy = !matches!(out.style, Style::Earth | Style::Orbit) || air.is_some_and(|a| a.visibility < 3000.0 || a.humidity > 0.9);
    let want = foggy && studio.level == crate::Level::High;
    let span = FOG_BOX;
    let floor_y = wx.climate.ground.max(cam_t.translation.y - 8.0);
    let centre = Vec3::new(cam_t.translation.x, floor_y + span.y * 0.5, cam_t.translation.z);
    // the texture was baked round where the box was then; slide it so the fog stays put
    let offset = (centre - out.baked_at) / span;
    // the fog shines by the light it scatters (Bevy's fog takes no light from the sky, so
    // that's scaled up to stand for it): the luminous void glows white all through
    let (colour, light_i, asym) = match out.style {
        Style::Void => (Color::srgb(1.0, 0.985, 0.96), 18.0, 0.3),
        Style::Islands => (Color::srgb(0.66, 0.7, 0.82), 3.0, 0.55),
        Style::Sea => (Color::srgb(0.55, 0.62, 0.68), 2.4, 0.6),
        Style::Orbit => (Color::srgb(0.0, 0.0, 0.0), 0.0, 0.5),
        Style::Earth => (Color::srgb(0.92, 0.93, 0.96), 2.6, 0.5),
    };
    let ambient_i = 0.0;
    if out.detail.is_empty() {
        out.detail = fog_detail();
    }
    match (want, out.volume) {
        (true, None) => {
            let tex = images.add(volume_image(bake_fog(wx, out.style, &out.detail, now, centre)));
            out.density = Some(tex.clone());
            out.last_bake = now;
            out.baked_at = centre;
            let e = commands
                .spawn((
                    FogVolume {
                        fog_color: colour,
                        density_factor: VOL_MAX,
                        density_texture: Some(tex),
                        density_texture_offset: offset,
                        absorption: FOG_ABSORB,
                        scattering: FOG_SCATTER,
                        scattering_asymmetry: asym,
                        light_intensity: light_i,
                        ..default()
                    },
                    Transform::from_translation(centre).with_scale(span),
                    FogBox,
                    Name::new("fog"),
                ))
                .id();
            out.volume = Some(e);
            commands.entity(cam).insert(VolumetricFog {
                ambient_color: Color::linear_rgb(look.horizon.x, look.horizon.y, look.horizon.z),
                ambient_intensity: ambient_i,
                jitter: 0.0,
                step_count: 40,
            });
            for (e, key) in &keys {
                if key {
                    commands.entity(e).insert(VolumetricLight);
                }
            }
            for e in &out.pool {
                commands.entity(*e).insert(VolumetricLight);
            }
        }
        (false, Some(v)) => {
            commands.entity(v).try_despawn();
            out.volume = None;
            commands.entity(cam).remove::<VolumetricFog>();
            for (e, _) in &keys {
                commands.entity(e).remove::<VolumetricLight>();
            }
            for e in &out.pool {
                commands.entity(*e).remove::<VolumetricLight>();
            }
        }
        (true, Some(v)) => {
            if now - out.last_bake > 0.5 {
                out.last_bake = now;
                out.baked_at = centre;
                let data = bake_fog(wx, out.style, &out.detail, now, centre);
                if let Some(h) = &out.density {
                    if let Some(mut img) = images.get_mut(h) {
                        img.data = Some(data);
                    }
                }
            }
            commands.entity(v).insert((
                Transform::from_translation(centre).with_scale(span),
                FogVolume {
                    fog_color: colour,
                    density_factor: VOL_MAX,
                    density_texture: out.density.clone(),
                    density_texture_offset: offset,
                    absorption: FOG_ABSORB,
                    scattering: FOG_SCATTER,
                    scattering_asymmetry: asym,
                    light_intensity: light_i,
                    ..default()
                },
            ));
            commands.entity(cam).insert(VolumetricFog {
                ambient_color: Color::linear_rgb(look.horizon.x, look.horizon.y, look.horizon.z),
                ambient_intensity: ambient_i,
                jitter: 0.0,
                step_count: 40,
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day() {
        let noon = sky_at(Style::Earth, 12.0);
        assert!(noon.sun.y > 0.8 && noon.night < 0.01 && noon.lux > 6000.0);
        let dusk = sky_at(Style::Earth, 18.2);
        assert!(dusk.horizon.x > dusk.horizon.z, "a warm horizon at dusk");
        let night = sky_at(Style::Earth, 0.0);
        assert!(night.night > 0.99 && night.lux < 200.0 && night.key.y > 0.2, "moonlight from above");
        // the sun rises in the east
        assert!(sky_at(Style::Earth, 7.0).sun.x > 0.5 && sky_at(Style::Earth, 17.0).sun.x < -0.5);
        assert!(noon.zenith.z > noon.zenith.x);
    }

    #[test]
    fn the_void() {
        // luminous: bright pale light all round, no harsh sun, three moons faint in it
        let v = sky_at(Style::Void, 16.5);
        assert_eq!(v.moons.iter().flatten().count(), 3);
        for d in [Vec3::Y, Vec3::X, Vec3::NEG_Y, Vec3::new(0.0, -0.3, 1.0).normalize()] {
            let c = sky_colour(&v, d);
            assert!(c.min_element() > 0.6 && c.max_element() < 1.6, "{d}: {c}");
        }
        // brightest at eye level, nothing dark anywhere
        assert!(sky_colour(&v, Vec3::X).length() > sky_colour(&v, Vec3::Y).length());
        assert_eq!(v.night, 0.0);
        assert!(v.visibility < 200.0);
        assert!(v.moons[0].unwrap().radius > v.moons[1].unwrap().radius && v.moons[1].unwrap().radius > v.moons[2].unwrap().radius);
        assert_ne!(sky_at(Style::Void, 3.0).moons[0].unwrap().dir, v.moons[0].unwrap().dir);
        assert_eq!(Style::parse("void"), Style::Void);
        assert_eq!(Style::parse("islands"), Style::Islands);
        assert_eq!(Style::parse("sea"), Style::Sea);
        assert_eq!(Style::parse("orbit"), Style::Orbit);
        assert_eq!(Style::parse("anything"), Style::Earth);
    }

    #[test]
    fn the_islands() {
        // fog all round: the same colour everywhere give or take a little, never black, never
        // bright, with nothing to mark a horizon
        let v = sky_at(Style::Islands, 16.5);
        for d in [Vec3::Y, Vec3::X, Vec3::NEG_Y, Vec3::new(0.0, -0.3, 1.0).normalize()] {
            let c = sky_colour(&v, d);
            assert!(c.length() > 0.04 && c.length() < 0.2, "{d}: {c}");
        }
        assert_eq!(v.key, v.moons[0].unwrap().dir);
        let m = v.moons[0].unwrap();
        assert!(sky_colour(&v, m.dir).length() > sky_colour(&v, Vec3::Y).length() * 1.5);
    }

    #[test]
    fn the_sea() {
        // darker than the islands overhead, the horizon lighter than the zenith, and the moon
        // low (so its light lies across the water) and the brightest thing in the sky
        let v = sky_at(Style::Sea, 0.0);
        let up = sky_colour(&v, Vec3::Y);
        assert!(up.length() < sky_colour(&sky_at(Style::Islands, 0.0), Vec3::Y).length());
        assert!(sky_colour(&v, Vec3::new(1.0, 0.02, 0.0).normalize()).length() > up.length());
        let m = v.moons[0].unwrap();
        assert!(m.dir.y > 0.05 && m.dir.y < 0.3, "{}", m.dir);
        assert_eq!(v.key, m.dir);
        assert!(sky_colour(&v, m.dir).length() > sky_colour(&v, Vec3::X).length() * 2.0);
    }

    #[test]
    fn in_orbit() {
        // the rim of the planet is 15.8 degrees below eye level, 250 km up
        assert!((orbit_limb().asin().to_degrees() + 15.8).abs() < 0.1, "{}", orbit_limb().asin().to_degrees());
        assert!(orbit_ground(Vec3::NEG_Y).is_some_and(|n| (n - Vec3::Y).length() < 1e-4));
        assert!(orbit_ground(Vec3::X).is_none());
        assert!(orbit_ground(Vec3::new(1.0, -0.25, 0.0).normalize()).is_none(), "just above the rim");
        assert!(orbit_ground(Vec3::new(1.0, -0.3, 0.0).normalize()).is_some(), "just below it");
        // by day: black overhead, the planet below lit, the sun far the brightest thing
        let day = sky_at(Style::Orbit, 12.0);
        assert!(day.night < 0.01 && day.lux > 10000.0);
        let up = sky_colour(&day, Vec3::Y);
        let down = sky_colour(&day, Vec3::NEG_Y);
        assert!(up.length() < 0.01 && down.length() > 0.1, "{up} {down}");
        assert!(sky_colour(&day, day.sun).length() > down.length() * 2.0);
        // at midnight the sun is behind the planet: night, the key light gone
        let night = sky_at(Style::Orbit, 0.0);
        assert!(night.night > 0.99 && night.lux < 1.0);
        assert!(sky_colour(&night, Vec3::NEG_Y).length() < down.length() * 0.2);
    }

    #[test]
    fn a_worlds_water() {
        use serde_json::json;
        assert_eq!(water_look(None), WaterLook::default());
        let dark = json!({"colour": [0.07, 0.11, 0.13], "swell": 0.75, "opacity": 0.93});
        assert_eq!(water_look(Some(&dark)), WaterLook { tint: [0.07, 0.11, 0.13], swell: 0.75, opacity: 0.93 });
        // nonsense is clamped or ignored, never passed to the shader
        let silly = json!({"colour": [4, -1, 0.5], "swell": 900, "opacity": -2});
        assert_eq!(water_look(Some(&silly)), WaterLook { tint: [1.0, 0.0, 0.5], swell: 3.0, opacity: 0.0 });
        let short = json!({"colour": [0.1, 0.2], "swell": "high"});
        assert_eq!(water_look(Some(&short)), WaterLook::default());
    }

    #[test]
    fn fog_from_the_weather() {
        use vox_core::weather::{Climate, Weather};
        let detail = fog_detail();
        assert_eq!(detail.len(), VOL[0] * VOL[1] * VOL[2]);
        let mean = |b: &[u8]| b.iter().map(|v| *v as f32).sum::<f32>() / b.len() as f32;
        let mut clear = Weather::new(Climate::preset("clear", 0.0).unwrap(), 3);
        clear.step(60.0);
        let mut fog = Weather::new(Climate::preset("fog", 0.0).unwrap(), 3);
        fog.step(60.0);
        let a = bake_fog(&clear, Style::Earth, &detail, 0.0, Vec3::new(0.0, 12.0, 0.0));
        let b = bake_fog(&fog, Style::Earth, &detail, 0.0, Vec3::new(0.0, 12.0, 0.0));
        assert!(mean(&b) > mean(&a) * 5.0 + 1.0, "fog is thicker than clear air: {} {}", mean(&a), mean(&b));
        // the void always has its mist, thicker low down
        let v = bake_fog(&clear, Style::Void, &detail, 0.0, Vec3::new(0.0, 12.0, 0.0));
        let [nx, ny, _] = VOL;
        let layer = |y: usize| (0..VOL[2]).map(|k| (0..nx).map(|i| v[(k * ny + y) * nx + i] as f32).sum::<f32>()).sum::<f32>();
        assert!(layer(0) > layer(ny - 1));
        // the detail drifts with the wind
        assert_ne!(bake_fog(&fog, Style::Earth, &detail, 0.0, Vec3::new(0.0, 12.0, 0.0)), bake_fog(&fog, Style::Earth, &detail, 30.0, Vec3::new(0.0, 12.0, 0.0)));
    }

    #[test]
    fn halves() {
        for v in [0.0f32, 1.0, -2.5, 0.5, 65504.0, 1e-3, 3.14159] {
            let back = half_to_f32(f16_bits(v));
            assert!((back - v).abs() <= v.abs() * 1e-3 + 1e-4, "{v} -> {back}");
        }
        assert_eq!(f16_bits(1e9), 0x7c00);
    }

    fn half_to_f32(h: u16) -> f32 {
        let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
        let e = ((h >> 10) & 0x1f) as i32;
        let m = (h & 0x3ff) as f32;
        if e == 0 { s * m * 2f32.powi(-24) } else { s * (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
    }

    #[test]
    fn the_cube() {
        let img = sky_cube(&sky_at(Style::Earth, 12.0), 8);
        let d = img.data.as_ref().unwrap();
        assert_eq!(d.len(), 8 * 8 * 6 * 8);
        let px = |face: usize, x: usize, y: usize, ch: usize| {
            let o = ((face * 64) + y * 8 + x) * 8 + ch * 2;
            half_to_f32(u16::from_le_bytes([d[o], d[o + 1]]))
        };
        // straight up is blue; looking down is dark ground
        assert!(px(2, 4, 4, 2) > px(2, 4, 4, 0));
        assert!(px(3, 4, 4, 1) < px(2, 4, 4, 1));
    }
}
