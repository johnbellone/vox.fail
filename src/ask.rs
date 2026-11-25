//! The public "ask" page: viewers log in with Twitch, pick a character, record a question (up to
//! 30 s by default), listen back and send it to the queue (questions.rs).
//!
//! It runs on its own port (7778 by default), separate from the control page, and serves only
//! the page and its small API: nothing on it can drive the avatar, approve or air anything.
//! It listens on localhost; to let viewers in, put a tunnel in front of it, for example
//! `cloudflared tunnel --url http://localhost:7778`, and share the https address it prints.
//!
//! Login uses Twitch's device code flow, so it works behind any tunnel address with no
//! redirect URL to register: the page shows a code and a link to twitch.tv/activate, the viewer
//! approves, and the app reads their Twitch username once and throws the token away. Without a
//! Twitch client ID the page can instead take a typed name (marked "unverified"), if allowed.

use std::{
    collections::HashMap,
    io::Read,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::questions::{Queue, Status, SubmitError, unix_now};

pub const TWITCH_AUTH: &str = "https://id.twitch.tv";
const SESSION_HEADER: &str = "x-ask-session";
const MAX_SESSIONS: usize = 5000;
const WORKERS: usize = 8;

#[derive(Clone)]
pub struct AskConfig {
    pub port: u16,
    pub twitch_client_id: Option<String>,
    pub auth_base: String,
    pub scopes: String,
    /// without a Twitch client ID: let viewers type a name instead
    pub open_names: bool,
    /// characters viewers may pick: (label, model path under assets/)
    pub characters: Vec<(String, String)>,
    pub title: String,
    pub font: Option<Arc<Vec<u8>>>,
}

#[derive(Clone)]
struct Login {
    user: String,
    verified: bool,
    account: String,
}

struct Device {
    code: String,
    interval: Duration,
    next_poll: Instant,
    expires: Instant,
}

struct Session {
    last: Instant,
    login: Option<Login>,
    device: Option<Device>,
}

/// Simple per-address counters: at most `max` events per `window`.
#[derive(Default)]
struct Limiter(HashMap<(String, &'static str), (Instant, u32)>);

impl Limiter {
    fn allow(&mut self, ip: &str, kind: &'static str, max: u32, window: Duration) -> bool {
        let now = Instant::now();
        if self.0.len() > 20000 {
            self.0
                .retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(600));
        }
        let e = self.0.entry((ip.to_string(), kind)).or_insert((now, 0));
        if now.duration_since(e.0) > window {
            *e = (now, 0);
        }
        e.1 += 1;
        e.1 <= max
    }
}

pub struct AskShared {
    pub cfg: Mutex<AskConfig>,
    pub queue: Arc<Mutex<Queue>>,
    sessions: Mutex<HashMap<String, Session>>,
    limiter: Mutex<Limiter>,
    pub status: Mutex<String>,
}

pub struct AskServer {
    pub shared: Arc<AskShared>,
    pub url: String,
}

fn token() -> String {
    let mut b = [0u8; 18];
    getrandom::fill(&mut b).expect("system random numbers");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Starts the ask server (port 0 picks a free one; handy for tests).
pub fn start(cfg: AskConfig, queue: Arc<Mutex<Queue>>) -> Result<AskServer, String> {
    let server = Server::http(("127.0.0.1", cfg.port))
        .map_err(|e| format!("can't listen on port {}: {e}", cfg.port))?;
    let addr = server.server_addr().to_ip().ok_or("no address")?;
    let url = format!("http://127.0.0.1:{}/", addr.port());
    let shared = Arc::new(AskShared {
        cfg: Mutex::new(cfg),
        queue,
        sessions: Mutex::new(HashMap::new()),
        limiter: Mutex::new(Limiter::default()),
        status: Mutex::new(String::new()),
    });
    let server = Arc::new(server);
    for i in 0..WORKERS {
        let (server, shared) = (server.clone(), shared.clone());
        std::thread::Builder::new()
            .name(format!("ask-{i}"))
            .spawn(move || {
                while let Ok(req) = server.recv() {
                    handle(&shared, req);
                }
            })
            .map_err(|e| e.to_string())?;
    }
    crate::note!("[ask] viewer page on {url}");
    Ok(AskServer { shared, url })
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ascii header")
}

fn with_common<R: Read>(r: Response<R>, ctype: &str) -> Response<R> {
    r.with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
        .with_header(header("Referrer-Policy", "no-referrer"))
        .with_header(header("X-Frame-Options", "DENY"))
        .with_header(header(
            "Content-Security-Policy",
            "default-src 'self'; media-src 'self' blob:; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ))
        .with_header(header("Permissions-Policy", "microphone=(self), camera=()"))
}

fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    with_common(
        Response::from_data(v.to_string().into_bytes()).with_status_code(code),
        "application/json",
    )
}

fn err(code: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_resp(code, json!({ "error": msg }))
}

fn req_header(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

/// The viewer's address: Cloudflare's header when the request came through the local tunnel.
fn client_ip(req: &Request) -> String {
    let peer = req.remote_addr().map(|a| a.ip());
    if peer.is_some_and(|p| p.is_loopback()) {
        if let Some(ip) = req_header(req, "cf-connecting-ip").or_else(|| {
            req_header(req, "x-forwarded-for")
                .and_then(|v| v.split(',').next().map(|s| s.trim().to_string()))
        }) {
            if ip.parse::<std::net::IpAddr>().is_ok() {
                return ip;
            }
        }
    }
    peer.map(|p| p.to_string()).unwrap_or_default()
}

fn read_body(req: &mut Request, limit: usize) -> Result<Vec<u8>, &'static str> {
    if req.body_length().is_some_and(|n| n > limit) {
        return Err("too big");
    }
    let mut b = Vec::new();
    req.as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut b)
        .map_err(|_| "read error")?;
    if b.len() > limit {
        return Err("too big");
    }
    Ok(b)
}

fn handle(sh: &AskShared, mut req: Request) {
    let ip = client_ip(&req);
    let path = req.url().split('?').next().unwrap_or("/").to_string();
    let query = req
        .url()
        .split_once('?')
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    let method = req.method().clone();
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(&ip, "any", 120, Duration::from_secs(60))
    {
        let _ = req.respond(err(429, "Too many requests. Slow down a little."));
        return;
    }
    let resp = match (&method, path.as_str()) {
        (Method::Get, "/") => with_common(
            Response::from_data(PAGE.as_bytes().to_vec()),
            "text/html; charset=utf-8",
        ),
        (Method::Get, "/app.js") => with_common(
            Response::from_data(APP_JS.as_bytes().to_vec()),
            "text/javascript; charset=utf-8",
        ),
        (Method::Get, "/app.css") => with_common(
            Response::from_data(APP_CSS.as_bytes().to_vec()),
            "text/css; charset=utf-8",
        ),
        (Method::Get, "/font.ttf") => match sh.cfg.lock().unwrap().font.clone() {
            Some(f) => with_common(Response::from_data(f.to_vec()), "font/ttf"),
            None => err(404, "no font"),
        },
        (Method::Get, "/api/info") => info(sh, &req, &ip),
        (Method::Get, p) if p.starts_with("/portrait/") => portrait(sh, p),
        (Method::Post, "/api/login/twitch") => login_twitch(sh, &ip),
        (Method::Post, "/api/login/poll") => login_poll(sh, &req),
        (Method::Post, "/api/login/name") => match read_body(&mut req, 2048) {
            Ok(b) => login_name(sh, &b, &ip),
            Err(_) => err(413, "Too big."),
        },
        (Method::Post, "/api/logout") => {
            if let Some(t) = req_header(&req, SESSION_HEADER) {
                sh.sessions.lock().unwrap().remove(&t);
            }
            json_resp(200, json!({ "ok": true }))
        }
        (Method::Post, "/api/ask") => ask(sh, &mut req, &query, &ip),
        (Method::Get, "/api/mine") => mine(sh, &req),
        (Method::Post, "/api/withdraw") => withdraw(sh, &req, &query),
        _ => err(404, "Not found."),
    };
    let _ = req.respond(resp);
}

/// /portrait/<n>.png: the picture of character n (made by tools/make_portraits.py).
fn portrait(sh: &AskShared, p: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let n = p
        .strip_prefix("/portrait/")
        .and_then(|x| x.strip_suffix(".png"))
        .and_then(|x| x.parse::<usize>().ok());
    let model = n.and_then(|i| {
        sh.cfg
            .lock()
            .unwrap()
            .characters
            .get(i)
            .map(|c| c.1.clone())
    });
    let stem = model
        .as_deref()
        .and_then(|m| std::path::Path::new(m).file_stem())
        .map(|s| s.to_string_lossy().into_owned());
    match stem.and_then(|s| {
        std::fs::read(
            crate::asset_base()
                .join("assets/portraits")
                .join(format!("{s}.png")),
        )
        .ok()
    }) {
        Some(b) => with_common(Response::from_data(b), "image/png"),
        None => err(404, "No picture."),
    }
}

fn session_login(sh: &AskShared, req: &Request) -> Option<Login> {
    let t = req_header(req, SESSION_HEADER)?;
    let mut s = sh.sessions.lock().unwrap();
    let sess = s.get_mut(&t)?;
    sess.last = Instant::now();
    sess.login.clone()
}

fn new_session(sh: &AskShared, login: Option<Login>, device: Option<Device>) -> Option<String> {
    let mut s = sh.sessions.lock().unwrap();
    let now = Instant::now();
    if s.len() >= MAX_SESSIONS {
        s.retain(|_, v| {
            now.duration_since(v.last) < Duration::from_secs(6 * 3600)
                && (v.login.is_some() || now.duration_since(v.last) < Duration::from_secs(1800))
        });
        if s.len() >= MAX_SESSIONS {
            return None;
        }
    }
    let t = token();
    s.insert(
        t.clone(),
        Session {
            last: now,
            login,
            device,
        },
    );
    Some(t)
}

fn info(sh: &AskShared, req: &Request, ip: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap().clone();
    let me = session_login(sh, req);
    let q = sh.queue.lock().unwrap();
    let mode = if cfg.twitch_client_id.is_some() {
        "twitch"
    } else if cfg.open_names {
        "name"
    } else {
        "none"
    };
    let can = me
        .as_ref()
        .map(|m| match q.can_submit(&m.account, ip, unix_now()) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "reason": e.message() }),
        });
    json_resp(
        200,
        json!({
            "title": cfg.title, "open": q.open, "login": mode,
            "characters": cfg.characters.iter().map(|c| c.0.clone()).collect::<Vec<_>>(),
            "max_secs": q.limits.max_secs, "waiting": q.waiting().len(),
            "me": me.map(|m| json!({ "user": m.user, "verified": m.verified })),
            "can_ask": can,
        }),
    )
}

