//! Shows over the network, through vox-relay (`relay/`).
//!
//! - **Host:** `net:host` (or `net:host:My title`) registers your show with the relay set by
//!   `relay_url=` (default `http://127.0.0.1:7779`) and streams it: your avatar's pose 30 times a
//!   second, the scene (character, background, set, lighting, what you wear) when it changes and
//!   every 2 s, and lighting cues as they happen. `net:stop` ends it. The control page shows the
//!   join code, the watch link and how many are watching.
//! - **Guest:** `--join http://relay:7779/<session id> --code ABC123 [--name Sam]` joins a show
//!   as its guest. Your own character stays yours (your mic, face capture and reactions) and
//!   stands on the host's left; the host appears beside you, and the host's camera frames you
//!   both. The host lets you on with `net:guest:on` (control page) and off with `net:guest:off`.
//! - **Watch:** `--watch http://relay:7779/<session id>` (plus `--code ABC123` for a private
//!   show) runs the app as a viewer: the host's scene rebuilt with the same commands, the host on
//!   your character and the guest beside them, exactly as streamed, 100 ms behind.
//!
//! Every avatar shown from the network is drawn from its stream of poses, blended between the
//! two around "now minus 100 ms". The host passes the guest's pose on to viewers on the host's
//! own clock, so viewers see what the host sees.
//!
//! The network runs on its own thread (blocking tungstenite, like stream chat); Bevy systems
//! only swap messages with it through channels.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use bevy::{
    gltf::{Gltf, GltfNode},
    prelude::*,
};

use crate::proto::{self, GUEST, HOST, Msg};
use crate::sim::{HZ, Tick};

#[allow(unused_imports)]
pub(crate) use vox_world::pose::{DELAY, Playout, Remote, Rig, Stream, WORLD_PLAYER, capture, debug, load_remotes, put, rig};
/// Ticks between poses (120 / 30).
const POSE_EVERY: u64 = (HZ as u64) / proto::POSE_HZ;
/// The scene (and a guest's hello) is re-sent this often, so anyone who joins late catches up.
const SCENE_EVERY: f32 = 2.0;
/// Where the guest stands: the co-host's spot on your left, turned toward you.
fn guest_spot() -> Transform {
    Transform::from_xyz(crate::cohost::COHOST_X, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.38))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    Host,
    Viewer,
    Guest,
}

/// What the network thread hands back.
enum Incoming {
    Data(Vec<u8>, Instant),
    Notice(serde_json::Value),
    Created { id: String, code: String },
    Status(String),
}

struct Link {
    out: mpsc::Sender<Vec<u8>>,
    inbox: Mutex<mpsc::Receiver<Incoming>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Resource)]
pub struct Net {
    pub relay: String,
    pub role: Option<Role>,
    link: Option<Link>,
    pub status: String,
    pub session: String,
    pub code: String,
    pub viewers: usize,
    /// host: the guest's name while one is connected
    pub guest: Option<String>,
    /// host: the guest is let on; guest and viewers: the guest is on stage
    pub guest_on: bool,
    /// your name as a guest
    pub name: String,
    started: Instant,
    /// your own avatar's rig
    body: Option<(Entity, Rig)>,
    hello: Option<proto::Hello>,
    resend: bool,
    last_pose_tick: u64,
    scene_sent: Vec<String>,
    scene_at: f32,
    cues: Vec<u8>,
    // receiving
    streams: [Stream; 2],
    remotes: [Option<Entity>; 2],
    scene_applied: Vec<String>,
    pending_cues: Vec<(u64, u8)>,
    // voices
    voice_out: Option<crate::netvoice::Sender>,
    player: Option<crate::netvoice::Player>,
    /// `net_output=`: where other voices play (default output if unset)
    pub voice_output: Option<String>,
}

impl Net {
    pub fn new(relay: String) -> Net {
        Net {
            relay: relay.trim_end_matches('/').to_string(),
            role: None,
            link: None,
            status: String::new(),
            session: String::new(),
            code: String::new(),
            viewers: 0,
            guest: None,
            guest_on: false,
            name: "Guest".into(),
            started: Instant::now(),
            body: None,
            hello: None,
            resend: false,
            last_pose_tick: 0,
            scene_sent: vec![],
            scene_at: f32::NEG_INFINITY,
            cues: vec![],
            streams: Default::default(),
            remotes: [None, None],
            scene_applied: vec![],
            pending_cues: vec![],
            voice_out: None,
            player: None,
            voice_output: None,
        }
    }

