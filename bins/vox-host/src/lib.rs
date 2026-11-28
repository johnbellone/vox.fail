//! vox-host: many worlds in one process, on one port, told apart by path.
//!
//! Each world is a [`vox_server::Server`] as it would be alone: its own thread, SQLite file and
//! snapshots. The host routes: `/w/<id>/ws` is a player's connection to world `<id>`, and
//! `/w/<id>/` its status. A world may also answer to an **alias**, the port it used to have
//! (`/w/7781/ws` for the yard), so levels whose portals say `":7781"` and clients already at
//! `wss://vox.fail/w/7780/ws` keep working unchanged. `GET /` lists the worlds.
//! `/w/<id>/link` is a neighbouring world's link to world `<id>` (see vox-server's
//! neighbours.rs); it reaches only a running world, and never starts one.
//!
//! **On demand** (with an idle time, [`Options::idle`]): a world starts when the first
//! connection for it arrives, and is saved and stopped once nobody has played in it or watched
//! it for that long; the next connection starts it again. Worlds marked `keep` (the entry
//! worlds, such as the Void) start with the host and never stop. Without an idle time every
//! world starts with the host and runs until it stops.
//!
//! **Instances** (with [`Options::instances`]): the directory asks for a private copy of a
//! dungeon for a raid party (`POST /api/host/instance`, with the world secret). The copy starts
//! from the dungeon's level (a template), lives in memory only, and is thrown away once it has
//! been empty for a while or its time is up. Who may enter is the directory's business: it
//! gives tickets for an instance only to its party.
//!
//! Phases 1, 3 and 4 of `docs/design/world-network.md`.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, RwLock, Weak,
        OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::future::join_all;
use serde::Deserialize;
use tokio::sync::Mutex;
use vox_server::{Config, Server};

/// A world's id: what goes in its path. Lower-case letters, digits, `-` and `_`, at most 64.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// One world to run: its id, the other names it answers to, its configuration, and whether it
/// always runs (`keep`) or only while it's used.
pub struct Spec {
    pub id: String,
    pub aliases: Vec<String>,
    pub cfg: Config,
    pub keep: bool,
}

/// How the host runs its worlds.
#[derive(Default)]
pub struct Options {
    /// stop a world (not a `keep` one) after this long unused; None: every world always runs
    pub idle: Option<Duration>,
    /// make instances when the directory asks; None: no instances here
    pub instances: Option<Instances>,
    /// big maps made of tiles, each tile a world made on its first visit
    pub atlases: Vec<Atlas>,
}

/// A big map of tiles (`docs/design/world-network.md`, *The atlas*): tile `(i, j)` is the
/// world `atlas-<name>-<i>-<j>`, made the first time someone goes there, its ground generated
/// from the seed, kept in `<data>/atlas-<name>/<i>-<j>.db`, and stopped when idle like any.
pub struct Atlas {
    pub name: String,
    pub seed: u64,
    /// tiles east to west, north to south
    pub size: [i32; 2],
    /// what a tile's world is configured from (the shared flags); its name, database and
    /// ground are its own
    pub base: Config,
    /// where the tiles are kept
    pub data: PathBuf,
    /// snapshots next to each tile's database
    pub snapshots: bool,
}

impl Atlas {
    /// The world for `id`, if it's one of this atlas's tiles.
    fn world(&self, id: &str) -> Option<World> {
        let tile = vox_core::terrain::Tile::from_id(id, &self.name, self.seed, self.size)?;
        let dir = self.data.join(format!("atlas-{}", self.name));
        let mut cfg = self.base.clone();
        cfg.name = id.to_string();
        cfg.db = dir.join(format!("{}-{}.db", tile.at[0], tile.at[1]));
        cfg.snapshots = self.snapshots.then(|| dir.join(format!("{}-{}.db.snapshots", tile.at[0], tile.at[1])));
        cfg.seed = None;
        cfg.level = format!("atlas-{}", self.name);
        cfg.tile = Some(tile);
        Some(World::new(id.to_string(), vec![], false, None, cfg))
    }
}

