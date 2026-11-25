//! A Vox world in the app: `--world ws://host:7780 [--name Sam]`.
//!
//! The world's blocks arrive in chunks and are drawn chunk by chunk. You walk (or fly) your
//! character around in third person and build: left click takes a block away, right click
//! puts the chosen one down, 1–9 or the wheel choose. Edits show at once and are undone if
//! the server refuses them (vox-core's [`View`]). Everyone else in the world is drawn
//! from their poses, played 100 ms behind like a show's guest.
//!
//! Keyboard and mouse: WASD walk, Space jump (up when flying), Ctrl down, Shift run, F fly,
//! mouse look (click the window to capture the mouse, Esc to let go). A controller: left
//! stick walk, right stick look, A jump, B down, Y fly, RT build, LT take away, bumpers choose.
//!
//! For automation the control page takes `world:place:x:y:z:block`, `world:remove:x:y:z`,
//! `world:goto:x:y:z`, `world:look:yaw:pitch` (degrees), `world:fly:on|off`, `world:slot:n`,
//! `world:click:left|right` (at the crosshair), `world:walk:x:y:seconds`, `world:jump`,
//! `world:time:<hours>|auto` and `world:clouds:<0-1>`.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use bevy::platform::time::Instant;
use bevy::{
    input::{
        gamepad::{Gamepad, GamepadButton},
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    },
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use serde::{Deserialize, Serialize};
use vox_core::{
    Chunk, ChunkKey, Why,
    body::Body,
    predict::View,
    ray::{self, Hit},
    weather::{Air, Climate, Weather},
    wire::{self, PROTOCOL, ToClient, ToServer},
};

use crate::pose::{self as net, Remote, Rig, Stream};
use crate::proto::{self, Pose};
use crate::pose::HZ;

/// How far you can build, in blocks from the eyes (the server allows a little more).
const REACH: f32 = 15.0;
/// Eye height in blocks (the server's reach check uses the same).
const EYES: f32 = 3.2;
/// The player's box, in blocks.
const HALF_WIDTH: f32 = 0.6;
const HEIGHT: f32 = 3.6;
/// Speeds in metres per second; gravity in m/s².
const WALK: f32 = 3.2;
const RUN: f32 = 6.0;
const FLY: f32 = 7.0;
const GRAVITY: f32 = 22.0;
/// A jump clears a block and a bit.
const JUMP_HEIGHT: f32 = 0.7;
/// Camera distance behind the head, metres.
const CAM_DIST: f32 = 3.4;
const POSE_EVERY: f64 = 1.0 / proto::POSE_HZ as f64;
/// The blocks on the bar when a world doesn't say otherwise.
const HOTBAR: [&str; 9] = ["grass", "dirt", "stone", "cobblestone", "bricks", "planks", "glass", "water", "lamp"];

enum Incoming {
    Status(String),
    Data(Vec<u8>),
}

struct Link {
    out: mpsc::Sender<Vec<u8>>,
    inbox: Mutex<mpsc::Receiver<Incoming>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What a player's pose message carries: where they stand in the world, which way they
/// face, and the pose itself.
#[derive(Serialize, Deserialize)]
struct Placed {
    at: [f32; 3],
    yaw: f32,
    pose: Pose,
}

/// Someone else in the world.
struct Other {
    name: String,
    entity: Option<Entity>,
    stream: Stream,
    /// (tick, position, facing) from each pose
    places: VecDeque<(u64, Vec3, f32)>,
}

/// How the camera follows you.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamMode {
    /// From high up at an angle (like Diablo), zooming in and out; the mouse points at things.
    Diablo,
    /// Close behind your shoulder, the mouse looks round.
    Behind,
}

/// A fixed camera for a place in the world: while you're in its zone the camera moves there
/// and either looks at a point or follows you with its eyes.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub name: String,
    /// the zone, in blocks (inclusive)
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// where the camera stands and what it looks at, m (no look: it watches you)
    pub at: Vec3,
    pub look: Option<Vec3>,
}

impl Shot {
    fn from_json(v: &serde_json::Value) -> Option<Shot> {
        let v3 = |k: &str| -> Option<[f32; 3]> {
            let a = v.get(k)?.as_array()?;
            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
        };
        let zone = v.get("zone")?;
        let z3 = |k: &str| -> Option<[f32; 3]> {
            let a = zone.get(k)?.as_array()?;
            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
        };
        let (a, b) = (z3("from")?, z3("to")?);
        Some(Shot {
            name: v.get("name").and_then(|n| n.as_str()).unwrap_or("shot").to_string(),
            from: std::array::from_fn(|i| a[i].min(b[i])),
            to: std::array::from_fn(|i| a[i].max(b[i]) + 1.0),
            at: Vec3::from(v3("at")?),
            look: v3("look").map(Vec3::from),
        })
    }

    fn contains(&self, p: [f32; 3]) -> bool {
        (0..3).all(|i| p[i] >= self.from[i] && p[i] < self.to[i])
    }
}

/// What lights up near you.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    Lamp,
    Fire,
}

/// A chunk's meshes.
#[derive(Component)]
pub struct ChunkMesh;

/// What you're standing as: your character hangs below this.
#[derive(Component)]
pub struct Walker;

#[derive(Component)]
pub struct WorldHud;

#[derive(Component)]
pub struct Crosshair;

#[derive(Resource)]
pub struct WorldClient {
    /// the server, if this app is in a world
    pub url: Option<String>,
    pub name: String,
    /// a sign-in token from the directory (`--token`, or `VOX_TOKEN`), for worlds that ask
    pub token: Option<String>,
    link: Option<Link>,
    pub status: String,
    pub you: Option<u32>,
    pub world: String,
    pub view: View,
    /// the server's block names, and its ids mapped to this app's palette
    names: Vec<String>,
    remap: Vec<u16>,
    block_size: f32,
    pub ready: bool,
    dirty: HashSet<ChunkKey>,
    meshes: HashMap<ChunkKey, Entity>,
    /// `world:` commands from the control page
    pub cmds: Vec<String>,
    hotbar: Vec<u16>,
    pub slot: usize,
    walker: Option<Entity>,
    hud: Option<Entity>,
    /// lamps and fires (confirmed), for lighting the nearest
    pub lights: HashMap<[i32; 3], LightKind>,
    lamp_id: u16,
    fire_id: u16,
    /// by server id: water, fire and the like, which you walk through
    liquid: Vec<bool>,
    /// the world's settings (its sky, its time), until the sky takes them
    settings: Option<serde_json::Value>,
    /// `world:time:` and `world:clouds:` for the sky
    pub sky_cmds: Vec<(String, String)>,
    /// the time of day (kept here for the state page)
    pub hours: f32,
    /// the air: from the server's keyframes, simulated here in between
    pub weather: Option<Weather>,
    weather_acc: f32,
    /// an endless floor: everything below this block height is solid (the void)
    pub floor: Option<i32>,
    // ---- the camera
    pub cam_mode: CamMode,
    /// how far the Diablo camera stands back, m (wanted and now)
    pub zoom: f32,
    zoom_now: f32,
    /// what the camera follows (smoothed), m
    focus: Option<Vec3>,
    pub shots: Vec<Shot>,
    /// the fixed shot in use (by `world:camera:fixed:<name>` or by being in its zone)
    pub shot: Option<usize>,
    forced_shot: Option<usize>,
    /// 0 = the follow camera, 1 = the fixed shot
    shot_blend: f32,
    /// the last shot used (blending out of it)
    last_shot: Option<usize>,
    /// where the camera is and which way it looks (last frame), m
    cam_at: Vec3,
    cam_dir: Vec3,
    /// which way "forward" is for walking, held while you keep walking through a camera change
    move_yaw: Option<f32>,
    /// walking to a point (clicked), in blocks
    goal: Option<Vec3>,
    /// the ray under the mouse (blocks), from the last camera
    pick: Option<(Vec3, Vec3)>,
    pub body: Body,
    spawn: [f32; 3],
    vy: f32,
    /// which way the character faces, and the camera's turn and tilt (radians)
    yaw: f32,
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    pub fly: bool,
    pub grounded: bool,
    captured: bool,
    pub target: Option<Hit>,
    moving: bool,
    others: HashMap<u32, Other>,
    rig: Option<(Entity, Rig)>,
    started: Instant,
    last_pose: f64,
    pub edits: u64,
    pub refused: u64,
    notice: Option<(String, f64)>,
    /// a scripted walk (automation): stick direction until a time, and a jump to make
    auto: Option<(Vec2, f64)>,
    auto_jump: bool,
    // ---- portals
    /// this world's portals (from its settings), and a number that changes with each world
    pub portals: Vec<crate::portals::PortalDef>,
    pub portals_gen: u32,
    /// where to come out in the next world (going through a portal)
    arrival: Option<Arrival>,
    /// on the way to another world: the one you came from, when you left and where you stood
    /// (to go back to if the way turns out to be closed)
    travel: Option<(String, f64, [f32; 3])>,
    /// `world:portal:<name>`: go through that portal now
    pub portal_go: Option<String>,
}

