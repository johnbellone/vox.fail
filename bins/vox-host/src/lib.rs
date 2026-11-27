//! vox-host: many worlds in one process, on one port, told apart by path.
//!
//! Each world is a [`vox_server::Server`] as it would be alone: its own thread, SQLite file and
//! snapshots. The host routes: `/w/<id>/ws` is a player's connection to world `<id>`, and
//! `/w/<id>/` its status. A world may also answer to an **alias**, the port it used to have
//! (`/w/7781/ws` for the yard), so levels whose portals say `":7781"` and clients already at
//! `wss://vox.fail/w/7780/ws` keep working unchanged. `GET /` lists the worlds.
//!
//! **On demand** (with an idle time, [`Host::start`]): a world starts when the first connection
//! for it arrives, and is saved and stopped once nobody has played in it or watched it for that
//! long; the next connection starts it again. Worlds marked `keep` (the entry worlds, such as
//! the Void) start with the host and never stop. Without an idle time every world starts with
//! the host and runs until it stops.
//!
//! Phases 1 and 3 of `docs/design/world-network.md`. Worlds that don't exist until asked for
//! (instances) come with the directory's world registry.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::future::join_all;
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

/// How often idle worlds are looked for.
const IDLE_CHECK: Duration = Duration::from_secs(15);

struct World {
    id: String,
    aliases: Vec<String>,
    keep: bool,
    /// what it starts from, each time it starts
    cfg: Config,
    /// running, and when it was last in use; None: stopped
    slot: Mutex<Option<(Server, Instant)>>,
}

impl World {
    /// The world, started if it wasn't, and marked in use now.
    async fn ensure(&self) -> Result<Server, String> {
        let mut slot = self.slot.lock().await;
        if let Some((server, used)) = slot.as_mut() {
            *used = Instant::now();
            return Ok(server.clone());
        }
        let cfg = self.cfg.clone();
        // opening its database and loading its chunks is blocking work
        let server = tokio::task::spawn_blocking(move || Server::start(cfg)).await.map_err(|e| e.to_string())??;
        eprintln!("vox-host: started {}", self.id);
        *slot = Some((server.clone(), Instant::now()));
        Ok(server)
    }

    /// Stop it if nobody has used it for `idle`. Returns whether it stopped.
    async fn stop_if_idle(&self, idle: Duration) -> bool {
        let mut slot = self.slot.lock().await;
        let Some((server, used)) = slot.as_mut() else { return false };
        let busy = server.status().await.is_none_or(|s| !s.players.is_empty() || s.watchers > 0);
        if busy {
            *used = Instant::now();
            return false;
        }
        if used.elapsed() < idle {
            return false;
        }
        let (server, _) = slot.take().expect("running");
        // held while saving, so a connection arriving now waits and then starts it afresh
        server.shutdown().await;
        eprintln!("vox-host: stopped {} (idle)", self.id);
        true
    }

    async fn stop(&self) {
        if let Some((server, _)) = self.slot.lock().await.take() {
            server.shutdown().await;
        }
    }

    async fn running(&self) -> Option<Server> {
        self.slot.lock().await.as_ref().map(|(s, _)| s.clone())
    }
}

/// The worlds. Clone it freely; [`Host::shutdown`] saves and stops them all.
#[derive(Clone)]
pub struct Host {
    worlds: Arc<Vec<World>>,
    /// every id and alias: which world it is
    names: Arc<HashMap<String, usize>>,
}

impl Host {
    /// Check the ids and aliases, then start the worlds that start now: all of them without an
    /// `idle` time, only the `keep` ones with one (the rest start when first asked for, and stop
    /// after `idle` unused). If one can't start, the ones already started are stopped again
    /// (and saved) and the error names the world. Call from inside a tokio runtime.
    pub async fn start(specs: Vec<Spec>, idle: Option<Duration>) -> Result<Host, String> {
        let mut seen = HashSet::new();
        let mut names = HashMap::new();
        for (i, s) in specs.iter().enumerate() {
            for n in std::iter::once(&s.id).chain(&s.aliases) {
                if !valid_id(n) {
                    return Err(format!("\"{n}\" can't name a world: lower-case letters, digits, - and _ only"));
                }
                if !seen.insert(n.clone()) {
                    return Err(format!("two worlds are called \"{n}\""));
                }
                names.insert(n.clone(), i);
            }
        }
        let worlds: Vec<World> = specs
            .into_iter()
            .map(|s| World { id: s.id, aliases: s.aliases, keep: s.keep || idle.is_none(), cfg: s.cfg, slot: Mutex::new(None) })
            .collect();
        let host = Host { worlds: Arc::new(worlds), names: Arc::new(names) };
        for w in host.worlds.iter().filter(|w| w.keep) {
            if let Err(e) = w.ensure().await {
                host.shutdown().await;
                return Err(format!("world \"{}\": {e}", w.id));
            }
        }
        if let Some(idle) = idle {
            let worlds = Arc::downgrade(&host.worlds);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(IDLE_CHECK.min(idle)).await;
                    // the host is gone: nothing left to look after
                    let Some(worlds) = worlds.upgrade() else { break };
                    for w in worlds.iter().filter(|w| !w.keep) {
                        w.stop_if_idle(idle).await;
                    }
                }
            });
        }
        Ok(host)
    }

    /// The routes: `/w/<name>/ws`, `/w/<name>/`, and `GET /` listing the worlds.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(list))
            .route("/w/{name}/", get(status))
            .route("/w/{name}/ws", get(connect))
            .with_state(self.clone())
    }

    /// The ids of the worlds, in the order they were given.
    pub fn ids(&self) -> Vec<String> {
        self.worlds.iter().map(|w| w.id.clone()).collect()
    }

    /// The ids of the worlds running now.
    pub async fn running(&self) -> Vec<String> {
        let mut out = Vec::new();
        for w in self.worlds.iter() {
            if w.running().await.is_some() {
                out.push(w.id.clone());
            }
        }
        out
    }

    /// Save every running world and stop them, all at once.
    pub async fn shutdown(&self) {
        join_all(self.worlds.iter().map(|w| w.stop())).await;
    }

    fn world(&self, name: &str) -> Option<&World> {
        self.names.get(name).map(|&i| &self.worlds[i])
    }
}

async fn connect(State(host): State<Host>, Path(name): Path<String>, ws: WebSocketUpgrade) -> Response {
    let Some(w) = host.world(&name) else { return StatusCode::NOT_FOUND.into_response() };
    match w.ensure().await {
        Ok(server) => server.accept(ws),
        Err(e) => {
            eprintln!("vox-host: can't start {}: {e}", w.id);
            (StatusCode::SERVICE_UNAVAILABLE, "that world can't be opened just now").into_response()
        }
    }
}

/// A world's status, without starting it.
async fn status(State(host): State<Host>, Path(name): Path<String>) -> Response {
    let Some(w) = host.world(&name) else { return StatusCode::NOT_FOUND.into_response() };
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
    for w in host.worlds.iter() {
        let s = match w.running().await {
            Some(server) => server.status().await,
            None => None,
        };
        worlds.push(serde_json::json!({
            "id": w.id,
            "aliases": w.aliases,
            "keep": w.keep,
            "running": s.is_some(),
            "players": s.as_ref().map_or(0, |s| s.players.len()),
        }));
    }
    Json(serde_json::json!({ "worlds": worlds }))
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
}