/// Making instances of dungeons.
pub struct Instances {
    /// what an instance's world is configured from (the shared flags: sign-in, the directory);
    /// its name, level and seed are the instance's own, and it keeps nothing on disk
    pub base: Config,
    /// the dungeons' levels: `<templates>/<template>.json`
    pub templates: PathBuf,
    /// what the directory shows to ask for one (the world secret)
    pub secret: String,
    /// an instance empty this long is thrown away
    pub idle: Duration,
    /// at most this many instances at once
    pub most: usize,
    /// players in one instance at most (a raid party's size)
    pub players: usize,
}

/// How often idle worlds are looked for.
const IDLE_CHECK: Duration = Duration::from_secs(15);
/// The longest an instance may be asked to last.
const LONGEST_INSTANCE: Duration = Duration::from_secs(4 * 3600);

struct World {
    id: String,
    aliases: Vec<String>,
    keep: bool,
    /// an instance: thrown away when it stops, and stopped by this time at the latest
    ends: Option<Instant>,
    /// what it starts from, each time it starts
    cfg: Config,
    /// running, and when it was last in use; None: stopped
    slot: Mutex<Option<(Server, Instant)>>,
    /// an instance that has stopped: gone for good, never started again
    gone: AtomicBool,
    /// [`STOPPED`], [`STARTING`] or [`RUNNING`], for what doorways to it show (read without
    /// waiting on `slot`, which a start holds)
    state: AtomicU8,
}

const STOPPED: u8 = 0;
const STARTING: u8 = 1;
const RUNNING: u8 = 2;

impl World {
    fn new(id: String, aliases: Vec<String>, keep: bool, ends: Option<Instant>, cfg: Config) -> World {
        World { id, aliases, keep, ends, cfg, slot: Mutex::new(None), gone: AtomicBool::new(false), state: AtomicU8::new(STOPPED) }
    }

    /// What doorways to it show.
    fn way(&self) -> vox_server::doors::Way {
        use vox_server::doors::Way;
        if self.gone.load(Ordering::Acquire) {
            return Way::Absent;
        }
        match self.state.load(Ordering::Acquire) {
            RUNNING => Way::Running,
            STARTING => Way::Starting,
            _ => Way::Stopped,
        }
    }

    /// The world, started if it wasn't, and marked in use now.
    async fn ensure(&self) -> Result<Server, String> {
        let mut slot = self.slot.lock().await;
        if let Some((server, used)) = slot.as_mut() {
            *used = Instant::now();
            return Ok(server.clone());
        }
        if self.gone.load(Ordering::Acquire) {
            return Err("that instance is over".into());
        }
        let cfg = self.cfg.clone();
        self.state.store(STARTING, Ordering::Release);
        // opening its database and loading its chunks is blocking work
        let started = tokio::task::spawn_blocking(move || Server::start(cfg)).await.map_err(|e| e.to_string()).and_then(|r| r);
        let server = match started {
            Ok(s) => s,
            Err(e) => {
                self.state.store(STOPPED, Ordering::Release);
                return Err(e);
            }
        };
        self.state.store(RUNNING, Ordering::Release);
        eprintln!("vox-host: started {}", self.id);
        *slot = Some((server.clone(), Instant::now()));
        Ok(server)
    }

    /// Stop it if nobody has used it for `idle`, or (an instance) its time is up. Returns
    /// whether it stopped.
    async fn stop_if_done(&self, idle: Duration) -> bool {
        let mut slot = self.slot.lock().await;
        let Some((server, used)) = slot.as_mut() else { return false };
        let over = self.ends.is_some_and(|e| Instant::now() >= e);
        if !over {
            let busy = server.status().await.is_none_or(|s| !s.players.is_empty() || s.watchers > 0);
            if busy {
                *used = Instant::now();
                return false;
            }
            if used.elapsed() < idle {
                return false;
            }
        }
        let (server, _) = slot.take().expect("running");
        if self.ends.is_some() {
            self.gone.store(true, Ordering::Release);
        }
        // held while saving, so a connection arriving now waits and then starts it afresh
        self.state.store(STOPPED, Ordering::Release);
        server.shutdown().await;
        eprintln!("vox-host: stopped {} ({})", self.id, if over { "its time is up" } else { "unused" });
        true
    }