/// Coming out of a portal: where you stand (blocks) and which way you and the camera face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrival {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub cam_yaw: f32,
    pub zoom: f32,
    pub move_yaw: Option<f32>,
}

impl Default for WorldClient {
    fn default() -> Self {
        WorldClient {
            url: None,
            name: String::new(),
            token: None,
            link: None,
            status: String::new(),
            you: None,
            world: String::new(),
            view: View::default(),
            names: Vec::new(),
            remap: Vec::new(),
            block_size: 0.5,
            ready: false,
            dirty: HashSet::new(),
            meshes: HashMap::new(),
            cmds: Vec::new(),
            hotbar: Vec::new(),
            slot: 0,
            walker: None,
            hud: None,
            lights: HashMap::new(),
            lamp_id: 0,
            fire_id: 0,
            settings: None,
            liquid: Vec::new(),
            sky_cmds: Vec::new(),
            hours: 0.0,
            weather: None,
            weather_acc: 0.0,
            floor: None,
            cam_mode: CamMode::Diablo,
            zoom: 13.0,
            zoom_now: 13.0,
            focus: None,
            shots: Vec::new(),
            shot: None,
            forced_shot: None,
            shot_blend: 0.0,
            last_shot: None,
            cam_at: Vec3::ZERO,
            cam_dir: Vec3::NEG_Z,
            move_yaw: None,
            goal: None,
            pick: None,
            body: Body { pos: [0.5, 0.0, 0.5], half: HALF_WIDTH, height: HEIGHT },
            spawn: [0.5, 0.0, 0.5],
            vy: 0.0,
            yaw: 0.0,
            cam_yaw: 0.0,
            cam_pitch: -0.25,
            fly: false,
            grounded: false,
            captured: false,
            target: None,
            moving: false,
            others: HashMap::new(),
            rig: None,
            started: Instant::now(),
            last_pose: 0.0,
            edits: 0,
            refused: 0,
            notice: None,
            auto: None,
            auto_jump: false,
            portals: Vec::new(),
            portals_gen: 0,
            arrival: None,
            travel: None,
            portal_go: None,
        }
    }
}

/// `ws://host:port` as given, or with `/ws` added; `http(s)://` becomes `ws(s)://`.
/// A portal's address: `:7781` is the world on port 7781 of the same host as `current`, or,
/// behind a proxy that puts worlds at `/w/<port>/ws` (as vox.pub does), the same host's
/// `/w/7781/ws`. `/some/path` is a path on the same host. Anything else is a full address.
pub fn portal_url(current: Option<&str>, target: &str) -> String {
    let t = target.trim();
    let cur = current.unwrap_or("ws://127.0.0.1:7780/ws");
    let (scheme, rest) = cur.split_once("://").unwrap_or(("ws", cur));
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
    if t.starts_with('/') {
        return format!("{scheme}://{hostport}{t}");
    }
    let Some(port) = t.strip_prefix(':') else { return t.to_string() };
    // worlds behind a proxy, by port in the path
    if let Some(p) = path.strip_prefix("w/") {
        if p.split('/').next().is_some_and(|n| n.parse::<u16>().is_ok()) {
            return format!("{scheme}://{hostport}/w/{port}/ws");
        }
    }
    // the host without its port (an IPv6 address keeps its brackets)
    let host = match hostport.rfind(':') {
        Some(i) if !hostport[i..].contains(']') => &hostport[..i],
        _ => hostport,
    };
    format!("{scheme}://{host}:{port}/ws")
}

pub fn ws_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = if let Some(r) = u.strip_prefix("http://") {
        format!("ws://{r}")
    } else if let Some(r) = u.strip_prefix("https://") {
        format!("wss://{r}")
    } else if u.starts_with("ws://") || u.starts_with("wss://") {
        u.to_string()
    } else {
        format!("ws://{u}")
    };
    if u.ends_with("/ws") { u } else { format!("{u}/ws") }
}