fn login_name(sh: &AskShared, body: &[u8], ip: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap().clone();
    if cfg.twitch_client_id.is_some() || !cfg.open_names {
        return err(403, "Log in with Twitch instead.");
    }
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(ip, "name", 5, Duration::from_secs(600))
    {
        return err(429, "Too many tries. Wait a few minutes.");
    }
    let name = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();
    if !(2..=25).contains(&name.chars().count())
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return err(400, "Use 2 to 25 letters, numbers or underscores.");
    }
    let login = Login {
        user: name.clone(),
        verified: false,
        account: format!("name:{}", name.to_ascii_lowercase()),
    };
    match new_session(sh, Some(login), None) {
        Some(t) => json_resp(
            200,
            json!({ "session": t, "user": name, "verified": false }),
        ),
        None => err(503, "Too busy right now. Try again soon."),
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into()
}

fn post_form(url: &str, form: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let mut r = agent()
        .post(url)
        .send_form(form.iter().copied())
        .map_err(|e| format!("can't reach Twitch: {e}"))?;
    let code = r.status().as_u16();
    let body = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok((code, serde_json::from_str(&body).unwrap_or(Value::Null)))
}

/// Step 1 of the device code flow: ask Twitch for a code the viewer approves on twitch.tv.
fn login_twitch(sh: &AskShared, ip: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap().clone();
    let Some(client_id) = cfg.twitch_client_id.clone() else {
        return err(403, "Twitch login isn't set up for this stream.");
    };
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(ip, "device", 6, Duration::from_secs(600))
    {
        return err(429, "Too many login attempts. Wait a few minutes.");
    }
    let (code, v) = match post_form(
        &format!("{}/oauth2/device", cfg.auth_base),
        &[("client_id", &client_id), ("scopes", &cfg.scopes)],
    ) {
        Ok(x) => x,
        Err(e) => {
            *sh.status.lock().unwrap() = format!("Twitch login: {e}");
            return err(502, "Couldn't reach Twitch. Try again.");
        }
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    let (Some(device_code), Some(user_code), Some(uri)) =
        (s("device_code"), s("user_code"), s("verification_uri"))
    else {
        *sh.status.lock().unwrap() = format!(
            "Twitch login: device request failed ({code}: {})",
            v.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("no details")
        );
        return err(502, "Twitch didn't accept the login request.");
    };
    if !(uri.starts_with("https://")
        || (cfg.auth_base != TWITCH_AUTH && uri.starts_with("http://127.0.0.1")))
    {
        return err(502, "Unexpected answer from Twitch.");
    }
    let interval = Duration::from_secs(
        v.get("interval")
            .and_then(|x| x.as_u64())
            .unwrap_or(5)
            .clamp(1, 30),
    );
    let expires = Duration::from_secs(
        v.get("expires_in")
            .and_then(|x| x.as_u64())
            .unwrap_or(1800)
            .min(1800),
    );
    let now = Instant::now();
    let dev = Device {
        code: device_code,
        interval,
        next_poll: now + interval,
        expires: now + expires,
    };
    match new_session(sh, None, Some(dev)) {
        Some(t) => json_resp(
            200,
            json!({ "session": t, "user_code": user_code, "verification_uri": uri, "interval": interval.as_secs() }),
        ),
        None => err(503, "Too busy right now. Try again soon."),
    }
}

/// Step 2: the page polls; we ask Twitch (no more often than it allows) whether it's approved.
fn login_poll(sh: &AskShared, req: &Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap().clone();
    let Some(client_id) = cfg.twitch_client_id.clone() else {
        return err(403, "Twitch login isn't set up.");
    };
    let Some(t) = req_header(req, SESSION_HEADER) else {
        return err(401, "No login in progress.");
    };
    let device_code = {
        let mut s = sh.sessions.lock().unwrap();
        let Some(sess) = s.get_mut(&t) else {
            return err(401, "No login in progress.");
        };
        sess.last = Instant::now();
        if let Some(l) = &sess.login {
            return json_resp(
                200,
                json!({ "state": "done", "user": l.user, "verified": l.verified }),
            );
        }
        let Some(d) = sess.device.as_mut() else {
            return err(401, "No login in progress.");
        };
        let now = Instant::now();
        if now > d.expires {
            s.remove(&t);
            return json_resp(200, json!({ "state": "expired" }));
        }
        if now < d.next_poll {
            return json_resp(200, json!({ "state": "pending" }));
        }
        d.next_poll = now + d.interval;
        d.code.clone()
    };
    let grant = "urn:ietf:params:oauth:grant-type:device_code";
    let (code, v) = match post_form(
        &format!("{}/oauth2/token", cfg.auth_base),
        &[
            ("client_id", &client_id),
            ("scopes", &cfg.scopes),
            ("device_code", &device_code),
            ("grant_type", grant),
        ],
    ) {
        Ok(x) => x,
        Err(_) => return json_resp(200, json!({ "state": "pending" })),
    };
    let Some(access) = v.get("access_token").and_then(|x| x.as_str()) else {
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
        if code == 400 && (msg.contains("pending") || msg.contains("slow_down")) {
            if msg.contains("slow_down") {
                if let Some(d) = sh
                    .sessions
                    .lock()
                    .unwrap()
                    .get_mut(&t)
                    .and_then(|s| s.device.as_mut())
                {
                    d.interval += Duration::from_secs(5);
                }
            }
            return json_resp(200, json!({ "state": "pending" }));
        }
        sh.sessions.lock().unwrap().remove(&t);
        return json_resp(
            200,
            json!({ "state": if msg.contains("denied") { "denied" } else { "expired" } }),
        );
    };
    // who is it? then throw the token away
    let who = agent()
        .get(&format!("{}/oauth2/validate", cfg.auth_base))
        .header("Authorization", &format!("OAuth {access}"))
        .call();
    let _ = post_form(
        &format!("{}/oauth2/revoke", cfg.auth_base),
        &[("client_id", &client_id), ("token", access)],
    );
    let v: Value = match who {
        Ok(mut r) if r.status().as_u16() == 200 => r
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|b| serde_json::from_str(&b).ok())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let login = v
        .get("login")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let same_app = v.get("client_id").and_then(|x| x.as_str()) == Some(client_id.as_str());
    if login.is_empty()
        || !same_app
        || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        sh.sessions.lock().unwrap().remove(&t);
        return json_resp(200, json!({ "state": "expired" }));
    }
    let l = Login {
        user: login.clone(),
        verified: true,
        account: format!("tw:{login}"),
    };
    if let Some(sess) = sh.sessions.lock().unwrap().get_mut(&t) {
        sess.login = Some(l);
        sess.device = None;
    }
    json_resp(
        200,
        json!({ "state": "done", "user": login, "verified": true }),
    )
}