    async fn stop(&self) {
        if let Some((server, _)) = self.slot.lock().await.take() {
            self.state.store(STOPPED, Ordering::Release);
            server.shutdown().await;
        }
    }

    async fn running(&self) -> Option<Server> {
        self.slot.lock().await.as_ref().map(|(s, _)| s.clone())
    }
}

/// The host as its worlds see it, without keeping it alive.
struct WeakHost {
    worlds: Weak<RwLock<Worlds>>,
    instances: Option<Weak<Instances>>,
    atlases: Weak<Vec<Atlas>>,
}

/// What a world asks its host about the others (ADR 0004 decision 12): whether a world by that
/// name is running, starting or stopped, and waking a stopped one.
struct HostWays {
    host: OnceLock<WeakHost>,
    /// worlds ask from their own threads; a wake runs on the host's runtime
    handle: tokio::runtime::Handle,
}

impl HostWays {
    fn host(&self) -> Option<Host> {
        let h = self.host.get()?;
        Some(Host {
            worlds: h.worlds.upgrade()?,
            instances: match &h.instances {
                Some(i) => Some(i.upgrade()?),
                None => None,
            },
            atlases: h.atlases.upgrade()?,
        })
    }
}

impl vox_server::doors::Ways for HostWays {
    fn way(&self, name: &str) -> vox_server::doors::Way {
        use vox_server::doors::Way;
        let Some(host) = self.host() else { return Way::Absent };
        match host.world(name) {
            Some(w) => w.way(),
            // a tile not made yet is made (and started) on its first visit
            None if host.is_tile(name) => Way::Stopped,
            None => Way::Absent,
        }
    }

    fn wake(&self, name: &str) {
        let Some(host) = self.host() else { return };
        // an instance is made only by the directory, for a party; nothing else is woken twice
        let Some(w) = host.world_or_tile(name).filter(|w| w.ends.is_none() && w.way() == vox_server::doors::Way::Stopped) else { return };
        self.handle.spawn(async move {
            if let Err(e) = w.ensure().await {
                eprintln!("vox-host: couldn't wake {}: {e}", w.id);
            }
        });
    }
}

/// The worlds by name (ids and aliases), and in the order they came.
#[derive(Default)]
struct Worlds {
    order: Vec<Arc<World>>,
    names: HashMap<String, Arc<World>>,
}

/// The worlds. Clone it freely; [`Host::shutdown`] saves and stops them all.
#[derive(Clone)]
pub struct Host {
    worlds: Arc<RwLock<Worlds>>,
    instances: Option<Arc<Instances>>,
    atlases: Arc<Vec<Atlas>>,
}

