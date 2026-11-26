//! A Vox world in the app: `--world ws://host:7780 [--name Sam]`.
//!
//! The world's blocks arrive in chunks and are drawn chunk by chunk. You walk (or fly) your
//! character around, first person by default, and build: break, place, pick, from the hotbar.
//! Edits show at once and are undone if the server refuses them (vox-core's [`View`]).
//! Everyone else in the world is drawn from their poses, played 100 ms behind.
//!
//! Controls are actions ([`crate::input`]): the keyboard and mouse or a gamepad, rebindable;
//! the HUD, quick wheel and menus are [`crate::hud`] and [`crate::menu`].
//!
//! The state is [`WorldClient`], here; its systems are in the child modules: `link` (the
//! connection), `receive` (what the server says), `chunks` (meshing), `avatar` (poses out and
//! in), `controls`, `camera`, `hud` (weather between keyframes, the status text), `commands`
//! (the control page's `world:...` commands).
//!
//! For automation the control page takes `world:place:x:y:z:block`, `world:remove:x:y:z`,
//! `world:goto:x:y:z`, `world:look:yaw:pitch` (degrees), `world:fly:on|off`, `world:slot:n`,
//! `world:click:left|right` (at the crosshair), `world:walk:x:y:seconds`, `world:jump`,
//! `world:time:<hours>|auto` and `world:clouds:<0-1>`.

mod avatar;
mod camera;
mod chunks;
mod commands;
mod controls;
mod hud;
mod link;
mod receive;
#[cfg(target_arch = "wasm32")]
mod web_link;

pub use avatar::*;
pub use camera::*;
pub use chunks::*;
pub use commands::*;
pub use controls::*;
pub use hud::*;
pub use link::*;
pub use receive::*;

use std::collections::{HashMap, HashSet, VecDeque};
// the browser's link keeps its own channels (the desktop's are vox-link's)
#[cfg(target_arch = "wasm32")]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use bevy::platform::time::Instant;
use bevy::{
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

/// The link to the world server: vox-link's thread on the desktop, the page's WebSocket in
/// the browser (`web_link`). Either way the app queues bytes out and drains [`Incoming`].
#[cfg(not(target_arch = "wasm32"))]
struct Link(vox_link::Link<()>);

#[cfg(target_arch = "wasm32")]
struct Link {
    out: mpsc::Sender<Vec<u8>>,
    inbox: Mutex<mpsc::Receiver<Incoming>>,
    stop: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Link {
    fn send(&self, bytes: Vec<u8>) {
        #[cfg(not(target_arch = "wasm32"))]
        self.0.send(bytes);
        #[cfg(target_arch = "wasm32")]
        {
            let _ = self.out.send(bytes);
        }
    }

    fn drain(&self) -> Vec<Incoming> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.0
                .drain()
                .into_iter()
                .filter_map(|e| match e {
                    vox_link::Event::Status(s) => Some(Incoming::Status(s)),
                    vox_link::Event::Data(b, _) => Some(Incoming::Data(b)),
                    vox_link::Event::Text(_) | vox_link::Event::App(()) => None,
                })
                .collect()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.inbox.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect()
        }
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
    /// Through your own eyes, the mouse (or right stick) looks round; the default.
    First,
    /// Close behind your shoulder, the mouse looks round.
    Behind,
    /// From high up at an angle (like Diablo), zooming in and out; the mouse points at things.
    Diablo,
}

impl CamMode {
    /// The next view, round the three.
    pub fn next(self) -> CamMode {
        match self {
            CamMode::First => CamMode::Behind,
            CamMode::Behind => CamMode::Diablo,
            CamMode::Diablo => CamMode::First,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            CamMode::First => "View: first person",
            CamMode::Behind => "View: behind",
            CamMode::Diablo => "View: from above",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            CamMode::First => "first",
            CamMode::Behind => "behind",
            CamMode::Diablo => "diablo",
        }
    }
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
    /// the host the sign-in token belongs to (where you started): portals to other hosts get no token
    token_host: Option<String>,
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
    /// watching, not playing (the front page's live view): no avatar, no input, no HUD, and a
    /// camera that drifts round the spawn on its own
    pub watch: bool,
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
            token_host: None,
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
            cam_mode: CamMode::First,
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
            watch: false,
        }
    }
}

