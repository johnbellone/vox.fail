//! vox-server: one shared build space.
//!
//! Players connect a WebSocket to `/ws` and speak [`vox_core::wire`]. One thread owns the
//! world (the blocks, the players, the edit order) and everything goes through it, so every
//! player sees the same edits in the same order. Blocks and an edit log are kept in SQLite,
//! written every few seconds and on shutdown.
//!
//! - `GET /` — a short status (world, players, blocks, edits) as JSON.
//! - `GET /ws` — a player's connection.
//!
//! The rules (bounds, palette, reach, rate) are vox-core's, so the app predicts edits
//! with the same checks the server makes.
//!
//! With a directory (`directory_url` and `world_secret`), the world reports each minute who
//! is there (signed-in players: where they stand, how many blocks they placed), which is how
//! players earn authority and raids are won, and fetches its rules every half minute: the
//! plots players claimed (only their builders may change blocks inside one) and the portals
//! they opened (kept as portal entities, which new arrivals get with the rest).

mod connection;
mod entities;
pub mod flags;
mod snapshot;
mod world;
#[allow(unused_imports)]
pub use connection::*;
#[allow(unused_imports)]
pub use world::*;
pub use snapshot::{Restore, SnapStore, list_snapshots, restore};

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use rusqlite::{Connection, params};
use tokio::sync::{mpsc, oneshot};
use vox_core::{
    Bounds, Chunk, ChunkKey, Why, World,
    weather::{Climate, Weather},
    wire::{self, Applied, MAX_MESSAGE, PROTOCOL, ToClient, ToServer},
    world::Bucket,
};

/// Messages waiting per player before they count as falling behind. A world is at most
/// 1024 chunks, all sent on joining.
const QUEUE: usize = 2048;
/// A pose blob is a skeleton's worth of joints; half of MAX_MESSAGE is generous.
const MAX_POSE_BLOB: usize = 32 * 1024;
/// Poses per second one connection may send (a client sends about 20).
const MAX_POSES_PER_SECOND: u32 = 60;
/// Voice packets per second one player may send (a talking client sends about 50), and the
/// burst allowed.
const MAX_VOICE_PER_SECOND: f32 = 60.0;
const VOICE_BURST: f32 = 100.0;
/// How long a new connection has to say hello.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// How far (in blocks, from the eyes) a player may edit.
pub const REACH: f32 = 16.0;
/// Eye height above the feet, in blocks.
const EYES: f32 = 3.2;

#[derive(Clone)]
pub struct Config {
    /// SQLite file; `:memory:` keeps nothing
    pub db: PathBuf,
    pub name: String,
    /// block names from `data/blocks.json`, in id order
    pub palette: Vec<String>,
    pub block_size: f32,
    /// a level file to fill a new world from
    pub seed: Option<String>,
    pub bounds: Bounds,
    pub max_players: usize,
    /// take players' positions as they say (test and automation worlds); otherwise moves
    /// faster than a player can go are sent back
    pub trust_moves: bool,
    /// where snapshots go (a folder; see vox-format): every `snapshot_every` and on shutdown,
    /// kept `keep_snapshots` seconds. None: no snapshots.
    pub snapshots: Option<PathBuf>,
    /// the `oxen` binary: snapshots are then commits in an Oxen repository (history older than
    /// `keep_snapshots` squashed away) instead of plain files. None: plain files.
    pub snapshot_oxen: Option<PathBuf>,
    pub snapshot_every: Duration,
    pub keep_snapshots: u64,
    /// how many may watch at once without playing (the front page's live view)
    pub max_watchers: usize,
    /// edits per second per player (and the burst allowed)
    pub edit_rate: f32,
    /// how far (blocks) speech carries to players nearby; must be finite and above zero
    pub voice_radius: f32,
    pub flush_every: Duration,
    /// the directory's public key: players must bring a sign-in token it signed, and go by
    /// the name in it. None: anyone may join under any name (a private or test world).
    pub directory: Option<vox_core::auth::VerifyingKey>,
    /// with a directory: also let people in without signing in, as guests
    pub guests: bool,
    /// the level it was made from (`void`), which portals to it show as their echo
    pub level: String,
    /// a tile of an atlas: a new world is filled with its generated ground (not `seed`), and its
    /// edges lead to the next tiles (`edge:east` and so on, through the directory like portals)
    pub tile: Option<vox_core::terrain::Tile>,
    /// the directory to report to and take plots and portals from (`http://127.0.0.1:7700`),
    /// with the secret it shares with its worlds
    pub directory_url: Option<String>,
    pub world_secret: Option<String>,
    pub report_every: Duration,
    pub rules_every: Duration,
}

