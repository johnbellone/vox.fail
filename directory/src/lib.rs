//! vox-directory: where people sign in, make their character and go into a world.
//!
//! - Accounts: signed in with Steam or Discord ([`social`]), or a name and a password
//!   (Argon2id). A web session is a random cookie; the database keeps only its SHA-256.
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

pub mod social;
pub use social::{Discord, Sites, Social, name_from};

const COOKIE: &str = "vox_session";
/// How long a web session lasts, s.
const SESSION: u64 = 30 * 24 * 3600;
/// Sign-in and sign-up attempts allowed per address (and per name) in `WINDOW`.
const ATTEMPTS: u32 = 10;
const WINDOW: Duration = Duration::from_secs(300);

/// A world people can go to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldInfo {
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
    tries: Mutex<HashMap<String, (u32, Instant)>>,
    /// a password hash checked when a name doesn't exist, so failing takes as long either way
    dummy: String,
}

#[derive(Clone)]
pub struct Directory(Arc<Inner>, Arc<(PathBuf, PathBuf, PathBuf)>);

/// The value of cookie `name` in a request.
fn cookie(h: &HeaderMap, name: &str) -> Option<String> {
    h.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("random");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn salt() -> SaltString {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("random");
    SaltString::encode_b64(&b).expect("salt")
}

fn sha(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|x| format!("{x:02x}")).collect()
}

/// A new signing key, from the system's randomness.
pub fn new_key() -> SigningKey {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("random");
    auth::key_from_seed(seed)
}