fn query_get<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

fn ask(
    sh: &AskShared,
    req: &mut Request,
    query: &str,
    ip: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(me) = session_login(sh, req) else {
        return err(401, "Please log in first.");
    };
    let cfg = sh.cfg.lock().unwrap().clone();
    let Some(ch) = query_get(query, "c")
        .and_then(|c| c.parse::<usize>().ok())
        .and_then(|i| cfg.characters.get(i).cloned())
    else {
        return err(400, "Pick a character.");
    };
    // check before reading the upload, so a closed queue or a ban costs nothing
    let (limit, pre) = {
        let q = sh.queue.lock().unwrap();
        let limit =
            (q.limits.max_secs.min(crate::questions::MAX_SECS_HARD) + 1.0) as usize * 48000 * 2
                + 4096;
        (limit, q.can_submit(&me.account, ip, unix_now()))
    };
    if let Err(e) = pre {
        return err(
            if e == SubmitError::Banned { 403 } else { 409 },
            &e.message(),
        );
    }
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(ip, "upload", 6, Duration::from_secs(600))
    {
        return err(429, "Too many uploads. Wait a few minutes.");
    }
    let body = match read_body(req, limit) {
        Ok(b) => b,
        Err(_) => return err(413, "That recording is too big."),
    };
    let mut q = sh.queue.lock().unwrap();
    match q.submit(
        &me.user,
        me.verified,
        &me.account,
        if me.verified { "" } else { ip },
        (&ch.0, &ch.1),
        &body,
        unix_now(),
    ) {
        Ok(id) => {
            let pos = q.position(id).map(|p| p.0).unwrap_or(0);
            json_resp(200, json!({ "id": id, "position": pos }))
        }
        Err(e) => err(
            if matches!(e, SubmitError::Banned) {
                403
            } else {
                400
            },
            &e.message(),
        ),
    }
}