impl Host {
    /// Check the ids and aliases, then start the worlds that start now: all of them without an
    /// idle time, only the `keep` ones with one (the rest start when first asked for, and stop
    /// after the idle time unused). If one can't start, the ones already started are stopped
    /// again (and saved) and the error names the world. Call from inside a tokio runtime.
    pub async fn start(specs: Vec<Spec>, options: Options) -> Result<Host, String> {
        let mut seen = HashSet::new();
        for s in &specs {
            for n in std::iter::once(&s.id).chain(&s.aliases) {
                if !valid_id(n) {
                    return Err(format!("\"{n}\" can't name a world: lower-case letters, digits, - and _ only"));
                }
                if !seen.insert(n.clone()) {
                    return Err(format!("two worlds are called \"{n}\""));
                }
            }
        }
        let idle = options.idle;
        // every world (and every tile and instance made later) can ask the host about the
        // others, for what its doorways show; the host is filled in once it exists
        let ways = Arc::new(HostWays { host: OnceLock::new(), handle: tokio::runtime::Handle::current() });
        let dyn_ways: Arc<dyn vox_server::doors::Ways> = ways.clone();
        let mut worlds = Worlds::default();
        for mut s in specs {
            s.cfg.ways = Some(dyn_ways.clone());
            let w = Arc::new(World::new(s.id, s.aliases, s.keep || idle.is_none(), None, s.cfg));
            for n in std::iter::once(&w.id).chain(&w.aliases) {
                worlds.names.insert(n.clone(), w.clone());
            }
            worlds.order.push(w);
        }
        for a in &options.atlases {
            if !a.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) || a.name.is_empty() || a.size.iter().any(|&n| !(1..=100_000).contains(&n)) {
                return Err(format!("atlas \"{}\": a name of lower-case letters and digits, and a size of 1 to 100000 tiles each way", a.name));
            }
        }
        let mut atlases = options.atlases;
        for a in &mut atlases {
            a.base.ways = Some(dyn_ways.clone());
        }
        let instances = options.instances.map(|mut i| {
            i.base.ways = Some(dyn_ways.clone());
            Arc::new(i)
        });
        let host = Host { worlds: Arc::new(RwLock::new(worlds)), instances, atlases: Arc::new(atlases) };
        let _ = ways.host.set(WeakHost {
            worlds: Arc::downgrade(&host.worlds),
            instances: host.instances.as_ref().map(Arc::downgrade),
            atlases: Arc::downgrade(&host.atlases),
        });
        for w in host.all().into_iter().filter(|w| w.keep) {
            if let Err(e) = w.ensure().await {
                host.shutdown().await;
                return Err(format!("world \"{}\": {e}", w.id));
            }
        }
        if idle.is_some() || host.instances.is_some() {
            let instance_idle = host.instances.as_ref().map(|i| i.idle);
            tokio::spawn(look_after(Arc::downgrade(&host.worlds), idle, instance_idle));
        }
        Ok(host)
    }

    /// The routes: `/w/<name>/ws`, `/w/<name>/link`, `/w/<name>/`, `GET /` listing the worlds, and (with
    /// instances) `POST /api/host/instance` for the directory.
    pub fn router(&self) -> Router {
        let mut r = Router::new().route("/", get(list)).route("/w/{name}/", get(status)).route("/w/{name}/ws", get(connect)).route("/w/{name}/link", get(link));
        if self.instances.is_some() {
            r = r.route("/api/host/instance", post(make_instance));
        }
        r.with_state(self.clone())
    }

    /// The ids of the worlds, in the order they were given (instances after).
    pub fn ids(&self) -> Vec<String> {
        self.all().iter().map(|w| w.id.clone()).collect()
    }

    /// The ids of the worlds running now.
    pub async fn running(&self) -> Vec<String> {
        let mut out = Vec::new();
        for w in self.all() {
            if w.running().await.is_some() {
                out.push(w.id.clone());
            }
        }
        out
    }

    /// Save every running world and stop them, all at once.
    pub async fn shutdown(&self) {
        join_all(self.all().iter().map(|w| w.stop())).await;
    }

    fn all(&self) -> Vec<Arc<World>> {
        self.worlds.read().unwrap_or_else(|e| e.into_inner()).order.clone()
    }

    fn world(&self, name: &str) -> Option<Arc<World>> {
        self.worlds.read().unwrap_or_else(|e| e.into_inner()).names.get(name).cloned()
    }

    /// A world by name, or (the first time it's asked for) a tile of an atlas, made now.
    fn world_or_tile(&self, name: &str) -> Option<Arc<World>> {
        if let Some(w) = self.world(name) {
            return Some(w);
        }
        let made = self.atlases.iter().find_map(|a| a.world(name))?;
        if let Some(dir) = made.cfg.db.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut ws = self.worlds.write().unwrap_or_else(|e| e.into_inner());
        // asked for twice at once: the first one made it
        if let Some(w) = ws.names.get(name) {
            return Some(w.clone());
        }
        let w = Arc::new(made);
        ws.names.insert(name.to_string(), w.clone());
        ws.order.push(w.clone());
        Some(w)
    }

    /// Is this the name of an atlas's tile (made or not)?
    fn is_tile(&self, name: &str) -> bool {
        self.atlases.iter().any(|a| vox_core::terrain::Tile::from_id(name, &a.name, a.seed, a.size).is_some())
    }
}