/// The key in `path` (64 hex digits), made and saved there the first time.
pub fn load_or_make_key(path: &std::path::Path) -> Result<SigningKey, String> {
    if let Ok(s) = std::fs::read_to_string(path) {
        let s = s.trim();
        let mut seed = [0u8; 32];
        if s.len() != 64 {
            return Err(format!("{} should hold 64 hex digits", path.display()));
        }
        for (i, b) in seed.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| format!("{} isn't hex", path.display()))?;
        }
        return Ok(auth::key_from_seed(seed));
    }
    let key = new_key();
    let hex: String = key.to_bytes().iter().map(|x| format!("{x:02x}")).collect();
    std::fs::write(path, hex + "\n").map_err(|e| format!("can't write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// A name people go by: 3-20 letters, digits, spaces, `_`, `-` or `.`, starting with a letter
/// or digit.
pub fn check_name(n: &str) -> Result<String, String> {
    let n = n.trim();
    let len = n.chars().count();
    if !(3..=20).contains(&len) {
        return Err("names are 3 to 20 characters".into());
    }
    if !n.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return Err("names start with a letter or a digit".into());
    }
    if !n.chars().all(|c| c.is_alphanumeric() || " _-.".contains(c)) || n.contains("  ") {
        return Err("names use letters, digits, single spaces, _ - and .".into());
    }
    Ok(n.to_string())
}

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

impl Directory {
    pub fn open(cfg: Config) -> Result<Directory, String> {
        let db = Connection::open(&cfg.db).map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS accounts (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 pass TEXT NOT NULL, created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (
                 hash TEXT PRIMARY KEY, account INTEGER NOT NULL, expires INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS characters (
                 account INTEGER PRIMARY KEY, model TEXT NOT NULL, wear TEXT NOT NULL,
                 updated INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS identities (
                 site TEXT NOT NULL, subject TEXT NOT NULL, account INTEGER NOT NULL,
                 created INTEGER NOT NULL, PRIMARY KEY (site, subject));
             CREATE INDEX IF NOT EXISTS identities_account ON identities (account);",
        )
        .map_err(|e| e.to_string())?;
        let catalogue = Catalogue::load(&cfg.data);
        if catalogue.characters.is_empty() {
            return Err(format!("no characters in {}/characters.json (tools/build_catalogue.py makes it)", cfg.data.display()));
        }
        Ok(Directory(
            Arc::new(Inner {
                db: Mutex::new(db),
                key: cfg.key,
                catalogue,
                worlds: cfg.worlds,
                secure: cfg.secure_cookies,
                downloads: cfg.downloads,
                behind_proxy: cfg.behind_proxy,
                social: cfg.social,
                http: social::http(),
                tries: Mutex::new(HashMap::new()),
                dummy: Argon2::default()
                    .hash_password(random_hex(16).as_bytes(), &salt())
                    .map(|h| h.to_string())
                    .map_err(|e| e.to_string())?,
            }),
            Arc::new((cfg.web, cfg.assets, cfg.data)),
        ))
    }

    /// The public half of the signing key, as hex: give it to world servers.
    pub fn public_key(&self) -> String {
        auth::public_hex(&self.0.key.verifying_key())
    }

    pub fn router(&self) -> Router {
        let (web, assets, data) = &*self.1;
        Router::new()
            .route("/api/me", get(me))
            .route("/api/signup", post(signup))
            .route("/api/signin", post(signin))
            .route("/api/signout", post(signout))
            .route("/api/catalogue", get(catalogue))
            .route("/api/character", put(set_character))
            .route("/api/token", post(token))
            .route("/api/worlds", get(worlds))
            .route("/api/key", get(key))
            .route("/auth/steam", get(social::steam_start))
            .route("/auth/steam/return", get(social::steam_return))
            .route("/auth/discord", get(social::discord_start))
            .route("/auth/discord/return", get(social::discord_return))
            .route_service("/", ServeFile::new(web.join("index.html")))
            .route_service("/create", ServeFile::new(web.join("create.html")))
            .route_service("/play", ServeFile::new(web.join("play.html")))
            .nest_service("/assets", ServeDir::new(assets))
            .nest_service("/data", ServeDir::new(data))
            // the browser client is big: served pre-compressed when tools/build_web.sh made the
            // .br / .gz files
            .fallback_service(ServeDir::new(web).precompressed_br().precompressed_gzip())
            .layer(axum::middleware::from_fn(headers))
            .with_state(self.clone())
    }

    /// Where a request came from (for counting sign-in tries).
    fn visitor(&self, addr: SocketAddr, h: &HeaderMap) -> String {
        if self.0.behind_proxy {
            if let Some(ip) = h.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
                return ip.trim().to_string();
            }
            if let Some(ip) = h.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit(',').next()) {
                return ip.trim().to_string();
            }
        }
        addr.ip().to_string()
    }

    /// Too many sign-in tries from `who` lately?
    fn tried(&self, who: &str) -> bool {
        self.tried_up_to(who, ATTEMPTS)
    }

    fn tried_up_to(&self, who: &str, most: u32) -> bool {
        let mut t = self.0.tries.lock().unwrap();
        let now = Instant::now();
        t.retain(|_, (_, at)| now.duration_since(*at) < WINDOW);
        let e = t.entry(who.to_string()).or_insert((0, now));
        e.0 += 1;
        e.0 > most
    }

    /// The account a request's session cookie belongs to.
    fn account(&self, h: &HeaderMap) -> Option<(u64, String)> {
        let cookie = cookie(h, COOKIE)?;
        let db = self.0.db.lock().unwrap();
        db.query_row(
            "SELECT a.id, a.name FROM sessions s JOIN accounts a ON a.id = s.account WHERE s.hash = ?1 AND s.expires > ?2",
            params![sha(&cookie), now() as i64],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    fn character(&self, account: u64) -> Option<Character> {
        let db = self.0.db.lock().unwrap();
        db.query_row("SELECT model, wear FROM characters WHERE account = ?1", params![account as i64], |r| {
            Ok(Character {
                model: r.get(0)?,
                wear: serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default(),
            })
        })
        .optional()
        .ok()
        .flatten()
    }

    /// A new session for `account`: its cookie.
    fn session(&self, account: u64) -> HeaderValue {
        let token = random_hex(32);
        let db = self.0.db.lock().unwrap();
        let _ = db.execute("DELETE FROM sessions WHERE expires < ?1", params![now() as i64]);
        let _ = db.execute(
            "INSERT INTO sessions (hash, account, expires) VALUES (?1, ?2, ?3)",
            params![sha(&token), account as i64, (now() + SESSION) as i64],
        );
        let secure = if self.0.secure { "; Secure" } else { "" };
        HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION}{secure}")).expect("cookie")
    }
}

/// Every response: no framing by other sites, no sniffing, and a tight content policy for
/// the pages (the browser client needs WebAssembly). And what Cloudflare (or a browser) may
/// keep: the game's files for five minutes (purge Cloudflare's cache after a release), the
/// API and sign-ins never, the pages only after checking back.
async fn headers(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path().to_string();
    let mut r = next.run(req).await;
    let keep = if path.starts_with("/api/") || path.starts_with("/auth/") {
        "no-store"
    } else if ["/pkg/", "/assets/", "/data/"].iter().any(|p| path.starts_with(p)) && r.status().is_success() {
        "public, max-age=300"
    } else {
        "no-cache"
    };
    let h = r.headers_mut();
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static(keep));
    }
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: blob:; connect-src 'self' blob: data: ws: wss: http: https:; frame-ancestors 'none'",
        ),
    );
    r
}

fn json_only(h: &HeaderMap) -> Res<()> {
    let ok = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    if ok { Ok(()) } else { Err(ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "send JSON".into())) }
}

#[derive(Deserialize)]
struct Login {
    name: String,
    password: String,
}