impl Config {
    pub fn new(db: impl Into<PathBuf>, palette: Vec<String>) -> Self {
        Config {
            db: db.into(),
            name: "world".into(),
            palette,
            block_size: 0.5,
            seed: None,
            bounds: Bounds::default(),
            max_players: 100,
            trust_moves: false,
            snapshots: None,
            snapshot_oxen: None,
            snapshot_every: Duration::from_secs(300),
            keep_snapshots: vox_format::store::KEEP_SECS,
            max_watchers: 200,
            edit_rate: 20.0,
            voice_radius: 32.0,
            flush_every: Duration::from_secs(3),
            directory: None,
            guests: false,
            level: String::new(),
            tile: None,
            directory_url: None,
            world_secret: None,
            report_every: Duration::from_secs(60),
            rules_every: Duration::from_secs(30),
        }
    }
}

/// What `GET /` says.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub world: String,
    pub players: Vec<String>,
    pub blocks: usize,
    pub seq: u64,
    /// how many are watching without playing
    pub watchers: usize,
}

impl Status {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({ "world": self.world, "players": self.players, "blocks": self.blocks, "seq": self.seq })
    }
}

enum Event {
    Join { conn: u64, tx: mpsc::Sender<Message>, protocol: u32, name: String, avatar: Vec<u8>, account: Option<u64>, arrive: Option<[f32; 3]> },
    Watch { conn: u64, tx: mpsc::Sender<Message>, protocol: u32 },
    Msg { conn: u64, msg: ToServer },
    Leave { conn: u64 },
    Tick,
    Status(oneshot::Sender<Status>),
    /// who's here for the directory (and start counting edits again)
    Report(oneshot::Sender<serde_json::Value>),
    /// the directory's plots, portals and raid parties (lists of account ids)
    Rules(Vec<Plot>, Vec<serde_json::Value>, Vec<Vec<u64>>),
    /// the directory's answer for a trip: where to go, and the ticket (if it gave one)
    Ticket { conn: u64, url: String, ticket: Option<String>, arrive: [f32; 3] },
    Shutdown,
}

/// A trip through a portal to ask the directory a ticket for: who, where to (the portal's
/// url), arriving where.
pub(crate) struct Trip {
    pub conn: u64,
    pub account: u64,
    pub to: String,
    pub arrive: [f32; 3],
}

/// Trips waiting for the directory at most; past this a player goes on without a ticket.
const TRIPS_QUEUED: usize = 256;

/// A claimed area (all heights): only its builders' accounts may change blocks inside.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
    pub builders: Vec<u64>,
}

impl Plot {
    fn from_json(v: &serde_json::Value) -> Option<Plot> {
        let i = |k: &str| v.get(k)?.as_i64().map(|n| n as i32);
        Some(Plot {
            x0: i("x0")?,
            z0: i("z0")?,
            x1: i("x1")?,
            z1: i("z1")?,
            builders: v.get("builders")?.as_array()?.iter().filter_map(|b| b.as_u64()).collect(),
        })
    }

    fn holds(&self, cell: [i32; 3]) -> bool {
        (self.x0..self.x1).contains(&cell[0]) && (self.z0..self.z1).contains(&cell[2])
    }
}

