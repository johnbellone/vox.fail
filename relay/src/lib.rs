//! warren-relay: lists running shows and relays their traffic.
//!
//! Every app connects out to the relay, so nobody needs a port forward, and the host uploads one
//! stream however many people watch.
//!
//! - `POST /sessions` `{title, character, public}` registers a show and returns its id, a join
//!   code and a host token.
//! - `GET /sessions` lists public shows with a live host as JSON; `GET /` shows the same list as
//!   a page.
//! - `GET /sessions/{id}/ws?role=host&token=…` is the host's connection;
//!   `role=viewer` (plus `code=…` for a private show) watches; `role=guest&code=…&name=…` joins
//!   as the guest. The host's app decides whether to let the guest on.
//! - `DELETE /sessions/{id}` with `Authorization: Bearer <token>` ends a show.
//!
//! The relay never reads the stream. It looks at one thing: the first byte of a binary message.
//! [`DROPPABLE`] (snapshots, voice) may be skipped for a viewer who falls behind; anything else is
//! reliable, and a viewer who can't keep up with reliable messages is disconnected. The host's
//! messages go to every viewer and the guest; the guest's go to the host. Text messages starting
//! `ping:` are answered `pong:` to the sender (for round-trip times). The relay itself tells the
//! host about the guest with JSON text: `{"relay":"guest_joined","name":…}`, `guest_left`, and
//! `{"relay":"viewers","n":…}`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// First byte of a binary message that may be skipped for a slow viewer.
pub const DROPPABLE: u8 = 0;
/// First byte of a binary message every viewer must get (events).
pub const RELIABLE: u8 = 1;

/// Messages waiting per connection before a viewer counts as falling behind.
const QUEUE: usize = 256;
/// Largest message accepted.
const MAX_MESSAGE: usize = 1 << 20;
/// Most shows at once.
const MAX_SESSIONS: usize = 1000;

#[derive(Clone)]
pub struct Relay {
    inner: Arc<Mutex<Inner>>,
    /// how long a show stays listed after its host disconnects
    expire: Duration,
    /// testing only: bad-network simulation (see [`Relay::simulate`])
    sim: Option<Sim>,
}

/// A bad network, for testing: every relayed message waits `delay` (± half of it, so they
/// arrive out of order) and droppable ones are lost with chance `loss`.
#[derive(Clone, Copy, Debug)]
pub struct Sim {
    pub delay: Duration,
    pub loss: f32,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Session>,
    next_peer: u64,
}

struct Session {
    title: String,
    character: String,
    public: bool,
    code: String,
    host_token: String,
    started: u64,
    /// when the host was last connected
    last_seen: Instant,
    host: Option<Peer>,
    guest: Option<(Peer, String)>,
    viewers: HashMap<u64, Peer>,
}

#[derive(Clone)]
struct Peer {
    id: u64,
    tx: mpsc::Sender<Message>,
}