async fn signup(
    State(d): State<Directory>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(l): Json<Login>,
) -> Res<Response> {
    json_only(&h)?;
    if d.tried(&d.visitor(addr, &h)) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many tries: wait a few minutes".into()));
    }
    let name = check_name(&l.name).map_err(bad)?;
    if !(8..=128).contains(&l.password.chars().count()) {
        return Err(bad("passwords are 8 to 128 characters"));
    }
    let pass = l.password.clone();
    let hash = tokio::task::spawn_blocking(move || {
        Argon2::default().hash_password(pass.as_bytes(), &salt()).map(|h| h.to_string())
    })
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "try again".into()))?
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "try again".into()))?;
    let id = {
        let db = d.0.db.lock().unwrap();
        match db.execute("INSERT INTO accounts (name, pass, created) VALUES (?1, ?2, ?3)", params![name, hash, now() as i64]) {
            Ok(_) => db.last_insert_rowid() as u64,
            Err(_) => return Err(ApiError(StatusCode::CONFLICT, "that name is taken".into())),
        }
    };
    Ok(([(header::SET_COOKIE, d.session(id))], Json(json!({ "name": name }))).into_response())
}

async fn signin(
    State(d): State<Directory>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(l): Json<Login>,
) -> Res<Response> {
    json_only(&h)?;
    let lname = l.name.trim().to_lowercase();
    if d.tried(&d.visitor(addr, &h)) || d.tried(&format!("name:{lname}")) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many tries: wait a few minutes".into()));
    }
    let row: Option<(i64, String, String)> = {
        let db = d.0.db.lock().unwrap();
        db.query_row("SELECT id, name, pass FROM accounts WHERE name = ?1", params![l.name.trim()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()
        .unwrap_or(None)
    };
    let pass = l.password.clone();
    // check a password even for a name that doesn't exist, so the time taken says nothing
    let stored = row.as_ref().map_or_else(|| d.0.dummy.clone(), |r| r.2.clone());
    let ok = tokio::task::spawn_blocking(move || {
        let s = stored;
        PasswordHash::new(&s).is_ok_and(|p| Argon2::default().verify_password(pass.as_bytes(), &p).is_ok())
    })
    .await
    .unwrap_or(false);
    match row {
        Some((id, name, _)) if ok => Ok(([(header::SET_COOKIE, d.session(id as u64))], Json(json!({ "name": name }))).into_response()),
        _ => Err(ApiError(StatusCode::UNAUTHORIZED, "wrong name or password".into())),
    }
}

async fn signout(State(d): State<Directory>, h: HeaderMap) -> Response {
    if let Some(c) = h.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in c.split(';') {
            if let Some((COOKIE, v)) = part.trim().split_once('=') {
                let _ = d.0.db.lock().unwrap().execute("DELETE FROM sessions WHERE hash = ?1", params![sha(v)]);
            }
        }
    }
    let gone = HeaderValue::from_static("vox_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    ([(header::SET_COOKIE, gone)], Json(json!({}))).into_response()
}

async fn me(State(d): State<Directory>, h: HeaderMap) -> Json<Value> {
    let offered = d.0.social.offered();
    match d.account(&h) {
        Some((id, name)) => {
            Json(json!({ "name": name, "character": d.character(id), "linked": d.linked(id), "sign_in_with": offered }))
        }
        None => Json(json!({ "name": null, "sign_in_with": offered })),
    }
}

async fn catalogue(State(d): State<Directory>) -> Json<Value> {
    Json(json!({ "characters": d.0.catalogue.characters, "wardrobe": d.0.catalogue.wardrobe }))
}

async fn set_character(State(d): State<Directory>, h: HeaderMap, Json(c): Json<Character>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.account(&h).ok_or(ApiError(StatusCode::UNAUTHORIZED, "sign in first".into()))?;
    let c = d.0.catalogue.check(&c).map_err(bad)?;
    let db = d.0.db.lock().unwrap();
    db.execute(
        "INSERT INTO characters (account, model, wear, updated) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(account) DO UPDATE SET model = ?2, wear = ?3, updated = ?4",
        params![id as i64, c.model, serde_json::to_string(&c.wear).unwrap_or_default(), now() as i64],
    )
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "couldn't save".into()))?;
    Ok(Json(json!({ "character": c })))
}

async fn token(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, name) = d.account(&h).ok_or(ApiError(StatusCode::UNAUTHORIZED, "sign in first".into()))?;
    let character = d.character(id).ok_or_else(|| bad("make your character first"))?;
    let t = now();
    let claims = Claims { account: id, name: name.clone(), character: character.clone(), issued: t, expires: t + auth::LIFETIME };
    Ok(Json(json!({
        "token": auth::sign(&claims, &d.0.key),
        "expires": claims.expires,
        "name": name,
        "character": character,
        "worlds": d.0.worlds,
    })))
}

async fn worlds(State(d): State<Directory>) -> Json<Value> {
    let downloads: Vec<Value> = d.0.downloads.iter().map(|(label, url)| json!({ "label": label, "url": url })).collect();
    Json(json!({ "worlds": d.0.worlds, "downloads": downloads }))
}

async fn key(State(d): State<Directory>) -> String {
    d.public_key()
}