/// A running world. Clone it freely; [`Server::shutdown`] saves and stops it.
#[derive(Clone)]
pub struct Server {
    tx: mpsc::Sender<Event>,
    /// who may join: the directory's key, and whether guests may too
    gate: Arc<Gate>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    next_conn: Arc<AtomicU64>,
}

impl Server {
    /// Open (or create) the world and start its thread. Call from inside a tokio runtime.
    pub fn start(cfg: Config) -> Result<Server, String> {
        let gate = Arc::new(Gate { key: cfg.directory, guests: cfg.guests, world: cfg.name.clone(), used: Mutex::new(HashMap::new()) });
        let office = match (&cfg.directory_url, &cfg.world_secret) {
            (Some(u), Some(s)) => Some((u.trim_end_matches('/').to_string(), s.clone(), cfg.name.clone())),
            _ => None,
        };
        let (trip_tx, trip_rx) = mpsc::channel(TRIPS_QUEUED);
        let (goal_tx, goal_rx) = mpsc::channel(GOALS_QUEUED);
        let link = match (&cfg.directory_url, &cfg.world_secret) {
            (Some(u), Some(s)) => Some((u.trim_end_matches('/').to_string(), s.clone(), cfg.name.clone(), cfg.report_every, cfg.rules_every)),
            _ => None,
        };
        // named after the world, so a host running many can tell them apart
        let thread_name = format!("world-{}", cfg.name);
        let mut state = WorldState::open(cfg)?;
        if office.is_some() {
            state.trips = Some(trip_tx);
            state.goals = Some(goal_tx);
        }
        let (tx, rx) = mpsc::channel(4096);
        let thread = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || state.run(rx))
            .map_err(|e| e.to_string())?;
        let ticker = tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if ticker.send(Event::Tick).await.is_err() {
                    break;
                }
            }
        });
        if let Some(link) = link {
            tokio::spawn(directory_link(tx.clone(), link));
        }
        if let Some((url, secret, world)) = office {
            tokio::spawn(objective_reached(goal_rx, url.clone(), secret.clone(), world.clone()));
            tokio::spawn(ticket_office(tx.clone(), trip_rx, url, secret, world));
        }
        Ok(Server { tx, gate, thread: Arc::new(Mutex::new(Some(thread))), next_conn: Arc::new(AtomicU64::new(1)) })
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(status_page))
            .route("/ws", get(connect))
            // behind a Cloudflare Tunnel, worlds share one address and are told apart by path
            // (vox.fail/w/7780/ws), which the tunnel passes on as it is
            .route("/w/{port}/", get(status_page))
            .route("/w/{port}/ws", get(connect))
            .with_state(self.clone())
    }

    /// Take a player's (or watcher's) WebSocket, as `GET /ws` does. For a host that routes
    /// connections to worlds itself (`vox-host`).
    pub fn accept(&self, ws: WebSocketUpgrade) -> Response {
        let server = self.clone();
        ws.max_message_size(MAX_MESSAGE).on_upgrade(move |socket| run_connection(server, socket))
    }

    /// This world's pages under `prefix` (`/w/void`): `<prefix>/` its status and `<prefix>/ws`
    /// a player's connection. For a host running several worlds on one port (`vox-host`).
    pub fn mount(&self, prefix: &str) -> Router {
        Router::new()
            .route(&format!("{prefix}/"), get(status_page))
            .route(&format!("{prefix}/ws"), get(connect))
            .with_state(self.clone())
    }

    pub async fn status(&self) -> Option<Status> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Event::Status(tx)).await.ok()?;
        rx.await.ok()
    }

    /// Save everything and stop. Players are disconnected.
    pub async fn shutdown(&self) {
        let _ = self.tx.send(Event::Shutdown).await;
        let thread = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(t) = thread {
            let _ = tokio::task::spawn_blocking(move || t.join()).await;
        }
    }
}