/// Every so often: stop what's unused (worlds after `idle`, instances after `instance_idle` or
/// when their time is up) and forget stopped instances. Ends with the host.
async fn look_after(worlds: Weak<RwLock<Worlds>>, idle: Option<Duration>, instance_idle: Option<Duration>) {
    let every = [idle, instance_idle].into_iter().flatten().min().unwrap_or(IDLE_CHECK).min(IDLE_CHECK);
    loop {
        tokio::time::sleep(every).await;
        let Some(worlds) = worlds.upgrade() else { break };
        let all = worlds.read().unwrap_or_else(|e| e.into_inner()).order.clone();
        for w in all {
            let limit = if w.ends.is_some() { instance_idle } else if w.keep { None } else { idle };
            let Some(limit) = limit else { continue };
            if w.stop_if_done(limit).await && w.ends.is_some() {
                let mut ws = worlds.write().unwrap_or_else(|e| e.into_inner());
                ws.order.retain(|o| !Arc::ptr_eq(o, &w));
                ws.names.remove(&w.id);
            }
        }
    }
}

async fn connect(State(host): State<Host>, Path(name): Path<String>, ws: WebSocketUpgrade) -> Response {
    let Some(w) = host.world_or_tile(&name) else { return StatusCode::NOT_FOUND.into_response() };
    match w.ensure().await {
        Ok(server) => server.accept(ws),
        // a stopped instance is gone, not asleep: it doesn't start again
        Err(_) if w.gone.load(Ordering::Acquire) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("vox-host: can't start {}: {e}", w.id);
            (StatusCode::SERVICE_UNAVAILABLE, "that world can't be opened just now").into_response()
        }
    }
}

/// A neighbour's link to a world. Only a running world takes one: a link that started its
/// neighbours would wake them in a chain across a whole atlas, and keep idle worlds from
/// stopping. The dialler tries again (with backoff) until the world is running.
async fn link(State(host): State<Host>, Path(name): Path<String>, ws: WebSocketUpgrade) -> Response {
    let Some(w) = host.world(&name) else { return StatusCode::NOT_FOUND.into_response() };
    match w.running().await {
        Some(server) => server.accept_link(ws),
        None => (StatusCode::SERVICE_UNAVAILABLE, "that world isn't running").into_response(),
    }
}

/// A world's status, without starting it.
async fn status(State(host): State<Host>, Path(name): Path<String>) -> Response {
    let Some(w) = host.world(&name) else {
        // a tile nobody has been to yet: there, but not made
        if host.is_tile(&name) {
            return Json(serde_json::json!({ "world": name, "running": false })).into_response();
        }
        return StatusCode::NOT_FOUND.into_response();
    };
    let stopped = || Json(serde_json::json!({ "world": w.id, "running": false })).into_response();
    let Some(server) = w.running().await else { return stopped() };
    match server.status().await {
        Some(s) => Json(serde_json::json!({
            "world": s.world, "players": s.players, "blocks": s.blocks, "seq": s.seq, "running": true,
        }))
        .into_response(),
        None => stopped(),
    }
}

async fn list(State(host): State<Host>) -> Json<serde_json::Value> {
    let mut worlds = Vec::new();
    for w in host.all() {
        let s = match w.running().await {
            Some(server) => server.status().await,
            None => None,
        };
        worlds.push(serde_json::json!({
            "id": w.id,
            "aliases": w.aliases,
            "keep": w.keep,
            "instance": w.ends.is_some(),
            "running": s.is_some(),
            "players": s.as_ref().map_or(0, |s| s.players.len()),
        }));
    }
    Json(serde_json::json!({ "worlds": worlds }))
}

