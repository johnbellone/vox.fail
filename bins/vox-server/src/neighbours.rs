//! Neighbouring worlds and the links to them (ADR 0005, step 2; bead vox-7jh.1).
//!
//! A world's neighbours are the atlas tiles next to it whose edges are open, and the worlds its
//! doorways lead to on this same host (a relative url: `":7781"` or `"/w/<id>/ws"`). Each pair
//! keeps one WebSocket: the world whose id sorts lower dials (with `vox-link`, which reconnects
//! with backoff), the other answers on `/link`. Both ends prove they know the world secret
//! ([`vox_core::neighbour`]) without sending it, and a heartbeat shows the link is alive. Nothing
//! else travels over it yet; the border margin is the next step (vox-7jh.2).
//!
//! Only our own host's worlds are dialled: a whole address on another host is never a
//! neighbour, so a proof made from the secret is only ever offered to a world we run beside.
//!
//! The world thread works out the [`Target`]s (it owns the portals and settings) and hands
//! them over a `watch` channel; [`keep_links`], a task of its own, turns them into links, and
//! [`Links`] holds what is linked, for the status page and tests. The world thread stays the
//! single owner of the world's state.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use vox_core::neighbour::{self, Answer, Hail, LINK_PROTOCOL, Side};

/// How often the dialler pings, and how long without hearing from the far end counts as down.
const PING_EVERY: Duration = Duration::from_secs(5);
const SILENT: Duration = Duration::from_secs(15);
/// How long each step of the handshake may take.
const HANDSHAKE_WITHIN: Duration = Duration::from_secs(10);
/// After a refusal (a goodbye, a wrong proof, a silent link), how long before dialling again:
/// `vox-link` only backs off when it can't connect, and a refused hail connects every time.
const AFTER_REFUSAL: Duration = Duration::from_secs(5);
/// How often worlds known only by a url are looked up (their status page names them), and how
/// long a name found is trusted before it is looked up again.
const LOOKUP_EVERY: Duration = Duration::from_secs(5);
const LOOKUP_KEPT: Duration = Duration::from_secs(30);
/// How often the link task looks at its links.
const STEP: Duration = Duration::from_millis(50);

/// A world this one meets: its id when known (an atlas tile), or a status page that names it
/// (a doorway's url may be an alias, such as a port), and where its `/link` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub id: Option<String>,
    pub status: Option<String>,
    pub link: String,
}

/// The targets for a world: `tiles` are the ids of the atlas tiles over its open edges, `urls`
/// its doorways' destinations. `base` is where this world's host answers (`ws://127.0.0.1:7790`);
/// `hosted` says the host is a `vox-host`, where `":7781"` is the alias of a world on the same
/// base, not another server on that port. Destinations that aren't relative (another host, a
/// dungeon, somewhere in this world) are left out.
pub(crate) fn plan(tiles: &[String], urls: &[String], base: &str, hosted: bool) -> Vec<Target> {
    let base = base.trim_end_matches('/');
    let http = |ws: &str| ws.replacen("wss://", "https://", 1).replacen("ws://", "http://", 1);
    let mut out: Vec<Target> = tiles.iter().filter(|t| neighbour::valid_world(t)).map(|t| Target { id: Some(t.clone()), status: None, link: format!("{base}/w/{t}/link") }).collect();
    for url in urls {
        let t = if let Some(port) = url.strip_prefix(':') {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            if hosted {
                Target { id: None, status: Some(format!("{}/w/{port}/", http(base))), link: format!("{base}/w/{port}/link") }
            } else {
                // another server on this machine: the same scheme and host, that port
                let Some((scheme, host)) = scheme_host(base) else { continue };
                Target { id: None, status: Some(format!("{}{host}:{port}/", http(scheme))), link: format!("{scheme}{host}:{port}/link") }
            }
        } else if let Some(name) = url.strip_prefix("/w/").and_then(|r| r.strip_suffix("/ws")) {
            if !neighbour::valid_world(name) {
                continue;
            }
            Target { id: None, status: Some(format!("{}/w/{name}/", http(base))), link: format!("{base}/w/{name}/link") }
        } else {
            continue;
        };
        out.push(t);
    }
    out.dedup();
    out
}