/// The viewer's own questions and where they stand. Never anyone else's.
fn mine(sh: &AskShared, req: &Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(me) = session_login(sh, req) else {
        return err(401, "Please log in first.");
    };
    let q = sh.queue.lock().unwrap();
    let now = unix_now();
    let list: Vec<Value> = q
        .items
        .iter()
        .filter(|x| x.account == me.account && now.saturating_sub(x.submitted) < 12 * 3600)
        .map(|x| {
            let pos = q.position(x.id);
            // a question waiting for review shows as "waiting": no need to say it hasn't been heard
            let state = match x.status {
                Status::New => "waiting",
                Status::Approved => "approved",
                Status::OnAir => "on_air",
                Status::Done => "done",
                Status::Skipped => "skipped",
            };
            json!({ "id": x.id, "character": x.character, "state": state, "position": pos.map(|p| p.0), "of": pos.map(|p| p.1), "votes": x.votes.len() })
        })
        .collect();
    json_resp(200, json!({ "questions": list }))
}

fn withdraw(sh: &AskShared, req: &Request, query: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(me) = session_login(sh, req) else {
        return err(401, "Please log in first.");
    };
    let Some(id) = query_get(query, "id").and_then(|i| i.parse::<u32>().ok()) else {
        return err(400, "Which question?");
    };
    let mut q = sh.queue.lock().unwrap();
    match q.get(id) {
        Some(x) if x.account == me.account && x.status.waiting() => {
            q.finish(id, Status::Skipped);
            json_resp(200, json!({ "ok": true }))
        }
        _ => err(404, "That question can't be withdrawn."),
    }
}

// ------------------------------------------------------------------------------------------
// The page
// ------------------------------------------------------------------------------------------

const PAGE: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ask a question</title><link rel="stylesheet" href="/app.css"></head><body>
<main>
<header><h1 id="title">Ask a question</h1><p class="sub">Record a question. If the streamer picks it, your character asks it live, in your voice.</p></header>
<section id="closed" class="card" hidden><p class="big">Questions are closed right now.</p><p class="mute">Check back when the stream opens them.</p></section>
<section id="login" class="card" hidden>
 <h2>1. Who's asking?</h2>
 <div id="tw" hidden><button id="twgo" class="primary">Log in with Twitch</button>
  <div id="code" hidden><p>Approve this code on Twitch:</p><p class="code" id="ucode"></p><p><a id="twlink" target="_blank" rel="noopener noreferrer">Open Twitch to approve</a></p><p class="mute" id="twwait">Waiting for Twitch...</p></div></div>
 <div id="nm" hidden><label for="name">Your name</label><div class="row"><input id="name" maxlength="25" autocomplete="nickname" placeholder="letters, numbers, _"><button id="namego" class="primary">Continue</button></div></div>
 <p id="nologin" class="mute" hidden>This stream hasn't set up sign-in for questions yet.</p>
</section>
<section id="ask" class="card" hidden>
 <p class="me">Asking as <b id="who"></b> <button id="out" class="link">not you?</button></p>
 <h2>2. Pick your character</h2><div id="chars" class="chars" role="radiogroup" aria-label="Character"></div>
 <h2>3. Record your question</h2>
 <p class="mute">Up to <span id="max">30</span> seconds. Keep it friendly: no hate, harassment or personal details.</p>
 <div class="rec"><button id="rec" class="primary rec-btn">Record</button><div class="bar"><div id="fill"></div></div><span id="t">0.0 s</span></div>
 <div id="review" hidden><audio id="play" controls></audio><div class="row"><button id="again">Record again</button><button id="send" class="primary">Send question</button></div></div>
 <p id="cant" class="warn" role="alert" hidden></p>