    /// Start hosting a show called `title`.
    pub fn host(&mut self, title: &str) -> String {
        let title = if title.trim().is_empty() { "Live show" } else { title.trim() };
        self.link = Some(spawn_link(Role::Host, self.relay.clone(), title.to_string(), String::new(), String::new(), String::new()));
        self.role = Some(Role::Host);
        self.scene_sent.clear();
        self.hello = None;
        self.status = format!("Connecting to {}", self.relay);
        self.status.clone()
    }

    fn split_url(url: &str) -> Result<(String, String), String> {
        let url = url.trim().trim_end_matches('/');
        let (relay, session) = url.rsplit_once('/').ok_or("expected http://relay:port/<session id>")?;
        Ok((relay.trim_end_matches("/sessions").to_string(), session.to_string()))
    }

    /// Watch the show at `url` (`http://relay:port/<session id>`).
    pub fn watch(&mut self, url: &str, code: &str) -> Result<(), String> {
        let (relay, session) = Self::split_url(url)?;
        self.relay = relay;
        self.session = session;
        self.link = Some(spawn_link(Role::Viewer, self.relay.clone(), String::new(), code.to_string(), self.session.clone(), String::new()));
        self.role = Some(Role::Viewer);
        self.status = "Connecting".into();
        Ok(())
    }

    /// Join the show at `url` as its guest.
    pub fn join(&mut self, url: &str, code: &str, name: &str) -> Result<(), String> {
        let (relay, session) = Self::split_url(url)?;
        if !name.trim().is_empty() {
            self.name = name.trim().chars().take(40).collect();
        }
        self.relay = relay;
        self.session = session;
        self.link = Some(spawn_link(Role::Guest, self.relay.clone(), String::new(), code.to_string(), self.session.clone(), self.name.clone()));
        self.role = Some(Role::Guest);
        self.status = "Connecting".into();
        Ok(())
    }

    pub fn stop(&mut self) -> String {
        self.link = None;
        self.voice_out = None;
        self.player = None;
        self.role = None;
        self.code.clear();
        self.viewers = 0;
        self.guest = None;
        self.guest_on = false;
        for s in &mut self.streams {
            s.clear();
        }
        self.status = "Show ended".into();
        self.status.clone()
    }

    pub fn command(&mut self, arg: &str) -> Option<String> {
        let (verb, rest) = arg.split_once(':').unwrap_or((arg, ""));
        Some(match verb {
            "host" => self.host(rest),
            "stop" => self.stop(),
            "relay" if !rest.is_empty() => {
                self.relay = rest.trim().trim_end_matches('/').to_string();
                crate::save_keys(&[("relay_url", self.relay.clone())]);
                format!("Relay: {}", self.relay)
            }
            "guest" if self.role == Some(Role::Host) => match rest {
                "on" => {
                    self.guest_on = true;
                    self.resend = true;
                    format!("{} is on", self.guest.clone().unwrap_or("The guest".into()))
                }
                "off" => {
                    self.guest_on = false;
                    self.send(&Msg::Guest(None));
                    "Guest off".into()
                }
                _ => return None,
            },
            _ => return None,
        })
    }

    fn send(&self, msg: &Msg) {
        if let Some(l) = &self.link {
            let _ = l.out.send(proto::encode(msg));
        }
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}


// ------------------------------------------------------------------------------------------
// The network thread

fn ws_url(relay: &str, path: &str) -> String {
    let base = if let Some(r) = relay.strip_prefix("https://") {
        format!("wss://{r}")
    } else if let Some(r) = relay.strip_prefix("http://") {
        format!("ws://{r}")
    } else {
        format!("ws://{relay}")
    };
    format!("{base}{path}")
}

fn create_session(relay: &str, title: &str) -> Result<(String, String, String), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into();
    let body = serde_json::json!({"title": title, "character": "", "public": true}).to_string();
    let mut r = agent
        .post(format!("{relay}/sessions"))
        .header("Content-Type", "application/json")
        .send(body.as_str())
        .map_err(|e| format!("can't reach the relay: {e}"))?;
    let code = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if code != 200 {
        return Err(format!("relay said {code}: {text}"));
    }
    let j: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let get = |k: &str| j[k].as_str().unwrap_or_default().to_string();
    Ok((get("id"), get("code"), get("host_token")))
}

