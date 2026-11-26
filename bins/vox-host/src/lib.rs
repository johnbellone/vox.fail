//! vox-host: many worlds in one process, on one port, told apart by path.
//!
//! Each world is a [`vox_server::Server`] as it would be alone: its own thread, SQLite file and
//! snapshots. The host only routes: `/w/<id>/ws` is a player's connection to world `<id>`, and
//! `/w/<id>/` its status. A world may also answer to an **alias**, the port it used to have
//! (`/w/7781/ws` for the yard), so levels whose portals say `":7781"` and clients already at
//! `wss://vox.fail/w/7780/ws` keep working unchanged. `GET /` lists the worlds.
//!
//! This is phase 1 of `docs/design/world-network.md`: the set of worlds is fixed at start.
//! Starting and stopping worlds on demand comes with the directory's world registry.

use std::{collections::HashSet, sync::Arc};

use axum::{Json, Router, routing::get};
use futures_util::future::join_all;
use vox_server::{Config, Server};

/// A world's id: what goes in its path. Lower-case letters, digits, `-` and `_`, at most 64.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// One world to run: its id, the other names it answers to, and its configuration.
pub struct Spec {
    pub id: String,
    pub aliases: Vec<String>,
    pub cfg: Config,
}

struct Running {
    id: String,
    aliases: Vec<String>,
    server: Server,
}

/// The running worlds. Clone it freely; [`Host::shutdown`] saves and stops them all.
#[derive(Clone)]
pub struct Host {
    worlds: Arc<Vec<Running>>,
}

impl Host {
    /// Check the ids and aliases, then start every world. If one can't start, the ones already
    /// started are stopped again (and saved) and the error names the world. Call from inside a
    /// tokio runtime.
    pub async fn start(specs: Vec<Spec>) -> Result<Host, String> {
        let mut names = HashSet::new();
        for s in &specs {
            for n in std::iter::once(&s.id).chain(&s.aliases) {
                if !valid_id(n) {
                    return Err(format!("\"{n}\" can't name a world: lower-case letters, digits, - and _ only"));
                }
                if !names.insert(n.clone()) {
                    return Err(format!("two worlds are called \"{n}\""));
                }
            }
        }
        let mut worlds = Vec::new();
        for s in specs {
            match Server::start(s.cfg) {
                Ok(server) => worlds.push(Running { id: s.id, aliases: s.aliases, server }),
                Err(e) => {
                    join_all(worlds.iter().map(|w| w.server.shutdown())).await;
                    return Err(format!("world \"{}\": {e}", s.id));
                }
            }
        }
        Ok(Host { worlds: Arc::new(worlds) })
    }

    /// Every world's routes, and `GET /` listing them.
    pub fn router(&self) -> Router {
        let host = self.clone();
        let mut r = Router::new().route("/", get(move || list(host.clone())));
        for w in self.worlds.iter() {
            for name in std::iter::once(&w.id).chain(&w.aliases) {
                r = r.merge(w.server.mount(&format!("/w/{name}")));
            }
        }
        r
    }

    /// The ids of the running worlds, in the order they were given.
    pub fn ids(&self) -> Vec<String> {
        self.worlds.iter().map(|w| w.id.clone()).collect()
    }

    /// Save every world and stop them, all at once.
    pub async fn shutdown(&self) {
        join_all(self.worlds.iter().map(|w| w.server.shutdown())).await;
    }
}

async fn list(host: Host) -> Json<serde_json::Value> {
    let statuses = join_all(host.worlds.iter().map(|w| w.server.status())).await;
    let worlds: Vec<_> = host
        .worlds
        .iter()
        .zip(statuses)
        .map(|(w, s)| {
            serde_json::json!({
                "id": w.id,
                "aliases": w.aliases,
                "players": s.as_ref().map_or(0, |s| s.players.len()),
                "running": s.is_some(),
            })
        })
        .collect();
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
