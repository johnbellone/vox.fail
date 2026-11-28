//! vox-server: one shared build space.
//!
//! Players connect a WebSocket to `/ws` and speak [`vox_core::wire`]. One thread owns the
//! world (the blocks, the players, the edit order) and everything goes through it, so every
//! player sees the same edits in the same order. Blocks and an edit log are kept in SQLite,
//! written every few seconds and on shutdown.
//!
//! - `GET /` — a short status (world, players, blocks, edits) as JSON.
//! - `GET /ws` — a player's connection.
//! - `GET /link` — a neighbouring world's link (with a world secret; see neighbours.rs).
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
mod neighbours;
mod props;
mod rescale;
mod snapshot;
mod world;
#[allow(unused_imports)]
pub use connection::*;
#[allow(unused_imports)]
pub use world::*;
pub use neighbours::Neighbour;
pub use snapshot::{Restore, SnapStore, list_snapshots, restore};

use std::{
    collections::{HashMap, HashSet, VecDeque},
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
use tokio::sync::{Notify, mpsc, oneshot, watch};
use vox_core::{
    Bounds, Chunk, ChunkKey, Why, World,
    edit::Role,
    entity::{Component, EntityId},
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
/// Entity edits (`Put`, `Remove`) per second per player, and the burst allowed.
const EDIT_RATE: f32 = 10.0;
/// How long a directory answer on advanced rights (world editors) is kept. Negative answers
/// are kept as long as positive ones: a newly granted editor waits up to this long for
/// advanced edits, and a revoked one keeps them up to this long. Failures (no answer, a non-200,
/// a timeout) are not kept at all, so the next edit asks again.
const EDITOR_CACHE: Duration = Duration::from_secs(5);
/// Rights questions waiting for the directory at most; past this an edit is refused.
const ASKS_QUEUED: usize = 256;
/// Player portal calls to the directory waiting at most; past this an edit is refused.
const PORTAL_ASKS_QUEUED: usize = 256;

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
    /// the folder of accepted props: `catalogue.json` and each prop's `<hash>.voxp` (see props.rs)
    pub props: PathBuf,
    /// every signed-in player is an editor (advanced edits), with no question to the directory,
    /// and so is every guest (judged as a local account, see `world::guest_account`). For local
    /// play and tests only: a live world asks the directory who may edit, and a world with a
    /// directory refuses this flag. Without it, guests may not edit.
    pub editors_all: bool,
    /// where this world's host answers, for dialling its neighbours (`ws://127.0.0.1:7790`): a
    /// lone server's own `--listen`, a host's. None: no links to neighbours. Links also need
    /// `world_secret`, which both ends prove they know.
    pub link_base: Option<String>,
    /// the host is a `vox-host`: a doorway's `":7781"` is the alias of a world on `link_base`
    /// (`/w/7781/link`), not another server on that port of this machine
    pub link_hosted: bool,
}

impl Config {
    pub fn new(db: impl Into<PathBuf>, palette: Vec<String>) -> Self {
        Config {
            db: db.into(),
            name: "world".into(),
            palette,
            block_size: vox_core::BLOCK_M,
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
            voice_radius: vox_core::in_blocks(16.0),
            flush_every: Duration::from_secs(3),
            directory: None,
            guests: false,
            level: String::new(),
            tile: None,
            directory_url: None,
            world_secret: None,
            report_every: Duration::from_secs(60),
            rules_every: Duration::from_secs(30),
            editors_all: false,
            props: PathBuf::from("data/props"),
            link_base: None,
            link_hosted: false,
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
    /// the directory's plots, portals and raid parties (lists of account ids), and when the fetch
    /// that got them began (a portal list that began before the world's last keyed-portal change
    /// predates it, and is not applied)
    Rules(Vec<Plot>, Vec<serde_json::Value>, Vec<Vec<u64>>, Instant),
    /// the directory's answer for a trip: where to go, and the ticket (if it gave one)
    Ticket { conn: u64, url: String, ticket: Option<String>, arrive: [f32; 3], motion: vox_core::travel::Motion, frame: vox_core::travel::Frame },
    /// the directory's answer on whether an account may make advanced edits (None: no answer,
    /// which refuses the edit)
    Editor { conn: u64, account: u64, editor: Option<bool> },
    /// the directory's answer to a player's portal call (`status` None: no answer in time)
    Portal { conn: u64, account: u64, call: PortalCall, status: Option<u16>, body: serde_json::Value },
    Shutdown,
}

/// A player's entity edit waiting for the directory to say whether they may make advanced
/// edits.
#[derive(Debug)]
pub(crate) enum Edit {
    /// place entity `entity` (None: a new one) as `component`
    Put { entity: Option<EntityId>, expect: Option<u64>, component: Component },
    /// remove entity `entity`
    Remove { entity: EntityId, expect: Option<u64> },
    /// build the node `node`: its recipe's blocks are applied a tick at a time (`world.rs`)
    Build { node: EntityId },
}

/// A build accepted and not yet finished: the blocks still to apply, in order, and who asked
/// for it. The blocks are made by `player` (their name goes in the edit log), as ordinary edits
/// are. A build keeps going when its builder leaves; the answer is then sent to nobody.
#[derive(Debug)]
pub(crate) struct BuildJob {
    /// the builder's connection and request id, for the answer
    pub conn: u64,
    pub id: u32,
    /// the node built (its entity id, for the answer)
    pub node: EntityId,
    /// the builder's player id and name, as their edits are made
    pub player: u32,
    pub name: String,
    /// the builder's directory account, when signed in (logged with their edits)
    pub account: Option<u64>,
    /// blocks left to apply: their cells and block ids, in order
    pub blocks: VecDeque<([i32; 3], u16)>,
    /// the edit number of the last block applied (or the number at the start, if none was)
    pub last_seq: u64,
    /// set when the job undoes a build (`ToServer::Unbuild`): then `blocks` holds the old blocks
    /// to put back and this the rest of what to check; None for a build
    pub undo: Option<Undo>,
    /// every block a build changed, as (cell, old, new), kept for its undo when it finishes
    /// (not filled for an undo)
    pub changed: Vec<([i32; 3], u16, u16)>,
}

/// What an undo of a build carries beside its queue of old blocks.
#[derive(Debug)]
pub(crate) struct Undo {
    /// the block the build put in each queued cell, in the queue's order: the old block goes back
    /// only where the cell still holds this
    pub expect: VecDeque<u16>,
    /// cells restored so far
    pub restored: usize,
    /// cells left as they are: changed since, or in a plot the builder has no part in now
    pub left: usize,
}

/// A finished build kept for its builder's session, so it can be undone.
#[derive(Debug)]
pub(crate) struct KeptBuild {
    /// the client's number for the build (its edit id)
    pub build: u32,
    /// the blocks it changed: cell, the block before, the block it put there
    pub changed: Vec<([i32; 3], u16, u16)>,
}

/// Builds kept per connection for undo, the oldest dropped first. A build is up to 32,768
/// blocks, so this bounds what one connection holds.
pub(crate) const KEPT_BUILDS: usize = 8;

/// An edit held while the directory answers, with the request id to refuse it under, and the
/// role it is judged as. A held edit for advanced rights (`editor_answer`) has no role yet: the
/// directory's answer sets it, so the placeholder there is never read.
#[derive(Debug)]
pub(crate) struct Pending {
    pub id: u32,
    pub edit: Edit,
    pub role: Role,
}

/// A directory call about a player's portal: a new one is charged (`Make`), a keyed one is moved
/// (`Update`), given another destination (`Link`) or removed (`Remove`); `Refund` gives a charge
/// back for a portal that was not placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortalCall {
    Make,
    Update,
    Link,
    Remove,
    Refund,
}

/// A call for the directory's portal endpoints. `conn` is the player waiting for the answer
/// (None: nobody waits, as for a refund).
pub(crate) struct PortalAsk {
    pub conn: Option<u64>,
    pub account: u64,
    pub call: PortalCall,
    /// the call's fields (the world and the account are added when it is sent)
    pub body: serde_json::Value,
}

/// A question for the directory: may this account make advanced edits here? The answer comes
/// back as [`Event::Editor`] for this connection.
pub(crate) struct Asking {
    pub conn: u64,
    pub account: u64,
}

/// A trip through a portal to ask the directory a ticket for: who, where to (the portal's
/// url), arriving where, moving how (in the arriving world's frame), and how the two worlds
/// meet.
pub(crate) struct Trip {
    pub conn: u64,
    pub account: u64,
    pub to: String,
    pub arrive: [f32; 3],
    pub motion: vox_core::travel::Motion,
    pub frame: vox_core::travel::Frame,
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
    /// The same area as vox-core's rules see it (for the entity checks).
    fn area(&self) -> vox_core::edit::PlotArea {
        vox_core::edit::PlotArea { x0: self.x0, z0: self.z0, x1: self.x1, z1: self.z1, builders: self.builders.clone() }
    }

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
    /// the neighbours and which are linked
    links: Arc<neighbours::Links>,
}

impl Server {
    /// Open (or create) the world and start its thread. Call from inside a tokio runtime.
    pub fn start(cfg: Config) -> Result<Server, String> {
        // as flags.rs refuses it: every account an editor is for local play only
        if cfg.editors_all && (cfg.directory_url.is_some() || cfg.directory.is_some()) {
            return Err("editors_all is for local play and tests; not with a directory".into());
        }
        let gate = Arc::new(Gate { key: cfg.directory, guests: cfg.guests, world: cfg.name.clone(), used: Mutex::new(HashMap::new()) });
        let office = match (&cfg.directory_url, &cfg.world_secret) {
            (Some(u), Some(s)) => Some((u.trim_end_matches('/').to_string(), s.clone(), cfg.name.clone())),
            _ => None,
        };
        let (trip_tx, trip_rx) = mpsc::channel(TRIPS_QUEUED);
        let (goal_tx, goal_rx) = mpsc::channel(GOALS_QUEUED);
        let (ask_tx, ask_rx) = mpsc::channel(ASKS_QUEUED);
        let (portal_tx, portal_rx) = mpsc::channel(PORTAL_ASKS_QUEUED);
        // woken when the directory has changed its portals, so the rules are fetched at once
        let rules_now = Arc::new(Notify::new());
        let link = match (&cfg.directory_url, &cfg.world_secret) {
            (Some(u), Some(s)) => Some((u.trim_end_matches('/').to_string(), s.clone(), cfg.name.clone(), cfg.report_every, cfg.rules_every)),
            _ => None,
        };
        // named after the world, so a host running many can tell them apart
        let thread_name = format!("world-{}", cfg.name);
        // links to neighbours need the secret (both ends prove they know it) and an address to
        // dial them from
        let linking = cfg.world_secret.is_some() && cfg.link_base.is_some();
        let links = Arc::new(neighbours::Links::new(cfg.name.clone(), cfg.world_secret.clone().filter(|_| linking)));
        let mut state = WorldState::open(cfg)?;
        let plan = linking.then(|| {
            let (plan_tx, plan_rx) = watch::channel(Vec::new());
            state.plan = Some(plan_tx);
            // the first plan now, so a neighbour dialling at once finds us knowing it
            state.send_plan();
            plan_rx
        });
        if office.is_some() {
            state.trips = Some(trip_tx);
            state.goals = Some(goal_tx);
            state.editors = Some(ask_tx);
            state.portals = Some(portal_tx);
            state.rules_now = Some(rules_now.clone());
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
            tokio::spawn(directory_link(tx.clone(), link, rules_now.clone()));
        }
        if let Some(plan) = plan {
            tokio::spawn(neighbours::keep_links(links.clone(), plan, tx.clone()));
        }
        if let Some((url, secret, world)) = office {
            tokio::spawn(objective_reached(goal_rx, url.clone(), secret.clone(), world.clone()));
            tokio::spawn(editor_office(tx.clone(), ask_rx, url.clone(), secret.clone(), world.clone()));
            tokio::spawn(portal_office(tx.clone(), portal_rx, rules_now, url.clone(), secret.clone(), world.clone()));
            tokio::spawn(ticket_office(tx.clone(), trip_rx, url, secret, world));
        }
        Ok(Server { tx, gate, thread: Arc::new(Mutex::new(Some(thread))), next_conn: Arc::new(AtomicU64::new(1)), links })
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(status_page))
            .route("/ws", get(connect))
            .route("/link", get(link))
            // behind a Cloudflare Tunnel, worlds share one address and are told apart by path
            // (vox.fail/w/7780/ws), which the tunnel passes on as it is
            .route("/w/{port}/", get(status_page))
            .route("/w/{port}/ws", get(connect))
            .route("/w/{port}/link", get(link))
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
            .route(&format!("{prefix}/link"), get(link))
            .with_state(self.clone())
    }

    /// Take a neighbouring world's link, as `GET /link` does. For `vox-host`.
    pub fn accept_link(&self, ws: WebSocketUpgrade) -> Response {
        let (links, tx) = (self.links.clone(), self.tx.clone());
        ws.max_message_size(vox_core::neighbour::MAX_LINK_MESSAGE).on_upgrade(move |socket| neighbours::listen(links, tx, socket))
    }

    /// This world's neighbours (atlas tiles over open edges, doorways' worlds on this host),
    /// and which are linked now.
    pub fn neighbours(&self) -> Vec<Neighbour> {
        self.links.list()
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
    /// entity edits they may still make (a rate of their own, apart from block edits)
    edits: Bucket,
    /// the entity edit held while the directory answers on advanced rights (one at a time)
    pending: Option<Pending>,
    /// their last finished builds, oldest first, for `ToServer::Unbuild` (dropped with them)
    kept: VecDeque<KeptBuild>,
}

/// One block edit in the log: (seq, at, player name, account, cell, old, new).
type BlockEdit = (u64, u64, String, Option<u64>, [i32; 3], u16, u16);

struct WorldState {
    cfg: Config,
    db: Connection,
    world: World,
    seq: u64,
    players: HashMap<u64, Player>,
    /// connections watching without playing: they get what everyone sees, names left out
    watchers: HashMap<u64, mpsc::Sender<Message>>,
    dirty: HashSet<ChunkKey>,
    /// edits not written yet
    log: Vec<BlockEdit>,
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
    /// the props the world accepts, read when it opened (props.rs)
    props: props::PropLibrary,
    /// where questions on advanced rights go (to the directory); None: no directory to ask
    editors: Option<mpsc::Sender<Asking>>,
    /// where player portal calls go (to the directory); None: no directory, so no player portals
    portals: Option<mpsc::Sender<PortalAsk>>,
    /// where each keyed portal leads (its rules row), by key, rebuilt on every rules fetch
    portal_routes: HashMap<i64, entities::Route>,
    /// refunds that could not be queued, tried once more on the next tick
    refunds_due: Vec<(u64, i64)>,
    /// when a keyed portal was last made, moved, linked or removed here (the directory has
    /// changed its list): a rules fetch that began before it is not applied to the portals
    last_keyed_commit: Option<Instant>,
    /// woken after a keyed-portal change here, so the rules are fetched again at once and the
    /// fetch that follows is one that began after the change (None: no directory)
    rules_now: Option<Arc<Notify>>,
    /// the directory's answers on advanced rights, by account, with when they came
    editor_cache: HashMap<u64, (Instant, bool)>,
    /// the settings the ground, climate and sun were last built from
    applied: serde_json::Value,
    /// the world's palette as a vox-art palette (one colour each, in block id order) and each
    /// name's block id, made once at open. None when the palette is not one vox-art takes, and
    /// then builds are refused.
    build_palette: Option<(vox_art::Palette, HashMap<String, u16>)>,
    /// builds accepted and not finished, the first being applied
    builds: VecDeque<BuildJob>,
    /// when the last build was started (one per ten seconds per world; a build that is refused
    /// after expansion still counts, so the expansion itself is limited too)
    last_build: Option<Instant>,
    /// the settings moved on from what `meta('settings')` holds, so the next flush writes them
    /// (a flush with nothing else dirty would otherwise skip them)
    settings_unsaved: bool,
    /// where the world's neighbours go to the link task (None: no links), and when they were
    /// last worked out
    plan: Option<watch::Sender<Vec<neighbours::Target>>>,
    plan_at: Instant,
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

/// Asking the directory about player portals for the world thread (a charge, a change or a
/// removal of one, a refund). Each call runs on its own task, so one slow answer holds nobody
/// up, and gives up after five seconds (no answer). The answer goes back to the player as
/// `Event::Portal`. A refund has no one waiting: a failed one is only logged.
async fn portal_office(tx: mpsc::Sender<Event>, mut asks: mpsc::Receiver<PortalAsk>, rules_now: Arc<Notify>, url: String, secret: String, world: String) {
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("no link to the directory for portals: {e}"),
    };
    while let Some(mut ask) = asks.recv().await {
        let (http, tx, rules_now, url, secret, world) = (http.clone(), tx.clone(), rules_now.clone(), url.clone(), secret.clone(), world.clone());
        tokio::spawn(async move {
            let path = match ask.call {
                PortalCall::Make => "portal",
                PortalCall::Update => "portal/update",
                PortalCall::Link => "portal/link",
                PortalCall::Remove => "portal/remove",
                PortalCall::Refund => "portal/refund",
            };
            // the world and the account are the world's own words, not the player's
            ask.body["world"] = serde_json::json!(world);
            ask.body["account"] = serde_json::json!(ask.account);
            // a refund that got no 200 is asked once more after a second: a charge given back
            // by a directory that was briefly away must not be lost
            let mut attempts = 0;
            let (status, reply) = loop {
                attempts += 1;
                let got = http.post(format!("{url}/api/world/{path}")).bearer_auth(&secret).json(&ask.body).send().await;
                let (status, reply) = match got {
                    Ok(r) => {
                        let status = r.status().as_u16();
                        (Some(status), r.json::<serde_json::Value>().await.unwrap_or(serde_json::Value::Null))
                    }
                    Err(e) => {
                        eprintln!("can't reach the directory for a portal: {e}");
                        (None, serde_json::Value::Null)
                    }
                };
                if ask.call == PortalCall::Refund && attempts == 1 && status != Some(200) {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
                break (status, reply);
            };
            // the directory's list has changed (or, for a refund, its charge): the world fetches
            // the rules now rather than at its next time, so a new portal can be walked through
            let changed = match ask.call {
                PortalCall::Refund => status == Some(200) && reply["refunded"].as_bool() == Some(true),
                _ => status == Some(200),
            };
            if changed {
                rules_now.notify_one();
            }
            match ask.conn {
                Some(conn) => {
                    let _ = tx.send(Event::Portal { conn, account: ask.account, call: ask.call, status, body: reply }).await;
                }
                None => {
                    if ask.call == PortalCall::Refund && !changed {
                        eprintln!("the directory didn't refund portal {} to account {} ({status:?})", ask.body["key"], ask.account);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `editors_all` (every account an editor) is for local play only: a world with a directory
    /// URL or a directory key refuses to start with it, as flags.rs does.
    #[test]
    fn editors_all_is_refused_with_a_directory() {
        let mut c = Config::new(":memory:", vec![]);
        c.editors_all = true;
        c.directory_url = Some("http://127.0.0.1:7700".into());
        assert_eq!(Server::start(c).err().as_deref(), Some("editors_all is for local play and tests; not with a directory"));
        let mut c = Config::new(":memory:", vec![]);
        c.editors_all = true;
        c.directory = Some(vox_core::auth::key_from_seed([9; 32]).verifying_key());
        assert!(Server::start(c).is_err());
    }
}
