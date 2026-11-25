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
//! they opened (added to the settings new arrivals get).

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
    wire::{self, Applied, PROTOCOL, ToClient, ToServer},
    world::Bucket,
};

/// Messages waiting per player before they count as falling behind. A world is at most
/// 1024 chunks, all sent on joining.
const QUEUE: usize = 2048;
/// Largest message a player may send.
const MAX_MESSAGE: usize = 64 * 1024;
/// How long a new connection has to say hello.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// How far (in blocks, from the eyes) a player may edit.
pub const REACH: f32 = 16.0;
/// Eye height above the feet, in blocks.
const EYES: f32 = 3.2;

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
    /// edits per second per player (and the burst allowed)
    pub edit_rate: f32,
    pub flush_every: Duration,
    /// the directory's public key: players must bring a sign-in token it signed, and go by
    /// the name in it. None: anyone may join under any name (a private or test world).
    pub directory: Option<vox_core::auth::VerifyingKey>,
    /// with a directory: also let people in without signing in, as guests
    pub guests: bool,
    /// the level it was made from (`void`), which portals to it show as their echo
    pub level: String,
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
            edit_rate: 20.0,
            flush_every: Duration::from_secs(3),
            directory: None,
            guests: false,
            level: String::new(),
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
}

impl Status {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({ "world": self.world, "players": self.players, "blocks": self.blocks, "seq": self.seq })
    }
}

enum Event {
    Join { conn: u64, tx: mpsc::Sender<Message>, protocol: u32, name: String, avatar: Vec<u8>, account: Option<u64> },
    Msg { conn: u64, msg: ToServer },
    Leave { conn: u64 },
    Tick,
    Status(oneshot::Sender<Status>),
    /// who's here for the directory (and start counting edits again)
    Report(oneshot::Sender<serde_json::Value>),
    /// the directory's plots and portals
    Rules(Vec<Plot>, Vec<serde_json::Value>),
    Shutdown,
}

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
    gate: Arc<(Option<vox_core::auth::VerifyingKey>, bool)>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    next_conn: Arc<AtomicU64>,
}

impl Server {
    /// Open (or create) the world and start its thread. Call from inside a tokio runtime.
    pub fn start(cfg: Config) -> Result<Server, String> {
        let gate = Arc::new((cfg.directory, cfg.guests));
        let link = match (&cfg.directory_url, &cfg.world_secret) {
            (Some(u), Some(s)) => Some((u.trim_end_matches('/').to_string(), s.clone(), cfg.name.clone(), cfg.report_every, cfg.rules_every)),
            _ => None,
        };
        let state = WorldState::open(cfg)?;
        let (tx, rx) = mpsc::channel(4096);
        let thread = std::thread::Builder::new()
            .name("world".into())
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

    pub async fn status(&self) -> Option<Status> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Event::Status(tx)).await.ok()?;
        rx.await.ok()
    }

    /// Save everything and stop. Players are disconnected.
    pub async fn shutdown(&self) {
        let _ = self.tx.send(Event::Shutdown).await;
        let thread = self.thread.lock().unwrap().take();
        if let Some(t) = thread {
            let _ = tokio::task::spawn_blocking(move || t.join()).await;
        }
    }
}

async fn status_page(State(server): State<Server>) -> Json<serde_json::Value> {
    Json(server.status().await.map(|s| s.json()).unwrap_or(serde_json::json!({ "error": "stopped" })))
}

async fn connect(State(server): State<Server>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |socket| run_connection(server, socket))
}