/// Percent-encode a query value.
fn q(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn spawn_link(role: Role, relay: String, title: String, code: String, session: String, name: String) -> Link {
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (in_tx, in_rx) = mpsc::channel::<Incoming>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new()
        .name("net".into())
        .spawn(move || run_link(role, relay, title, code, session, name, out_rx, in_tx, stop2))
        .expect("net thread");
    Link {
        out: out_tx,
        inbox: Mutex::new(in_rx),
        stop,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_link(
    role: Role,
    relay: String,
    title: String,
    code: String,
    mut session: String,
    name: String,
    out: mpsc::Receiver<Vec<u8>>,
    inbox: mpsc::Sender<Incoming>,
    stop: Arc<AtomicBool>,
) {
    let mut token = String::new();
    let mut wait = Duration::from_millis(500);
    while !stop.load(Ordering::Relaxed) {
        if role == Role::Host && token.is_empty() {
            match create_session(&relay, &title) {
                Ok((id, c, t)) => {
                    session = id.clone();
                    token = t;
                    let _ = inbox.send(Incoming::Created { id, code: c });
                }
                Err(e) => {
                    let _ = inbox.send(Incoming::Status(e));
                    std::thread::sleep(wait);
                    wait = (wait * 2).min(Duration::from_secs(10));
                    continue;
                }
            }
        }
        let query = match role {
            Role::Host => format!("role=host&token={token}"),
            Role::Viewer => format!("role=viewer&code={}", q(&code)),
            Role::Guest => format!("role=guest&code={}&name={}", q(&code), q(&name)),
        };
        let url = ws_url(&relay, &format!("/sessions/{session}/ws?{query}"));
        let mut sock = match tungstenite::connect(&url) {
            Ok((s, _)) => s,
            Err(e) => {
                let status = match &e {
                    tungstenite::Error::Http(r) => r.status().as_u16(),
                    _ => 0,
                };
                if status == 404 && role == Role::Host {
                    token.clear(); // the relay forgot the show: register again
                }
                let _ = inbox.send(Incoming::Status(match status {
                    404 => "No such show".into(),
                    403 => "Wrong join code".into(),
                    409 => "Someone else is the guest".into(),
                    _ => format!("Can't connect: {e}"),
                }));
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_secs(10));
                continue;
            }
        };
        wait = Duration::from_millis(500);
        let short = Some(Duration::from_millis(5));
        match sock.get_mut() {
            tungstenite::stream::MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(short);
                let _ = s.set_nodelay(true);
            }
            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                let _ = s.get_mut().set_read_timeout(short);
                let _ = s.get_mut().set_nodelay(true);
            }
            _ => {}
        }
        let _ = inbox.send(Incoming::Status("Connected".into()));
        'live: while !stop.load(Ordering::Relaxed) {
            // send what's queued
            while let Ok(bytes) = out.try_recv() {
                if sock.send(tungstenite::Message::binary(bytes)).is_err() {
                    break 'live;
                }
            }
            // read what's arrived (the short timeout keeps sends flowing)
            loop {
                match sock.read() {
                    Ok(tungstenite::Message::Binary(b)) => {
                        let _ = inbox.send(Incoming::Data(b.to_vec(), Instant::now()));
                    }
                    Ok(tungstenite::Message::Text(t)) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str()) {
                            let _ = inbox.send(Incoming::Notice(v));
                        }
                    }
                    Ok(tungstenite::Message::Close(_)) => break 'live,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
                    {
                        break;
                    }
                    Err(_) => break 'live,
                }
            }
        }
        let _ = sock.close(None);
        if !stop.load(Ordering::Relaxed) {
            let _ = inbox.send(Incoming::Status("Reconnecting".into()));
            std::thread::sleep(wait);
        }
    }
}

// ------------------------------------------------------------------------------------------
// Bevy systems