/// `ws://127.0.0.1:7790/x` as (`ws://`, `127.0.0.1`), `ws://[::1]:7790` as (`ws://`, `[::1]`).
fn scheme_host(base: &str) -> Option<(&str, &str)> {
    let i = base.find("://")? + 3;
    let (scheme, rest) = base.split_at(i);
    let end = if rest.starts_with('[') { rest.find(']')? + 1 } else { rest.find([':', '/']).unwrap_or(rest.len()) };
    Some((scheme, &rest[..end])).filter(|(_, h)| !h.is_empty())
}

/// A neighbour as the status page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbour {
    pub world: String,
    /// this world dials it (its id sorts after ours); otherwise it dials us
    pub dials: bool,
    pub linked: bool,
}

#[derive(Debug, Default)]
struct Entry {
    dials: bool,
    linked: bool,
    /// which link set `linked` (a listener that ends late mustn't mark a newer link down)
    link: u64,
}

/// This world's neighbours and which are linked, shared by the link task, the `/link` handler
/// and the status page.
pub(crate) struct Links {
    pub world: String,
    /// None: no links (no world secret, or nowhere to dial from)
    pub secret: Option<String>,
    table: Mutex<BTreeMap<String, Entry>>,
    next: AtomicU64,
}

impl Links {
    pub fn new(world: String, secret: Option<String>) -> Links {
        Links { world, secret, table: Mutex::new(BTreeMap::new()), next: AtomicU64::new(1) }
    }

    pub fn list(&self) -> Vec<Neighbour> {
        let t = self.table.lock().unwrap_or_else(|e| e.into_inner());
        t.iter().map(|(w, e)| Neighbour { world: w.clone(), dials: e.dials, linked: e.linked }).collect()
    }

    /// The neighbours now: new ones are added (not linked), gone ones forgotten.
    fn set(&self, ids: impl Iterator<Item = String>) {
        let mut t = self.table.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = BTreeMap::new();
        for id in ids {
            let e = t.remove(&id).unwrap_or_default();
            let dials = self.world < id;
            next.insert(id, Entry { dials, ..e });
        }
        *t = next;
    }

    /// May `from` dial us? Only a neighbour whose id sorts before ours: one link per pair.
    fn listens_for(&self, from: &str) -> bool {
        let t = self.table.lock().unwrap_or_else(|e| e.into_inner());
        t.get(from).is_some_and(|e| !e.dials)
    }

    /// `id`'s link is up; returns its number (for [`Links::down`]), or None if `id` is no longer
    /// a neighbour.
    fn up(&self, id: &str) -> Option<u64> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let mut t = self.table.lock().unwrap_or_else(|e| e.into_inner());
        let e = t.get_mut(id)?;
        e.linked = true;
        e.link = n;
        Some(n)
    }

    /// Link `n` to `id` is down (a newer link, if there is one, stays up).
    fn down(&self, id: &str, n: u64) {
        let mut t = self.table.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = t.get_mut(id)
            && e.link == n
        {
            e.linked = false;
        }
    }
}

/// A number used once in a handshake, so a proof can't be replayed: from the operating system's
/// random source. None when there is none: that handshake is given up rather than run with a
/// guessable nonce.
fn nonce() -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    getrandom::fill(&mut out).ok()?;
    Some(out)
}

fn binary(m: &Answer) -> Message {
    Message::Binary(neighbour::encode(m).into())
}

/// The listening end of a link (`GET /link`): check the hail, prove ourselves, check the
/// dialler's proof, then answer pings until it goes quiet, closes, or this world stops.
/// Most handshakes not yet proved at once, across every world in this process: a hail that
/// can't prove itself holds a socket for up to the handshake's time limit, so they are capped.
const UNPROVED_MOST: usize = 32;
static UNPROVED: AtomicUsize = AtomicUsize::new(0);

/// One handshake counted in [`UNPROVED`] while it lasts.
struct Unproved;

impl Unproved {
    fn take() -> Option<Unproved> {
        let n = UNPROVED.fetch_add(1, Ordering::AcqRel);
        if n >= UNPROVED_MOST {
            UNPROVED.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Unproved)
    }
}

