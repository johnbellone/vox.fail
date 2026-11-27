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
    wire::{self, PROTOCOL, ToClient, ToServer, VoicePacket},
};

use crate::pose::{self as net, Remote, Rig, Stream};
use crate::voice;
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
pub(crate) const FLY: f32 = 7.0;
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

/// How long to wait for the world to answer a trip through a portal, s.
const ASK_WITHIN: f64 = 5.0;

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
    /// the settings as last applied (Welcome, or a live change), through `settings_key`: a
    /// Settings entity that matches this is not applied again
    settings_seen: Option<serde_json::Value>,
    /// `world:time:` and `world:clouds:` for the sky
    pub sky_cmds: Vec<(String, String)>,
    /// the time of day (kept here for the state page)
    pub hours: f32,
    /// the air: from the server's keyframes, simulated here in between
    pub weather: Option<Weather>,
    weather_acc: f32,
    /// a copy of the weather being stepped on a worker thread (desktop), so a step (a couple of
    /// milliseconds) never holds up a frame; dropped (and so cancelled) when the weather is
    /// replaced
    #[cfg(not(target_arch = "wasm32"))]
    weather_task: Option<bevy::tasks::Task<Weather>>,
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
    /// the mouse is captured for looking (behind camera); the editor's pick reads it
    pub(crate) captured: bool,
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
    /// on a tile of an atlas: the edges that lead on to the next tile (`east`, `west`, `south`,
    /// `north`); walking up to one asks the world the way (`edge:<name>`)
    pub edges: Vec<String>,
    pub portals_gen: u32,
    /// where to come out in the next world (going through a portal)
    arrival: Option<Arrival>,
    /// on the way to another world: the one you came from, when you left and where you stood
    /// (to go back to if the way turns out to be closed)
    travel: Option<(String, f64, [f32; 3])>,
    /// a trip asked for and not yet answered: the arrival worked out here (the world's answer
    /// says where exactly) and when it was asked
    asked: Option<(Arrival, f64)>,
    /// the ticket for the world being travelled to: sent with the next hello only (it's good
    /// for that world, once)
    ticket: Option<String>,
    /// `world:portal:<name>`: go through that portal now
    pub portal_go: Option<String>,
    /// watching, not playing (the front page's live view): no avatar, no input, no HUD, and a
    /// camera that drifts round the spawn on its own
    pub watch: bool,
    /// voice packets from the server, for the voice plugin to play (see [`crate::voice::plugin`])
    pub(crate) voice_in: Vec<(u32, VoicePacket)>,
    /// how far nearby voice carries in this world, blocks (the server's `--voice-radius`, sent
    /// with the world's settings)
    pub voice_radius: f32,
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
            settings_seen: None,
            liquid: Vec::new(),
            sky_cmds: Vec::new(),
            hours: 0.0,
            weather: None,
            weather_acc: 0.0,
            #[cfg(not(target_arch = "wasm32"))]
            weather_task: None,
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
            edges: Vec::new(),
            portals_gen: 0,
            arrival: None,
            travel: None,
            asked: None,
            ticket: None,
            portal_go: None,
            watch: false,
            voice_in: Vec::new(),
            voice_radius: crate::voice::plugin::NEARBY_RADIUS,
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

    /// Sends one packet of your voice to the world (the caller checks [`Self::voice_ready`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn send_voice(&self, p: VoicePacket) {
        self.send(&ToServer::Voice(p));
    }

    /// The link is up and you are in the world, so voice can be sent.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn voice_ready(&self) -> bool {
        // (a watcher is not a player: the server would drop its voice and disconnect it)
        self.ready && self.link.is_some() && !self.watch
    }

    /// In a world with every chunk that has arrived drawn (nothing waiting to be meshed): what
    /// a screenshot waits for, as meshing a big world takes many frames on a slow renderer.
    pub fn settled(&self) -> bool {
        self.ready && self.dirty.is_empty()
    }

    /// New air (a keyframe from the server, or none in a new world). A step still running on
    /// the old air is dropped, so it can't come back over this.
    pub(crate) fn set_weather(&mut self, wx: Option<Weather>) {
        self.weather = wx;
        self.weather_acc = 0.0;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.weather_task = None;
        }
    }

    /// Where you stand (blocks, feet) and which way the camera faces (`cam_yaw`, the
    /// -Z-forward convention [`crate::voice::mix`] uses; the character's own `yaw` is the
    /// model's +Z convention and lags the view in third person).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn listener(&self) -> ([f32; 3], f32) {
        (self.body.pos, self.cam_yaw)
    }

    /// Where a player was last seen (blocks, feet), from their newest pose. Poses carry
    /// metres, so this divides by the block size to match [`WorldClient::listener`].
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn place_of(&self, player: u32) -> Option<[f32; 3]> {
        let size = if self.block_size > 0.0 { self.block_size } else { 0.5 };
        self.others.get(&player)?.places.back().map(|(_, at, _)| (*at / size).to_array())
    }

    /// Queues a voice packet from the server. The oldest go first when the queue is full.
    pub(crate) fn push_voice(&mut self, player: u32, packet: VoicePacket) {
        if self.voice_in.len() >= voice::plugin::VOICE_QUEUE_MAX {
            self.voice_in.remove(0);
        }
        self.voice_in.push((player, packet));
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

    /// The world's settings, changed while you are in it: the outdoors system applies them
    /// as it does on joining (it takes them with [`Self::take_settings`]).
    pub fn set_settings(&mut self, s: serde_json::Value) {
        self.settings = Some(s);
    }

    /// Applies a world's settings: the endless floor, the fixed cameras, the block bar and
    /// voice range. On joining (`join`) it also sets the camera's starting angle and zoom. A
    /// live change leaves the camera where you have it, and keeps the hotbar slot in range.
    pub fn apply_settings(&mut self, set: &serde_json::Value, join: bool) {
        self.floor = set.get("floor").and_then(|v| v.as_i64()).map(|f| f as i32);
        self.shots = set
            .get("cameras")
            .and_then(|v| v.as_array())
            .map(|l| l.iter().filter_map(Shot::from_json).collect())
            .unwrap_or_default();
        // the last world's shots are gone; a live change may also have shortened the list
        self.shot = None;
        self.last_shot = None;
        self.forced_shot = None;
        if join {
            self.shot_blend = 0.0;
        }
        if join || set.get("voice_radius").is_some() {
            self.voice_radius = crate::voice::plugin::radius_from(set.get("voice_radius"));
        }
        if join && let Some(c) = set.get("camera") {
            if let Some(y) = c.get("yaw").and_then(|v| v.as_f64()) {
                self.cam_yaw = (y as f32).to_radians();
            }
            if let Some(p) = c.get("pitch").and_then(|v| v.as_f64()).filter(|p| p.is_finite()) {
                self.cam_pitch = (p as f32).to_radians().clamp(-1.45, 1.3);
            }
            if let Some(z) = c.get("zoom").and_then(|v| v.as_f64()) {
                self.zoom = (z as f32).clamp(4.0, 45.0);
                self.zoom_now = self.zoom;
            }
        }
        // a world may choose its own block bar
        let names: Vec<String> = set
            .get("hotbar")
            .and_then(|v| v.as_array())
            .map(|l| l.iter().filter_map(|n| n.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let ids: Vec<u16> = names.iter().filter_map(|n| self.block_id(n)).take(9).collect();
        if !ids.is_empty() {
            self.hotbar = ids;
            if join {
                self.slot = 0;
            }
        }
        self.slot = self.slot.min(self.hotbar.len().saturating_sub(1));
    }

    /// A Settings entity changed while you are in the world. Applied unless it matches what
    /// you have (`settings_key`), which is what a world's Welcome already gave you. Returns
    /// whether it was applied.
    pub fn apply_live_settings(&mut self, new: serde_json::Value) -> bool {
        let key = settings_key(&new);
        if self.settings_seen.as_ref() == Some(&key) {
            return false;
        }
        self.apply_settings(&new, false);
        self.set_settings(new);
        self.settings_seen = Some(key);
        true
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

    /// Into the doorway `portal` (to another world) from `side`: ask this world the way. It
    /// checks we're there and answers with where to go and a ticket for it
    /// ([`WorldClient::travelled`]). Not in a world (nothing to ask): straight there.
    pub fn through(&mut self, portal: &str, side: f32, url: &str, a: Arrival) {
        if !self.ready || self.asked.is_some() {
            if self.link.is_none() {
                self.travel(url, a);
            }
            return;
        }
        self.send(&ToServer::Through { portal: portal.into(), side: if side < 0.0 { -1 } else { 1 } });
        self.asked = Some((a, self.now()));
    }

    /// The world's answer to [`WorldClient::through`]: go to `url` with `ticket`, coming out at
    /// `arrive` (where the world there will put us).
    pub fn travelled(&mut self, url: &str, ticket: Option<String>, arrive: [f32; 3]) {
        if !arrive.iter().all(|v| v.is_finite()) {
            return;
        }
        let mut a = match self.asked.take() {
            Some((a, _)) => a,
            None => self.arrival(arrive, 0.0),
        };
        a.pos = arrive;
        self.ticket = ticket;
        self.travel(url, a);
    }

    /// At an edge of a tile that leads on: ask the world the way over it. Once per trip: an
    /// unanswered ask waits (see [`WorldClient::travel_check`]).
    pub fn edge_check(&mut self) {
        if self.edges.is_empty() || !self.ready || self.travel.is_some() || self.asked.is_some() {
            return;
        }
        let (b, p) = (self.view.confirmed.bounds, self.body.pos);
        let at = |e: &str| match e {
            "east" => p[0] >= b.hi[0] as f32 - 1.0,
            "west" => p[0] <= b.lo[0] as f32 + 1.0,
            "south" => p[2] >= b.hi[2] as f32 - 1.0,
            "north" => p[2] <= b.lo[2] as f32 + 1.0,
            _ => false,
        };
        if let Some(e) = self.edges.iter().find(|e| at(e)).cloned() {
            let a = self.arrival(p, 0.0);
            self.through(&format!("edge:{e}"), 1.0, "", a);
        }
    }

    /// The way through a portal was closed (no world answered): go back where you were.
    pub fn travel_check(&mut self) {
        // asked the way and no answer: this world didn't let us through
        if let Some((_, at)) = self.asked
            && self.now() - at > ASK_WITHIN
        {
            self.asked = None;
            self.notice("The way is closed");
        }
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
    pub(crate) fn stands_on(&self, c: [i32; 3]) -> bool {
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

    /// Everyone else in the world as (their number, their name), by name: for the voice HUD
    /// (who is speaking) and the menu's per-player mute.
    pub fn others_named(&self) -> Vec<(u32, String)> {
        let mut v: Vec<(u32, String)> = self.others.iter().map(|(id, o)| (*id, o.name.clone())).collect();
        v.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        v
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
        self.set_weather(None);
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

/// A world's settings as the Settings entity holds them: without the keys the server merges
/// into a Welcome's copy (`voice_radius`, `portals`), so the two compare equal when nothing
/// changed.
fn settings_key(v: &serde_json::Value) -> serde_json::Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("voice_radius");
        o.remove("portals");
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armed() -> BuildRepeat {
        let mut r = BuildRepeat::default();
        r.step((false, false), (false, false), 0.0, 0.0);
        r
    }

    /// Voice compares where you stand with where others were seen: both must be in blocks
    /// (poses carry metres), and the listener faces where the camera faces.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn voice_positions_are_in_blocks_and_the_listener_follows_the_camera() {
        let mut w = WorldClient::new(None, "Sam".into());
        w.body.pos = [100.0, 0.0, 100.0];
        w.cam_yaw = 1.25;
        w.yaw = 3.0; // the character's own heading must not matter
        let mut places = VecDeque::new();
        // one block away from the listener: 50.5 m is block 101 at half a metre a block
        places.push_back((1, Vec3::new(50.5, 0.0, 50.0), 0.0));
        w.others.insert(7, Other { name: "Ann".into(), entity: None, stream: Stream::default(), places });
        assert_eq!(w.place_of(7), Some([101.0, 0.0, 100.0]));
        assert_eq!(w.place_of(8), None);
        let (at, yaw) = w.listener();
        assert_eq!(at, [100.0, 0.0, 100.0]);
        assert_eq!(yaw, 1.25);
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

    fn settings_world() -> WorldClient {
        let mut w = WorldClient::new(None, "me".into());
        w.names = vec!["grass".into(), "dirt".into(), "stone".into()];
        w
    }

    #[test]
    fn settings_key_drops_only_the_keys_the_server_merges() {
        let s = serde_json::json!({"floor": -3, "voice_radius": 20.0, "portals": [], "sky": "dusk"});
        assert_eq!(settings_key(&s), serde_json::json!({"floor": -3, "sky": "dusk"}));
    }

    #[test]
    fn unchanged_settings_after_welcome_are_not_applied_again() {
        let mut w = settings_world();
        // what Welcome gives: the merged copy, with voice_radius and portals
        let welcome = serde_json::json!({"floor": -3, "hotbar": ["stone"], "voice_radius": 20.0, "portals": []});
        w.apply_settings(&welcome, true);
        w.settings_seen = Some(settings_key(&welcome));
        // the Settings entity itself has the same json, without the merged keys
        let entity = serde_json::json!({"floor": -3, "hotbar": ["stone"]});
        assert!(!w.apply_live_settings(entity), "identical: skipped");
        assert!(w.settings.is_none(), "nothing handed to the outdoors system");
    }

    #[test]
    fn changed_hotbar_is_applied_and_slot_stays_in_range() {
        let mut w = settings_world();
        w.hotbar = vec![1, 2, 3, 1, 2, 3];
        w.slot = 5;
        assert!(w.apply_live_settings(serde_json::json!({"hotbar": ["stone"]})));
        assert_eq!(w.hotbar(), &[3]);
        assert_eq!(w.slot, 0);

        w.hotbar = vec![1, 2, 3];
        w.slot = 2;
        assert!(w.apply_live_settings(serde_json::json!({"hotbar": ["grass", "dirt"]})));
        assert_eq!(w.hotbar(), &[1, 2]);
        assert_eq!(w.slot, 1);
    }

    #[test]
    fn live_change_without_voice_radius_keeps_it_and_the_camera() {
        let mut w = settings_world();
        w.voice_radius = 20.0;
        w.cam_yaw = 1.0;
        w.cam_pitch = -0.3;
        w.zoom = 13.0;
        assert!(w.apply_live_settings(serde_json::json!({"floor": 0, "camera": {"yaw": 90.0, "zoom": 30.0}})));
        assert_eq!(w.floor, Some(0), "applied");
        assert_eq!(w.voice_radius, 20.0, "not in the change, so unchanged");
        assert_eq!(w.cam_yaw, 1.0);
        assert_eq!(w.cam_pitch, -0.3);
        assert_eq!(w.zoom, 13.0);

        assert!(w.apply_live_settings(serde_json::json!({"voice_radius": 8.0})));
        assert_eq!(w.voice_radius, 8.0);
    }
}