fn clean(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(max).collect()
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// May this player in, and under what name? With a directory key, a good token gives their
/// name and account; without one they're a guest (if guests are let in). Without a key, any
/// name goes.
fn admit(
    gate: &(Option<vox_core::auth::VerifyingKey>, bool),
    name: String,
    token: Option<&str>,
    now: u64,
) -> Result<(String, Option<u64>), String> {
    let Some(key) = &gate.0 else { return Ok((name, None)) };
    match token {
        Some(t) => vox_core::auth::verify(t, key, now).map(|c| (c.name, Some(c.account))),
        None if gate.1 => {
            let n = clean(&name, 24);
            Ok((if n.is_empty() { "guest".into() } else { format!("{n} (guest)") }, None))
        }
        None => Err("this world needs you to sign in first".into()),
    }
}

async fn run_connection(server: Server, socket: WebSocket) {
    let conn = server.next_conn.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    // the first message must be a hello
    let hello = tokio::time::timeout(HELLO_WITHIN, async {
        while let Some(Ok(m)) = stream.next().await {
            if let Message::Binary(b) = m {
                return wire::decode::<ToServer>(&b).ok();
            }
        }
        None
    })
    .await;
    let Ok(Some(ToServer::Hello { protocol, name, avatar, token })) = hello else {
        let bye = wire::encode(&ToClient::Bye { why: "say hello first".into() });
        let _ = sink.send(Message::Binary(bye.into())).await;
        return;
    };
    // who they are: from their sign-in, when this world checks it
    let (name, account) = match admit(&server.gate, name, token.as_deref(), unix_now()) {
        Ok(x) => x,
        Err(why) => {
            let bye = wire::encode(&ToClient::Bye { why });
            let _ = sink.send(Message::Binary(bye.into())).await;
            return;
        }
    };
    let (tx, mut rx) = mpsc::channel::<Message>(QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let name = clean(&name, 32);
    if server.tx.send(Event::Join { conn, tx, protocol, name, avatar, account }).await.is_err() {
        return;
    }
    while let Some(Ok(m)) = stream.next().await {
        match m {
            Message::Binary(b) => match wire::decode::<ToServer>(&b) {
                Ok(ToServer::Hello { .. }) | Err(_) => break,
                Ok(msg) => {
                    if server.tx.send(Event::Msg { conn, msg }).await.is_err() {
                        break;
                    }
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
        if writer.is_finished() {
            break;
        }
    }
    let _ = server.tx.send(Event::Leave { conn }).await;
    let _ = writer.await;
}

/// Talking to the directory: rules every `rules_every`, a report every `report_every`. A
/// directory that's down is tried again next time; the world goes on meanwhile.
async fn directory_link(tx: mpsc::Sender<Event>, (url, secret, world, report_every, rules_every): (String, String, String, Duration, Duration)) {
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("no link to the directory: {e}"),
    };
    let mut next_report = Instant::now() + report_every;
    let mut next_rules = Instant::now();
    let mut said = false;
    loop {
        if Instant::now() >= next_rules {
            next_rules = Instant::now() + rules_every;
            let got = http.get(format!("{url}/api/world/{world}/rules")).bearer_auth(&secret).send().await;
            match got {
                Ok(r) if r.status().is_success() => {
                    if let Ok(v) = r.json::<serde_json::Value>().await {
                        let plots = v["plots"].as_array().map(|a| a.iter().filter_map(Plot::from_json).collect()).unwrap_or_default();
                        let portals = v["portals"].as_array().cloned().unwrap_or_default();
                        if tx.send(Event::Rules(plots, portals)).await.is_err() {
                            return;
                        }
                        said = false;
                    }
                }
                Ok(r) if !said => {
                    eprintln!("the directory said {} to this world ({world}): check --directory-url, --name and VOX_WORLD_SECRET", r.status());
                    said = true;
                }
                Err(e) if !said => {
                    eprintln!("can't reach the directory at {url}: {e}");
                    said = true;
                }
                _ => {}
            }
        }
        if Instant::now() >= next_report {
            next_report = Instant::now() + report_every;
            let (rtx, rrx) = oneshot::channel();
            if tx.send(Event::Report(rtx)).await.is_err() {
                return;
            }
            if let Ok(body) = rrx.await {
                let _ = http.post(format!("{url}/api/world/report")).bearer_auth(&secret).json(&body).send().await;
            }
        }
        let wake = next_rules.min(next_report);
        tokio::time::sleep(wake.saturating_duration_since(Instant::now()).max(Duration::from_millis(20))).await;
    }
}

struct Player {
    id: u32,
    name: String,
    avatar: Vec<u8>,
    tx: mpsc::Sender<Message>,
    at: Option<[f32; 3]>,
    bucket: Bucket,
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
    dirty: HashSet<ChunkKey>,
    /// edits not written yet: (seq, at, player name, cell, old, new)
    log: Vec<(u64, u64, String, [i32; 3], u16, u16)>,
    spawn: [f32; 3],
    /// the world's settings (JSON: sky, time), from its seed level when it was made
    settings: String,
    started: Instant,
    last_flush: Instant,
    /// the air over the world, stepped twice a second and sent every 15
    weather: Weather,
    weather_at: Instant,
    keyframe_at: Instant,
    /// claimed plots and player-opened portals, from the directory
    plots: Vec<Plot>,
    portals: Vec<serde_json::Value>,
}

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

impl WorldState {
    fn open(mut cfg: Config) -> Result<WorldState, String> {
        let e = |e: rusqlite::Error| e.to_string();
        let db = Connection::open(&cfg.db).map_err(e)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS chunks(x INTEGER, y INTEGER, z INTEGER, version INTEGER NOT NULL,
                 data BLOB NOT NULL, PRIMARY KEY(x, y, z));
             CREATE TABLE IF NOT EXISTS edits(seq INTEGER PRIMARY KEY, at INTEGER NOT NULL, player TEXT NOT NULL,
                 x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, old INTEGER NOT NULL, new INTEGER NOT NULL);",
        )
        .map_err(e)?;
        let meta = |k: &str| -> Option<String> {
            db.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0)).ok()
        };
        // the world keeps the palette it was made with (ids are stored in its chunks); new
        // blocks are added at the end
        let fresh = meta("palette").is_none();
        if let Some(stored) = meta("palette") {
            let mut names: Vec<String> = serde_json::from_str(&stored).map_err(|e| e.to_string())?;
            for n in &cfg.palette {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
            cfg.palette = names;
        }
        let seq: u64 = meta("seq").and_then(|s| s.parse().ok()).unwrap_or(0);
        let settings = match meta("settings") {
            Some(s) if !fresh => s,
            _ => cfg.seed.as_deref().map(vox_core::level::settings).unwrap_or_else(|| "{}".into()),
        };
        let mut world = World::new(cfg.bounds, cfg.palette.len() as u16);
        let mut dirty = HashSet::new();
        if fresh {
            if let Some(level) = &cfg.seed {
                vox_core::level::fill(&mut world, level, &cfg.palette)?;
                dirty.extend(world.keys());
            }
        } else {
            let mut q = db.prepare("SELECT x, y, z, version, data FROM chunks").map_err(e)?;
            let rows = q
                .query_map([], |r| {
                    Ok((ChunkKey([r.get(0)?, r.get(1)?, r.get(2)?]), r.get::<_, u32>(3)?, r.get::<_, Vec<u8>>(4)?))
                })
                .map_err(e)?;
            for row in rows {
                let (k, version, data) = row.map_err(e)?;
                world.put_chunk(k, Chunk::decode(&data, version)?);
            }
        }
        let spawn = match world.ground(0, 0, cfg.bounds.hi[1] - 1) {
            Some(y) => [0.5, y as f32, 0.5],
            None => [0.5, 0.0, 0.5],
        };
        let set: serde_json::Value = serde_json::from_str(&settings).unwrap_or_default();
        let ground = set.get("floor").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32 * cfg.block_size;
        let climate = match meta("climate").and_then(|c| serde_json::from_str::<Climate>(&c).ok()) {
            Some(c) if !fresh => c.sane(),
            _ => Climate::from_settings(set.get("weather"), ground),
        };
        let seed = cfg.name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
        let mut weather = Weather::new(climate, seed);
        weather.sun = match set.get("sky").and_then(|v| v.as_str()) {
            Some("earth") | None => sun_at(set.get("time").and_then(|v| v.as_f64()).unwrap_or(15.0) as f32),
            _ => 0.5,
        };
        // let the air settle before anyone arrives
        weather.step(120.0);
        let mut s = WorldState {
            cfg,
            db,
            world,
            seq,
            players: HashMap::new(),
            dirty,
            log: Vec::new(),
            spawn,
            settings,
            started: Instant::now(),
            last_flush: Instant::now(),
            weather,
            weather_at: Instant::now(),
            keyframe_at: Instant::now(),
            plots: Vec::new(),
            portals: Vec::new(),
        };
        if fresh {
            s.flush()?;
        }
        Ok(s)
    }

    fn run(mut self, mut rx: mpsc::Receiver<Event>) {
        while let Some(ev) = rx.blocking_recv() {
            match ev {
                Event::Join { conn, tx, protocol, name, avatar, account } => self.join(conn, tx, protocol, name, avatar, account),
                Event::Msg { conn, msg } => self.message(conn, msg),
                Event::Leave { conn } => self.leave(conn),
                Event::Tick => {
                    let behind = self.weather_at.elapsed().as_secs_f32();
                    if behind >= WEATHER_STEP {
                        self.weather.step(behind.min(5.0));
                        self.weather_at = Instant::now();
                    }
                    if self.keyframe_at.elapsed().as_secs_f32() >= KEYFRAME_EVERY {
                        self.send_weather();
                    }
                    if self.last_flush.elapsed() >= self.cfg.flush_every {
                        self.save();
                    }
                }
                Event::Status(reply) => {
                    let mut players: Vec<_> = self.players.values().map(|p| p.name.clone()).collect();
                    players.sort();
                    let _ = reply.send(Status {
                        world: self.cfg.name.clone(),
                        players,
                        blocks: self.world.count(),
                        seq: self.seq,
                    });
                }
                Event::Report(reply) => {
                    let players: Vec<serde_json::Value> = self
                        .players
                        .values_mut()
                        .filter_map(|p| {
                            let a = p.account?;
                            let placed = std::mem::take(&mut p.placed);
                            Some(serde_json::json!({ "account": a, "at": p.at, "edits": placed }))
                        })
                        .collect();
                    let _ = reply.send(serde_json::json!({
                        "world": self.cfg.name, "level": self.cfg.level, "spawn": self.spawn, "players": players,
                    }));
                }
                Event::Rules(plots, portals) => {
                    self.plots = plots;
                    self.portals = portals;
                }
                Event::Shutdown => break,
            }
        }
        for p in self.players.values() {
            let _ = p.tx.try_send(Message::Binary(wire::encode(&ToClient::Bye { why: "the world is closing".into() }).into()));
        }
        self.save();
    }

    fn save(&mut self) {
        if let Err(e) = self.flush() {
            eprintln!("couldn't save the world: {e}");
        }
    }

    /// Write changed chunks and the edit log in one transaction.
    fn flush(&mut self) -> Result<(), String> {
        self.last_flush = Instant::now();
        if self.dirty.is_empty() && self.log.is_empty() && self.seq > 0 {
            return Ok(());
        }
        let e = |e: rusqlite::Error| e.to_string();
        let t = self.db.transaction().map_err(e)?;
        for k in self.dirty.drain() {
            match self.world.chunk(k) {
                Some(c) if !c.is_empty() => {
                    t.execute(
                        "INSERT OR REPLACE INTO chunks(x, y, z, version, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![k.0[0], k.0[1], k.0[2], c.version, c.encode()],
                    )
                    .map_err(e)?;
                }
                _ => {
                    t.execute("DELETE FROM chunks WHERE x = ?1 AND y = ?2 AND z = ?3", params![k.0[0], k.0[1], k.0[2]])
                        .map_err(e)?;
                }
            }
        }
        for (seq, at, player, c, old, new) in self.log.drain(..) {
            t.execute(
                "INSERT OR REPLACE INTO edits(seq, at, player, x, y, z, old, new) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![seq as i64, at as i64, player, c[0], c[1], c[2], old, new],
            )
            .map_err(e)?;
        }
        let palette = serde_json::to_string(&self.cfg.palette).map_err(|e| e.to_string())?;
        t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('palette', ?1)", [palette]).map_err(e)?;
        t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('seq', ?1)", [self.seq.to_string()]).map_err(e)?;
        t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('settings', ?1)", [self.settings.clone()]).map_err(e)?;
        t.commit().map_err(e)
    }

    /// Queue a message for one player; a player who can't keep up with reliable messages
    /// is dropped. Returns false if they were.
    fn send_to(tx: &mpsc::Sender<Message>, m: &ToClient) -> bool {
        match tx.try_send(Message::Binary(wire::encode(m).into())) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => m.droppable(),
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    fn broadcast(&mut self, m: &ToClient, except: Option<u64>) {
        let mut gone = Vec::new();
        for (conn, p) in &self.players {
            if Some(*conn) != except && !Self::send_to(&p.tx, m) {
                gone.push(*conn);
            }
        }
        for conn in gone {
            self.leave(conn);
        }
    }

    fn join(&mut self, conn: u64, tx: mpsc::Sender<Message>, protocol: u32, name: String, avatar: Vec<u8>, account: Option<u64>) {
        let bye = |why: &str| {
            let _ = tx.try_send(Message::Binary(wire::encode(&ToClient::Bye { why: why.into() }).into()));
        };
        if protocol != PROTOCOL {
            return bye(&format!("this world speaks version {PROTOCOL}; please update"));
        }
        if self.players.len() >= self.cfg.max_players {
            return bye("the world is full");
        }
        let id = conn as u32;
        let name = if name.is_empty() { format!("player {id}") } else { name };
        let mut ok = Self::send_to(
            &tx,
            &ToClient::Welcome {
                you: id,
                world: self.cfg.name.clone(),
                palette: self.cfg.palette.clone(),
                block_size: self.cfg.block_size,
                bounds: self.cfg.bounds,
                spawn: self.spawn,
                settings: self.settings_now(),
                name: name.clone(),
            },
        );
        for k in self.world.keys() {
            let c = self.world.chunk(k).expect("listed");
            ok &= Self::send_to(&tx, &ToClient::Chunk { key: k, version: c.version, data: c.encode() });
        }
        ok &= Self::send_to(&tx, &ToClient::Ready);
        ok &= Self::send_to(&tx, &ToClient::Weather(Box::new(self.weather.keyframe())));
        for p in self.players.values() {
            ok &= Self::send_to(&tx, &ToClient::Joined { player: p.id, name: p.name.clone(), avatar: p.avatar.clone() });
        }
        if !ok {
            return;
        }
        self.broadcast(&ToClient::Joined { player: id, name: name.clone(), avatar: avatar.clone() }, None);
        eprintln!("{name} joined ({} here)", self.players.len() + 1);
        let bucket = Bucket::new(self.cfg.edit_rate, self.cfg.edit_rate);
        self.players.insert(conn, Player { id, name: name.clone(), avatar, tx, at: None, bucket, account, placed: 0 });
    }

    /// The settings a new arrival gets: the world's own, with the portals players opened.
    fn settings_now(&self) -> String {
        if self.portals.is_empty() {
            return self.settings.clone();
        }
        let mut s: serde_json::Value = serde_json::from_str(&self.settings).unwrap_or_else(|_| serde_json::json!({}));
        let Some(obj) = s.as_object_mut() else { return self.settings.clone() };
        let list = obj.entry("portals").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = list.as_array_mut() {
            a.extend(self.portals.iter().cloned());
        }
        s.to_string()
    }

    fn leave(&mut self, conn: u64) {
        if let Some(p) = self.players.remove(&conn) {
            eprintln!("{} left ({} here)", p.name, self.players.len());
            self.broadcast(&ToClient::Left { player: p.id }, None);
        }
    }

    fn send_weather(&mut self) {
        self.keyframe_at = Instant::now();
        let m = ToClient::Weather(Box::new(self.weather.keyframe()));
        self.broadcast(&m, None);
    }

    fn message(&mut self, conn: u64, msg: ToServer) {
        let now = self.started.elapsed().as_secs_f64();
        let Some(p) = self.players.get_mut(&conn) else { return };
        match msg {
            ToServer::Hello { .. } => {}
            ToServer::Climate(c) => {
                if !p.bucket.take(now) {
                    return;
                }
                let c = Climate { ground: self.weather.climate.ground, ..c.sane() };
                eprintln!("{} changed the weather", p.name);
                self.weather.climate = c;
                if let Ok(j) = serde_json::to_string(&c) {
                    let _ = self.db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('climate', ?1)", [j]);
                }
                self.send_weather();
            }
            ToServer::Pose { at, blob } => {
                if at.iter().all(|v| v.is_finite()) {
                    p.at = Some(at);
                }
                let m = ToClient::Pose { player: p.id, blob };
                self.broadcast(&m, Some(conn));
            }
            ToServer::Edit { id, cell, block } => {
                let checked = if !p.bucket.take(now) {
                    Err(Why::TooFast)
                } else {
                    // the world's own checks first (bounds, palette), then reach, then plots
                    let account = p.account;
                    let claimed = self.plots.iter().any(|pl| pl.holds(cell) && !account.is_some_and(|a| pl.builders.contains(&a)));
                    self.world.check(cell, block).and_then(|_| {
                        if !within_reach(p.at, cell) {
                            Err(Why::TooFar)
                        } else if claimed {
                            Err(Why::Claimed)
                        } else {
                            self.world.edit(cell, block)
                        }
                    })
                };
                match checked {
                    Err(why) => {
                        if !Self::send_to(&p.tx, &ToClient::Refused { id, why }) {
                            self.leave(conn);
                        }
                    }
                    Ok(old) => {
                        if block != 0 {
                            p.placed = p.placed.saturating_add(1);
                        }
                        self.seq += 1;
                        let a = Applied { seq: self.seq, player: p.id, id, cell, block };
                        self.log.push((self.seq, now_unix(), p.name.clone(), cell, old, block));
                        self.dirty.insert(ChunkKey::of(cell));
                        self.broadcast(&ToClient::Edits(vec![a]), None);
                    }
                }
            }
        }
    }
}

fn within_reach(at: Option<[f32; 3]>, cell: [i32; 3]) -> bool {
    let Some(at) = at else { return false };
    let eye = [at[0], at[1] + EYES, at[2]];
    let d2: f32 = (0..3).map(|i| (cell[i] as f32 + 0.5 - eye[i]).powi(2)).sum();
    d2 <= REACH * REACH
}