impl Drop for Unproved {
    fn drop(&mut self) {
        UNPROVED.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) async fn listen(links: Arc<Links>, world: mpsc::Sender<crate::Event>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    // too many unproved at once: this one waits for another day (the dialler retries)
    let Some(unproved) = Unproved::take() else {
        let _ = sink.send(binary(&Answer::Bye { why: "busy".into() })).await;
        let _ = sink.close().await;
        return;
    };
    let me = links.world.clone();
    let refuse = async |sink: &mut futures_util::stream::SplitSink<WebSocket, Message>, from: &str, why: String| {
        if !from.is_empty() {
            eprintln!("{me}: refused a link from {from}: {why}");
        }
        let _ = sink.send(binary(&Answer::Bye { why })).await;
        let _ = sink.close().await;
    };
    let Some(secret) = links.secret.clone() else {
        return refuse(&mut sink, "", "this world keeps no links".into()).await;
    };
    let mut next_hail = async || -> Option<Result<Hail, String>> {
        loop {
            match tokio::time::timeout(HANDSHAKE_WITHIN, stream.next()).await.ok()?? {
                Ok(Message::Binary(b)) => return Some(neighbour::decode_hail(&b)),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    };
    let (from, theirs) = match next_hail().await {
        Some(Ok(Hail::Hello { protocol, from, nonce })) => {
            if protocol != LINK_PROTOCOL {
                return refuse(&mut sink, &from, format!("this world speaks link protocol {LINK_PROTOCOL}, not {protocol}")).await;
            }
            (from, nonce)
        }
        _ => return refuse(&mut sink, "", "say hello first".into()).await,
    };
    if !links.listens_for(&from) {
        return refuse(&mut sink, &from, format!("not a neighbour of {me} that dials it")).await;
    }
    let Some(ours) = nonce() else {
        return refuse(&mut sink, &from, "no random source here".into()).await;
    };
    let challenge = Answer::Challenge { world: me.clone(), nonce: ours, proof: neighbour::proof(&secret, Side::Listener, &theirs, &me, &from) };
    if sink.send(binary(&challenge)).await.is_err() {
        return;
    }
    match next_hail().await {
        Some(Ok(Hail::Proof { proof })) if neighbour::same(&proof, &neighbour::proof(&secret, Side::Dialler, &ours, &from, &me)) => {}
        _ => return refuse(&mut sink, &from, "wrong proof".into()).await,
    }
    // proved: no longer counted against the cap
    drop(unproved);
    if sink.send(binary(&Answer::Linked)).await.is_err() {
        return;
    }
    let Some(n) = links.up(&from) else { return };
    eprintln!("{me}: linked with {from}");
    let mut heard = Instant::now();
    loop {
        if world.is_closed() || heard.elapsed() > SILENT {
            break;
        }
        match tokio::time::timeout(Duration::from_secs(1), stream.next()).await {
            Err(_) => {}
            Ok(Some(Ok(Message::Binary(b)))) => match neighbour::decode_hail(&b) {
                Ok(Hail::Ping { t }) => {
                    heard = Instant::now();
                    if sink.send(binary(&Answer::Pong { t })).await.is_err() {
                        break;
                    }
                }
                // nothing else is spoken once linked
                _ => break,
            },
            Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => break,
            Ok(Some(Ok(_))) => {}
        }
    }
    links.down(&from, n);
    eprintln!("{me}: link with {from} down");
    let _ = sink.close().await;
}

/// The dialling side's `vox-link` setup: one address, and what was queued while disconnected is
/// thrown away (each connection starts with a fresh hello).
struct To(String);

impl vox_link::Dial<()> for To {
    fn address(&mut self, _: &dyn Fn(())) -> Result<String, String> {
        Ok(self.0.clone())
    }
    fn drop_stale(&self) -> bool {
        true
    }
}

#[derive(Debug, PartialEq)]
enum Phase {
    /// not connected (or connecting)
    Away,
    /// hello sent with this nonce
    Hailed([u8; 32]),
    /// our proof sent
    Proved,
    Linked(u64),
}

/// One link this world dials.
struct Dialler {
    link: vox_link::Link<()>,
    url: String,
    phase: Phase,
    heard: Instant,
    pinged: Instant,
}

impl Dialler {
    fn start(me: &str, id: &str, url: &str) -> Dialler {
        let link = vox_link::spawn(&format!("link-{me}-{id}"), To(url.to_string()));
        Dialler { link, url: url.to_string(), phase: Phase::Away, heard: Instant::now(), pinged: Instant::now() }
    }

    /// Handle what has arrived and ping when due. Returns Some(why) when the link should be
    /// dropped and dialled again after [`AFTER_REFUSAL`].
    fn step(&mut self, links: &Links, id: &str, secret: &str, clock: Instant) -> Option<String> {
        let me = &links.world;
        for ev in self.link.drain() {
            match ev {
                vox_link::Event::Status(s) if s == "Connected" => {
                    let Some(n) = nonce() else { return Some("no random source here".into()) };
                    self.link.send(neighbour::encode(&Hail::Hello { protocol: LINK_PROTOCOL, from: me.clone(), nonce: n }));
                    self.phase = Phase::Hailed(n);
                    self.heard = Instant::now();
                }
                vox_link::Event::Status(_) => self.lost(links, id),
                vox_link::Event::Data(b, _) => {
                    let answer = match neighbour::decode_answer(&b) {
                        Ok(a) => a,
                        Err(e) => return Some(format!("a garbled answer ({e})")),
                    };
                    self.heard = Instant::now();
                    match (answer, &self.phase) {
                        (Answer::Challenge { world, nonce: theirs, proof }, Phase::Hailed(ours)) => {
                            // the listener must be the world we meant, and know the secret
                            if world != id || !neighbour::same(&proof, &neighbour::proof(secret, Side::Listener, ours, &world, me)) {
                                return Some(format!("{world} couldn't prove it is our neighbour"));
                            }
                            self.link.send(neighbour::encode(&Hail::Proof { proof: neighbour::proof(secret, Side::Dialler, &theirs, me, &world) }));
                            self.phase = Phase::Proved;
                        }
                        (Answer::Linked, Phase::Proved) => {
                            let Some(n) = links.up(id) else { return Some("no longer a neighbour".into()) };
                            eprintln!("{me}: linked with {id}");
                            self.phase = Phase::Linked(n);
                            self.pinged = Instant::now();
                        }
                        (Answer::Pong { .. }, Phase::Linked(_)) => {}
                        (Answer::Bye { why }, _) => return Some(why),
                        (a, _) => return Some(format!("{a:?} out of turn")),
                    }
                }
                vox_link::Event::Text(_) | vox_link::Event::App(()) => {}
            }
        }
        if self.phase != Phase::Away && self.heard.elapsed() > SILENT {
            return Some("silent".into());
        }
        if matches!(self.phase, Phase::Linked(_)) && self.pinged.elapsed() >= PING_EVERY {
            self.pinged = Instant::now();
            let t = u64::try_from(clock.elapsed().as_millis()).unwrap_or(u64::MAX);
            self.link.send(neighbour::encode(&Hail::Ping { t }));
        }
        None
    }

    /// The connection went: if it was linked, it's down (vox-link dials again by itself).
    fn lost(&mut self, links: &Links, id: &str) {
        if let Phase::Linked(n) = self.phase {
            links.down(id, n);
            eprintln!("{}: link with {id} down", links.world);
        }
        self.phase = Phase::Away;
    }
}

/// What a status page said a world is called: its id (None: no answer, or not a world id), and
/// when it was asked.
struct Lookup {
    id: Option<String>,
    at: Instant,
    asking: bool,
}

/// Keep the links to this world's neighbours, as `plan` says they are, until the world stops
/// (`world` closes). Dials the neighbours whose ids sort after ours; the others dial us.
pub(crate) async fn keep_links(links: Arc<Links>, plan: watch::Receiver<Vec<Target>>, world: mpsc::Sender<crate::Event>) {
    let Some(secret) = links.secret.clone() else { return };
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(2)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("{}: no links to neighbours: {e}", links.world),
    };
    let lookups: Arc<Mutex<HashMap<String, Lookup>>> = Arc::default();
    let mut dialling: HashMap<String, Dialler> = HashMap::new();
    let mut wait_until: HashMap<String, Instant> = HashMap::new();
    let mut looked = None::<Instant>;
    let clock = Instant::now();
    let mut every = tokio::time::interval(STEP);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    while !world.is_closed() {
        every.tick().await;
        let targets = plan.borrow().clone();
        // worlds known by a url: ask their status pages what they're called, now and then
        if looked.is_none_or(|t: Instant| t.elapsed() >= LOOKUP_EVERY) {
            looked = Some(Instant::now());
            let mut l = lookups.lock().unwrap_or_else(|e| e.into_inner());
            l.retain(|url, _| targets.iter().any(|t| t.status.as_ref() == Some(url)));
            for url in targets.iter().filter(|t| t.id.is_none()).filter_map(|t| t.status.clone()) {
                let due = l.get(&url).is_none_or(|k| !k.asking && (k.id.is_none() || k.at.elapsed() >= LOOKUP_KEPT));
                if !due {
                    continue;
                }
                let had = l.get(&url).and_then(|k| k.id.clone());
                l.insert(url.clone(), Lookup { id: had, at: Instant::now(), asking: true });
                let (http, lookups) = (http.clone(), lookups.clone());
                tokio::spawn(async move {
                    let got = match http.get(&url).send().await {
                        Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
                        _ => None,
                    };
                    let id = got.and_then(|v| v["world"].as_str().map(String::from)).filter(|w| neighbour::valid_world(w));
                    let mut l = lookups.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(k) = l.get_mut(&url) {
                        *k = Lookup { id, at: Instant::now(), asking: false };
                    }
                });
            }
        }
        // the neighbours now, by id, with where each answers
        let mut want: BTreeMap<String, String> = BTreeMap::new();
        {
            let l = lookups.lock().unwrap_or_else(|e| e.into_inner());
            for t in &targets {
                let id = t.id.clone().or_else(|| l.get(t.status.as_ref()?)?.id.clone());
                if let Some(id) = id.filter(|id| *id != links.world) {
                    want.entry(id).or_insert_with(|| t.link.clone());
                }
            }
        }
        links.set(want.keys().cloned());
        // links to worlds no longer neighbours (or now somewhere else) are dropped
        dialling.retain(|id, d| want.get(id) == Some(&d.url));
        wait_until.retain(|id, _| want.contains_key(id));
        for (id, url) in want.iter().filter(|(id, _)| links.world < **id) {
            if !dialling.contains_key(id) && wait_until.get(id).is_none_or(|t| Instant::now() >= *t) {
                dialling.insert(id.clone(), Dialler::start(&links.world, id, url));
            }
        }
        let mut dropped = Vec::new();
        for (id, d) in dialling.iter_mut() {
            if let Some(why) = d.step(&links, id, &secret, clock) {
                d.lost(&links, id);
                eprintln!("{}: link to {id} refused: {why}", links.world);
                dropped.push(id.clone());
            }
        }
        for id in dropped {
            dialling.remove(&id);
            wait_until.insert(id, Instant::now() + AFTER_REFUSAL);
        }
    }
    // dropping the diallers stops their threads
    for (id, mut d) in dialling {
        d.lost(&links, &id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_are_only_on_this_host() {
        let tiles = vec!["atlas-earth-1-0".to_string()];
        let urls: Vec<String> = [":7781", "/w/yard/ws", "wss://elsewhere.example/w/x/ws", "dungeon:crypt", ":x", "/w/../ws", ":"].map(String::from).to_vec();
        let hosted = plan(&tiles, &urls, "ws://127.0.0.1:7790", true);
        assert_eq!(
            hosted,
            vec![
                Target { id: Some("atlas-earth-1-0".into()), status: None, link: "ws://127.0.0.1:7790/w/atlas-earth-1-0/link".into() },
                Target { id: None, status: Some("http://127.0.0.1:7790/w/7781/".into()), link: "ws://127.0.0.1:7790/w/7781/link".into() },
                Target { id: None, status: Some("http://127.0.0.1:7790/w/yard/".into()), link: "ws://127.0.0.1:7790/w/yard/link".into() },
            ]
        );
        // a lone server: `:7781` is the server on that port of this machine
        let lone = plan(&[], &[":7781".into()], "ws://127.0.0.1:7780", false);
        assert_eq!(lone, vec![Target { id: None, status: Some("http://127.0.0.1:7781/".into()), link: "ws://127.0.0.1:7781/link".into() }]);
        let v6 = plan(&[], &[":7781".into()], "ws://[::1]:7780/", false);
        assert_eq!(v6[0].link, "ws://[::1]:7781/link");
    }

    #[test]
    fn the_lower_id_dials() {
        let l = Links::new("atlas-earth-1-0".into(), Some("s".into()));
        l.set(["atlas-earth-0-0".to_string(), "atlas-earth-2-0".to_string()].into_iter());
        assert!(l.listens_for("atlas-earth-0-0"));
        assert!(!l.listens_for("atlas-earth-2-0"), "we dial that one");
        assert!(!l.listens_for("yard"), "not a neighbour");
        let n = l.up("atlas-earth-0-0").unwrap();
        let m = l.up("atlas-earth-0-0").unwrap();
        l.down("atlas-earth-0-0", n);
        assert!(l.list()[0].linked, "the newer link stays up");
        l.down("atlas-earth-0-0", m);
        assert!(!l.list()[0].linked);
    }

    #[test]
    fn nonces_differ() {
        assert_ne!(nonce().unwrap(), nonce().unwrap());
    }
}
