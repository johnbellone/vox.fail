//! vox-directory: where people sign in, make their character and go into a world.
//!
//! - Accounts: signed in with Steam or Discord ([`social`]), or a name and a password
//!   (Argon2id). A web session is a random cookie; the database keeps only its SHA-256.
//! - What players hold and do ([`game`]): authority (the currency every world shares, earned by
//!   playing and spent on feats), world currencies, the ledger, the inventory, plots, portals
//!   and raids; and the reports world servers send.
//! - Friends, groups and raid parties ([`people`]).
//! - Characters: one per account for now: which model, and what they wear (a hat, something
//!   on the face, something in the hand), from `data/characters.json` and
//!   `data/wardrobe.json`.
//! - Sign-in tokens: `POST /api/token` signs who you are and what you look like with the
//!   directory's Ed25519 key ([`vox_core::auth`]). World servers started with the
//!   directory's public key (`vox-server --directory-key`) let you in with it, with no call
//!   back here.
//! - The web pages (`web/`): sign in or up, make your character, play in the browser (the
//!   WebAssembly client in `web/pkg`) or download the desktop client. The game's assets and
//!   data are served from here too, so the browser client can load them.
//!
//! The API speaks JSON. Writes need `Content-Type: application/json`, which a form on another
//! site can't send without asking first, and the session cookie is `SameSite=Lax`.

mod util;
mod state;
mod routes;
#[allow(unused_imports)]
pub use util::*;
#[allow(unused_imports)]
pub use state::*;
#[allow(unused_imports)]
pub use routes::*;

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::SaltString,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower_http::services::{ServeDir, ServeFile};
use vox_core::auth::{self, Character, Claims, SigningKey};

pub mod game;
pub mod people;
mod structures;
pub mod social;
pub use social::{Discord, Sites, Social, name_from};

const COOKIE: &str = "vox_session";
/// How long a web session lasts, s.
const SESSION: u64 = 30 * 24 * 3600;
/// Sign-in and sign-up attempts allowed per address (and per name) in `WINDOW`.
const ATTEMPTS: u32 = 10;
const WINDOW: Duration = Duration::from_secs(300);
/// Password hashes worked out at once.
const HASHING_AT_ONCE: usize = 4;
/// Addresses and names whose sign-in tries are counted at once.
const MAX_TRACKED: usize = 100_000;

/// A world people can go to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldInfo {
    /// what its server calls it (`vox-server --name void`): how its reports are matched up
    pub id: String,
    pub name: String,
    /// its WebSocket address (`ws://host:7780/ws`; `:7780` = this host, that port)
    pub url: String,
    #[serde(default)]
    pub about: String,
}

pub struct Config {
    /// SQLite file; `:memory:` keeps nothing
    pub db: PathBuf,
    /// signs the tokens; world servers check them with its public half
    pub key: SigningKey,
    /// the web pages (index.html, create.html, play.html, pkg/ for the browser client)
    pub web: PathBuf,
    /// the game's assets (models, portraits, shaders, block tiles) and data (blocks, levels)
    pub assets: PathBuf,
    pub data: PathBuf,
    pub worlds: Vec<WorldInfo>,
    /// mark the session cookie Secure (serve over HTTPS)
    pub secure_cookies: bool,
    /// where the desktop client can be downloaded (shown on the play page), if anywhere
    pub downloads: Vec<(String, String)>,
    /// behind Cloudflare (or another proxy): a visitor's address is in `CF-Connecting-IP`, or
    /// the last one in `X-Forwarded-For`, not the connection's
    pub behind_proxy: bool,
    /// signing in with Steam and Discord
    pub social: Social,
    /// what world servers show to report to the directory and fetch their rules (plots,
    /// portals); None: no reports
    pub world_secret: Option<String>,
    /// least time between one world's reports, s (60 in production; 0 in tests)
    pub report_every: u64,
    /// the host that makes instances of dungeons (`http://127.0.0.1:7790`, a `vox-host
    /// --instances`), asked with the world secret; None: no dungeons
    pub instance_host: Option<String>,
}

impl WorldInfo {
    /// A world from `id=Name=address` or `Name=address` (its id then its name in lower case,
    /// without "the", spaces as dashes: "The Void" is `void`).
    pub fn parse(spec: &str) -> Option<WorldInfo> {
        let parts: Vec<&str> = spec.splitn(3, '=').map(str::trim).collect();
        let (id, name, url) = match parts[..] {
            [id, name, url] => (id.to_string(), name, url),
            [name, url] => (world_id(name), name, url),
            _ => return None,
        };
        (!id.is_empty() && !name.is_empty() && !url.is_empty()).then(|| WorldInfo { id, name: name.into(), url: url.into(), about: String::new() })
    }
}

/// "The Void" → `void`, "Ember Isles" → `ember-isles`.
pub fn world_id(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let lower = lower.strip_prefix("the ").unwrap_or(&lower);
    lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-")
}

/// Who you can be and what you can wear.
#[derive(Clone, Default)]
struct Catalogue {
    characters: Vec<Value>,
    wardrobe: Vec<Value>,
    /// models by path
    models: Vec<String>,
    /// wardrobe item id → slot
    slots: HashMap<String, String>,
}

impl Catalogue {
    fn load(data: &std::path::Path) -> Catalogue {
        let read = |f: &str, k: &str| -> Vec<Value> {
            std::fs::read_to_string(data.join(f))
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.get(k).and_then(|a| a.as_array()).cloned())
                .unwrap_or_default()
        };
        let characters = read("characters.json", "characters");
        let wardrobe = read("wardrobe.json", "items");
        let models = characters.iter().filter_map(|c| c.get("model")?.as_str().map(String::from)).collect();
        let slots = wardrobe
            .iter()
            .filter_map(|i| Some((i.get("id")?.as_str()?.to_string(), i.get("slot")?.as_str()?.to_string())))
            .collect();
        Catalogue { characters, wardrobe, models, slots }
    }

    /// A character as asked for, if it's one you can be.
    fn check(&self, c: &Character) -> Result<Character, String> {
        if !self.models.contains(&c.model) {
            return Err("pick one of the characters".into());
        }
        let mut wear: Vec<String> = Vec::new();
        let mut used = Vec::new();
        for w in &c.wear {
            let slot = self.slots.get(w).ok_or_else(|| format!("there's nothing called {w} to wear"))?;
            if used.contains(slot) {
                return Err(format!("one {slot} at a time"));
            }
            used.push(slot.clone());
            wear.push(w.clone());
        }
        Ok(Character { model: c.model.clone(), wear })
    }
}

struct Inner {
    db: Mutex<Connection>,
    key: SigningKey,
    catalogue: Catalogue,
    worlds: Vec<WorldInfo>,
    secure: bool,
    downloads: Vec<(String, String)>,
    behind_proxy: bool,
    social: Social,
    http: reqwest::Client,
    world_secret: Option<String>,
    report_every: u64,
    instance_host: Option<String>,
    tries: Mutex<HashMap<String, (u32, Instant)>>,
    /// password hashes being worked out at once (each takes about 19 MB and a core)
    hashing: tokio::sync::Semaphore,
    /// a password hash checked when a name doesn't exist, so failing takes as long either way
    dummy: String,
}

#[derive(Clone)]
pub struct Directory(Arc<Inner>, Arc<(PathBuf, PathBuf, PathBuf)>);

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn bad(s: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, s.into())
}

type Res<T> = Result<T, ApiError>;