/// Finds your own avatar's rig once its scene is ready.
pub fn find_body(
    mut net: ResMut<Net>,
    av: Res<crate::Avatar>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    if net.role.is_none() {
        return;
    }
    let (Some(root), Some(_), Some(gh)) = (av.root, av.player, av.gltf.as_ref()) else {
        net.body = None;
        return;
    };
    if net.body.as_ref().is_some_and(|(r, b)| *r == root && b.complete()) {
        return;
    }
    let Some(gltf) = gltfs.get(gh) else { return };
    let Some(rig) = rig(gltf, &nodes, root, &children, &names) else { return };
    let model = av.models[av.model].1.clone();
    let fresh = net.body.as_ref().is_none_or(|(r, b)| *r != root || b.hash != rig.hash);
    debug(|| {
        let found = rig.entities.iter().filter(|e| e.is_some()).count();
        format!("body {model}: {found}/{} nodes", rig.entities.len())
    });
    if fresh {
        let hello = proto::Hello {
            version: proto::VERSION,
            name: net.name.clone(),
            model,
            nodes: rig.hash,
        };
        if matches!(net.role, Some(Role::Host | Role::Guest)) {
            net.send(&Msg::Hello(hello.clone()));
        }
        net.hello = Some(hello);
    }
    net.body = Some((root, rig));
}

/// The commands that rebuild your scene on a viewer's screen (as a saved scene preset).
fn scene_commands(
    av: &crate::Avatar,
    show: &crate::show::Show,
    studio: &crate::lighting::Studio,
    wardrobe: &crate::wardrobe::Wardrobe,
) -> Vec<String> {
    let mut v = vec![
        format!("model:{}", av.models[av.model].0),
        format!("bg:{}", av.background.name()),
        format!("set:{}", show.set),
        format!("light:{}", if studio.on { "studio" } else { "flat" }),
        "wear:clear".into(),
    ];
    v.extend(
        wardrobe
            .worn
            .iter()
            .flatten()
            .map(|o| format!("wear:{}", crate::wardrobe::ITEMS[o.item].name)),
    );
    v
}

/// Lighting cues to send: taken each frame before `lighting::update` empties them.
pub fn take_cues(mut net: ResMut<Net>, cues: Res<crate::lighting::Cues>) {
    if net.role != Some(Role::Host) {
        return;
    }
    for c in &cues.0 {
        net.cues.push(match c {
            crate::lighting::Cue::Sub => 0,
            crate::lighting::Cue::Raid => 1,
            crate::lighting::Cue::BigHit => 2,
        });
    }
}

/// Host and guest: send the scene, cues and poses. Runs after the face and animation are
/// final, before the springs (which every screen runs for itself).
#[allow(clippy::too_many_arguments)]
pub fn send(
    mut net: ResMut<Net>,
    av: Res<crate::Avatar>,
    show: Res<crate::show::Show>,
    studio: Res<crate::lighting::Studio>,
    wardrobe: Res<crate::wardrobe::Wardrobe>,
    tick: Res<Tick>,
    time: Res<Time<Real>>,
    tf: Query<&Transform>,
    cam: Query<&Transform, With<crate::MainCamera>>,
    remotes: Query<&Remote>,
) {
    let role = match net.role {
        Some(r @ (Role::Host | Role::Guest)) if net.link.is_some() => r,
        _ => return,
    };
    // real time: a host rendering slowly still re-sends on time
    let now = time.elapsed_secs();
    if net.resend {
        net.resend = false;
        if let Some(h) = net.hello.clone() {
            net.send(&Msg::Hello(h));
        }
        net.scene_at = f32::NEG_INFINITY;
    }
    let due = now - net.scene_at > SCENE_EVERY;
    if role == Role::Host {
        let scene = scene_commands(&av, &show, &studio, &wardrobe);
        if scene != net.scene_sent || due {
            net.send(&Msg::Scene(scene.clone()));
            net.scene_sent = scene;
            net.scene_at = now;
            // who's on stage beside you
            let guest = if net.guest_on { net.streams[GUEST as usize].hello.clone() } else { None };
            net.send(&Msg::Guest(guest));
        }
        for c in std::mem::take(&mut net.cues) {
            net.send(&Msg::Event(proto::Event {
                tick: tick.0,
                what: proto::What::Cue(c),
            }));
        }
    } else if due {
        // a guest says hello now and then, in case the host restarted
        if let Some(h) = net.hello.clone() {
            net.send(&Msg::Hello(h));
        }
        net.scene_at = now;
    }
    if tick.0 < net.last_pose_tick + POSE_EVERY {
        return;
    }
    net.last_pose_tick = tick.0;
    let who = if role == Role::Host { HOST } else { GUEST };
    if let Some((root, rig)) = &net.body {
        let camera = if role == Role::Host { cam.single().ok().copied() } else { None };
        let pose = capture(tick.0, who, *root, rig, camera, &tf);
        net.send(&Msg::Pose(pose));
    }
    // the host passes on the guest as shown here, on the host's clock
    if role == Role::Host && net.guest_on {
        for r in &remotes {
            if let (GUEST, Some(inner), Some(rig)) = (r.who, r.inner, &r.rig) {
                let pose = capture(tick.0, GUEST, inner, rig, None, &tf);
                net.send(&Msg::Pose(pose));
            }
        }
    }
}