impl WorldClient {
    pub fn new(url: Option<String>, name: String) -> Self {
        let token = std::env::args()
            .skip_while(|a| a != "--token")
            .nth(1)
            .or_else(|| std::env::var("VOX_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());
        WorldClient { url: url.map(|u| ws_url(&u)), name, token, ..default() }
    }

    /// With a sign-in token (for worlds that ask for one).
    pub fn with_token(mut self, token: Option<String>) -> Self {
        if token.is_some() {
            self.token = token;
        }
        self
    }

    pub fn active(&self) -> bool {
        self.url.is_some()
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn send(&self, m: &ToServer) {
        if let Some(l) = &self.link {
            let _ = l.out.send(wire::encode(m));
        }
    }

    pub fn notice(&mut self, s: impl Into<String>) {
        let at = self.now();
        self.notice = Some((s.into(), at));
    }

    /// This app's palette id for a server block id.
    fn local(&self, id: u16) -> u16 {
        if id == 0 { 0 } else { self.remap.get(id as usize).copied().unwrap_or(1) }
    }

    /// The world's settings, once, when they've just arrived.
    pub fn take_settings(&mut self) -> Option<serde_json::Value> {
        self.settings.take()
    }

    fn light_kind(&self, id: u16) -> Option<LightKind> {
        if id == 0 {
            None
        } else if id == self.lamp_id {
            Some(LightKind::Lamp)
        } else if id == self.fire_id {
            Some(LightKind::Fire)
        } else {
            None
        }
    }

    pub fn block_size(&self) -> f32 {
        self.block_size
    }

    /// Where you'd come out after turning by `turn` (radians) and moving to `pos` (blocks):
    /// everything about the view turned with you, so walking on carries on the same way.
    pub fn arrival(&self, pos: [f32; 3], turn: f32) -> Arrival {
        Arrival {
            pos,
            yaw: self.yaw + turn,
            cam_yaw: self.cam_yaw + turn,
            zoom: self.zoom,
            move_yaw: self.move_yaw.map(|y| y + turn),
        }
    }

    /// Step out of a portal in this world.
    pub fn arrive(&mut self, a: Arrival) {
        self.body.pos = a.pos;
        self.yaw = a.yaw;
        self.cam_yaw = a.cam_yaw;
        self.zoom = a.zoom;
        self.zoom_now = a.zoom;
        self.move_yaw = a.move_yaw;
        // the place you were walking to is in the world you left
        if self.goal.is_some() {
            self.goal = None;
        }
        self.focus = None;
    }

    /// Go through a portal to another world (another server): come out at `a`.
    pub fn travel(&mut self, url: &str, a: Arrival) {
        let url = ws_url(&portal_url(self.url.as_deref(), url));
        if let Some(back) = self.url.clone() {
            if self.travel.is_none() {
                self.travel = Some((back, self.now(), self.body.pos));
            }
        }
        crate::note!("[world] through a portal to {url}");
        self.url = Some(url);
        self.link = None;
        self.status.clear();
        // nothing goes out until the new world has said hello (a pose first would be refused)
        self.ready = false;
        self.arrival = Some(a);
    }

    /// The way through a portal was closed (no world answered): go back where you were.
    pub fn travel_check(&mut self) {
        let Some((back, at, pos)) = self.travel.clone() else { return };
        // nobody there (the link says so at once), or no welcome for a long while
        let refused = self.status.starts_with("Can't reach");
        if !refused && self.now() - at < 20.0 {
            return;
        }
        self.travel = None;
        let a = self.arrival(pos, 0.0);
        crate::note!("[world] the way is closed ({}): back to {back}", self.status);
        self.url = Some(back);
        self.link = None;
        self.status.clear();
        self.ready = false;
        self.arrival = Some(a);
        self.notice("The way is closed");
    }

    pub fn travelling(&self) -> bool {
        self.travel.is_some()
    }

    /// Solid to walk into (water and fire aren't; the endless floor is).
    fn stands_on(&self, c: [i32; 3]) -> bool {
        if self.floor.is_some_and(|f| c[1] < f) {
            return true;
        }
        let b = self.view.get(c);
        b != 0 && !self.liquid.get(b as usize).copied().unwrap_or(false)
    }

    /// Something the mouse or crosshair stops at (any block, or the endless floor).
    fn solid_at(&self, c: [i32; 3]) -> bool {
        self.floor.is_some_and(|f| c[1] < f) || self.view.solid(c)
    }

    /// The air where you are.
    pub fn air(&self) -> Option<Air> {
        let p = Vec3::from(self.body.pos) * self.block_size + Vec3::Y * 1.6;
        self.weather.as_ref().map(|w| w.air(p.to_array()))
    }

    /// Ask the server to change the weather.
    fn set_climate(&mut self, c: Climate) {
        self.send(&ToServer::Climate(c));
        if let Some(w) = &mut self.weather {
            w.climate = Climate { ground: w.climate.ground, ..c.sane() };
        }
    }

    fn climate(&self) -> Climate {
        self.weather.as_ref().map_or_else(Climate::default, |w| w.climate)
    }

    pub fn block_name(&self, id: u16) -> &str {
        if id == 0 { "air" } else { self.names.get(id as usize - 1).map_or("?", |s| s.as_str()) }
    }

    fn block_id(&self, name: &str) -> Option<u16> {
        if name == "air" {
            return Some(0);
        }
        self.names.iter().position(|n| n == name).map(|i| i as u16 + 1)
    }

    pub fn selected(&self) -> u16 {
        self.hotbar.get(self.slot).copied().unwrap_or(1)
    }

    pub fn players(&self) -> Vec<String> {
        let mut v: Vec<String> = self.others.values().map(|o| o.name.clone()).collect();
        v.sort();
        v
    }

    fn mark(&mut self, c: [i32; 3]) {
        self.dirty.extend(ChunkKey::touched(c));
    }

    /// Make an edit: shown now, sent to the server.
    fn edit(&mut self, cell: [i32; 3], block: u16) {
        if block != 0 && self.body.blocked(&|c| c == cell, self.body.pos) {
            return self.notice("You're standing there");
        }
        let eye = [self.body.pos[0], self.body.pos[1] + EYES, self.body.pos[2]];
        let d2: f32 = (0..3).map(|i| (cell[i] as f32 + 0.5 - eye[i]).powi(2)).sum();
        if d2 > (REACH + 0.5).powi(2) {
            return self.notice(Why::TooFar.to_string());
        }
        match self.view.edit(cell, block) {
            Ok(id) => {
                self.send(&ToServer::Edit { id, cell, block });
                self.mark(cell);
                self.edits += 1;
            }
            Err(why) => self.notice(why.to_string()),
        }
    }

    fn connect(&mut self) {
        let Some(url) = self.url.clone() else { return };
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
        let (in_tx, in_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let _ = std::thread::Builder::new()
            .name("world link".into())
            .spawn(move || run_link(url, out_rx, in_tx, stop2));
        #[cfg(target_arch = "wasm32")]
        web_link::open(url, out_rx, in_tx, stop2);
        self.link = Some(Link { out: out_tx, inbox: Mutex::new(in_rx), stop });
        self.status = "Connecting".into();
    }

    /// A Welcome: start over with this world.
    fn welcome(
        &mut self,
        you: u32,
        world: String,
        palette: Vec<String>,
        block_size: f32,
        bounds: vox_core::Bounds,
        spawn: [f32; 3],
        local: &crate::blocks::Palette,
    ) {
        self.you = Some(you);
        self.world = world;
        self.view = View::new(vox_core::World::new(bounds, palette.len() as u16));
        let fallback = local.id("stone").unwrap_or(1);
        self.remap = std::iter::once(0).chain(palette.iter().map(|n| local.id(n).unwrap_or(fallback))).collect();
        self.names = palette;
        if (block_size - local.size).abs() > 1e-4 {
            crate::note!("[world] the world's blocks are {block_size} m, this app's {} m", local.size);
        }
        self.block_size = local.size;
        self.hotbar = HOTBAR.iter().filter_map(|n| self.block_id(n)).collect();
        for i in 1..=self.names.len() as u16 {
            if self.hotbar.len() >= 9 {
                break;
            }
            if !self.hotbar.contains(&i) {
                self.hotbar.push(i);
            }
        }
        self.slot = self.slot.min(self.hotbar.len().saturating_sub(1));
        self.spawn = spawn;
        self.body.pos = spawn;
        // start facing away from the camera, into the world
        self.yaw = std::f32::consts::PI;
        self.cam_yaw = 0.0;
        self.vy = 0.0;
        self.ready = false;
        self.dirty.clear();
        self.lamp_id = self.block_id("lamp").unwrap_or(0);
        self.fire_id = self.block_id("fire").unwrap_or(0);
        self.lights.clear();
        self.liquid = std::iter::once(false)
            .chain(self.names.iter().map(|n| local.id(n).and_then(|i| local.def(i)).is_some_and(|d| d.liquid || d.flame)))
            .collect();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_link(url: String, out: mpsc::Receiver<Vec<u8>>, inbox: mpsc::Sender<Incoming>, stop: Arc<AtomicBool>) {
    let mut wait = Duration::from_millis(500);
    while !stop.load(Ordering::Relaxed) {
        let mut sock = match tungstenite::connect(&url) {
            Ok((s, _)) => s,
            Err(e) => {
                let _ = inbox.send(Incoming::Status(format!("Can't reach the world: {e}")));
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_secs(10));
                continue;
            }
        };
        wait = Duration::from_millis(500);
        let short = Some(Duration::from_millis(5));
        match sock.get_mut() {
            tungstenite::stream::MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(short);
                let _ = s.set_nodelay(true);
            }
            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                let _ = s.get_mut().set_read_timeout(short);
                let _ = s.get_mut().set_nodelay(true);
            }
            _ => {}
        }
        // anything queued before connecting (old edits, poses) is stale: the hello goes first
        while out.try_recv().is_ok() {}
        let _ = inbox.send(Incoming::Status("Connected".into()));
        let mut bye = false;
        'live: while !stop.load(Ordering::Relaxed) {
            while let Ok(bytes) = out.try_recv() {
                if sock.send(tungstenite::Message::binary(bytes)).is_err() {
                    break 'live;
                }
            }
            loop {
                match sock.read() {
                    Ok(tungstenite::Message::Binary(b)) => {
                        bye |= matches!(wire::decode::<ToClient>(&b), Ok(ToClient::Bye { .. }));
                        let _ = inbox.send(Incoming::Data(b.to_vec()));
                    }
                    Ok(tungstenite::Message::Close(_)) => break 'live,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
                    {
                        break;
                    }
                    Err(_) => break 'live,
                }
            }
        }
        let _ = sock.close(None);
        if bye {
            return; // the server said why; trying again won't help
        }
        if !stop.load(Ordering::Relaxed) {
            let _ = inbox.send(Incoming::Status("Reconnecting".into()));
            std::thread::sleep(wait);
        }
    }
}

/// Connect, and take in what the server sends.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut w: ResMut<WorldClient>,
    av: Res<crate::LocalAvatar>,
    blocks: Res<crate::blocks::Blocks>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    if !w.active() {
        return;
    }
    let Some(local) = blocks.palette.as_ref() else { return };
    if w.link.is_none() && w.status.is_empty() {
        w.connect();
    }
    #[cfg(target_arch = "wasm32")]
    web_link::pump();
    let msgs: Vec<Incoming> = match &w.link {
        Some(l) => l.inbox.lock().unwrap().try_iter().collect(),
        None => return,
    };
    let now = w.now();
    for m in msgs {
        match m {
            Incoming::Status(s) => {
                if s == "Connected" {
                    // say hello with what you look like
                    let hello = proto::Hello {
                        version: proto::VERSION,
                        name: w.name.clone(),
                        model: av.model.clone(),
                        nodes: w.rig.as_ref().map_or(0, |r| r.1.hash),
                    };
                    let avatar = postcard::to_stdvec(&hello).unwrap_or_default();
                    let name = w.name.clone();
                    let token = w.token.clone();
                    w.send(&ToServer::Hello { protocol: PROTOCOL, name, avatar, token });
                } else {
                    w.ready = false;
                }
                w.status = s;
            }
            Incoming::Data(b) => {
                let Ok(msg) = wire::decode::<ToClient>(&b) else { continue };
                match msg {
                    ToClient::Welcome { you, world, palette, block_size, bounds, spawn, settings, name } => {
                        w.name = name;
                        for (_, e) in w.meshes.drain() {
                            commands.entity(e).try_despawn();
                        }
                        let others: Vec<_> = w.others.drain().filter_map(|(_, o)| o.entity).collect();
                        for e in others {
                            commands.entity(e).try_despawn();
                        }
                        w.welcome(you, world, palette, block_size, bounds, spawn, local);
                        w.settings = Some(serde_json::from_str(&settings).unwrap_or(serde_json::json!({})));
                        // the endless floor, fixed cameras and the camera's starting angle
                        let set = w.settings.clone().unwrap_or_default();
                        w.floor = set.get("floor").and_then(|v| v.as_i64()).map(|f| f as i32);
                        w.shots = set
                            .get("cameras")
                            .and_then(|v| v.as_array())
                            .map(|l| l.iter().filter_map(Shot::from_json).collect())
                            .unwrap_or_default();
                        // the last world's shots are gone
                        w.shot = None;
                        w.last_shot = None;
                        w.forced_shot = None;
                        w.shot_blend = 0.0;
                        if let Some(c) = set.get("camera") {
                            if let Some(y) = c.get("yaw").and_then(|v| v.as_f64()) {
                                w.cam_yaw = (y as f32).to_radians();
                            }
                            if let Some(z) = c.get("zoom").and_then(|v| v.as_f64()) {
                                w.zoom = (z as f32).clamp(4.0, 45.0);
                                w.zoom_now = w.zoom;
                            }
                        }
                        w.focus = None;
                        // its portals; and if you came through one, where you come out
                        w.portals = crate::portals::parse(&set);
                        w.portals_gen = w.portals_gen.wrapping_add(1);
                        w.travel = None;
                        if let Some(a) = w.arrival.take() {
                            w.arrive(a);
                        }
                        // a world may choose its own block bar
                        let names: Vec<String> = w
                            .settings
                            .as_ref()
                            .and_then(|s| s.get("hotbar"))
                            .and_then(|v| v.as_array())
                            .map(|l| l.iter().filter_map(|n| n.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let ids: Vec<u16> = names.iter().filter_map(|n| w.block_id(n)).take(9).collect();
                        if !ids.is_empty() {
                            w.hotbar = ids;
                            w.slot = 0;
                        }
                        w.status = format!("Loading {}", w.world);
                    }
                    ToClient::Chunk { key, version, data } => match Chunk::decode(&data, version) {
                        Ok(c) => {
                            // note its lamps and fires
                            let o = key.origin();
                            w.lights.retain(|l, _| ChunkKey::of(*l) != key);
                            if !c.is_empty() {
                                let n = vox_core::CHUNK;
                                for y in 0..n {
                                    for z in 0..n {
                                        for x in 0..n {
                                            let cell = [o[0] + x, o[1] + y, o[2] + z];
                                            if let Some(k) = w.light_kind(c.get(cell)) {
                                                w.lights.insert(cell, k);
                                            }
                                        }
                                    }
                                }
                            }
                            w.view.chunk(key, c);
                            w.dirty.insert(key);
                            for d in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
                                w.dirty.insert(ChunkKey([key.0[0] + d[0], key.0[1] + d[1], key.0[2] + d[2]]));
                            }
                        }
                        Err(e) => crate::note!("[world] bad chunk {key:?}: {e}"),
                    },
                    ToClient::Ready => {
                        w.ready = true;
                        w.status = format!("In {}", w.world);
                        crate::note!("[world] in {}: {} blocks", w.world, w.view.confirmed.count());
                    }
                    ToClient::Edits(list) => {
                        let me = w.you.unwrap_or(u32::MAX);
                        for a in list {
                            match w.light_kind(a.block) {
                                Some(k) => {
                                    w.lights.insert(a.cell, k);
                                }
                                None => {
                                    w.lights.remove(&a.cell);
                                }
                            }
                            if let Some(c) = w.view.applied(&a, me) {
                                w.mark(c);
                            }
                        }
                    }
                    ToClient::Refused { id, why } => {
                        if let Some(c) = w.view.refused(id) {
                            w.mark(c);
                        }
                        w.refused += 1;
                        w.notice(why.to_string());
                    }
                    ToClient::Joined { player, name, avatar } => {
                        let model = postcard::from_bytes::<proto::Hello>(&avatar)
                            .ok()
                            .map(|h| h.model)
                            .filter(|m| m.ends_with(".glb") && !m.contains("..") && !m.starts_with('/'))
                            .unwrap_or_else(|| av.fallback.clone());
                        let e = commands
                            .spawn((
                                Remote::world_player(model.clone(), assets.load::<Gltf>(model)),
                                Transform::from_translation(Vec3::new(0.0, -1000.0, 0.0)),
                                Visibility::default(),
                                Name::new(format!("player {name}")),
                            ))
                            .id();
                        w.notice(format!("{name} is here"));
                        let old = w.others.insert(
                            player,
                            Other { name, entity: Some(e), stream: Stream::default(), places: VecDeque::new() },
                        );
                        if let Some(e) = old.and_then(|o| o.entity) {
                            commands.entity(e).try_despawn();
                        }
                    }
                    ToClient::Left { player } => {
                        if let Some(o) = w.others.remove(&player) {
                            if let Some(e) = o.entity {
                                commands.entity(e).try_despawn();
                            }
                            w.notice(format!("{} left", o.name));
                        }
                    }
                    ToClient::Pose { player, blob } => {
                        let Ok(p) = postcard::from_bytes::<Placed>(&blob) else { continue };
                        if let Some(o) = w.others.get_mut(&player) {
                            let tick = p.pose.tick;
                            o.stream.push(p.pose, now);
                            let i = o.places.partition_point(|q| q.0 < tick);
                            o.places.insert(i, (tick, Vec3::from(p.at), p.yaw));
                            while o.places.len() > 120 {
                                o.places.pop_front();
                            }
                        }
                    }
                    ToClient::Weather(k) => match Weather::from_keyframe(&k) {
                        Ok(wx) => {
                            w.weather = Some(wx);
                            w.weather_acc = 0.0;
                        }
                        Err(e) => crate::note!("[world] bad weather: {e}"),
                    },
                    ToClient::Bye { why } => {
                        w.status = why;
                        w.ready = false;
                    }
                }
            }
        }
    }
}

/// Rebuild the meshes of chunks that changed.
pub fn remesh(
    mut w: ResMut<WorldClient>,
    blocks: Res<crate::blocks::Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    if !w.ready || w.dirty.is_empty() {
        return;
    }
    let Some(pal) = blocks.palette.as_ref() else { return };
    let keys: Vec<ChunkKey> = w.dirty.drain().collect();
    let started = Instant::now();
    let count = keys.len();
    for k in keys {
        if let Some(e) = w.meshes.remove(&k) {
            commands.entity(e).try_despawn();
        }
        // (a chunk the server hasn't got may still hold your own pending blocks)
        if w.view.confirmed.chunk(k).is_none() && w.view.pending() == 0 {
            continue;
        }
        if let Some(e) = spawn_chunk(&w, k, pal, &blocks, &mut meshes, &mut commands) {
            w.meshes.insert(k, e);
        }
    }
    if count > 16 {
        crate::note!("[world] meshed {count} chunks in {} ms", started.elapsed().as_millis());
    }
}

fn spawn_chunk(
    w: &WorldClient,
    k: ChunkKey,
    pal: &crate::blocks::Palette,
    blocks: &crate::blocks::Blocks,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) -> Option<Entity> {
    let o = IVec3::from(k.origin());
    let n = vox_core::CHUNK;
    // the chunk and a block all round it, copied out once: the mesher looks at each block's
    // neighbours many times over
    let side = (n + 2) as usize;
    let at = |l: IVec3| (l.y as usize * side + l.z as usize) * side + l.x as usize;
    let mut grid = vec![0u16; side * side * side];
    let mut cells = Vec::new();
    for y in -1..=n {
        for z in -1..=n {
            for x in -1..=n {
                let c = o + IVec3::new(x, y, z);
                let b = w.local(w.view.get(c.to_array()));
                grid[at(IVec3::new(x, y, z) + 1)] = b;
                if b != 0 && (0..n).contains(&x) && (0..n).contains(&y) && (0..n).contains(&z) {
                    cells.push((c, b));
                }
            }
        }
    }
    if cells.is_empty() {
        return None;
    }
    let get = |p: IVec3| {
        let l = p - o + 1;
        if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(n + 2)).all() {
            grid[at(l)]
        } else {
            w.local(w.view.get(p.to_array()))
        }
    };
    let crate::blocks::Meshes { opaque, glass, water, flame } = crate::blocks::mesh_cells(cells, get, pal, &blocks.rects);
    let e = commands
        .spawn((
            ChunkMesh,
            Transform::default(),
            Visibility::default(),
            Name::new(format!("chunk {:?}", k.0)),
        ))
        .id();
    commands.entity(e).with_children(|c| {
        if opaque.faces() > 0 {
            c.spawn((Mesh3d(meshes.add(opaque.into_mesh())), MeshMaterial3d(blocks.world_solid.clone()), crate::NoBevel));
        }
        if glass.faces() > 0 {
            c.spawn((Mesh3d(meshes.add(glass.into_mesh())), MeshMaterial3d(blocks.glass.clone()), crate::NoBevel));
        }
        if flame.faces() > 0 {
            c.spawn((Mesh3d(meshes.add(flame.into_mesh())), MeshMaterial3d(blocks.flame.clone()), crate::NoBevel));
        }
        if water.faces() > 0 {
            c.spawn((
                Mesh3d(meshes.add(water.into_mesh())),
                MeshMaterial3d(blocks.water.clone()),
                bevy::light::NotShadowCaster,
                crate::NoBevel,
            ));
        }
    });
    Some(e)
}

/// Put your character under a walker that moves through the world (again after a model
/// change), and find its nodes for sending poses.
pub fn attach(
    mut w: ResMut<WorldClient>,
    av: Res<crate::LocalAvatar>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<bevy::gltf::GltfNode>>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if !w.active() {
        return;
    }
    let walker = match w.walker {
        Some(e) => e,
        None => {
            let e = commands.spawn((Walker, Transform::default(), Visibility::default(), Name::new("walker"))).id();
            w.walker = Some(e);
            e
        }
    };
    let Some(root) = av.root else { return };
    if parents.get(root).map(|p| p.parent()) != Ok(walker) {
        commands.entity(root).insert(ChildOf(walker));
    }
    if w.rig.as_ref().is_some_and(|(r, g)| *r == root && g.complete()) {
        return;
    }
    let (true, Some(gh)) = (av.ready, av.gltf.as_ref()) else { return };
    let Some(gltf) = gltfs.get(gh) else { return };
    if let Some(rig) = net::rig(gltf, &nodes, root, &children, &names) {
        w.rig = Some((root, rig));
    }
}

fn run_command(w: &mut WorldClient, cmd: &str) {
    let parts: Vec<&str> = cmd.split(':').collect();
    let num = |i: usize| parts.get(i).and_then(|v| v.trim().parse::<f32>().ok());
    let cell = |i: usize| -> Option<[i32; 3]> {
        Some([num(i)? .floor() as i32, num(i + 1)?.floor() as i32, num(i + 2)?.floor() as i32])
    };
    match parts.first().copied().unwrap_or("") {
        "place" => match (cell(1), parts.get(4).and_then(|n| w.block_id(n))) {
            (Some(c), Some(b)) => w.edit(c, b),
            _ => w.notice("world:place:x:y:z:block"),
        },
        "remove" => match cell(1) {
            Some(c) => w.edit(c, 0),
            None => w.notice("world:remove:x:y:z"),
        },
        "goto" => {
            if let (Some(x), Some(y), Some(z)) = (num(1), num(2), num(3)) {
                w.body.pos = [x, y, z];
                w.vy = 0.0;
            }
        }
        "look" => {
            if let (Some(y), Some(p)) = (num(1), num(2)) {
                w.cam_yaw = y.to_radians();
                w.cam_pitch = p.to_radians().clamp(-1.45, 1.3);
            }
        }
        "fly" => w.fly = parts.get(1).is_none_or(|v| *v != "off"),
        "portal" => match parts.get(1) {
            Some(n) => w.portal_go = Some(n.to_string()),
            None => w.notice("world:portal:<name>"),
        },
        "camera" => match parts.get(1).copied() {
            Some("diablo") => {
                w.cam_mode = CamMode::Diablo;
                w.forced_shot = None;
            }
            Some("behind") => {
                w.cam_mode = CamMode::Behind;
                w.forced_shot = None;
            }
            Some("auto") => w.forced_shot = None,
            Some("fixed") => {
                let name = parts.get(2).copied().unwrap_or("");
                match w.shots.iter().position(|s| s.name == name) {
                    Some(i) => w.forced_shot = Some(i),
                    None => w.notice(format!("No camera called {name}")),
                }
            }
            _ => w.notice("world:camera:diablo|behind|auto|fixed:<name>"),
        },
        "zoom" => {
            if let Some(z) = num(1) {
                w.zoom = z.clamp(4.0, 45.0);
            }
        }
        // walk to a point (blocks), as a click on the ground does
        "moveto" => {
            if let (Some(x), Some(z)) = (num(1), num(2)) {
                w.goal = Some(Vec3::new(x, w.body.pos[1], z));
            }
        }
        "weather" => match parts.get(1).and_then(|n| Climate::preset(n, w.climate().ground)) {
            Some(c) => {
                w.set_climate(c);
                w.notice(format!("Weather: {}", parts[1]));
            }
            None => w.notice("world:weather:clear|haze|mist|fog|luminous|rain|storm|windy|snow"),
        },
        "wind" => {
            if let Some(speed) = num(1) {
                let mut c = w.climate();
                c.wind_speed = speed;
                if let Some(from) = num(2) {
                    c.wind_from = from;
                }
                w.set_climate(c);
            }
        }
        "humidity" => {
            if let Some(h) = num(1) {
                let c = Climate { humidity: h, ..w.climate() };
                w.set_climate(c);
            }
        }
        "temperature" => {
            if let Some(t) = num(1) {
                let c = Climate { temperature: t, ..w.climate() };
                w.set_climate(c);
            }
        }
        "time" | "clouds" => {
            let verb = parts[0].to_string();
            w.sky_cmds.push((verb, parts.get(1).copied().unwrap_or("").to_string()));
        }
        // walk:x:y:seconds — push the stick (x right, y forward) for a while
        "walk" => {
            if let (Some(x), Some(y), Some(t)) = (num(1), num(2), num(3)) {
                let until = w.now() + t.clamp(0.0, 30.0) as f64;
                w.auto = Some((Vec2::new(x, y), until));
            }
        }
        "jump" => w.auto_jump = true,
        "slot" => {
            if let Some(n) = num(1) {
                w.slot = (n as usize).saturating_sub(1).min(w.hotbar.len().saturating_sub(1));
            }
        }
        "click" => {
            if let Some(h) = w.target {
                if parts.get(1) == Some(&"left") {
                    w.edit(h.cell, 0);
                } else {
                    let c = [h.cell[0] + h.normal[0], h.cell[1] + h.normal[1], h.cell[2] + h.normal[2]];
                    let b = w.selected();
                    w.edit(c, b);
                }
            }
        }
        _ => w.notice(format!("Unknown world command: {cmd}")),
    }
}

/// The camera's forward direction for a turn and tilt.
fn facing(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

/// Walking, flying, looking, choosing and building.
///
/// Diablo camera (the default): WASD or the left stick walk relative to the screen, a left
/// click (or holding it) walks to the point under the mouse, the wheel zooms, Q and E turn the
/// view, right click builds where the mouse points, Shift + right click takes a block away.
/// Behind camera (V switches): the mouse looks round once captured and the crosshair builds.
#[allow(clippy::too_many_arguments)]
pub fn controls(
    mut w: ResMut<WorldClient>,
    mut av: ResMut<crate::LocalAvatar>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    pads: Query<&Gamepad>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<crate::WorldCamera>>,
    real: Res<Time<Real>>,
    mut walkers: Query<&mut Transform, With<Walker>>,
) {
    if !w.active() {
        return;
    }
    let cmds = std::mem::take(&mut w.cmds);
    for c in cmds {
        run_command(&mut w, &c);
    }
    if !w.ready {
        return;
    }
    let dt = real.delta_secs().min(0.25);
    let size = w.block_size;
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::KeyV) {
        w.cam_mode = if w.cam_mode == CamMode::Diablo { CamMode::Behind } else { CamMode::Diablo };
        let s = if w.cam_mode == CamMode::Diablo { "Camera: from above" } else { "Camera: behind" };
        w.notice(s);
    }
    let diablo = w.cam_mode == CamMode::Diablo;
    // ---- the mouse: captured for looking behind; free (a pointer) from above
    let mut clicked = (mouse.just_pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Right));
    let mut cursor_at = None;
    if let Ok((win, mut c)) = windows.single_mut() {
        if diablo {
            if w.captured {
                c.grab_mode = CursorGrabMode::None;
                c.visible = true;
                w.captured = false;
            }
            cursor_at = win.cursor_position();
        } else if !w.captured && clicked.0 {
            c.grab_mode = CursorGrabMode::Locked;
            c.visible = false;
            w.captured = true;
            clicked = (false, false);
        } else if w.captured && keys.just_pressed(KeyCode::Escape) {
            c.grab_mode = CursorGrabMode::None;
            c.visible = true;
            w.captured = false;
        }
    }
    // the ray under the pointer, from where the camera was last frame
    w.pick = None;
    if let (Some(pos), Ok((cam, gt))) = (cursor_at, cams.single()) {
        if let Ok(ray) = cam.viewport_to_world(gt, pos) {
            w.pick = Some((ray.origin / size, Vec3::from(ray.direction)));
        }
    }
    // ---- look, zoom and turn
    if w.captured && !diablo {
        w.cam_yaw -= motion.delta.x * 0.0035;
        w.cam_pitch -= motion.delta.y * 0.0035;
    }
    let pad = pads.iter().next();
    let (mut stick, mut up, mut down) = (Vec2::ZERO, false, false);
    let (mut jump, mut toggle_fly, mut run) = (false, false, false);
    let (mut place, mut remove) =
        if diablo { (clicked.1 && !shift, clicked.1 && shift) } else { (clicked.1 && w.captured, clicked.0 && w.captured) };
    let mut cycle = 0i32;
    let mut pad_build = false;
    if let Some(p) = pad {
        let look = p.right_stick();
        if look.length() > 0.15 {
            w.cam_yaw -= look.x * 2.6 * dt;
            if diablo {
                w.zoom = (w.zoom * (1.0 - look.y * 1.5 * dt)).clamp(4.0, 45.0);
            } else {
                w.cam_pitch += look.y * 2.0 * dt;
            }
        }
        let ls = p.left_stick();
        if ls.length() > 0.15 {
            stick += ls;
        }
        jump |= p.just_pressed(GamepadButton::South);
        up |= p.pressed(GamepadButton::South);
        down |= p.pressed(GamepadButton::East);
        toggle_fly |= p.just_pressed(GamepadButton::North);
        run |= p.pressed(GamepadButton::LeftThumb);
        let (rt, lt) = (p.just_pressed(GamepadButton::RightTrigger2), p.just_pressed(GamepadButton::LeftTrigger2));
        place |= rt;
        remove |= lt;
        pad_build = rt || lt;
        cycle += p.just_pressed(GamepadButton::RightTrigger) as i32 - p.just_pressed(GamepadButton::LeftTrigger) as i32;
        if p.just_pressed(GamepadButton::Select) {
            w.cam_mode = if diablo { CamMode::Behind } else { CamMode::Diablo };
        }
    }
    w.cam_pitch = w.cam_pitch.clamp(-1.45, 1.3);
    if diablo {
        // Q and E turn the view an eighth of a turn
        if keys.just_pressed(KeyCode::KeyQ) {
            w.cam_yaw += std::f32::consts::FRAC_PI_4;
        }
        if keys.just_pressed(KeyCode::KeyE) {
            w.cam_yaw -= std::f32::consts::FRAC_PI_4;
        }
    }
    // ---- keys
    let k = |c: KeyCode| keys.pressed(c);
    stick.y += k(KeyCode::KeyW) as i32 as f32 - k(KeyCode::KeyS) as i32 as f32;
    stick.x += k(KeyCode::KeyD) as i32 as f32 - k(KeyCode::KeyA) as i32 as f32;
    jump |= keys.just_pressed(KeyCode::Space);
    jump |= std::mem::take(&mut w.auto_jump);
    if let Some((dir, until)) = w.auto {
        if w.now() < until {
            stick += dir;
        } else {
            w.auto = None;
        }
    }
    up |= k(KeyCode::Space);
    down |= k(KeyCode::ControlLeft) || k(KeyCode::ControlRight);
    run |= shift && !diablo;
    toggle_fly |= keys.just_pressed(KeyCode::KeyF);
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (i, d) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*d) && i < w.hotbar.len() {
            w.slot = i;
        }
    }
    if scroll.delta.y.abs() > 0.0 {
        if diablo && !shift {
            // the wheel zooms the camera in and out
            w.zoom = (w.zoom * (1.0 - scroll.delta.y.signum() * 0.12)).clamp(4.0, 45.0);
        } else {
            cycle -= scroll.delta.y.signum() as i32;
        }
    }
    if cycle != 0 && !w.hotbar.is_empty() {
        let n = w.hotbar.len() as i32;
        w.slot = ((w.slot as i32 + cycle).rem_euclid(n)) as usize;
    }
    if toggle_fly {
        w.fly = !w.fly;
        w.vy = 0.0;
        let s = if w.fly { "Flying" } else { "Walking" };
        w.notice(s);
    }
    // ---- where to walk
    let stick = stick.clamp_length_max(1.0);
    // "forward" is up the screen: the camera's direction on the ground; through a cut to a
    // fixed camera it stays as it was until you let go
    let screen_yaw = {
        let d = w.cam_dir;
        if d.x.abs() + d.z.abs() > 1e-3 { (-d.x).atan2(-d.z) } else { w.cam_yaw }
    };
    let basis = if stick.length() > 0.1 {
        *w.move_yaw.get_or_insert(screen_yaw)
    } else {
        w.move_yaw = None;
        screen_yaw
    };
    let fwd = Vec3::new(-basis.sin(), 0.0, -basis.cos());
    let right = Vec3::new(basis.cos(), 0.0, -basis.sin());
    let mut dir = fwd * stick.y + right * stick.x;
    // a click (or holding the button) on the ground walks there
    if diablo && mouse.pressed(MouseButton::Left) && !shift {
        if let Some((o, d)) = w.pick {
            if let Some(h) = ray::raycast_with(|c| w.solid_at(c), o.to_array(), d.to_array(), 160.0) {
                let p = IVec3::from(h.cell).as_vec3() + Vec3::new(0.5, 1.0, 0.5);
                w.goal = Some(p);
            }
        }
    }
    if dir.length() > 0.1 {
        w.goal = None;
    } else if let Some(g) = w.goal {
        let to = Vec3::new(g.x - w.body.pos[0], 0.0, g.z - w.body.pos[2]);
        if to.length() < 0.4 {
            w.goal = None;
        } else {
            dir = to.normalize() * (to.length() / 2.0).clamp(0.35, 1.0);
        }
    }
    let speed = if w.fly { FLY } else if run { RUN } else { WALK } / size;
    // in steps of at most 50 ms, so a slow frame (or a slow machine) still moves in real time
    let frame = real.delta_secs().min(0.25);
    let steps = (frame / 0.05).ceil().max(1.0) as usize;
    let dt = frame / steps as f32;
    let mut jump = jump;
    let before = w.body.pos;
    for _ in 0..steps {
        let mut vel = dir * speed;
        if w.fly {
            vel.y = (up as i32 - down as i32) as f32 * FLY / size;
        } else {
            if w.grounded && std::mem::take(&mut jump) {
                w.vy = (2.0 * GRAVITY * JUMP_HEIGHT).sqrt() / size;
            }
            w.vy -= GRAVITY / size * dt;
            vel.y = w.vy;
        }
        let step = if w.grounded && !w.fly { 1.05 } else { 0.0 };
        let mut body = w.body;
        let contact = {
            let wr: &WorldClient = &w;
            body.travel(|c| wr.stands_on(c), (vel * dt).to_array(), step)
        };
        w.body = body;
        if contact.hit[1] {
            w.vy = 0.0;
        }
        w.grounded = !w.fly && (contact.ground || (w.vy <= 0.0 && w.body.on_ground(|c| w.stands_on(c))));
    }
    // walking to a point and stuck against something: give up
    if w.goal.is_some() && dir.length() > 0.1 {
        let moved = Vec2::new(w.body.pos[0] - before[0], w.body.pos[2] - before[2]).length();
        if moved < speed * frame * 0.05 {
            w.goal = None;
        }
    }
    if w.body.pos[1] < w.view.confirmed.bounds.lo[1] as f32 - 20.0 {
        w.body.pos = w.spawn;
        w.vy = 0.0;
        w.notice("Back to the start");
    }
    // ---- face where you walk; walk while moving
    let moving = dir.length() > 0.1;
    if moving {
        let goal = dir.x.atan2(dir.z);
        let mut d = goal - w.yaw;
        d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        w.yaw += d * (1.0 - (-frame * 12.0).exp());
    }
    let walk = moving && !w.fly && av.can_walk;
    if walk != w.moving {
        w.moving = walk;
        av.walking = walk;
    }
    if let Some(e) = w.walker {
        if let Ok(mut t) = walkers.get_mut(e) {
            t.translation = Vec3::from(w.body.pos) * size;
            t.rotation = Quat::from_rotation_y(w.yaw);
        }
    }
    // ---- what you'd build on: the crosshair (behind), the pointer (from above), or with a
    // controller from above, the spot in front of your feet
    let head = Vec3::from(w.body.pos) + Vec3::Y * EYES;
    let in_reach = |h: &Hit| {
        let d2: f32 = (0..3).map(|i| (h.cell[i] as f32 + 0.5 - head[i]).powi(2)).sum();
        d2 <= REACH * REACH
    };
    w.target = if diablo {
        let from_pad = pad_build || (cursor_at.is_none() && pad.is_some());
        if from_pad {
            let ahead = Vec3::new(w.yaw.sin(), 0.0, w.yaw.cos());
            let o = head;
            let d = (ahead * 2.2 - Vec3::Y * 1.6).normalize();
            ray::raycast_with(|c| w.solid_at(c), o.to_array(), d.to_array(), 8.0)
        } else if let Some((o, d)) = w.pick {
            ray::raycast_with(|c| w.solid_at(c), o.to_array(), d.to_array(), 200.0).filter(in_reach)
        } else {
            None
        }
    } else {
        let f = facing(w.cam_yaw, w.cam_pitch);
        let cam = w.cam_at / size;
        // start the ray level with the head, so blocks between the camera and you don't count
        let start = cam + f * (head - cam).dot(f).max(0.0);
        ray::raycast_with(|c| w.solid_at(c), start.to_array(), f.to_array(), REACH + 4.0).filter(in_reach)
    };
    if let Some(h) = w.target {
        if remove {
            w.edit(h.cell, 0);
        } else if place {
            let c = [h.cell[0] + h.normal[0], h.cell[1] + h.normal[1], h.cell[2] + h.normal[2]];
            let b = w.selected();
            w.edit(c, b);
        }
    }
}