</section>
<section id="mine" class="card" hidden><h2>Your questions</h2><ul id="list"></ul></section>
<p id="msg" class="warn" role="status"></p>
<footer>The streamer listens to every question privately before it can air. Recordings are deleted once your question is answered or skipped.</footer>
</main><script src="/app.js"></script></body></html>"#;

const APP_CSS: &str = r#"@font-face{font-family:Pixel;src:url(/font.ttf)}
:root{color-scheme:dark;--bg:#14151b;--card:#1f212b;--ink:#f1f2f7;--mute:#a3a8bb;--acc:#ffc93c;--acc-ink:#1b1a1f;--line:#343748;--warn:#ff8a7a;--ok:#7ddc9b}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}
main{max-width:560px;margin:0 auto;padding:24px 16px 48px}
h1{font-family:Pixel,system-ui;font-size:20px;line-height:1.5;margin:0 0 8px;color:var(--acc)}h2{font-size:15px;margin:18px 0 8px}
.sub,.mute{color:var(--mute)}.mute{font-size:14px}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;margin:16px 0}
button{font:inherit;color:var(--ink);background:#2a2d3a;border:1px solid var(--line);border-radius:8px;padding:10px 14px;cursor:pointer;min-height:44px}
button:hover{border-color:var(--acc)}button:disabled{opacity:.45;cursor:not-allowed}
button.primary{background:var(--acc);color:var(--acc-ink);border-color:var(--acc);font-weight:600}
button.link{background:none;border:none;color:var(--mute);text-decoration:underline;padding:0;min-height:0}
button:focus-visible,input:focus-visible,a:focus-visible{outline:3px solid var(--acc);outline-offset:2px}
input{font:inherit;color:var(--ink);background:#14151b;border:1px solid var(--line);border-radius:8px;padding:10px;width:100%;min-height:44px}
label{display:block;margin-bottom:6px}.row{display:flex;gap:8px;margin-top:10px}.row input{flex:1}
a{color:var(--acc)}.code{font-family:Pixel,monospace;font-size:22px;letter-spacing:.12em;margin:6px 0}
.chars{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:6px;max-height:260px;overflow:auto;padding:2px}
.chars button{display:flex;flex-direction:column;align-items:center;gap:4px;text-align:center;font-size:14px;min-height:40px;padding:8px 6px}.chars img{image-rendering:pixelated;width:56px;height:56px}.chars button[aria-checked=true]{background:var(--acc);color:var(--acc-ink);border-color:var(--acc)}
.rec{display:flex;align-items:center;gap:12px;margin:10px 0}.rec-btn{min-width:110px}.rec-btn.on{background:var(--warn);border-color:var(--warn)}
.bar{flex:1;height:10px;background:#14151b;border:1px solid var(--line);border-radius:6px;overflow:hidden}#fill{height:100%;width:0;background:var(--acc)}
#t{font-variant-numeric:tabular-nums;color:var(--mute);min-width:56px;text-align:right}
audio{width:100%;margin-top:8px}.warn{color:var(--warn)}.ok{color:var(--ok)}.me{margin:0 0 4px;color:var(--mute)}.me b{color:var(--ink)}
#list{list-style:none;padding:0;margin:0}#list li{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:10px 0;border-top:1px solid var(--line)}#list li:first-child{border-top:none}
.state{font-weight:600}.state.on_air,.state.approved{color:var(--ok)}
footer{color:var(--mute);font-size:13px;margin-top:24px}
@media (prefers-reduced-motion:reduce){*{transition:none!important}}"#;

const APP_JS: &str = r#"'use strict';
const $=id=>document.getElementById(id);
let sess=null;try{sess=sessionStorage.getItem('ask')}catch(e){}
let info=null,pick=-1,wav=null,recState=null,polling=null;
const api=async(path,opt={})=>{opt.headers=Object.assign({},opt.headers||{},sess?{'X-Ask-Session':sess}:{});
  const r=await fetch(path,opt);let j={};try{j=await r.json()}catch(e){}if(!r.ok)throw new Error(j.error||('Error '+r.status));return j;};
const say=(t,ok)=>{$('msg').textContent=t||'';$('msg').className=ok?'ok':'warn'};
const save=s=>{sess=s;try{s?sessionStorage.setItem('ask',s):sessionStorage.removeItem('ask')}catch(e){}};
function show(id,on){$(id).hidden=!on}
async function load(){
  try{info=await api('/api/info')}catch(e){say('The stream is offline or unreachable.');return}
  if(info.title){$('title').textContent=info.title;document.title=info.title}
  $('max').textContent=Math.round(info.max_secs);
  const logged=!!info.me;
  show('closed',!info.open);
  show('login',info.open&&!logged);show('tw',info.login==='twitch');show('nm',info.login==='name');show('nologin',info.login==='none');
  show('ask',info.open&&logged);show('mine',logged);
  if(logged){$('who').textContent=info.me.user+(info.me.verified?'':' (unverified)');
    if(!$('chars').children.length)info.characters.forEach((c,i)=>{const b=document.createElement('button');const im=document.createElement('img');im.src='/portrait/'+i+'.png';im.alt='';im.width=56;im.height=56;im.onerror=()=>im.remove();b.appendChild(im);const s=document.createElement('span');s.textContent=c;b.appendChild(s);b.setAttribute('role','radio');b.setAttribute('aria-checked','false');b.onclick=()=>{pick=i;[...$('chars').children].forEach((x,j)=>x.setAttribute('aria-checked',j===i?'true':'false'));upd()};$('chars').appendChild(b)});
    const ok=!info.can_ask||info.can_ask.ok;$('cant').hidden=ok;$('cant').textContent=ok?'':info.can_ask.reason+(info.can_ask.reason.includes('waiting')?' You can send another once it\u2019s answered.':'');$('rec').disabled=!ok&&!recState;
    mine();}
  upd();
}
function upd(){$('send').disabled=!(wav&&pick>=0&&info&&info.open&&(!info.can_ask||info.can_ask.ok));}
$('twgo').onclick=async()=>{say('');$('twgo').disabled=true;
  try{const j=await api('/api/login/twitch',{method:'POST'});save(j.session);$('ucode').textContent=j.user_code;$('twlink').href=j.verification_uri;show('code',true);
    const tick=async()=>{try{const p=await api('/api/login/poll',{method:'POST'});
      if(p.state==='done'){show('code',false);$('twgo').disabled=false;load();return}
      if(p.state==='pending'){setTimeout(tick,(j.interval||5)*1000);return}
      save(null);show('code',false);$('twgo').disabled=false;say(p.state==='denied'?'Login was cancelled on Twitch.':'That code expired. Try again.');}catch(e){setTimeout(tick,6000)}};
    setTimeout(tick,(j.interval||5)*1000);
  }catch(e){$('twgo').disabled=false;say(e.message)}};
$('namego').onclick=async()=>{say('');try{const j=await api('/api/login/name',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({name:$('name').value.trim()})});save(j.session);load()}catch(e){say(e.message)}};
$('name').addEventListener('keydown',e=>{if(e.key==='Enter')$('namego').click()});
$('out').onclick=async()=>{try{await api('/api/logout',{method:'POST'})}catch(e){}save(null);location.reload()};
// recording: raw samples from the mic, downsampled to 24 kHz mono 16-bit WAV in the page
async function startRec(){
  say('');let stream;
  try{stream=await navigator.mediaDevices.getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:true,autoGainControl:true}})}
  catch(e){say('Microphone blocked. Allow microphone access for this page and try again.');return}
  const ctx=new (window.AudioContext||window.webkitAudioContext)();const src=ctx.createMediaStreamSource(stream);const proc=ctx.createScriptProcessor(4096,1,1);
  const chunks=[];const t0=performance.now();const max=info.max_secs;
  proc.onaudioprocess=e=>{chunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));const s=(performance.now()-t0)/1000;$('fill').style.width=Math.min(100,s/max*100)+'%';$('t').textContent=s.toFixed(1)+' s';if(s>=max)stopRec()};
  src.connect(proc);proc.connect(ctx.destination);
  recState={stream,ctx,src,proc,chunks};$('rec').textContent='Stop';$('rec').classList.add('on');show('review',false);wav=null;upd();
}
function stopRec(){
  const r=recState;if(!r)return;recState=null;
  try{r.proc.disconnect();r.src.disconnect()}catch(e){}r.stream.getTracks().forEach(t=>t.stop());
  const rate=r.ctx.sampleRate;r.ctx.close();
  let n=0;r.chunks.forEach(c=>n+=c.length);const all=new Float32Array(n);let o=0;r.chunks.forEach(c=>{all.set(c,o);o+=c.length});
  const out=downsample(trim(all,rate),rate,24000);
  $('rec').textContent='Record';$('rec').classList.remove('on');
  if(out.length<24000*0.8){say('That was too short. Hold Record and ask your question.');return}
  wav=new Blob([encode(out,24000)],{type:'audio/wav'});$('play').src=URL.createObjectURL(wav);show('review',true);upd();
}
function trim(s,rate){const th=0.01,pad=Math.floor(rate*0.2);let a=0,b=s.length-1;while(a<s.length&&Math.abs(s[a])<th)a++;while(b>a&&Math.abs(s[b])<th)b--;return s.subarray(Math.max(0,a-pad),Math.min(s.length,b+pad))}
function downsample(s,from,to){if(from===to)return s;const r=from/to,n=Math.floor(s.length/r),o=new Float32Array(n);for(let i=0;i<n;i++){const a=Math.floor(i*r),b=Math.min(s.length,Math.floor((i+1)*r));let sum=0;for(let j=a;j<b;j++)sum+=s[j];o[i]=sum/Math.max(1,b-a)}return o}
function encode(s,rate){const b=new ArrayBuffer(44+s.length*2),v=new DataView(b);const w=(o,t)=>{for(let i=0;i<t.length;i++)v.setUint8(o+i,t.charCodeAt(i))};
  w(0,'RIFF');v.setUint32(4,36+s.length*2,true);w(8,'WAVE');w(12,'fmt ');v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,rate,true);v.setUint32(28,rate*2,true);v.setUint16(32,2,true);v.setUint16(34,16,true);w(36,'data');v.setUint32(40,s.length*2,true);
  for(let i=0;i<s.length;i++){const x=Math.max(-1,Math.min(1,s[i]));v.setInt16(44+i*2,x<0?x*32768:x*32767,true)}return b}