impl WorldClient {
    pub fn new(url: Option<String>, name: String) -> Self {
        let token = std::env::args()
            .skip_while(|a| a != "--token")
            .nth(1)
            .or_else(|| std::env::var("VOX_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());
        let url = url.map(|u| ws_url(&u));
        let token_host = url.as_deref().map(url_host);
        WorldClient { url, name, token, token_host, ..default() }
    }

    /// Watch instead of play: see [`WorldClient::watch`].
    pub fn watching(mut self, watch: bool) -> Self {
        self.watch = watch;
        self
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

    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn send(&self, m: &ToServer) {
        if let Some(l) = &self.link {
            l.send(wire::encode(m));
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
        let refused = self.status.starts_with("Can't reach") || self.status.starts_with("Reconnecting");
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

    /// Put `block` in your hand: its hotbar slot if it has one, else in place of the block in
    /// the current slot.
    pub fn pick_block(&mut self, block: u16) {
        if block == 0 || self.liquid.get(block as usize).copied().unwrap_or(false) {
            return;
        }
        if let Some(i) = self.hotbar.iter().position(|b| *b == block) {
            self.slot = i;
        } else if self.slot < self.hotbar.len() {
            self.hotbar[self.slot] = block;
        } else {
            self.hotbar.push(block);
            self.slot = self.hotbar.len() - 1;
        }
    }

    /// Water, lava and the like: not for holding.
    pub fn is_liquid(&self, block: u16) -> bool {
        self.liquid.get(block as usize).copied().unwrap_or(false)
    }

    /// The blocks in the hotbar, by slot.
    pub fn hotbar(&self) -> &[u16] {
        &self.hotbar
    }

    /// Put `block` in hotbar slot `i` (0 to 8).
    pub fn set_hotbar(&mut self, i: usize, block: u16) {
        if i >= 9 || block == 0 {
            return;
        }
        while self.hotbar.len() <= i {
            self.hotbar.push(block);
        }
        self.hotbar[i] = block;
    }

    /// Every block this world has, by id (1 up).
    pub fn block_ids(&self) -> std::ops::RangeInclusive<u16> {
        1..=self.names.len() as u16
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.link = Some(Link(vox_link::spawn("world link", link::WorldDial { url })));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
            let (in_tx, in_rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            web_link::open(url, out_rx, in_tx, stop.clone());
            self.link = Some(Link { out: out_tx, inbox: Mutex::new(in_rx), stop });
        }
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
        // the last world's air stays behind: this one sends its own, or has none (orbit)
        self.weather = None;
        self.weather_acc = 0.0;
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
        let spawn = if spawn.iter().all(|v| v.is_finite()) { spawn } else { [0.0, 8.0, 0.0] };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn armed() -> BuildRepeat {
        let mut r = BuildRepeat::default();
        r.step((false, false), (false, false), 0.0, 0.0);
        r
    }

    #[test]
    fn holding_break_repeats_while_the_aim_stays_put() {
        let mut r = armed();
        assert_eq!(r.step((true, false), (true, false), 1.0, 0.0), (true, false));
        assert_eq!(r.step((true, false), (false, false), 1.1, 0.0), (false, false), "not before the first delay");
        assert_eq!(r.step((true, false), (false, false), 1.31, 0.0), (true, false));
        assert_eq!(r.step((true, false), (false, false), 1.54, 0.01), (true, false), "small wobbles are fine");
    }

    #[test]
    fn looking_round_while_holding_breaks_nothing_more() {
        let mut r = armed();
        assert_eq!(r.step((true, false), (true, false), 1.0, 0.0), (true, false));
        r.step((true, false), (false, false), 1.1, 0.05);
        r.step((true, false), (false, false), 1.2, 0.05);
        for t in [1.4, 1.7, 2.0, 3.0] {
            assert_eq!(r.step((true, false), (false, false), t, 0.0), (false, false), "swept away at {t}");
        }
        // a new press works again
        r.step((false, false), (false, false), 3.1, 0.0);
        assert_eq!(r.step((true, false), (true, false), 3.2, 0.3), (true, false));
    }

    #[test]
    fn a_button_held_on_arrival_does_nothing_until_let_go() {
        let mut r = BuildRepeat::default();
        assert_eq!(r.step((true, false), (true, false), 0.0, 0.0), (false, false));
        assert_eq!(r.step((true, false), (false, false), 1.0, 0.0), (false, false));
        r.step((false, false), (false, false), 1.1, 0.0);
        assert_eq!(r.step((false, true), (false, true), 1.2, 0.0), (false, true));
    }

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