/// The behind camera: behind and a little right of the head, pulled in if a block is in the
/// way. Metres.
fn behind_spot(w: &WorldClient) -> Vec3 {
    let size = w.block_size;
    let f = facing(w.cam_yaw, w.cam_pitch);
    let right = Vec3::new(w.cam_yaw.cos(), 0.0, -w.cam_yaw.sin());
    let head = (Vec3::from(w.body.pos) + Vec3::Y * (EYES + 0.4)) * size;
    let back = (-f * CAM_DIST + right * 0.45).normalize();
    let dist = (CAM_DIST * CAM_DIST + 0.45 * 0.45).sqrt();
    let hit = ray::raycast_with(|c| w.solid_at(c), (head / size).to_array(), back.to_array(), dist / size);
    let d = hit.map_or(dist, |h| (h.distance * size - 0.15).max(0.2));
    head + back * d
}

/// The camera: Diablo-style from above (zooming in and out, the tilt steepening as it goes
/// out), behind you, or a world's fixed shot for where you are, blended smoothly between.
/// Gizmo lines go round the block you'd build on.
pub fn camera(
    mut w: ResMut<WorldClient>,
    real: Res<Time<Real>>,
    mut cams: Query<&mut Transform, With<crate::WorldCamera>>,
    mut gizmos: Gizmos,
    mut clear: ResMut<ClearColor>,
) {
    if !w.active() || !w.ready {
        return;
    }
    // a sky instead of the show's background
    let sky = Color::srgb(0.55, 0.74, 0.95);
    if clear.0 != sky {
        clear.0 = sky;
    }
    let dt = real.delta_secs().min(0.25);
    let size = w.block_size;
    let ease = |rate: f32| 1.0 - (-dt * rate).exp();
    let chest = (Vec3::from(w.body.pos) + Vec3::Y * 2.4) * size;
    let focus = match w.focus {
        Some(f) if f.distance(chest) < 6.0 => f.lerp(chest, ease(9.0)),
        _ => chest,
    };
    w.focus = Some(focus);
    w.zoom_now += (w.zoom - w.zoom_now) * ease(8.0);
    // ---- the follow camera
    let (follow_at, follow_dir) = match w.cam_mode {
        CamMode::Diablo => {
            // steeper the further out: 38° looking down close in, 62° far out
            let t = ((w.zoom_now - 4.0) / 41.0).clamp(0.0, 1.0);
            let tilt = (38.0 + 24.0 * t).to_radians();
            let dir = facing(w.cam_yaw, -tilt);
            (focus - dir * w.zoom_now, dir)
        }
        CamMode::Behind => (behind_spot(&w), facing(w.cam_yaw, w.cam_pitch)),
    };
    // ---- a fixed shot for where you are
    let here = w.body.pos;
    let zone = w.forced_shot.or_else(|| w.shots.iter().position(|s| s.contains(here)));
    if zone.is_some() {
        if zone != w.shot {
            w.last_shot = w.shot.or(w.last_shot);
        }
        w.shot = zone;
    } else if w.shot.is_some() {
        w.last_shot = w.shot.take();
    }
    let want = if w.shot.is_some() { 1.0 } else { 0.0 };
    w.shot_blend += (want - w.shot_blend) * ease(2.6);
    let shot_view = |i: usize| {
        let s = &w.shots[i];
        let look = s.look.unwrap_or(chest);
        (s.at, (look - s.at).normalize_or(Vec3::NEG_Z))
    };
    let (at, dir) = match w.shot.or(w.last_shot) {
        Some(i) if w.shot_blend > 0.001 => {
            let (sa, sd) = shot_view(i);
            let k = w.shot_blend * w.shot_blend * (3.0 - 2.0 * w.shot_blend);
            (follow_at.lerp(sa, k), follow_dir.lerp(sd, k).normalize_or(sd))
        }
        _ => (follow_at, follow_dir),
    };
    w.cam_at = at;
    w.cam_dir = dir;
    for mut t in &mut cams {
        *t = Transform::from_translation(at).looking_to(dir, Vec3::Y);
    }
    if let Some(h) = w.target {
        let centre = (IVec3::from(h.cell).as_vec3() + Vec3::splat(0.5)) * size;
        gizmos.cube(Transform::from_translation(centre).with_scale(Vec3::splat(size * 1.02)), Color::srgba(1.0, 1.0, 1.0, 0.8));
    }
    if let Some(g) = w.goal {
        let p = Vec3::new(g.x, g.y.floor(), g.z) * size + Vec3::Y * 0.02;
        gizmos.circle(Isometry3d::new(p, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.25, Color::srgba(1.0, 1.0, 1.0, 0.7));
    }
}

/// Run the weather between the server's keyframes (four times a second).
pub fn weather(mut w: ResMut<WorldClient>, real: Res<Time<Real>>) {
    if w.weather.is_none() {
        return;
    }
    w.weather_acc += real.delta_secs();
    if w.weather_acc >= 0.25 {
        let dt = w.weather_acc.min(2.0);
        w.weather_acc = 0.0;
        if let Some(wx) = &mut w.weather {
            wx.step(dt);
        }
    }
}

/// "from the south-west" for a wind blowing towards (x, z).
fn compass(wind: [f32; 3]) -> &'static str {
    // where it comes from: opposite to where it blows, clockwise from north (-z)
    let from = (-wind[0]).atan2(wind[2]).to_degrees().rem_euclid(360.0);
    const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    NAMES[((from + 22.5) / 45.0) as usize % 8]
}

