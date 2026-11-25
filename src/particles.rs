//! Particle effects in the same block style as everything else: little voxel cubes and voxel
//! shapes (hearts, Zs, drops, stars), some of them glowing so the bloom picks them up.
//!
//! - **Weather** that stays on: rain or snow, dust motes in the light, drifting confetti. Each
//!   set picks its own (`fx:auto`, the default: rain on the night set, dust in the diner,
//!   confetti at the rally), or choose one: `fx:rain`, `fx:snow`, `fx:dust`, `fx:drift`,
//!   `fx:off`.
//! - **Bursts:** `fx:fireworks` (and every sub), `fx:confetti` (and every raid), `fx:sparks`
//!   (and every big hit), `fx:hearts`, `fx:stars`.
//! - **On the character:** a stream of Zs while it dozes, sweat drops when you shout, and hearts,
//!   sparks or confetti when chat says `<3`, a fire emoji or `GG` (one burst every few seconds at
//!   most).
//!
//! How many particles live at once follows the quality setting.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    Rain,
    Snow,
    Dust,
    Drift,
}

impl Weather {
    pub fn name(self) -> &'static str {
        match self {
            Weather::Rain => "rain",
            Weather::Snow => "snow",
            Weather::Dust => "dust",
            Weather::Drift => "drift",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        [Weather::Rain, Weather::Snow, Weather::Dust, Weather::Drift]
            .into_iter()
            .find(|w| w.name() == s)
    }
    /// The weather a set has on its own.
    pub fn for_set(set: Option<&str>) -> Option<Self> {
        match set? {
            "night" => Some(Weather::Rain),
            "diner" => Some(Weather::Dust),
            "rally" => Some(Weather::Drift),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Burst {
    Fireworks,
    Confetti,
    Sparks,
    Hearts,
    Stars,
    /// a smaller confetti pop (chat's GG)
    Pop,
}

impl Burst {
    pub fn name(self) -> &'static str {
        match self {
            Burst::Fireworks => "fireworks",
            Burst::Confetti => "confetti",
            Burst::Sparks => "sparks",
            Burst::Hearts => "hearts",
            Burst::Stars => "stars",
            Burst::Pop => "pop",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        [
            Burst::Fireworks,
            Burst::Confetti,
            Burst::Sparks,
            Burst::Hearts,
            Burst::Stars,
            Burst::Pop,
        ]
        .into_iter()
        .find(|b| b.name() == s)
    }
}

/// What chat's words set off, if anything.
pub fn chat_burst(text: &str) -> Option<Burst> {
    let t = text.to_lowercase();
    let words: Vec<&str> = t
        .split(|c: char| !c.is_alphanumeric() && c != '<' && c != '3')
        .filter(|w| !w.is_empty())
        .collect();
    if t.contains("<3")
        || t.contains('\u{2764}')
        || t.contains('\u{1f496}')
        || t.contains('\u{1f60d}')
    {
        Some(Burst::Hearts)
    } else if t.contains('\u{1f525}') {
        Some(Burst::Sparks)
    } else if words.iter().any(|w| *w == "gg" || *w == "ggs") || t.contains('\u{1f389}') {
        Some(Burst::Pop)
    } else if t.contains('\u{2b50}') || t.contains('\u{2728}') {
        Some(Burst::Stars)
    } else {
        None
    }
}

#[derive(Resource)]
pub struct Particles {
    pub on: bool,
    /// None = the set's own weather; Some(None) = none
    pub weather: Option<Option<Weather>>,
    pub queue: Vec<Burst>,
    /// chat bursts: not more than one every few seconds
    last_chat: f32,
    shapes: HashMap<&'static str, Handle<Mesh>>,
    mats: HashMap<(u32, bool), Handle<StandardMaterial>>,
    count: usize,
    rng: u32,
    timers: HashMap<&'static str, f32>,
}

#[derive(Component)]
pub struct Particle {
    vel: Vec3,
    age: f32,
    life: f32,
    gravity: f32,
    drag: f32,
    spin: Vec3,
    size: f32,
    /// side-to-side flutter (confetti, snow, hearts)
    sway: f32,
    /// grows as it rises (smoke-like), else shrinks away at the end
    grow: bool,
    /// a firework rocket: bursts when it stops climbing
    rocket: Option<u32>,
    /// weather comes back to the top when it reaches the floor
    weather: bool,
}

impl Particle {
    fn new(vel: Vec3, life: f32, size: f32) -> Self {
        Particle {
            vel,
            age: 0.0,
            life,
            gravity: 0.0,
            drag: 0.0,
            spin: Vec3::ZERO,
            size,
            sway: 0.0,
            grow: false,
            rocket: None,
            weather: false,
        }
    }
}

impl Particles {
    pub fn new(on: bool, weather: Option<&str>) -> Self {
        let weather = match weather {
            None | Some("auto") | Some("") => None,
            Some("off") | Some("none") => Some(None),
            Some(w) => Weather::parse(w).map(Some),
        };
        Particles {
            on,
            weather,
            queue: vec![],
            last_chat: -100.0,
            shapes: HashMap::new(),
            mats: HashMap::new(),
            count: 0,
            rng: 0x2545_f491,
            timers: HashMap::new(),
        }
    }

    /// fx:on (toggle), fx:auto, fx:off, fx:<weather>, fx:<burst>
    pub fn command(&mut self, arg: &str) -> Option<String> {
        if arg == "on" {
            self.on = !self.on;
            return Some(format!("Effects {}", if self.on { "on" } else { "off" }));
        }
        if arg == "auto" {
            self.weather = None;
            return Some("Weather: the set's own".into());
        }
        if arg == "off" || arg == "none" {
            self.weather = Some(None);
            return Some("Weather off".into());
        }
        if let Some(w) = Weather::parse(arg) {
            self.weather = Some(Some(w));
            return Some(format!("Weather: {}", w.name()));
        }
        let b = Burst::parse(arg)?;
        self.queue.push(b);
        Some(format!("Effect: {}", b.name()))
    }

    pub fn weather_setting(&self) -> String {
        match self.weather {
            None => "auto".into(),
            Some(None) => "off".into(),
            Some(Some(w)) => w.name().into(),
        }
    }

    pub fn json(&self, set: Option<&str>) -> serde_json::Value {
        let now = match self.weather {
            None => Weather::for_set(set),
            Some(w) => w,
        };
        serde_json::json!({"on": self.on, "weather": self.weather_setting(), "now": now.map(|w| w.name()), "live": self.count})
    }

    fn r(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng & 0xFF_FFFF) as f32 / 16_777_216.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.r()
    }
}

/// How many particles may live at once, by quality.
pub fn budget(level: crate::lighting::Level) -> usize {
    match level {
        crate::lighting::Level::Low => 120,
        crate::lighting::Level::Medium => 400,
        crate::lighting::Level::High => 900,
    }
}

/// A mesh of unit-ish voxels from a little picture ('#' = a voxel), centred, `cell` on a side.
pub fn voxel_shape(rows: &[&str], cell: f32) -> Mesh {
    let h = rows.len() as f32;
    let w = rows.iter().map(|r| r.len()).max().unwrap_or(1) as f32;
    let filled = |x: i32, y: i32| -> bool {
        y >= 0
            && (y as usize) < rows.len()
            && x >= 0
            && rows[y as usize].as_bytes().get(x as usize) == Some(&b'#')
    };
    let (mut p, mut n, mut idx) = (vec![], vec![], vec![]);
    for (yi, row) in rows.iter().enumerate() {
        for (xi, ch) in row.bytes().enumerate() {
            if ch != b'#' {
                continue;
            }
            let (x, y) = (xi as i32, yi as i32);
            let cx = (x as f32 - w / 2.0) * cell;
            let cy = (h / 2.0 - y as f32 - 1.0) * cell;
            let (x0, x1, y0, y1, z0, z1) = (cx, cx + cell, cy, cy + cell, -cell / 2.0, cell / 2.0);
            let mut quad = |c: [[f32; 3]; 4], nrm: [f32; 3]| {
                let b = p.len() as u32;
                p.extend_from_slice(&c);
                n.extend_from_slice(&[nrm; 4]);
                idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            };
            quad(
                [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
                [0.0, 0.0, 1.0],
            );
            quad(
                [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
                [0.0, 0.0, -1.0],
            );
            if !filled(x, y - 1) {
                quad(
                    [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
                    [0.0, 1.0, 0.0],
                );
            }
            if !filled(x, y + 1) {
                quad(
                    [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
                    [0.0, -1.0, 0.0],
                );
            }
            if !filled(x + 1, y) {
                quad(
                    [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
                    [1.0, 0.0, 0.0],
                );
            }
            if !filled(x - 1, y) {
                quad(
                    [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
                    [-1.0, 0.0, 0.0],
                );
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
    .with_inserted_indices(Indices::U32(idx))
}

const HEART: [&str; 6] = [
    ".##.##.", "#######", "#######", ".#####.", "..###..", "...#...",
];
const ZED: [&str; 5] = ["#####", "...#.", "..#..", ".#...", "#####"];
const DROP: [&str; 4] = [".#.", "###", "###", ".#."];
const STAR: [&str; 5] = ["..#..", ".###.", "#####", ".###.", "..#.."];

pub fn setup(mut p: ResMut<Particles>, mut meshes: ResMut<Assets<Mesh>>) {
    p.shapes
        .insert("cube", meshes.add(Cuboid::new(1.0, 1.0, 1.0)));
    p.shapes
        .insert("flake", meshes.add(Cuboid::new(1.0, 1.0, 0.25)));
    p.shapes
        .insert("streak", meshes.add(Cuboid::new(0.25, 1.0, 0.25)));
    p.shapes
        .insert("heart", meshes.add(voxel_shape(&HEART, 1.0 / 7.0)));
    p.shapes
        .insert("zed", meshes.add(voxel_shape(&ZED, 1.0 / 5.0)));
    p.shapes
        .insert("drop", meshes.add(voxel_shape(&DROP, 1.0 / 4.0)));
    p.shapes
        .insert("star", meshes.add(voxel_shape(&STAR, 1.0 / 5.0)));
}

fn hue(h: f32) -> Color {
    Color::hsl(h.rem_euclid(1.0) * 360.0, 0.85, 0.6)
}

struct Spawn {
    shape: &'static str,
    colour: Color,
    glow: bool,
    at: Vec3,
    p: Particle,
}

/// Every frame, before the lighting takes this frame's cues: what happened (subs, raids, big
/// hits, chat's words) goes into the burst queue, which the next tick empties. A frame that
/// runs no tick leaves it waiting; a frame that runs several handles it once.
pub fn take_cues(
    mut p: ResMut<Particles>,
    cues: Res<crate::lighting::Cues>,
    chat: Res<crate::chat::Chat>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    for c in &cues.0 {
        p.queue.push(match c {
            crate::lighting::Cue::Sub => Burst::Fireworks,
            crate::lighting::Cue::Raid => Burst::Confetti,
            crate::lighting::Cue::BigHit => Burst::Sparks,
        });
    }
    if now - p.last_chat > 3.0 {
        if let Some(b) = chat.said.iter().find_map(|t| chat_burst(t)) {
            p.queue.push(b);
            p.last_chat = now;
        }
    }
}

/// One fixed tick (`FixedUpdate`): spawn what's queued and due, then move everything.
#[allow(clippy::too_many_arguments)]
pub fn step(
    mut p: ResMut<Particles>,
    av: Res<crate::Avatar>,
    show: Res<crate::show::Show>,
    studio: Res<crate::lighting::Studio>,
    time: Res<Time>,
    heads: Query<(&crate::Joint, &GlobalTransform)>,
    mut parts: Query<(Entity, &mut Particle, &mut crate::sim::Interp)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let budget = budget(studio.level);
    let head = heads
        .iter()
        .find(|(j, _)| **j == crate::Joint::Head)
        .map(|(_, g)| g.translation())
        .unwrap_or(Vec3::new(0.0, 1.5, 0.0));
    let mut spawn: Vec<Spawn> = vec![];
    let room = |p: &Particles, n: usize| -> usize { budget.saturating_sub(p.count).min(n) };
    if p.on {
        for b in std::mem::take(&mut p.queue) {
            match b {
                Burst::Fireworks => {
                    for i in 0..3 {
                        let x = [-1.1, 1.2, 0.2][i];
                        let mut q = Particle::new(
                            Vec3::new(p.range(-0.15, 0.15), 3.2 + i as f32 * 0.35, 0.0),
                            3.0,
                            0.05,
                        );
                        q.gravity = 3.0;
                        q.rocket = Some((p.r() * 1000.0) as u32);
                        q.age = -(i as f32) * 0.45; // one after another
                        spawn.push(Spawn {
                            shape: "cube",
                            colour: Color::srgb(1.0, 0.85, 0.5),
                            glow: true,
                            at: Vec3::new(x, 0.2, -1.4),
                            p: q,
                        });
                    }
                }
                Burst::Confetti | Burst::Pop => {
                    let n = room(&p, if b == Burst::Pop { 30 } else { 140 });
                    for i in 0..n {
                        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                        let at = if b == Burst::Pop {
                            head + Vec3::new(0.0, 0.3, 0.2)
                        } else {
                            Vec3::new(side * 1.6, 0.3, 0.6)
                        };
                        let v = if b == Burst::Pop {
                            Vec3::new(p.range(-1.2, 1.2), p.range(1.5, 2.8), p.range(-0.5, 1.0))
                        } else {
                            Vec3::new(
                                -side * p.range(0.6, 1.8),
                                p.range(3.0, 4.6),
                                p.range(-0.6, 0.6),
                            )
                        };
                        let mut q = Particle::new(v, p.range(3.0, 4.5), p.range(0.035, 0.05));
                        q.gravity = 2.2;
                        q.drag = 1.6;
                        q.sway = p.range(0.6, 1.4);
                        q.spin =
                            Vec3::new(p.range(-9.0, 9.0), p.range(-9.0, 9.0), p.range(-9.0, 9.0));
                        let c = hue(p.r());
                        spawn.push(Spawn {
                            shape: "flake",
                            colour: c,
                            glow: false,
                            at,
                            p: q,
                        });
                    }
                }
                Burst::Sparks => {
                    let n = room(&p, 36);
                    for _ in 0..n {
                        let d =
                            Vec3::new(p.range(-1.0, 1.0), p.range(-0.2, 1.0), p.range(-0.3, 1.0))
                                .normalize_or_zero();
                        let mut q = Particle::new(
                            d * p.range(1.5, 3.2),
                            p.range(0.35, 0.7),
                            p.range(0.018, 0.03),
                        );
                        q.gravity = 4.0;
                        q.drag = 2.0;
                        let c = Color::srgb(1.0, p.range(0.45, 0.85), 0.15);
                        spawn.push(Spawn {
                            shape: "cube",
                            colour: c,
                            glow: true,
                            at: head + Vec3::new(0.0, 0.25, 0.25),
                            p: q,
                        });
                    }
                }
                Burst::Hearts => {
                    let n = room(&p, 8);
                    for _ in 0..n {
                        let mut q = Particle::new(
                            Vec3::new(p.range(-0.15, 0.15), p.range(0.35, 0.6), 0.0),
                            p.range(2.5, 3.5),
                            p.range(0.12, 0.18),
                        );
                        q.sway = p.range(0.5, 1.0);
                        q.grow = false;
                        let at =
                            Vec3::new(p.range(-0.9, 0.9), p.range(0.2, 0.9), p.range(0.2, 0.6));
                        spawn.push(Spawn {
                            shape: "heart",
                            colour: Color::srgb(0.95, p.range(0.2, 0.35), 0.4),
                            glow: false,
                            at,
                            p: q,
                        });
                    }
                }
                Burst::Stars => {
                    let n = room(&p, 10);
                    for i in 0..n {
                        let a = i as f32 / 10.0 * std::f32::consts::TAU;
                        let mut q =
                            Particle::new(Vec3::new(a.cos() * 0.6, 0.4, a.sin() * 0.3), 1.2, 0.08);
                        q.drag = 1.5;
                        q.spin = Vec3::new(0.0, 0.0, 3.0);
                        spawn.push(Spawn {
                            shape: "star",
                            colour: Color::srgb(1.0, 0.9, 0.35),
                            glow: true,
                            at: head + Vec3::new(0.0, 0.3, 0.1),
                            p: q,
                        });
                    }
                }
            }
        }
        // on the character: Zs while dozing, sweat when shouting
        let every = |p: &mut Particles, key: &'static str, gap: f32| -> bool {
            let due = p.timers.get(key).is_none_or(|t| now >= *t);
            if due {
                p.timers.insert(key, now + gap);
            }
            due
        };
        if av.asleep && every(&mut p, "zzz", 1.3) && room(&p, 1) > 0 {
            let mut q = Particle::new(Vec3::new(0.12, 0.25, 0.05), 3.0, 0.09);
            q.sway = 0.6;
            spawn.push(Spawn {
                shape: "zed",
                colour: Color::srgb(0.85, 0.9, 1.0),
                glow: false,
                at: head + Vec3::new(0.3, 0.55, 0.0),
                p: q,
            });
        }
        if av.face_override.is_some_and(|f| f.0 == crate::face::RAGE)
            && every(&mut p, "sweat", 0.25)
            && room(&p, 2) > 0
        {
            for side in [-1.0f32, 1.0] {
                let mut q = Particle::new(
                    Vec3::new(side * p.range(0.4, 0.8), p.range(0.6, 1.1), 0.1),
                    0.7,
                    0.05,
                );
                q.gravity = 4.0;
                spawn.push(Spawn {
                    shape: "drop",
                    colour: Color::srgb(0.55, 0.8, 1.0),
                    glow: false,
                    at: head + Vec3::new(side * 0.33, 0.45, 0.15),
                    p: q,
                });
            }
        }
        // weather
        let weather = match p.weather {
            None => Weather::for_set(show.live_set()),
            Some(w) => w,
        };
        if let Some(w) = weather {
            let (rate, cap) = match w {
                Weather::Rain => (160.0, 0.6),
                Weather::Snow => (40.0, 0.6),
                Weather::Dust => (6.0, 0.15),
                Weather::Drift => (5.0, 0.15),
            };
            let quality = match studio.level {
                crate::lighting::Level::Low => 0.3,
                crate::lighting::Level::Medium => 0.6,
                crate::lighting::Level::High => 1.0,
            };
            let carry = p.timers.get("weather").copied().unwrap_or(0.0) + rate * quality * dt;
            let n = carry.floor() as usize;
            p.timers.insert("weather", carry - n as f32);
            let n = n.min(((budget as f32 * cap) as usize).saturating_sub(p.count));
            for _ in 0..n {
                let at = Vec3::new(
                    p.range(-3.0, 3.0),
                    match w {
                        Weather::Rain | Weather::Snow => p.range(3.0, 3.6),
                        _ => p.range(0.3, 2.6),
                    },
                    p.range(-1.9, 1.9),
                );
                let mut q;
                let (shape, colour, glow);
                match w {
                    Weather::Rain => {
                        q = Particle::new(Vec3::new(-0.3, -7.0, 0.0), 1.0, 0.09);
                        (shape, colour, glow) = ("streak", Color::srgb(0.6, 0.7, 0.9), false);
                    }
                    Weather::Snow => {
                        q = Particle::new(Vec3::new(0.0, -0.5, 0.0), 9.0, p.range(0.02, 0.035));
                        q.sway = p.range(0.2, 0.5);
                        q.spin = Vec3::new(p.range(-1.0, 1.0), p.range(-1.0, 1.0), 0.0);
                        (shape, colour, glow) = ("cube", Color::srgb(0.95, 0.97, 1.0), false);
                    }
                    Weather::Dust => {
                        q = Particle::new(
                            Vec3::new(
                                p.range(-0.04, 0.04),
                                p.range(-0.02, 0.04),
                                p.range(-0.03, 0.03),
                            ),
                            p.range(6.0, 10.0),
                            p.range(0.008, 0.014),
                        );
                        q.sway = 0.08;
                        (shape, colour, glow) = ("cube", Color::srgb(1.0, 0.92, 0.75), true);
                    }
                    Weather::Drift => {
                        q = Particle::new(Vec3::new(0.0, -0.35, 0.0), 8.0, 0.035);
                        q.sway = p.range(0.4, 0.9);
                        q.spin =
                            Vec3::new(p.range(-4.0, 4.0), p.range(-4.0, 4.0), p.range(-4.0, 4.0));
                        (shape, colour, glow) = ("flake", hue(p.r()), false);
                    }
                }
                q.weather = matches!(w, Weather::Rain | Weather::Snow);
                spawn.push(Spawn {
                    shape,
                    colour,
                    glow,
                    at,
                    p: q,
                });
            }
        }
    } else {
        p.queue.clear();
    }
    // move, age, and pop firework rockets
    let mut live = 0;
    let mut bursts = vec![];
    for (e, mut q, mut ip) in &mut parts {
        match advance(&mut q, &mut ip.curr, dt, p.on) {
            Fate::Lives => live += 1,
            Fate::Gone => {
                commands.entity(e).despawn();
            }
            Fate::Bursts(at, seed) => {
                bursts.push((at, seed));
                commands.entity(e).despawn();
            }
        }
    }
    for (at, seed) in bursts {
        let h = (seed % 1000) as f32 / 1000.0;
        let n = budget.saturating_sub(live + spawn.len()).min(60);
        for i in 0..n {
            let d = Vec3::new(p.range(-1.0, 1.0), p.range(-1.0, 1.0), p.range(-1.0, 1.0))
                .normalize_or_zero();
            let mut q = Particle::new(d * p.range(1.6, 2.4), p.range(1.0, 1.6), 0.035);
            q.gravity = 1.2;
            q.drag = 1.8;
            let colour = hue(h + if i % 3 == 0 { 0.08 } else { 0.0 });
            spawn.push(Spawn {
                shape: "cube",
                colour,
                glow: true,
                at,
                p: q,
            });
        }
    }
    for s in spawn {
        let key = (
            s.colour
                .to_srgba()
                .to_u8_array()
                .iter()
                .take(3)
                .fold(0u32, |a, b| a << 8 | *b as u32),
            s.glow,
        );
        let mat = p
            .mats
            .entry(key)
            .or_insert_with(|| {
                let lin = s.colour.to_linear();
                mats.add(StandardMaterial {
                    base_color: s.colour,
                    emissive: if s.glow {
                        LinearRgba::rgb(lin.red * 3.0, lin.green * 3.0, lin.blue * 3.0)
                    } else {
                        LinearRgba::BLACK
                    },
                    perceptual_roughness: 0.7,
                    ..default()
                })
            })
            .clone();
        let Some(mesh) = p.shapes.get(s.shape).cloned() else {
            continue;
        };
        let size = s.p.size;
        let t = Transform::from_translation(s.at).with_scale(Vec3::splat(size));
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            t,
            crate::sim::Interp::new(t),
            s.p,
            Name::new("particle"),
        ));
        live += 1;
    }
    // many colours of confetti: keep the material cache from growing without end
    if p.mats.len() > 400 {
        p.mats.clear();
    }
    p.count = live;
}

/// What one tick did to a particle.
#[derive(Debug, PartialEq)]
enum Fate {
    Lives,
    Gone,
    /// a firework rocket that stopped climbing: burst here, with this seed
    Bursts(Vec3, u32),
}

/// Age, fall, drag, sway and spin one particle by `dt`.
fn advance(q: &mut Particle, t: &mut Transform, dt: f32, on: bool) -> Fate {
    q.age += dt;
    if q.age < 0.0 {
        t.scale = Vec3::ZERO;
        return Fate::Lives;
    }
    if q.age >= q.life || !on {
        return Fate::Gone;
    }
    let drag = (-q.drag * dt).exp();
    q.vel.y -= q.gravity * dt;
    q.vel *= drag;
    let sway = if q.sway > 0.0 {
        Vec3::new(
            (q.age * 3.1 + q.size * 97.0).sin(),
            0.0,
            (q.age * 2.3).cos() * 0.5,
        ) * q.sway
            * dt
    } else {
        Vec3::ZERO
    };
    t.translation += q.vel * dt + sway;
    let spin = q.spin * dt;
    t.rotate_local(Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z));
    if let Some(seed) = q.rocket {
        if q.vel.y <= 0.3 {
            return Fate::Bursts(t.translation, seed);
        }
    }
    if q.weather && t.translation.y < 0.02 {
        return Fate::Gone;
    }
    let u = q.age / q.life;
    let s = if q.grow {
        q.size * (1.0 + u)
    } else {
        q.size * (1.0 - ((u - 0.75) / 0.25).clamp(0.0, 1.0))
    };
    t.scale = Vec3::splat(s.max(0.0001));
    Fate::Lives
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Steps a firework rocket and a confetti flake on the fixed clock, keeping each tick's pose.
    #[derive(Component)]
    struct Probe {
        q: Particle,
        trail: Vec<(Vec3, Fate)>,
    }

    fn run_probe(time: Res<Time>, mut probes: Query<(&mut Probe, &mut crate::sim::Interp)>) {
        let dt = time.delta_secs();
        for (mut pr, mut ip) in &mut probes {
            let pr = &mut *pr;
            let f = advance(&mut pr.q, &mut ip.curr, dt, true);
            let at = ip.curr.translation;
            pr.trail.push((at, f));
        }
    }

    fn probe_trails(fps: f64) -> Vec<Vec<(Vec3, Fate)>> {
        let mut app = crate::sim::tests::test_app(Duration::from_secs_f64(1.0 / fps));
        app.add_systems(FixedUpdate, run_probe);
        let mut rocket = Particle::new(Vec3::new(0.1, 3.4, 0.0), 3.0, 0.05);
        rocket.gravity = 3.0;
        rocket.rocket = Some(42);
        let mut flake = Particle::new(Vec3::new(-1.0, 4.0, 0.3), 4.0, 0.04);
        flake.gravity = 2.2;
        flake.drag = 1.6;
        flake.sway = 1.0;
        flake.spin = Vec3::new(3.0, -5.0, 7.0);
        let ids: Vec<Entity> = [rocket, flake]
            .into_iter()
            .map(|q| {
                let t = Transform::default();
                app.world_mut()
                    .spawn((Probe { q, trail: vec![] }, crate::sim::Interp::new(t), t))
                    .id()
            })
            .collect();
        for _ in 0..(fps * 1.5) as usize {
            app.update();
        }
        ids.iter()
            .map(|e| std::mem::take(&mut app.world_mut().get_mut::<Probe>(*e).unwrap().trail))
            .collect()
    }

    #[test]
    fn particles_move_the_same_at_any_frame_rate() {
        let runs: Vec<_> = [30.0, 60.0, 144.0].iter().map(|f| probe_trails(*f)).collect();
        for which in 0..2 {
            let n = runs.iter().map(|r| r[which].len()).min().unwrap();
            assert!(n > 150, "{n} ticks");
            for r in &runs[1..] {
                for k in 0..n {
                    let (a, fa) = &r[which][k];
                    let (b, fb) = &runs[0][which][k];
                    assert!(a.distance(*b) < 1e-5 && fa == fb, "particle {which}, tick {k}");
                }
            }
        }
        // the rocket bursts on the same tick at every frame rate
        let burst_tick = |r: &Vec<Vec<(Vec3, Fate)>>| {
            r[0].iter().position(|(_, f)| matches!(f, Fate::Bursts(..)))
        };
        let t30 = burst_tick(&runs[0]);
        assert!(t30.is_some());
        assert!(runs.iter().all(|r| burst_tick(r) == t30));
    }

    #[test]
    fn a_waiting_rocket_stays_hidden() {
        let mut q = Particle::new(Vec3::Y, 3.0, 0.05);
        q.age = -0.5;
        let mut t = Transform::default();
        assert_eq!(advance(&mut q, &mut t, 1.0 / 120.0, true), Fate::Lives);
        assert_eq!(t.scale, Vec3::ZERO);
        assert_eq!(advance(&mut q, &mut t, 1.0 / 120.0, false), Fate::Lives, "not yet live");
        q.age = 0.1;
        assert_eq!(advance(&mut q, &mut t, 1.0 / 120.0, false), Fate::Gone, "effects off");
    }

    #[test]
    fn commands_and_set_weather() {
        let mut p = Particles::new(true, None);
        assert_eq!(p.weather_setting(), "auto");
        assert_eq!(Weather::for_set(Some("night")), Some(Weather::Rain));
        assert_eq!(Weather::for_set(Some("hospital")), None);
        p.command("snow").unwrap();
        assert_eq!(p.weather_setting(), "snow");
        p.command("off").unwrap();
        assert_eq!(p.weather_setting(), "off");
        p.command("fireworks").unwrap();
        assert_eq!(p.queue, vec![Burst::Fireworks]);
        assert!(p.command("lasers").is_none());
        assert_eq!(Particles::new(true, Some("rain")).weather_setting(), "rain");
        assert_eq!(
            budget(crate::lighting::Level::Low) < budget(crate::lighting::Level::High),
            true
        );
    }

    #[test]
    fn chat_words() {
        assert_eq!(chat_burst("love this <3"), Some(Burst::Hearts));
        assert_eq!(
            chat_burst("\u{2764}\u{fe0f}\u{2764}\u{fe0f}"),
            Some(Burst::Hearts)
        );
        assert_eq!(chat_burst("that was \u{1f525}"), Some(Burst::Sparks));
        assert_eq!(chat_burst("GG everyone"), Some(Burst::Pop));
        assert_eq!(
            chat_burst("eggs and ham"),
            None,
            "gg inside a word doesn't count"
        );
        assert_eq!(chat_burst("hello"), None);
    }

    #[test]
    fn voxel_shapes_are_closed_boxes() {
        let m = voxel_shape(&HEART, 1.0 / 7.0);
        let n = m.count_vertices() / 4;
        let cells = HEART.iter().map(|r| r.matches('#').count()).sum::<usize>();
        // front and back of every cell, plus the outline's sides
        assert!(
            n > cells * 2 && n < cells * 6,
            "{n} faces for {cells} cells"
        );
        let one = voxel_shape(&["#"], 1.0);
        assert_eq!(one.count_vertices(), 24, "a single cell is a cube");
    }
}