#[derive(Deserialize)]
struct Make {
    /// the instance's id (the directory makes it: `<template>-<random>`)
    id: String,
    /// the dungeon it's a copy of
    template: String,
    /// how long it may last
    minutes: u32,
    /// make it if it isn't there (false: only say whether it is; 410 if not)
    #[serde(default = "yes")]
    make: bool,
}

fn yes() -> bool {
    true
}

/// The directory asks for an instance. Already there: fine. Otherwise it's made from the
/// template and started, unless the directory only asked whether it's there (`make: false`:
/// 410 Gone, so it can count a fresh copy against the party's limit before asking again).
async fn make_instance(State(host): State<Host>, h: HeaderMap, Json(m): Json<Make>) -> Response {
    let Some(inst) = &host.instances else { return StatusCode::NOT_FOUND.into_response() };
    let given = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if !given.is_some_and(|g| same(g.trim(), &inst.secret)) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_id(&m.id) || !valid_id(&m.template) {
        return (StatusCode::BAD_REQUEST, "bad name").into_response();
    }
    if let Some(w) = host.world(&m.id) {
        return if w.ends.is_some() {
            Json(serde_json::json!({ "id": m.id, "made": false })).into_response()
        } else {
            (StatusCode::CONFLICT, "that's a world, not an instance").into_response()
        };
    }
    if !m.make {
        return (StatusCode::GONE, "not here").into_response();
    }
    let seed = match std::fs::read_to_string(inst.templates.join(format!("{}.json", m.template))) {
        Ok(s) => s,
        Err(_) => return (StatusCode::NOT_FOUND, "no such dungeon").into_response(),
    };
    let lasts = Duration::from_secs(u64::from(m.minutes) * 60).min(LONGEST_INSTANCE);
    let mut cfg = inst.base.clone();
    cfg.name = m.id.clone();
    cfg.db = ":memory:".into();
    cfg.snapshots = None;
    cfg.seed = Some(seed);
    cfg.level = m.template.clone();
    cfg.max_players = inst.players;
    let w = Arc::new(World::new(m.id.clone(), vec![], false, Some(Instant::now() + lasts), cfg));
    {
        let mut ws = host.worlds.write().unwrap_or_else(|e| e.into_inner());
        // asked twice at once: the first one made it
        if ws.names.contains_key(&m.id) {
            return Json(serde_json::json!({ "id": m.id, "made": false })).into_response();
        }
        if ws.order.iter().filter(|o| o.ends.is_some()).count() >= inst.most {
            return (StatusCode::SERVICE_UNAVAILABLE, "no room for another instance just now").into_response();
        }
        ws.names.insert(m.id.clone(), w.clone());
        ws.order.push(w.clone());
    }
    if let Err(e) = w.ensure().await {
        let mut ws = host.worlds.write().unwrap_or_else(|e| e.into_inner());
        ws.order.retain(|o| !Arc::ptr_eq(o, &w));
        ws.names.remove(&m.id);
        eprintln!("vox-host: can't start instance {}: {e}", m.id);
        return (StatusCode::SERVICE_UNAVAILABLE, "that dungeon can't be opened just now").into_response();
    }
    eprintln!("vox-host: made instance {} of {} for {} min", m.id, m.template, lasts.as_secs() / 60);
    Json(serde_json::json!({ "id": m.id, "made": true })).into_response()
}

/// Compare two secrets in time that doesn't depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        for ok in ["void", "yard", "7781", "crypt-2", "a_b"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "Void", "../x", "a/b", "a b", "a~b", &"x".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn secrets() {
        assert!(same("s3cret", "s3cret"));
        assert!(!same("s3cret", "s3creT") && !same("s3cret", "s3cre") && !same("", "x"));
    }
}