struct Player {
    id: u32,
    name: String,
    avatar: Vec<u8>,
    tx: mpsc::Sender<Message>,
    /// where they stand, as checked (only positions they could have reached count)
    at: Option<[f32; 3]>,
    moves: vox_core::moves::Moves,
    /// when they were last told to go back (at most twice a second)
    corrected_at: f64,
    bucket: Bucket,
    /// voice packets they may still send
    voice: Bucket,
    /// their directory account, when signed in
    account: Option<u64>,
    /// blocks they placed since the last report
    placed: u32,
}

struct WorldState {
    cfg: Config,
    db: Connection,
    world: World,
    seq: u64,
    players: HashMap<u64, Player>,
    /// connections watching without playing: they get what everyone sees, names left out
    watchers: HashMap<u64, mpsc::Sender<Message>>,
    dirty: HashSet<ChunkKey>,
    /// edits not written yet: (seq, at, player name, cell, old, new)
    log: Vec<(u64, u64, String, [i32; 3], u16, u16)>,
    spawn: [f32; 3],
    /// the world's settings (JSON: sky, time), from its seed level when it was made
    settings: String,
    started: Instant,
    last_flush: Instant,
    /// the air over the world, stepped twice a second and sent every 15; none where there's
    /// no air (`"weather": "none"`: orbit)
    weather: Option<Weather>,
    weather_at: Instant,
    keyframe_at: Instant,
    /// claimed plots, from the directory
    plots: Vec<Plot>,
    /// which raid party each signed-in account is in (index into the directory's list); an
    /// account listed in two parties keeps the first
    party_of: HashMap<u64, u32>,
    /// snapshots: the store, the last one taken, zones changed since, and whether the next
    /// one must hold every zone (no usable last one)
    snaps: Option<vox_format::Snapshots<SnapStore>>,
    snap_prev: Option<vox_format::Manifest>,
    snap_changed: HashSet<vox_format::ZoneKey>,
    snap_full: bool,
    last_snapshot: Instant,
    /// where trips go to get a ticket from the directory (None: no directory; players go on
    /// without one)
    trips: Option<mpsc::Sender<Trip>>,
    /// the place to reach (the world's Objective entity, see `refresh_from_entities`), and
    /// whether it has been; reaching it ends a raid on the dungeon
    objective: Option<Objective>,
    reached: bool,
    /// where "the objective was reached, by these accounts" goes (to the directory)
    goals: Option<mpsc::Sender<Vec<u64>>>,
    /// the world's entities (doorways, spawn, objective, settings), written with the blocks
    entities: entities::Entities,
    /// the settings the ground, climate and sun were last built from
    applied: serde_json::Value,
    /// the settings moved on from what `meta('settings')` holds, so the next flush writes them
    /// (a flush with nothing else dirty would otherwise skip them)
    settings_unsaved: bool,
}

/// A place in a world to reach: a dungeon's objective.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Objective {
    pub name: String,
    /// blocks (feet)
    pub at: [f32; 3],
    /// how near counts, blocks
    pub radius: f32,
}

impl Objective {
    fn holds(&self, p: [f32; 3]) -> bool {
        (0..3).map(|i| (p[i] - self.at[i]).powi(2)).sum::<f32>() <= self.radius * self.radius
    }
}

/// Objectives reached waiting for the directory at most.
const GOALS_QUEUED: usize = 16;

/// Steps and keyframes of the weather, s.
const WEATHER_STEP: f32 = 0.5;
const KEYFRAME_EVERY: f32 = 15.0;

/// How strongly the sun heats the ground at a time of day (0 at night, 1 at noon).
fn sun_at(hours: f32) -> f32 {
    let phase = (hours - 6.0) / 24.0 * std::f32::consts::TAU;
    (phase.sin() * 0.88).clamp(0.0, 1.0)
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