$('rec').onclick=()=>recState?stopRec():startRec();
$('again').onclick=()=>{wav=null;show('review',false);startRec()};
$('send').onclick=async()=>{if(!wav||pick<0)return;$('send').disabled=true;say('Sending...');
  try{const j=await api('/api/ask?c='+pick,{method:'POST',headers:{'Content-Type':'audio/wav'},body:wav});wav=null;show('review',false);say('Sent! You are number '+j.position+' in line.',true);load()}
  catch(e){say(e.message);upd()}};
const LABEL={waiting:'In line',approved:'Coming up',on_air:'On air now!',done:'Answered. Thanks!',skipped:'Not picked this time'};
async function mine(){
  try{const j=await api('/api/mine');const ul=$('list');ul.textContent='';
    if(!j.questions.length){const li=document.createElement('li');li.textContent='Nothing sent yet.';li.className='mute';ul.appendChild(li)}
    j.questions.slice().reverse().forEach(q=>{const li=document.createElement('li');const l=document.createElement('span');
      l.textContent='#'+q.id+' as '+q.character+(q.position?' · '+q.position+' of '+q.of:'')+(q.votes?' · '+q.votes+' votes':'');
      const s=document.createElement('span');s.className='state '+q.state;s.textContent=LABEL[q.state]||q.state;li.append(l,s);
      if(q.state==='waiting'||q.state==='approved'){const b=document.createElement('button');b.textContent='Withdraw';b.className='link';b.onclick=async()=>{try{await api('/api/withdraw?id='+q.id,{method:'POST'});load()}catch(e){say(e.message)}};li.appendChild(b)}
      ul.appendChild(li)});
    const live=j.questions.some(q=>q.state==='waiting'||q.state==='approved'||q.state==='on_air');
    const hint=$('mine').querySelector('.hint');if(live&&!hint){const p=document.createElement('p');p.className='mute hint';p.textContent='Tell chat "!up" and your question number to move it up the line.';$('mine').appendChild(p)}
  }catch(e){}
}
load();setInterval(()=>{if(!recState)load()},10000);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A stand-in for id.twitch.tv: device code, one "pending" answer, then a token for `viewer1`.
    fn fake_twitch(client_id: &'static str, login: &'static str) -> (String, Arc<AtomicU32>) {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let polls = Arc::new(AtomicU32::new(0));
        let p = polls.clone();
        std::thread::spawn(move || {
            for mut r in server.incoming_requests() {
                let mut body = String::new();
                let _ = r.as_reader().read_to_string(&mut body);
                let url = r.url().to_string();
                let (code, v) = match url.as_str() {
                    "/oauth2/device" => {
                        assert!(body.contains(&format!("client_id={client_id}")), "{body}");
                        (
                            200,
                            json!({"device_code":"DEV123","user_code":"ABCDEFGH","verification_uri":format!("http://127.0.0.1:{port}/activate?device-code=ABCDEFGH"),"interval":1,"expires_in":1800}),
                        )
                    }
                    "/oauth2/token" => {
                        assert!(body.contains("device_code=DEV123") && body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"), "{body}");
                        if p.fetch_add(1, Ordering::SeqCst) == 0 {
                            (400, json!({"status":400,"message":"authorization_pending"}))
                        } else {
                            (
                                200,
                                json!({"access_token":"TOKEN1","refresh_token":"R","expires_in":14000,"scope":[],"token_type":"bearer"}),
                            )
                        }
                    }
                    "/oauth2/validate" => {
                        let auth = r
                            .headers()
                            .iter()
                            .find(|h| h.field.equiv("Authorization"))
                            .map(|h| h.value.as_str().to_string());
                        assert_eq!(auth.as_deref(), Some("OAuth TOKEN1"));
                        (
                            200,
                            json!({"client_id":client_id,"login":login,"user_id":"42","scopes":[],"expires_in":14000}),
                        )
                    }
                    "/oauth2/revoke" => (200, json!({})),
                    _ => (404, json!({})),
                };
                let _ = r.respond(Response::from_data(v.to_string()).with_status_code(code));
            }
        });
        (format!("http://127.0.0.1:{port}"), polls)
    }

    fn real_question() -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("demo/questions/bdl-a0042.wav"),
        )
        .unwrap()
    }

    fn server(
        name: &str,
        client_id: Option<&str>,
        auth_base: &str,
        open_names: bool,
    ) -> (AskServer, Arc<Mutex<Queue>>) {
        let dir = std::env::temp_dir().join(format!("avatar-ask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut q = Queue::load(dir);
        q.open = true;
        let q = Arc::new(Mutex::new(q));
        let cfg = AskConfig {
            port: 0,
            twitch_client_id: client_id.map(|s| s.to_string()),
            auth_base: auth_base.to_string(),
            scopes: String::new(),
            open_names,
            characters: vec![
                ("Dog".into(), "models/dog.glb".into()),
                ("Cat".into(), "models/cat.glb".into()),
            ],
            title: "Ask the dog".into(),
            font: None,
        };
        (start(cfg, q.clone()).unwrap(), q)
    }

    fn call(method: &str, url: &str, sess: Option<&str>, body: Option<Vec<u8>>) -> (u16, Value) {
        let a: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut r = match (method, body) {
            ("GET", _) => {
                let mut rq = a.get(url);
                if let Some(s) = sess {
                    rq = rq.header("X-Ask-Session", s);
                }
                rq.call().unwrap()
            }
            (_, b) => {
                let mut rq = a.post(url);
                if let Some(s) = sess {
                    rq = rq.header("X-Ask-Session", s);
                }
                rq.send(b.unwrap_or_default()).unwrap()
            }
        };
        let code = r.status().as_u16();
        let s = r.body_mut().read_to_string().unwrap_or_default();
        (code, serde_json::from_str(&s).unwrap_or(Value::Null))
    }

    #[test]
    fn twitch_login_then_ask_with_a_real_recording() {
        let (base, polls) = fake_twitch("client-abc", "viewer1");
        let (srv, q) = server("twitch", Some("client-abc"), &base, false);
        let u = |p: &str| format!("{}{}", srv.url.trim_end_matches('/'), p);
        let (_, info) = call("GET", &u("/api/info"), None, None);
        assert_eq!(info["login"], "twitch");
        assert_eq!(info["characters"][1], "Cat");
        // portraits for the picker (from assets/portraits)
        let (code, png) = call("GET", &u("/portrait/1.png"), None, None);
        assert_eq!(code, 200, "cat portrait");
        let _ = png;
        assert_eq!(call("GET", &u("/portrait/9.png"), None, None).0, 404);
        assert_eq!(call("GET", &u("/portrait/../x.png"), None, None).0, 404);
        assert!(info["me"].is_null());
        // can't ask before logging in
        assert_eq!(
            call("POST", &u("/api/ask?c=0"), None, Some(real_question())).0,
            401
        );
        let (code, start) = call("POST", &u("/api/login/twitch"), None, None);
        assert_eq!(code, 200, "{start}");
        assert_eq!(start["user_code"], "ABCDEFGH");
        let s = start["session"].as_str().unwrap().to_string();
        // the page polls; we only ask Twitch once per interval
        let mut state = String::new();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(150));
            let (_, p) = call("POST", &u("/api/login/poll"), Some(&s), None);
            state = p["state"].as_str().unwrap_or("").to_string();
            if state == "done" {
                assert_eq!(p["user"], "viewer1");
                break;
            }
        }
        assert_eq!(state, "done");
        assert_eq!(
            polls.load(Ordering::SeqCst),
            2,
            "pending once, then the token"
        );
        let (_, info) = call("GET", &u("/api/info"), Some(&s), None);
        assert_eq!(info["me"]["user"], "viewer1");
        assert_eq!(info["can_ask"]["ok"], true);
        // ask
        let (code, a) = call("POST", &u("/api/ask?c=1"), Some(&s), Some(real_question()));
        assert_eq!(code, 200, "{a}");
        let id = a["id"].as_u64().unwrap() as u32;
        {
            let q = q.lock().unwrap();
            let x = q.get(id).unwrap();
            assert_eq!(
                (
                    x.user.as_str(),
                    x.verified,
                    x.account.as_str(),
                    x.character.as_str(),
                    x.model.as_str()
                ),
                ("viewer1", true, "tw:viewer1", "Cat", "models/cat.glb")
            );
            assert!((x.secs - 2.58).abs() < 0.05, "{}", x.secs);
        }
        // a second one is refused while the first waits
        let (code, e) = call("POST", &u("/api/ask?c=1"), Some(&s), Some(real_question()));
        assert_eq!(code, 409, "{e}");
        let (_, m) = call("GET", &u("/api/mine"), Some(&s), None);
        assert_eq!(m["questions"][0]["state"], "waiting");
        assert_eq!(m["questions"][0]["position"], 1);
        // somebody else can't see or withdraw it
        let (code, other) = call("POST", &u("/api/login/twitch"), None, None);
        assert_eq!(code, 200);
        let other = other["session"].as_str().unwrap();
        assert_eq!(
            call("GET", &u("/api/mine"), Some(other), None).0,
            401,
            "not logged in yet"
        );
        assert_eq!(
            call(
                "POST",
                &u(&format!("/api/withdraw?id={id}")),
                Some("forged"),
                None
            )
            .0,
            401
        );
        // withdraw
        assert_eq!(
            call(
                "POST",
                &u(&format!("/api/withdraw?id={id}")),
                Some(&s),
                None
            )
            .0,
            200
        );
        assert_eq!(q.lock().unwrap().get(id).unwrap().status, Status::Skipped);
    }

    #[test]
    fn ask_page_has_no_control_commands_and_checks_uploads() {
        let (srv, q) = server("guard", None, TWITCH_AUTH, true);
        let u = |p: &str| format!("{}{}", srv.url.trim_end_matches('/'), p);
        // nothing from the control page exists here
        for p in ["/cmd/face:1", "/state", "/questions", "/q/1.wav"] {
            assert_eq!(call("GET", &u(p), None, None).0, 404, "{p}");
        }
        // the page itself, with a strict content policy
        let a: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let r = a.get(&u("/")).call().unwrap();
        let csp = r
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            csp.contains("default-src 'self'") && !csp.contains("unsafe"),
            "{csp}"
        );
        // open mode: typed names, checked
        assert_eq!(
            call(
                "POST",
                &u("/api/login/name"),
                None,
                Some(br#"{"name":"<script>"}"#.to_vec())
            )
            .0,
            400
        );
        let (code, l) = call(
            "POST",
            &u("/api/login/name"),
            None,
            Some(br#"{"name":"Guest_7"}"#.to_vec()),
        );
        assert_eq!(code, 200);
        let s = l["session"].as_str().unwrap().to_string();
        // Twitch login isn't offered without a client ID
        assert_eq!(call("POST", &u("/api/login/twitch"), None, None).0, 403);
        // garbage, an unknown character, a huge upload
        assert_eq!(
            call(
                "POST",
                &u("/api/ask?c=0"),
                Some(&s),
                Some(b"hello".to_vec())
            )
            .0,
            400
        );
        assert_eq!(
            call("POST", &u("/api/ask?c=9"), Some(&s), Some(real_question())).0,
            400
        );
        assert_eq!(
            call(
                "POST",
                &u("/api/ask?c=0"),
                Some(&s),
                Some(vec![0u8; 4_000_000])
            )
            .0,
            413
        );
        let (code, a) = call("POST", &u("/api/ask?c=0"), Some(&s), Some(real_question()));
        assert_eq!(code, 200, "{a}");
        let id = a["id"].as_u64().unwrap() as u32;
        assert!(!q.lock().unwrap().get(id).unwrap().verified);
        // closing the queue stops new questions
        q.lock().unwrap().open = false;
        let (_, info) = call("GET", &u("/api/info"), Some(&s), None);
        assert_eq!(info["open"], false);
        // with names turned off, nobody can log in
        srv.shared.cfg.lock().unwrap().open_names = false;
        assert_eq!(
            call(
                "POST",
                &u("/api/login/name"),
                None,
                Some(br#"{"name":"Other"}"#.to_vec())
            )
            .0,
            403
        );
    }
}