/// Everyone: take what the network thread received.
pub fn receive(
    mut net: ResMut<Net>,
    mut av: ResMut<crate::Avatar>,
    mut inject: ResMut<crate::Inject>,
    mut cues: ResMut<crate::lighting::Cues>,
) {
    let Some(link) = &net.link else {
        return;
    };
    let msgs: Vec<Incoming> = link.inbox.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect();
    let role = net.role;
    for m in msgs {
        match m {
            Incoming::Status(s) => {
                debug(|| format!("status: {s}"));
                net.status = s;
                av.status = format!("Network: {}", net.status);
            }
            Incoming::Created { id, code } => {
                net.session = id.clone();
                net.code = code.clone();
                net.status = format!("Live: {}/{} (code {})", net.relay, id, code);
                av.status = net.status.clone();
            }
            Incoming::Notice(v) => match v["relay"].as_str() {
                Some("viewers") => {
                    let n = v["n"].as_u64().unwrap_or(0) as usize;
                    net.resend |= n > net.viewers;
                    net.viewers = n;
                }
                Some("guest_joined") => {
                    net.guest = v["name"].as_str().map(String::from);
                    net.resend = true;
                    av.status = format!(
                        "{} wants to join: Let guest on (control page)",
                        net.guest.clone().unwrap_or_default()
                    );
                }
                Some("guest_left") => {
                    net.guest = None;
                    net.guest_on = false;
                    net.streams[GUEST as usize].clear();
                    net.send(&Msg::Guest(None));
                }
                _ => {}
            },
            Incoming::Data(bytes, at) => {
                let Some(msg) = proto::decode(&bytes) else {
                    debug(|| format!("undecodable message, {} bytes", bytes.len()));
                    continue;
                };
                debug(|| match &msg {
                    Msg::Pose(p) => format!("pose for {} tick {} ({} nodes)", p.who, p.tick, p.nodes.len()),
                    Msg::Voice(v) => format!("voice from {} seq {} ({} bytes)", v.who, v.seq, v.data.len()),
                    m => format!("{m:?}"),
                });
                let arrived = at.duration_since(net.started).as_secs_f64();
                match (role, msg) {
                    // what a host hears: its guest
                    (Some(Role::Host), Msg::Hello(h)) => net.streams[GUEST as usize].hello = Some(h),
                    (Some(Role::Host), Msg::Pose(p)) if p.who == GUEST => {
                        net.streams[GUEST as usize].push(p, arrived)
                    }
                    // what viewers and the guest hear: the host
                    (Some(Role::Viewer | Role::Guest), Msg::Hello(h)) => {
                        net.streams[HOST as usize].hello = Some(h)
                    }
                    (Some(r @ (Role::Viewer | Role::Guest)), Msg::Scene(cmds)) => {
                        // a guest keeps their own character and clothes
                        let cmds: Vec<String> = if r == Role::Guest {
                            cmds.into_iter()
                                .filter(|c| !c.starts_with("model:") && !c.starts_with("wear:"))
                                .collect()
                        } else {
                            cmds
                        };
                        if cmds != net.scene_applied {
                            inject.0.extend(cmds.iter().cloned());
                            net.scene_applied = cmds;
                        }
                    }
                    (Some(Role::Viewer | Role::Guest), Msg::Guest(g)) => {
                        net.guest_on = g.is_some();
                        let s = &mut net.streams[GUEST as usize];
                        if g.is_none() {
                            s.poses.clear();
                        }
                        s.hello = g;
                    }
                    (Some(Role::Viewer | Role::Guest), Msg::Pose(p)) => {
                        let who = p.who as usize;
                        // both arrive on the host's clock: one clock times them
                        net.streams[HOST as usize].clock.heard(p.tick as f64 / HZ, arrived);
                        if who < 2 {
                            let s = &mut net.streams[who];
                            let pos = s.poses.partition_point(|q| q.tick < p.tick);
                            if s.poses.get(pos).is_none_or(|q| q.tick != p.tick) {
                                s.poses.insert(pos, p);
                            }
                            while s.poses.len() > 120 {
                                s.poses.pop_front();
                            }
                        }
                    }
                    (Some(Role::Viewer | Role::Guest), Msg::Event(e)) => match e.what {
                        proto::What::Cue(c) => net.pending_cues.push((e.tick, c)),
                    },
                    // voices: a host plays its guest (once let on) and passes it on to viewers;
                    // a guest plays the host; viewers play both
                    (Some(r), Msg::Voice(v)) => {
                        let play = match r {
                            Role::Host => v.who == GUEST && net.guest_on,
                            Role::Guest => v.who == HOST,
                            Role::Viewer => true,
                        };
                        if !play {
                            continue;
                        }
                        if net.player.is_none() {
                            let p = crate::netvoice::Player::open(net.voice_output.as_deref());
                            debug(|| p.status.clone());
                            net.player = Some(p);
                        }
                        if let Some(p) = &net.player {
                            p.hear(&v);
                        }
                        if r == Role::Host {
                            net.send(&Msg::Voice(v));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // cues play when the shown time reaches them
    let shown = net.streams[HOST as usize].shown;
    let due: Vec<u8> = net
        .pending_cues
        .iter()
        .filter(|(t, _)| (*t as f64) <= shown)
        .map(|(_, c)| *c)
        .collect();
    net.pending_cues.retain(|(t, _)| (*t as f64) > shown);
    for c in due {
        cues.0.push(match c {
            0 => crate::lighting::Cue::Sub,
            1 => crate::lighting::Cue::Raid,
            _ => crate::lighting::Cue::BigHit,
        });
    }
}

/// Spawns and clears the avatars drawn from the network, and frames the guest in the shot.
pub fn stage(
    mut net: ResMut<Net>,
    mut av: ResMut<crate::Avatar>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    // who should be on screen from the network, and as what
    let want: [Option<String>; 2] = match net.role {
        Some(Role::Host) => [
            None,
            net.guest_on.then(|| net.streams[GUEST as usize].hello.as_ref().map(|h| h.model.clone())).flatten(),
        ],
        Some(Role::Viewer) => [
            None, // the host is drawn on your own character
            net.streams[GUEST as usize].hello.as_ref().map(|h| h.model.clone()),
        ],
        Some(Role::Guest) => [net.streams[HOST as usize].hello.as_ref().map(|h| h.model.clone()), None],
        None => [None, None],
    };
    for who in 0..2 {
        let have = net.remotes[who];
        match (&want[who], have) {
            (None, Some(e)) => {
                commands.entity(e).try_despawn();
                net.remotes[who] = None;
            }
            (Some(model), None) if model.ends_with(".glb") && !model.contains("..") => {
                let at = if who == GUEST as usize { guest_spot() } else { Transform::default() };
                let e = commands
                    .spawn((
                        Remote {
                            who: who as u8,
                            model: model.clone(),
                            gltf: assets.load::<Gltf>(model.clone()),
                            inner: None,
                            rig: None,
                        },
                        at,
                        Visibility::default(),
                        Name::new(if who == GUEST as usize { "network guest" } else { "network host" }),
                    ))
                    .id();
                net.remotes[who] = Some(e);
            }
            _ => {}
        }
    }
    // a guest on stage widens the shot, like a co-host
    av.remote_guest = match net.role {
        Some(Role::Host) => net.guest_on && net.remotes[GUEST as usize].is_some(),
        Some(Role::Guest) => true,
        _ => false,
    };
}

/// Host and guest: send your mic's voice while the show runs.
pub fn voice(mut net: ResMut<Net>, mic: Option<Res<crate::Mic>>) {
    let sending = matches!(net.role, Some(Role::Host | Role::Guest)) && net.link.is_some();
    let Some(mic) = mic else { return };
    if !sending || mic.is_changed() {
        if net.voice_out.take().is_some() {
            *mic.0.net_tap.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
    if sending && net.voice_out.is_none() {
        let (tx, rx) = mpsc::sync_channel(64);
        *mic.0.net_tap.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        let who = if net.role == Some(Role::Host) { HOST } else { GUEST };
        if let Some(l) = &net.link {
            net.voice_out = Some(crate::netvoice::Sender::start(who, rx, l.out.clone()));
        }
    }
}

/// Viewer: keep your own character still and quiet; the stream drives it.
pub fn hold_still(net: Res<Net>, mut av: ResMut<crate::Avatar>) {
    if net.role != Some(Role::Viewer) {
        return;
    }
    av.idle_life = false;
    av.reactions = false;
    av.auto_talk = false;
    av.next_blink = f32::MAX;
    av.sleep_after = f32::MAX;
    av.asleep = false;
    av.hush = true;
}

/// Everyone receiving: show each network avatar "now minus 100 ms", blended between the two
/// nearest poses. Runs where `send` runs: after the face, before the springs.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    mut net: ResMut<Net>,
    time: Res<Time<Real>>,
    mut tf: Query<&mut Transform, (Without<crate::FacePart>, Without<crate::MainCamera>)>,
    mut faces: Query<(&crate::FacePart, &mut Transform), Without<crate::MainCamera>>,
    mut cam: Query<&mut Transform, (With<crate::MainCamera>, Without<crate::FacePart>)>,
    remotes: Query<&Remote>,
) {
    let Some(role) = net.role else { return };
    let now = net.now();
    let dt = time.delta_secs_f64();
    // the tick to show for each stream: viewers and the guest show everything on the host's
    // clock; a host shows its guest on the guest's
    let host_t = net.streams[HOST as usize].clock.at(now, dt);
    let guest_t = if role == Role::Host {
        net.streams[GUEST as usize].clock.at(now, dt)
    } else {
        host_t
    };
    let ticks = [host_t.map(|t| t * HZ), guest_t.map(|t| t * HZ)];
    for (who, t) in ticks.iter().enumerate() {
        if let Some(t) = t {
            net.streams[who].shown = *t;
        }
    }
    // the host's pose (and camera) on a viewer's own character, or a guest's camera
    if matches!(role, Role::Viewer | Role::Guest) {
        if let Some(t) = ticks[HOST as usize] {
            if let Some((a, b, s)) = net.streams[HOST as usize].around(t) {
                if let (Some(ca), Some(cb), Ok(mut c)) = (a.camera, b.camera, cam.single_mut()) {
                    *c = crate::sim::blend_transform(&proto::unpack(&ca), &proto::unpack(&cb), s);
                }
                let host_hash = net.streams[HOST as usize].hello.as_ref().map(|h| h.nodes);
                if role == Role::Viewer {
                    if let Some((root, rig)) = &net.body {
                        if host_hash.is_none_or(|h| h == rig.hash) {
                            put(rig, *root, &a, &b, s, &mut tf);
                            let mask = if s < 1.0 { a.face } else { b.face };
                            for (fp, mut ft) in &mut faces {
                                if let Some(i) = crate::face::FACE_PARTS.iter().position(|p| *p == fp.0) {
                                    ft.scale = if mask & (1 << i) != 0 { Vec3::ONE } else { Vec3::ZERO };
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // network avatars standing beside you
    for r in &remotes {
        let (Some(inner), Some(rig), Some(t)) = (r.inner, &r.rig, ticks.get(r.who as usize).copied().flatten()) else {
            continue;
        };
        if let Some((a, b, s)) = net.streams[r.who as usize].around(t) {
            put(rig, inner, &a, &b, s, &mut tf);
        }
    }
}

/// Guest: your own character stands on the host's left. Runs after `send`, so the pose you
/// send stays centred and the host places it.
pub fn place_guest(net: Res<Net>, av: Res<crate::Avatar>, mut tf: Query<&mut Transform, Without<crate::MainCamera>>) {
    if net.role != Some(Role::Guest) {
        return;
    }
    let Some(mut t) = av.root.and_then(|r| tf.get_mut(r).ok()) else {
        return;
    };
    let spot = guest_spot();
    t.translation = spot.translation + spot.rotation * t.translation;
    t.rotation = spot.rotation * t.rotation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Pose;

    #[test]
    fn playout_stays_100ms_behind_through_jitter() {
        let mut c = Playout::default();
        let mut shown = vec![];
        // poses every 1/30 s, arriving 40-90 ms late (deterministic jitter)
        let mut next_pose = 0.0f64;
        let mut k = 0u32;
        let mut in_flight: Vec<(f64, f64)> = vec![]; // (arrives, sender time)
        let dt = 1.0 / 60.0;
        for f in 0..600 {
            let now = f as f64 * dt;
            while next_pose <= now {
                k += 1;
                let jitter = 0.04 + 0.05 * ((k * 7919 % 101) as f64 / 100.0);
                in_flight.push((next_pose + jitter, next_pose));
                next_pose += 1.0 / 30.0;
            }
            for (arrives, sent) in in_flight.iter().filter(|p| p.0 <= now) {
                c.heard(*sent, *arrives);
            }
            in_flight.retain(|p| p.0 > now);
            if let Some(t) = c.at(now, dt) {
                shown.push((now, t));
            }
        }
        // after settling: about 140 ms behind real time (40 ms best delay + 100 ms buffer)
        let (now, t) = *shown.last().unwrap();
        let behind = now - t;
        assert!((0.12..0.17).contains(&behind), "behind by {behind}");
        // never runs backwards, and moves at real speed within 1%
        for w in shown.windows(2).skip(60) {
            let step = w[1].1 - w[0].1;
            assert!(step > 0.0 && (step - dt).abs() <= 0.01 * dt + 1e-9, "step {step}");
        }
    }

    #[test]
    fn playout_jumps_only_when_far_off() {
        let mut c = Playout::default();
        c.heard(10.0, 0.05);
        let t0 = c.at(0.1, 0.0).unwrap();
        assert!((t0 - (10.0 - 0.05 + 0.1 - DELAY)).abs() < 1e-9);
        // the sender restarts its clock far ahead: snap there
        c.heard(50.0, 0.2);
        let t1 = c.at(0.2, 0.1).unwrap();
        assert!((t1 - (50.0 - DELAY)).abs() < 1e-6);
    }

    fn pose(tick: u64) -> Pose {
        Pose {
            tick,
            who: HOST,
            root: proto::pack(&Transform::from_xyz(tick as f32, 0.0, 0.0)),
            camera: None,
            face: tick as u32,
            nodes: vec![],
        }
    }

    #[test]
    fn stream_picks_the_poses_around_a_tick() {
        let mut s = Stream::default();
        for t in [12u64, 0, 4, 8, 8] {
            s.push(pose(t), 0.0);
        }
        assert_eq!(s.poses.iter().map(|p| p.tick).collect::<Vec<_>>(), vec![0, 4, 8, 12], "sorted, no repeats");
        let (a, b, k) = s.around(5.0).unwrap();
        assert_eq!((a.tick, b.tick), (4, 8));
        assert!((k - 0.25).abs() < 1e-6);
        // past the newest: hold it
        let (a, b, k) = s.around(20.0).unwrap();
        assert_eq!((a.tick, b.tick, k), (12, 12, 0.0));
        // old poses go, keeping a little history
        assert!(s.poses.len() <= 2);
    }

    #[test]
    fn urls() {
        assert_eq!(ws_url("http://a:7779", "/x"), "ws://a:7779/x");
        assert_eq!(ws_url("https://relay.example", "/x"), "wss://relay.example/x");
        assert_eq!(q("Sam O'Neil&co"), "Sam%20O%27Neil%26co");
        assert_eq!(
            Net::split_url("http://r:7779/sessions/abc/").unwrap(),
            ("http://r:7779".to_string(), "abc".to_string())
        );
    }
}