/// The weather where you are, in a line.
pub fn weather_line(a: &Air) -> String {
    let speed = a.wind[0].hypot(a.wind[2]);
    let vis = if a.visibility >= 10_000.0 { format!("{:.0} km", a.visibility / 1000.0) } else { format!("{:.0} m", a.visibility) };
    let mut s = format!(
        "{:.0} C   humidity {:.0}%   dew point {:.0} C   wind {:.1} m/s from {}   visibility {}",
        a.temperature,
        a.humidity * 100.0,
        a.dew_point,
        speed,
        compass(a.wind),
        vis
    );
    if a.rain > 0.2 {
        s += &format!("   {} {:.1} mm/h", if a.temperature < 0.5 { "snow" } else { "rain" }, a.rain);
    }
    s
}

/// A crosshair, the block bar and what's happening.
pub fn hud(
    mut w: ResMut<WorldClient>,
    mut commands: Commands,
    mut texts: Query<&mut Text, With<WorldHud>>,
    mut crosshairs: Query<&mut Visibility, With<Crosshair>>,
) {
    if !w.active() {
        return;
    }
    let hud = match w.hud {
        Some(e) => e,
        None => {
            commands.spawn((
                Text::new("+"),
                TextFont { font_size: bevy::text::FontSize::Px(28.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                TextShadow::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(50.0),
                    top: Val::Percent(50.0),
                    margin: UiRect { left: Val::Px(-8.0), top: Val::Px(-18.0), ..default() },
                    ..default()
                },
                Crosshair,
            ));
            // on a dark glass panel, so it reads on the bright void as well as at night
            let e = commands
                .spawn((
                    Text::new(""),
                    TextFont { font_size: bevy::text::FontSize::Px(16.0), ..default() },
                    TextColor(Color::srgb(0.96, 0.96, 0.98)),
                    TextLayout::justify(Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(12.0),
                        left: Val::Px(12.0),
                        right: Val::Px(12.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.55)),
                    WorldHud,
                ))
                .id();
            w.hud = Some(e);
            e
        }
    };
    let now = w.now();
    if w.notice.as_ref().is_some_and(|(_, at)| now - at > 3.0) {
        w.notice = None;
    }
    let bar: Vec<String> = w
        .hotbar
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let n = w.block_name(*b);
            if i == w.slot { format!("[{} {n}]", i + 1) } else { format!("{} {n}", i + 1) }
        })
        .collect();
    let mut line = bar.join("   ");
    let who = w.players();
    let top = match &w.notice {
        Some((n, _)) => n.clone(),
        None if !w.ready => w.status.clone(),
        None if who.is_empty() => format!("{} | on your own{}", w.world, if w.fly { " | flying" } else { "" }),
        None => format!("{} | with {}{}", w.world, who.join(", "), if w.fly { " | flying" } else { "" }),
    };
    line = format!("{top}\n{line}");
    if let Some(a) = w.air() {
        line = format!("{}\n{line}", weather_line(&a));
    }
    // the crosshair only behind you: from above the mouse is the pointer
    let show = if w.cam_mode == CamMode::Behind && w.shot.is_none() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut crosshairs {
        if *v != show {
            *v = show;
        }
    }
    if let Ok(mut t) = texts.get_mut(hud) {
        if t.0 != line {
            t.0 = line;
        }
    }
}