#[derive(Deserialize)]
pub struct NewSession {
    pub title: String,
    #[serde(default)]
    pub character: String,
    #[serde(default = "yes")]
    pub public: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Created {
    pub id: String,
    pub code: String,
    pub host_token: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Listed {
    pub id: String,
    pub title: String,
    pub character: String,
    pub viewers: usize,
    pub guest: bool,
    pub started: u64,
}

#[derive(Deserialize)]
pub struct Join {
    role: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Host,
    Guest,
    Viewer,
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("random numbers");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Six characters with no look-alikes (no 0/O, 1/I/L).
fn random_code() -> String {
    const ABC: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut b = [0u8; 6];
    getrandom::fill(&mut b).expect("random numbers");
    b.iter()
        .map(|x| ABC[*x as usize % ABC.len()] as char)
        .collect()
}

/// Trim, drop control characters, and cut to `max` characters.
fn clean(s: &str, max: usize) -> String {
    s.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Constant-time comparison, so a token can't be guessed byte by byte from timings.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl Relay {
    pub fn new(expire: Duration) -> Relay {
        Relay {
            inner: Arc::new(Mutex::new(Inner::default())),
            expire,
            sim: None,
        }
    }

    /// Simulate a bad network on everything this relay passes on.
    pub fn simulate(mut self, sim: Sim) -> Relay {
        self.sim = Some(sim);
        self
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(page))
            .route("/sessions", post(create).get(list))
            .route("/sessions/{id}", axum::routing::delete(end))
            .route("/sessions/{id}/ws", get(connect))
            .with_state(self.clone())
    }

    /// Drop shows whose host has been gone longer than `expire`. Run every few seconds.
    pub fn reap(&self) {
        let mut inner = self.inner.lock().unwrap();
        let expire = self.expire;
        inner
            .sessions
            .retain(|_, s| s.host.is_some() || s.last_seen.elapsed() < expire);
    }

    fn listed(&self) -> Vec<Listed> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<Listed> = inner
            .sessions
            .iter()
            .filter(|(_, s)| s.public && s.host.is_some())
            .map(|(id, s)| Listed {
                id: id.clone(),
                title: s.title.clone(),
                character: s.character.clone(),
                viewers: s.viewers.len(),
                guest: s.guest.is_some(),
                started: s.started,
            })
            .collect();
        v.sort_by(|a, b| b.viewers.cmp(&a.viewers).then(a.started.cmp(&b.started)));
        v
    }
}

async fn create(State(relay): State<Relay>, Json(req): Json<NewSession>) -> Response {
    let title = clean(&req.title, 60);
    if title.is_empty() {
        return (StatusCode::BAD_REQUEST, "a title is needed").into_response();
    }
    let mut inner = relay.inner.lock().unwrap();
    if inner.sessions.len() >= MAX_SESSIONS {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many shows").into_response();
    }
    let created = Created {
        id: random_hex(8),
        code: random_code(),
        host_token: random_hex(24),
    };
    inner.sessions.insert(
        created.id.clone(),
        Session {
            title,
            character: clean(&req.character, 40),
            public: req.public,
            code: created.code.clone(),
            host_token: created.host_token.clone(),
            started: now_unix(),
            last_seen: Instant::now(),
            host: None,
            guest: None,
            viewers: HashMap::new(),
        },
    );
    Json(created).into_response()
}

async fn list(State(relay): State<Relay>) -> Json<Vec<Listed>> {
    Json(relay.listed())
}

async fn page(State(relay): State<Relay>) -> Html<String> {
    let rows: String = relay
        .listed()
        .iter()
        .map(|s| {
            format!(
                "<li><b>{}</b> <span>{}</span> <small>{} watching{}</small> <code>{}</code></li>",
                html_escape(&s.title),
                html_escape(&s.character),
                s.viewers,
                if s.guest { ", with a guest" } else { "" },
                html_escape(&s.id)
            )
        })
        .collect();
    let body = if rows.is_empty() {
        "<p>No shows are live right now.</p>".to_string()
    } else {
        format!("<ul>{rows}</ul>")
    };
    Html(format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
         <meta http-equiv=refresh content=10><title>Live shows</title>\
         <style>body{{font:16px system-ui;margin:16px;max-width:720px;background:#fff;color:#111}}\
         li{{margin:8px 0}}small{{color:#555}}code{{color:#777;font-size:12px}}\
         @media (prefers-color-scheme:dark){{body{{background:#111;color:#eee}}small,code{{color:#aaa}}}}</style>\
         <h1>Live shows</h1>{body}"
    ))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn end(State(relay): State<Relay>, Path(id): Path<String>, headers: HeaderMap) -> StatusCode {
    let mut inner = relay.inner.lock().unwrap();
    let Some(s) = inner.sessions.get(&id) else {
        return StatusCode::NOT_FOUND;
    };
    if !bearer(&headers).is_some_and(|t| same(t, &s.host_token)) {
        return StatusCode::UNAUTHORIZED;
    }
    // dropping the session drops every sender, which closes every connection
    inner.sessions.remove(&id);
    StatusCode::NO_CONTENT
}

async fn connect(
    State(relay): State<Relay>,
    Path(id): Path<String>,
    Query(join): Query<Join>,
    ws: WebSocketUpgrade,
) -> Response {
    let role = match join.role.as_str() {
        "host" => Role::Host,
        "guest" => Role::Guest,
        "viewer" => Role::Viewer,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "role must be host, guest or viewer",
            )
                .into_response();
        }
    };
    // check before upgrading, so a refusal is a plain HTTP status
    {
        let inner = relay.inner.lock().unwrap();
        let Some(s) = inner.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "no such show").into_response();
        };
        let ok = match role {
            Role::Host => same(&join.token, &s.host_token),
            Role::Guest => same(&join.code.to_uppercase(), &s.code),
            Role::Viewer => s.public || same(&join.code.to_uppercase(), &s.code),
        };
        if !ok {
            return (StatusCode::FORBIDDEN, "wrong token or code").into_response();
        }
        if role == Role::Guest && s.guest.is_some() {
            return (StatusCode::CONFLICT, "the guest spot is taken").into_response();
        }
    }
    let name = clean(&join.name, 40);
    ws.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| run(relay, id, role, name, socket))
}

/// Send to one peer: a full queue skips a droppable message and drops the peer otherwise.
/// Returns false when the peer should be disconnected.
fn offer(peer: &Peer, msg: &Message) -> bool {
    match peer.tx.try_send(msg.clone()) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            matches!(msg, Message::Binary(b) if b.first() == Some(&DROPPABLE))
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

fn control(json: serde_json::Value) -> Message {
    Message::Text(json.to_string().into())
}

async fn run(relay: Relay, id: String, role: Role, name: String, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(QUEUE);
    let me = {
        let mut inner = relay.inner.lock().unwrap();
        inner.next_peer += 1;
        let peer = Peer {
            id: inner.next_peer,
            tx,
        };
        let Some(s) = inner.sessions.get_mut(&id) else {
            return;
        };
        match role {
            Role::Host => {
                // a reconnecting host replaces the old connection
                s.host = Some(peer.clone());
                s.last_seen = Instant::now();
                if let Some((_, n)) = &s.guest {
                    let _ = peer.tx.try_send(control(
                        serde_json::json!({"relay":"guest_joined","name":n}),
                    ));
                }
                let _ = peer.tx.try_send(control(
                    serde_json::json!({"relay":"viewers","n":s.viewers.len()}),
                ));
            }
            Role::Guest => {
                if s.guest.is_some() {
                    return;
                }
                s.guest = Some((peer.clone(), name.clone()));
                if let Some(h) = &s.host {
                    offer(
                        h,
                        &control(serde_json::json!({"relay":"guest_joined","name":name})),
                    );
                }
            }
            Role::Viewer => {
                s.viewers.insert(peer.id, peer.clone());
                if let Some(h) = &s.host {
                    offer(
                        h,
                        &control(serde_json::json!({"relay":"viewers","n":s.viewers.len()})),
                    );
                }
            }
        }
        // keep only a weak sender here: when the show ends (or this peer is dropped for
        // falling behind), every strong sender goes and the writer closes the connection
        (peer.id, peer.tx.downgrade())
    };
    let (me_id, me_tx) = me;

    // writer: whatever is queued for this connection
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    // reader: route what this connection sends
    while let Some(Ok(msg)) = stream.next().await {
        match &msg {
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Text(t) if t.as_str().starts_with("ping:") => {
                let reply = format!("pong:{}", &t.as_str()[5..]);
                if let Some(tx) = me_tx.upgrade() {
                    let _ = tx.try_send(Message::Text(reply.into()));
                }
                continue;
            }
            _ => {}
        }
        if let Some(sim) = relay.sim {
            let mut r = [0u8; 4];
            getrandom::fill(&mut r).ok();
            let u = u32::from_le_bytes(r) as f32 / u32::MAX as f32;
            let droppable = matches!(&msg, Message::Binary(b) if b.first() == Some(&DROPPABLE));
            if droppable && u < sim.loss {
                continue;
            }
            let wait = sim.delay.mul_f32(0.5 + u);
            let (relay, id) = (relay.clone(), id.clone());
            tokio::spawn(async move {
                tokio::time::sleep(wait).await;
                route(&relay, &id, role, me_id, &msg);
            });
            continue;
        }
        if !route(&relay, &id, role, me_id, &msg) {
            break;
        }
    }

    // leaving
    {
        let mut inner = relay.inner.lock().unwrap();
        if let Some(s) = inner.sessions.get_mut(&id) {
            match role {
                Role::Host => {
                    if s.host.as_ref().map(|h| h.id) == Some(me_id) {
                        s.host = None;
                        s.last_seen = Instant::now();
                    }
                }
                Role::Guest => {
                    if s.guest.as_ref().map(|(g, _)| g.id) == Some(me_id) {
                        s.guest = None;
                        if let Some(h) = &s.host {
                            offer(h, &control(serde_json::json!({"relay":"guest_left"})));
                        }
                    }
                }
                Role::Viewer => {
                    s.viewers.remove(&me_id);
                    if let Some(h) = &s.host {
                        offer(
                            h,
                            &control(serde_json::json!({"relay":"viewers","n":s.viewers.len()})),
                        );
                    }
                }
            }
        }
    }
    writer.abort();
}

/// Pass a message on from `role`. Returns false when this connection should stop (the show is
/// gone, or a newer host connection replaced it).
fn route(relay: &Relay, id: &str, role: Role, me_id: u64, msg: &Message) -> bool {
    let mut inner = relay.inner.lock().unwrap();
    let Some(s) = inner.sessions.get_mut(id) else {
        return false;
    };
    match role {
        Role::Host => {
            if s.host.as_ref().map(|h| h.id) != Some(me_id) {
                return false; // replaced by a newer host connection
            }
            s.last_seen = Instant::now();
            s.viewers.retain(|_, v| offer(v, msg));
            if let Some((g, _)) = &s.guest {
                if !offer(g, msg) {
                    s.guest = None;
                }
            }
        }
        Role::Guest => {
            if let Some(h) = &s.host {
                offer(h, msg);
            }
        }
        Role::Viewer => {}
    }
    true
}