/// Send your pose 30 times a second: where you stand, which way you face, and the body.
pub fn send_pose(mut w: ResMut<WorldClient>, tf: Query<&Transform>) {
    if !w.ready || w.link.is_none() {
        return;
    }
    let now = w.now();
    if now - w.last_pose < POSE_EVERY {
        return;
    }
    w.last_pose = now;
    let Some((root, rig)) = &w.rig else { return };
    let tick = (now * HZ) as u64;
    let pose = net::capture(tick, net::WORLD_PLAYER, *root, rig, None, &tf);
    let at = Vec3::from(w.body.pos) * w.block_size;
    let blob = postcard::to_stdvec(&Placed { at: at.to_array(), yaw: w.yaw, pose }).unwrap_or_default();
    let pos = w.body.pos;
    w.send(&ToServer::Pose { at: pos, blob });
}

/// Everyone else, 100 ms behind, blended between their nearest poses.
pub fn apply_others(
    mut w: ResMut<WorldClient>,
    time: Res<Time<Real>>,
    remotes: Query<&Remote>,
    mut tf: Query<&mut Transform, Without<crate::WorldCamera>>,
) {
    if !w.ready {
        return;
    }
    let now = w.now();
    let dt = time.delta_secs_f64();
    for o in w.others.values_mut() {
        let Some(e) = o.entity else { continue };
        let Some(t) = o.stream.clock.at(now, dt) else { continue };
        let tick = t * HZ;
        let Some((a, b, s)) = o.stream.around(tick) else { continue };
        let place = |tick: u64| o.places.iter().find(|p| p.0 == tick).map(|p| (p.1, p.2));
        if let (Some(pa), Some(pb)) = (place(a.tick), place(b.tick)) {
            if let Ok(mut t) = tf.get_mut(e) {
                t.translation = pa.0.lerp(pb.0, s);
                t.rotation = Quat::from_rotation_y(pa.1).slerp(Quat::from_rotation_y(pb.1), s);
            }
        }
        while o.places.len() > 2 && o.places[1].0 < a.tick {
            o.places.pop_front();
        }
        if let Ok(r) = remotes.get(e) {
            if let (Some(inner), Some(rig)) = (r.inner, &r.rig) {
                net::put(rig, inner, &a, &b, s, &mut tf);
            }
        }
    }
}

/// For the control page's state.
pub fn json(w: &WorldClient) -> serde_json::Value {
    if !w.active() {
        return serde_json::Value::Null;
    }
    serde_json::json!({
        "url": w.url,
        "time": (w.hours * 100.0).round() / 100.0,
        "camera": match w.cam_mode { CamMode::Diablo => "diablo", CamMode::Behind => "behind" },
        "shot": w.shot.map(|i| w.shots[i].name.clone()),
        "zoom": w.zoom,
        "floor": w.floor,
        "portals": w.portals.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        "travelling": w.travelling(),
        "weather": w.air().map(|a| serde_json::json!({
            "temperature": a.temperature,
            "humidity": a.humidity,
            "dew_point": a.dew_point,
            "wind": a.wind,
            "cloud": a.cloud,
            "rain": a.rain,
            "visibility": a.visibility,
            "line": weather_line(&a),
        })),
        "climate": w.weather.as_ref().map(|x| serde_json::to_value(x.climate).unwrap_or_default()),
        "status": w.status,
        "world": w.world,
        "you": w.you,
        "ready": w.ready,
        "players": w.players(),
        "blocks": w.view.confirmed.count(),
        "pending": w.view.pending(),
        "seq": w.view.seq,
        "chunks": w.meshes.len(),
        "pos": w.body.pos,
        "grounded": w.grounded,
        "fly": w.fly,
        "target": w.target.map(|h| h.cell),
        "slot": w.slot + 1,
        "block": w.block_name(w.selected()),
        "edits": w.edits,
        "refused": w.refused,
        "notice": w.notice.as_ref().map(|n| n.0.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(ws_url("ws://h:7780"), "ws://h:7780/ws");
        assert_eq!(ws_url("ws://h:7780/ws"), "ws://h:7780/ws");
        assert_eq!(ws_url("http://h:7780/"), "ws://h:7780/ws");
        assert_eq!(ws_url("https://w.example"), "wss://w.example/ws");
        assert_eq!(ws_url("h:7780"), "ws://h:7780/ws");
    }

    #[test]
    fn facing_matches_the_stage_camera() {
        // no turn, no tilt: looking down -z, like the stage camera at the character
        let f = facing(0.0, 0.0);
        assert!((f - Vec3::NEG_Z).length() < 1e-6);
        // tilted down: looking down
        assert!(facing(0.0, -0.5).y < 0.0);
        // a quarter turn left
        assert!((facing(std::f32::consts::FRAC_PI_2, 0.0) - Vec3::NEG_X).length() < 1e-6);
    }

    #[test]
    fn commands_and_prediction() {
        let mut w = WorldClient::new(Some("ws://x".into()), "me".into());
        w.names = vec!["grass".into(), "stone".into()];
        w.remap = vec![0, 1, 3];
        w.hotbar = vec![1, 2];
        w.view = View::new(vox_core::World::new(vox_core::Bounds::default(), 2));
        w.view.confirmed.set([0, -1, 0], 1);
        // place next to you, not in you
        run_command(&mut w, "place:2:0:0:stone");
        assert_eq!(w.view.get([2, 0, 0]), 2);
        assert_eq!(w.local(w.view.get([2, 0, 0])), 3);
        run_command(&mut w, "place:0:1:0:stone");
        assert_eq!(w.view.get([0, 1, 0]), 0, "inside the player");
        run_command(&mut w, "remove:0:-1:0");
        assert_eq!(w.view.get([0, -1, 0]), 0);
        assert_eq!(w.view.pending(), 2);
        run_command(&mut w, "place:60:0:0:stone");
        assert_eq!(w.view.pending(), 2, "out of reach");
        run_command(&mut w, "slot:2");
        assert_eq!(w.selected(), 2);
        run_command(&mut w, "look:90:-30");
        assert!((w.cam_yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }
}

/// The link in the browser: the page's own WebSocket. Its callbacks fill the same inbox the
/// desktop's thread does; what goes out is sent each frame by [`web_link::pump`].
#[cfg(target_arch = "wasm32")]
mod web_link {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

    use super::{Incoming, ToClient, wire};

    struct Open {
        ws: Option<WebSocket>,
        url: String,
        out: mpsc::Receiver<Vec<u8>>,
        inbox: mpsc::Sender<Incoming>,
        stop: Arc<AtomicBool>,
        bye: Rc<RefCell<bool>>,
        retry_at: Option<f64>,
        wait: f64,
    }

    thread_local! {
        static LINK: RefCell<Option<Open>> = const { RefCell::new(None) };
    }

    fn now() -> f64 {
        js_sys::Date::now() / 1000.0
    }

    fn connect(o: &mut Open) {
        let ws = match WebSocket::new(&o.url) {
            Ok(ws) => ws,
            Err(e) => {
                let _ = o.inbox.send(Incoming::Status(format!("Can't reach the world: {e:?}")));
                o.retry_at = Some(now() + o.wait);
                return;
            }
        };
        ws.set_binary_type(BinaryType::Arraybuffer);
        let inbox = o.inbox.clone();
        let on_open = Closure::<dyn FnMut()>::new(move || {
            let _ = inbox.send(Incoming::Status("Connected".into()));
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();
        let inbox = o.inbox.clone();
        let bye = o.bye.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let b = js_sys::Uint8Array::new(&buf).to_vec();
                if matches!(wire::decode::<ToClient>(&b), Ok(ToClient::Bye { .. })) {
                    *bye.borrow_mut() = true;
                }
                let _ = inbox.send(Incoming::Data(b));
            }
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();
        let inbox = o.inbox.clone();
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_e: CloseEvent| {
            let _ = inbox.send(Incoming::Status("Reconnecting".into()));
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();
        o.ws = Some(ws);
    }

    pub fn open(url: String, out: mpsc::Receiver<Vec<u8>>, inbox: mpsc::Sender<Incoming>, stop: Arc<AtomicBool>) {
        LINK.with(|l| {
            if let Some(old) = l.borrow_mut().take() {
                if let Some(ws) = old.ws {
                    let _ = ws.close();
                }
            }
            let mut o = Open { ws: None, url, out, inbox, stop, bye: Rc::new(RefCell::new(false)), retry_at: None, wait: 0.5 };
            connect(&mut o);
            *l.borrow_mut() = Some(o);
        });
    }

    /// Send what's queued; reconnect a dropped link (unless the server said goodbye).
    pub fn pump() {
        LINK.with(|l| {
            let mut l = l.borrow_mut();
            let Some(o) = l.as_mut() else { return };
            if o.stop.load(Ordering::Relaxed) {
                if let Some(ws) = o.ws.take() {
                    let _ = ws.close();
                }
                *l = None;
                return;
            }
            let state = o.ws.as_ref().map(|ws| ws.ready_state());
            match state {
                Some(WebSocket::OPEN) => {
                    let ws = o.ws.as_ref().unwrap();
                    while let Ok(b) = o.out.try_recv() {
                        let _ = ws.send_with_u8_array(&b);
                    }
                }
                Some(WebSocket::CONNECTING) => {
                    // anything queued before connecting is stale: the hello goes first
                    while o.out.try_recv().is_ok() {}
                }
                _ => {
                    if *o.bye.borrow() {
                        return;
                    }
                    match o.retry_at {
                        None => o.retry_at = Some(now() + o.wait),
                        Some(t) if now() >= t => {
                            o.retry_at = None;
                            o.wait = (o.wait * 2.0).min(10.0);
                            connect(o);
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}
