//! Chat-thrown props. Viewers type `!tomato`, `!egg` or `!confetti` in Twitch chat and it flies at
//! the avatar: tomatoes and eggs splat on its face (and it flinches), confetti rains down.
//!
//! Twitch chat is read anonymously over IRC (no login, no OAuth: the read-only `justinfan` user),
//! so all you set is the channel name. YouTube, Kick or anything else: point a chat bot
//! (Streamer.bot, Nightbot, Mix It Up...) at `http://127.0.0.1:7777/cmd/throw:tomato` or
//! `throw:tomato:ViewerName`.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    time::Duration,
};

use bevy::{gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAssetRoot};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Throw {
    Tomato,
    Egg,
    Confetti,
}

pub const THROWS: [Throw; 3] = [Throw::Tomato, Throw::Egg, Throw::Confetti];

/// Thrower name for throws the app makes itself (a sub's confetti): no credit line, no cooldown.
pub const QUIET: &str = "\u{1}";

impl Throw {
    pub fn parse(s: &str) -> Option<Throw> {
        match s
            .trim()
            .trim_start_matches('!')
            .to_ascii_lowercase()
            .as_str()
        {
            "tomato" | "tomatoes" => Some(Throw::Tomato),
            "egg" | "eggs" => Some(Throw::Egg),
            "confetti" | "party" => Some(Throw::Confetti),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Throw::Tomato => "tomato",
            Throw::Egg => "egg",
            Throw::Confetti => "confetti",
        }
    }
}

/// Where a chat message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Platform {
    #[default]
    Twitch,
    YouTube,
    Kick,
}

impl Platform {
    pub fn tag(self) -> &'static str {
        match self {
            Platform::Twitch => "tw",
            Platform::YouTube => "yt",
            Platform::Kick => "kick",
        }
    }
    /// The platform's own colour, for name signs.
    pub fn colour(self) -> [u8; 3] {
        match self {
            Platform::Twitch => [0x91, 0x46, 0xff],
            Platform::YouTube => [0xff, 0x00, 0x33],
            Platform::Kick => [0x3c, 0xc8, 0x10],
        }
    }
}

/// One viewer across the app: platform plus name, so "bob" on Kick isn't "bob" on Twitch.
/// Twitch names are logins (case doesn't matter); a ban from another platform comes in as
/// "kick:bob" or "yt:Bob".
pub fn viewer_key(platform: Platform, user: &str) -> String {
    format!("{}:{}", platform.tag(), user.to_ascii_lowercase())
}

/// A ban or timeout's target as a viewer key (Twitch bans name just the login).
pub fn ban_key(user: &str) -> String {
    match user.split_once(':') {
        Some((p, u)) if p == "kick" || p == "yt" => format!("{p}:{}", u.to_ascii_lowercase()),
        _ => format!("tw:{}", user.to_ascii_lowercase()),
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatMsg {
    pub platform: Platform,
    pub user: String,
    pub text: String,
    /// subscriber / VIP / moderator / the broadcaster (from the message's badges)
    pub sub: bool,
    pub vip: bool,
    pub moderator: bool,
    pub broadcaster: bool,
    /// bits cheered with this message
    pub bits: u32,
}

impl ChatMsg {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(user: &str, text: &str) -> Self {
        ChatMsg {
            user: user.into(),
            text: text.into(),
            ..Default::default()
        }
    }
}

/// Who may use a chat feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Everyone,
    Subs,
    Mods,
}

impl Who {
    pub fn parse(s: &str) -> Option<Who> {
        match s.trim().to_ascii_lowercase().as_str() {
            "everyone" | "all" => Some(Who::Everyone),
            "subs" | "subscribers" => Some(Who::Subs),
            "mods" | "moderators" => Some(Who::Mods),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Who::Everyone => "everyone",
            Who::Subs => "subs",
            Who::Mods => "mods",
        }
    }
    pub fn allows(self, m: &ChatMsg) -> bool {
        m.broadcaster
            || match self {
                Who::Everyone => true,
                Who::Subs => m.sub || m.vip || m.moderator,
                Who::Mods => m.moderator,
            }
    }
}

#[derive(Debug, PartialEq)]
pub enum IrcLine {
    Ping(String),
    Msg(ChatMsg),
    /// a moderator banned (None) or timed out (seconds) someone in chat
    Ban(String, Option<u64>),
    Other,
}

/// What the chat thread hands to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Msg(ChatMsg),
    Ban(String, Option<u64>),
    Raid {
        from: String,
        viewers: u32,
    },
    /// a new or returning sub (`gifts` > 0: someone gifted that many)
    Sub {
        user: String,
        months: u32,
        gifts: u32,
    },
}

fn tags_of(line: &str) -> HashMap<&str, String> {
    let mut m = HashMap::new();
    if let Some(t) = line
        .strip_prefix('@')
        .and_then(|l| l.split_once(' '))
        .map(|x| x.0)
    {
        for kv in t.split(';') {
            if let Some((k, v)) = kv.split_once('=') {
                // IRCv3 tag escapes
                m.insert(
                    k,
                    v.replace("\\s", " ")
                        .replace("\\:", ";")
                        .replace("\\\\", "\\"),
                );
            }
        }
    }
    m
}

/// Raids and subs arrive as USERNOTICE lines; None for anything else.
pub fn parse_notice(line: &str) -> Option<ChatEvent> {
    if !line.contains(" USERNOTICE ") {
        return None;
    }
    let t = tags_of(line);
    let num = |k: &str| t.get(k).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let name = |k: &str| t.get(k).cloned().unwrap_or_default();
    let who = if name("display-name").is_empty() {
        name("login")
    } else {
        name("display-name")
    };
    match t.get("msg-id").map(|s| s.as_str())? {
        "raid" => Some(ChatEvent::Raid {
            from: if name("msg-param-displayName").is_empty() {
                name("msg-param-login")
            } else {
                name("msg-param-displayName")
            },
            viewers: num("msg-param-viewerCount"),
        }),
        "sub" | "resub" => Some(ChatEvent::Sub {
            user: who,
            months: num("msg-param-cumulative-months").max(1),
            gifts: 0,
        }),
        "submysterygift" => Some(ChatEvent::Sub {
            user: who,
            months: 0,
            gifts: num("msg-param-mass-gift-count").max(1),
        }),
        // single gifts that are part of a mystery gift repeat it; only standalone ones count
        "subgift" if !t.contains_key("msg-param-community-gift-id") => Some(ChatEvent::Sub {
            user: who,
            months: 0,
            gifts: 1,
        }),
        _ => None,
    }
}

/// One line from Twitch IRC (tags, if any, are ignored).
pub fn parse_irc(line: &str) -> IrcLine {
    let line = line.trim_end_matches(['\r', '\n']);
    // @ban-duration=600;... :tmi.twitch.tv CLEARCHAT #channel :user  (no user: whole chat cleared)
    if let Some(i) = line.find(" CLEARCHAT ") {
        let tags = if line.starts_with('@') {
            line.split_once(' ').map(|x| x.0).unwrap_or("")
        } else {
            ""
        };
        if let Some((_, user)) = line[i + 11..].split_once(" :") {
            let secs = tags
                .split(';')
                .find_map(|t| {
                    t.strip_prefix("@ban-duration=")
                        .or_else(|| t.strip_prefix("ban-duration="))
                })
                .and_then(|v| v.parse().ok());
            let user = user.trim();
            if !user.is_empty() {
                return IrcLine::Ban(user.to_ascii_lowercase(), secs);
            }
        }
        return IrcLine::Other;
    }
    let tags = tags_of(line);
    let line = if line.starts_with('@') {
        line.split_once(' ').map(|x| x.1).unwrap_or("")
    } else {
        line
    };
    if let Some(p) = line.strip_prefix("PING ") {
        return IrcLine::Ping(p.to_string());
    }
    // :nick!nick@nick.tmi.twitch.tv PRIVMSG #channel :message text
    if let Some(rest) = line.strip_prefix(':') {
        if let Some((prefix, tail)) = rest.split_once(' ') {
            if let Some(tail) = tail.strip_prefix("PRIVMSG ") {
                if let Some((_, text)) = tail.split_once(" :") {
                    let user = prefix.split('!').next().unwrap_or("").to_string();
                    let badges = tags.get("badges").cloned().unwrap_or_default();
                    let has = |b: &str| badges.split(',').any(|x| x.split('/').next() == Some(b));
                    return IrcLine::Msg(ChatMsg {
                        platform: Platform::Twitch,
                        user,
                        text: text.to_string(),
                        sub: has("subscriber")
                            || has("founder")
                            || tags.get("subscriber").is_some_and(|v| v == "1"),
                        vip: has("vip"),
                        moderator: has("moderator") || tags.get("mod").is_some_and(|v| v == "1"),
                        broadcaster: has("broadcaster"),
                        bits: tags.get("bits").and_then(|b| b.parse().ok()).unwrap_or(0),
                    });
                }
            }
        }
    }
    IrcLine::Other
}

/// `!up 12` or `!vote 12`: a vote for viewer question 12.
pub fn vote_of(text: &str) -> Option<u32> {
    let mut w = text.split_whitespace();
    let cmd = w.next()?.to_ascii_lowercase();
    if cmd != "!up" && cmd != "!vote" {
        return None;
    }
    w.next()?.trim_start_matches('#').parse().ok()
}

/// The first word of a chat message, if it's a throw command.
pub fn command_of(text: &str) -> Option<Throw> {
    let w = text.split_whitespace().next()?;
    if !w.starts_with('!') {
        return None;
    }
    Throw::parse(w)
}

/// Keeps a flood of `!tomato` from turning into a blur: one throw per viewer every few seconds,
/// a short gap between any two throws, and only a few in the air at once.
pub struct RateGate {
    pub per_user_s: f32,
    pub global_s: f32,
    pub max_in_air: usize,
    last_user: HashMap<String, f32>,
    last_any: f32,
}

impl RateGate {
    pub fn new() -> Self {
        RateGate {
            per_user_s: 12.0,
            global_s: 0.6,
            max_in_air: 3,
            last_user: HashMap::new(),
            last_any: -100.0,
        }
    }
    pub fn allow(&mut self, user: &str, now: f32, in_air: usize) -> bool {
        if in_air >= self.max_in_air || now - self.last_any < self.global_s {
            return false;
        }
        let key = user.to_ascii_lowercase();
        if self
            .last_user
            .get(&key)
            .is_some_and(|t| now - t < self.per_user_s)
        {
            return false;
        }
        self.last_user.insert(key, now);
        self.last_any = now;
        true
    }
}

/// Reads one Twitch channel anonymously, reconnecting if the connection drops.
pub fn start_twitch(
    host: String,
    port: u16,
    channel: String,
    status: Arc<Mutex<String>>,
) -> Receiver<ChatEvent> {
    let (tx, rx) = mpsc::channel();
    let channel = channel.trim().trim_start_matches('#').to_ascii_lowercase();
    std::thread::Builder::new()
        .name("twitch".into())
        .spawn(move || loop {
            let addr = (host.as_str(), port).to_socket_addrs().ok().and_then(|mut a| a.next());
            let conn = addr.and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(8)).ok());
            let Some(stream) = conn else {
                *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: can't reach chat, retrying ({channel})");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(360)));
            let nick = format!("justinfan{}", 10000 + (std::process::id() % 80000));
            let mut w = &stream;
            // twitch.tv/commands + tags: also hear about bans and timeouts (CLEARCHAT)
            if w.write_all(format!("CAP REQ :twitch.tv/commands twitch.tv/tags\r\nPASS SCHMOOPIIE\r\nNICK {nick}\r\nJOIN #{channel}\r\n").as_bytes()).is_err() {
                continue;
            }
            *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: reading #{channel}");
            let mut r = BufReader::new(&stream);
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => match parse_irc(&line) {
                        IrcLine::Ping(p) => {
                            let _ = (&stream).write_all(format!("PONG {p}\r\n").as_bytes());
                        }
                        IrcLine::Msg(m) => {
                            if tx.send(ChatEvent::Msg(m)).is_err() {
                                return; // app closed
                            }
                        }
                        IrcLine::Ban(u, secs) => {
                            if tx.send(ChatEvent::Ban(u, secs)).is_err() {
                                return;
                            }
                        }
                        IrcLine::Other => {
                            if let Some(ev) = parse_notice(&line) {
                                if tx.send(ev).is_err() {
                                    return;
                                }
                            }
                        }
                    },
                }
            }
            *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: disconnected from #{channel}, reconnecting");
            std::thread::sleep(Duration::from_secs(3));
        })
        .expect("spawn twitch thread");
    rx
}

// ------------------------------------------------------------------------------------------
// Bevy side
// ------------------------------------------------------------------------------------------

#[derive(Resource)]
pub struct Chat {
    pub enabled: bool,
    pub channel: Option<String>,
    pub status: Arc<Mutex<String>>,
    rx: Option<Mutex<Receiver<ChatEvent>>>,
    /// `!up <n>` votes for viewer questions: (voter, question)
    pub votes: Vec<(String, u32)>,
    /// bans and timeouts from the channel's moderators: (login, seconds or None for a ban)
    pub bans: Vec<(String, Option<u64>)>,
    pub gate: RateGate,
    /// throws waiting to be launched (from chat, the control page or the keyboard)
    pub queue: Vec<(Throw, String)>,
    /// hits for the avatar to react to (main.rs plays the flinch): (kind, size)
    pub hits: Vec<(Throw, f32)>,
    /// "name threw a tomato" shown under the avatar
    pub credit: Option<(String, f32)>,
    /// cheers: (when, kind, who, size) launched one after another, past the usual limits
    pub barrage: Vec<(f32, Throw, String, f32)>,
    /// every `!command` message, for the other chat features (dress-up, polls) to read this frame
    pub inbox: Vec<ChatMsg>,
    /// every message this frame (the crowd shows recent chatters and their !me picks)
    pub seen: Vec<ChatMsg>,
    /// banned or timed-out viewers (viewer keys) and until when (app seconds; infinite = ban):
    /// their messages are ignored everywhere
    pub muted: HashMap<String, f32>,
    /// viewers banned this frame (viewer keys), so the crowd can show them out
    pub banned_now: Vec<String>,
    /// every chat line this frame (effects react to hearts and GGs)
    pub said: Vec<String>,
    /// raids and subs this frame
    pub events: Vec<ChatEvent>,
    pub twitch_host: (String, u16),
    /// YouTube and Kick readers (src/streamchat.rs), keyed by platform
    pub feeds: Vec<(&'static str, crate::streamchat::Feed)>,
    cube: Option<Handle<Mesh>>,
    colors: Vec<Handle<StandardMaterial>>,
}

impl Chat {
    #[allow(dead_code)]
    pub fn new(enabled: bool, channel: Option<String>) -> Self {
        Self::with_host(enabled, channel, None)
    }
    pub fn with_host(enabled: bool, channel: Option<String>, host: Option<(String, u16)>) -> Self {
        let mut c = Chat {
            enabled,
            channel: None,
            status: Arc::new(Mutex::new(
                "Twitch: off (set a channel on the control page)".into(),
            )),
            rx: None,
            gate: RateGate::new(),
            queue: vec![],
            votes: vec![],
            bans: vec![],
            hits: vec![],
            credit: None,
            barrage: vec![],
            inbox: vec![],
            seen: vec![],
            muted: HashMap::new(),
            banned_now: vec![],
            said: vec![],
            events: vec![],
            twitch_host: host.unwrap_or(("irc.chat.twitch.tv".into(), 6667)),
            feeds: vec![],
            cube: None,
            colors: vec![],
        };
        if let Some(ch) = channel.filter(|c| !c.trim().is_empty()) {
            c.connect(ch);
        }
        c
    }
    pub fn connect(&mut self, channel: String) {
        let ch = channel.trim().trim_start_matches('#').to_ascii_lowercase();
        // a Twitch login: letters, digits and underscores (anything else could add IRC lines)
        if !ch.is_empty() && (ch.len() > 25 || !ch.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
            self.rx = None;
            self.channel = None;
            *self.status.lock().unwrap_or_else(|e| e.into_inner()) = "Twitch: not a channel name".into();
            return;
        }
        if ch.is_empty() {
            self.rx = None;
            self.channel = None;
            *self.status.lock().unwrap_or_else(|e| e.into_inner()) = "Twitch: off".into();
            return;
        }
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: connecting to #{ch}");
        self.rx = Some(Mutex::new(start_twitch(
            self.twitch_host.0.clone(),
            self.twitch_host.1,
            ch.clone(),
            self.status.clone(),
        )));
        self.channel = Some(ch);
    }
    /// Starts (or with None, stops) reading another platform's chat.
    pub fn set_feed(&mut self, platform: &'static str, feed: Option<crate::streamchat::Feed>) {
        self.feeds.retain(|f| f.0 != platform);
        if let Some(f) = feed {
            self.feeds.push((platform, f));
        }
    }
    pub fn status_line(&self) -> String {
        let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for f in &self.feeds {
            s += &format!(" | {}", f.1.status.lock().unwrap_or_else(|e| e.into_inner()));
        }
        format!(
            "{s} | chat throws {}",
            if self.enabled { "on" } else { "off" }
        )
    }
}

#[derive(Component)]
pub struct Flying {
    kind: Throw,
    /// 1 normal, bigger for big cheers
    size: f32,
    from: Vec3,
    t0: f32,
    dur: f32,
    spin: Vec3,
    aim: Vec2,
}

#[derive(Component)]
pub struct Splat {
    t0: f32,
    life: f32,
}

#[derive(Component)]
pub struct Confetto {
    vel: Vec3,
    spin: Vec3,
    t0: f32,
}

pub fn setup(
    mut chat: ResMut<Chat>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    chat.cube = Some(meshes.add(Cuboid::new(0.035, 0.035, 0.006)));
    for c in [
        (230, 57, 70),
        (255, 190, 11),
        (58, 134, 255),
        (131, 56, 236),
        (6, 214, 160),
        (255, 255, 255),
    ] {
        chat.colors.push(mats.add(StandardMaterial {
            base_color: Color::srgb_u8(c.0, c.1, c.2),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        }));
    }
}

/// Chat messages -> queued throws -> things in the air.
pub fn launch(
    mut chat: ResMut<Chat>,
    mut av: ResMut<crate::Avatar>,
    time: Res<Time>,
    flying: Query<(), With<Flying>>,
    head: Query<(&crate::Joint, &GlobalTransform)>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let mut events: Vec<ChatEvent> = chat
        .rx
        .as_ref()
        .map(|rx| rx.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect())
        .unwrap_or_default();
    for f in &chat.feeds {
        // (an empty ban is a feed's "are you still there" ping)
        events.extend(
            f.1.rx
                .lock()
                .unwrap()
                .try_iter()
                .filter(|e| !matches!(e, ChatEvent::Ban(u, _) if u.is_empty())),
        );
    }
    chat.inbox.clear();
    chat.events.clear();
    chat.seen.clear();
    chat.said.clear();
    chat.banned_now.clear();
    chat.muted.retain(|_, until| *until > now);
    for ev in events {
        match ev {
            ChatEvent::Msg(m) if chat.muted.contains_key(&viewer_key(m.platform, &m.user)) => {}
            ChatEvent::Msg(m) => {
                chat.seen.push(m.clone());
                chat.said.push(m.text.clone());
                if m.bits > 0 && chat.enabled {
                    cheer(&mut chat, &m.user, m.bits, now);
                }
                if let Some(id) = vote_of(&m.text) {
                    chat.votes.push((m.user, id));
                } else if let Some(t) = command_of(&m.text).filter(|_| chat.enabled) {
                    chat.queue.push((t, m.user));
                } else if m.text.starts_with('!') {
                    chat.inbox.push(m);
                }
            }
            ChatEvent::Ban(u, secs) => {
                let key = ban_key(&u);
                chat.muted.insert(
                    key.clone(),
                    secs.map(|s| now + s as f32).unwrap_or(f32::INFINITY),
                );
                chat.banned_now.push(key.clone());
                // viewer questions are signed in with Twitch: only Twitch bans reach them
                if key.starts_with("tw:") {
                    chat.bans.push((u, secs));
                }
            }
            other => chat.events.push(other),
        }
    }
    // cheers go out on their own schedule
    let due: Vec<(f32, Throw, String, f32)> = chat
        .barrage
        .iter()
        .filter(|b| b.0 <= now)
        .cloned()
        .collect();
    chat.barrage.retain(|b| b.0 > now);
    let in_air = flying.iter().count();
    let queue = std::mem::take(&mut chat.queue);
    let mut launched = 0;
    let items: Vec<(Throw, String, f32, bool)> = queue
        .into_iter()
        .map(|(k, u)| (k, u, 1.0, false))
        .chain(due.into_iter().map(|(_, k, u, s)| (k, u, s, true)))
        .collect();
    for (kind, user, size, cheered) in items {
        // the streamer's own buttons and cheers skip the per-viewer cooldown
        let quiet = user == QUIET;
        let mine = user.is_empty() || quiet;
        if !mine && !cheered && !chat.gate.allow(&user, now, in_air + launched) {
            continue;
        }
        launched += 1;
        let who = if mine { String::new() } else { user };
        if !cheered && !quiet {
            chat.credit = Some((
                if who.is_empty() {
                    format!("{}!", kind.name())
                } else {
                    format!(
                        "{who} threw {}",
                        if kind == Throw::Confetti {
                            "confetti".into()
                        } else {
                            format!("a {}", kind.name())
                        }
                    )
                },
                now,
            ));
        }
        let head_pos = head
            .iter()
            .find(|(j, _)| **j == crate::Joint::Head)
            .map(|(_, t)| t.translation())
            .unwrap_or(Vec3::new(0.0, 1.1, 0.0));
        match kind {
            Throw::Confetti => {
                let (Some(cube), false) = (chat.cube.clone(), chat.colors.is_empty()) else {
                    continue;
                };
                for i in 0..70 {
                    let r = |k: u32| crate::hash01(i * 7 + k, now);
                    let pos = head_pos
                        + Vec3::new((r(1) - 0.5) * 1.6, 0.7 + r(2) * 0.6, (r(3) - 0.2) * 0.6);
                    let t = Transform::from_translation(pos).with_rotation(Quat::from_euler(
                        EulerRot::XYZ,
                        r(4) * 6.0,
                        r(5) * 6.0,
                        r(6) * 6.0,
                    ));
                    commands.spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(chat.colors[(i as usize) % chat.colors.len()].clone()),
                        t,
                        crate::sim::Interp::new(t),
                        Confetto {
                            vel: Vec3::new(
                                (r(7) - 0.5) * 0.6,
                                -0.2 - r(8) * 0.5,
                                (r(9) - 0.5) * 0.3,
                            ),
                            spin: Vec3::new(r(10) * 9.0, r(11) * 9.0, r(12) * 4.0),
                            t0: now,
                        },
                    ));
                }
                chat.hits.push((kind, size));
            }
            _ => {
                let side = if av.rand01() < 0.5 { -1.0 } else { 1.0 };
                let from = head_pos
                    + Vec3::new(
                        side * (1.3 + av.rand01() * 0.4),
                        -0.7 - av.rand01() * 0.3,
                        1.4,
                    );
                let path = if kind == Throw::Tomato {
                    "props/lead/tomato.glb"
                } else {
                    "props/lead/egg.glb"
                };
                let aim = Vec2::new((av.rand01() - 0.5) * 0.18, (av.rand01() - 0.5) * 0.14);
                // the eyes catch it coming (eyes_left looks toward the left of the picture)
                if now - av.last_hit > 0.3 {
                    av.glance = Some((
                        if side < 0.0 {
                            "eyes_left"
                        } else {
                            "eyes_right"
                        },
                        now + 0.5,
                    ));
                }
                commands.spawn((
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Transform::from_translation(from).with_scale(Vec3::splat(1.7 * size)),
                    Flying {
                        kind,
                        size,
                        from,
                        t0: now,
                        dur: 0.55 + av.rand01() * 0.15,
                        spin: Vec3::new(av.rand01() * 14.0, av.rand01() * 10.0, 4.0),
                        aim,
                    },
                ));
            }
        }
    }
}

/// Bits: one tomato per 100 bits (up to 8) in a quick volley; 500+ adds a giant one at the end.
/// Under 100 bits: a single egg.
pub fn cheer(chat: &mut Chat, user: &str, bits: u32, now: f32) {
    let n = (bits / 100).clamp(1, 8);
    let kind = if bits < 100 {
        Throw::Egg
    } else {
        Throw::Tomato
    };
    for i in 0..n {
        chat.barrage
            .push((now + i as f32 * 0.22, kind, user.to_string(), 1.0));
    }
    if bits >= 500 {
        chat.barrage.push((
            now + n as f32 * 0.22 + 0.5,
            Throw::Tomato,
            user.to_string(),
            2.6,
        ));
    }
    chat.credit = Some((format!("{user} cheered {bits} bits!"), now));
}

/// Arc toward the face; on arrival, a splat sticks to the head and the avatar flinches.
pub fn fly(
    mut chat: ResMut<Chat>,
    time: Res<Time>,
    mut q: Query<(Entity, &Flying, &mut Transform)>,
    head: Query<(Entity, &crate::Joint, &GlobalTransform)>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let Some((head_e, _, head_tf)) = head.iter().find(|(_, j, _)| **j == crate::Joint::Head) else {
        return;
    };
    for (e, f, mut t) in &mut q {
        let face_local = Vec3::new(f.aim.x, 0.27 + f.aim.y, 0.34);
        let target = head_tf.transform_point(face_local);
        let u = ((now - f.t0) / f.dur).clamp(0.0, 1.0);
        let pos = f.from.lerp(target, u) + Vec3::Y * (4.0 * u * (1.0 - u) * 0.45);
        t.translation = pos;
        t.rotation = Quat::from_euler(EulerRot::XYZ, f.spin.x * u, f.spin.y * u, f.spin.z * u);
        if u >= 1.0 {
            commands.entity(e).despawn();
            let (path, rot, scale) = match f.kind {
                Throw::Tomato => (
                    "props/lead/fx_splat.glb",
                    Quat::from_rotation_z((f.aim.x * 3.0).clamp(-0.4, 0.4)),
                    1.0,
                ),
                // the egg splat is modelled lying flat; stand it up against the face
                _ => (
                    "props/lead/fx_eggsplat.glb",
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    0.8,
                ),
            };
            let t = Transform::from_translation(Vec3::new(f.aim.x, 0.30 + f.aim.y, 0.31))
                .with_rotation(rot)
                .with_scale(Vec3::splat(scale * f.size.sqrt()));
            commands.spawn((
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                t,
                crate::sim::Interp::new(t),
                Splat { t0: now, life: 3.5 },
                ChildOf(head_e),
            ));
            chat.hits.push((f.kind, f.size));
        }
    }
}

/// One fixed tick (`FixedUpdate`): splats slide down and shrink at the end; confetti bits fall
/// and spin.
pub fn fade(
    time: Res<Time>,
    mut splats: Query<(Entity, &Splat, &mut crate::sim::Interp), Without<Confetto>>,
    mut bits: Query<(Entity, &Confetto, &mut crate::sim::Interp), Without<Splat>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (e, s, mut ip) in &mut splats {
        let t = &mut ip.curr;
        let age = now - s.t0;
        if age > s.life {
            commands.entity(e).despawn();
        } else if age > s.life - 0.5 {
            // slide down and shrink away
            let k = (s.life - age) / 0.5;
            t.scale = Vec3::splat(t.scale.x.max(0.01) * (0.9 + 0.1 * k));
            t.translation.y -= 0.12 * dt;
        }
    }
    for (e, c, mut ip) in &mut bits {
        let t = &mut ip.curr;
        let age = now - c.t0;
        if age > 3.0 {
            commands.entity(e).despawn();
            continue;
        }
        let v = c.vel + Vec3::new((age * 3.0 + c.spin.x).sin() * 0.25, 0.0, 0.0);
        t.translation += v * dt;
        t.rotate_local(Quat::from_euler(
            EulerRot::XYZ,
            c.spin.x * dt,
            c.spin.y * dt,
            c.spin.z * dt,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_twitch_lines() {
        assert_eq!(
            parse_irc("PING :tmi.twitch.tv\r\n"),
            IrcLine::Ping(":tmi.twitch.tv".into())
        );
        assert_eq!(
            parse_irc(
                ":some_viewer!some_viewer@some_viewer.tmi.twitch.tv PRIVMSG #mychannel :!tomato get him\r\n"
            ),
            IrcLine::Msg(ChatMsg::new("some_viewer", "!tomato get him"))
        );
        assert_eq!(
            parse_irc("@badge-info=;color=#FF0000 :v!v@v.tmi.twitch.tv PRIVMSG #c :!Egg"),
            IrcLine::Msg(ChatMsg::new("v", "!Egg"))
        );
        // badges and bits
        let IrcLine::Msg(m) = parse_irc(
            "@badge-info=subscriber/8;badges=subscriber/6,vip/1;bits=250;mod=0 :fan!fan@fan.tmi.twitch.tv PRIVMSG #c :Cheer250 nice",
        ) else {
            panic!()
        };
        assert!(m.sub && m.vip && !m.moderator && !m.broadcaster);
        assert_eq!(m.bits, 250);
        assert!(Who::Subs.allows(&m) && !Who::Mods.allows(&m));
        let IrcLine::Msg(m) =
            parse_irc("@badges=moderator/1;mod=1 :mo!mo@mo.tmi.twitch.tv PRIVMSG #c :hi")
        else {
            panic!()
        };
        assert!(m.moderator && Who::Mods.allows(&m));
        assert!(!Who::Subs.allows(&ChatMsg::new("x", "hi")));
        // raids and subs (real Twitch USERNOTICE shapes)
        assert_eq!(
            parse_notice(
                "@badges=;display-name=CoolStreamer;login=coolstreamer;msg-id=raid;msg-param-displayName=CoolStreamer;msg-param-login=coolstreamer;msg-param-viewerCount=42;system-msg=42\\sraiders\\sfrom\\sCoolStreamer :tmi.twitch.tv USERNOTICE #show"
            ),
            Some(ChatEvent::Raid {
                from: "CoolStreamer".into(),
                viewers: 42
            })
        );
        assert_eq!(
            parse_notice(
                "@display-name=Fan_1;login=fan_1;msg-id=resub;msg-param-cumulative-months=7 :tmi.twitch.tv USERNOTICE #show :love it"
            ),
            Some(ChatEvent::Sub {
                user: "Fan_1".into(),
                months: 7,
                gifts: 0
            })
        );
        assert_eq!(
            parse_notice(
                "@display-name=Santa;login=santa;msg-id=submysterygift;msg-param-mass-gift-count=5 :tmi.twitch.tv USERNOTICE #show"
            ),
            Some(ChatEvent::Sub {
                user: "Santa".into(),
                months: 0,
                gifts: 5
            })
        );
        assert_eq!(
            parse_notice(
                "@display-name=Santa;msg-id=subgift;msg-param-community-gift-id=123 :tmi.twitch.tv USERNOTICE #show"
            ),
            None,
            "part of the mystery gift above"
        );
        assert_eq!(
            parse_notice(":fan!fan@fan.tmi.twitch.tv PRIVMSG #c :USERNOTICE"),
            None
        );
        assert_eq!(
            parse_irc(":tmi.twitch.tv 001 justinfan123 :Welcome, GLHF!"),
            IrcLine::Other
        );
        assert_eq!(command_of("!TOMATO lol"), Some(Throw::Tomato));
        assert_eq!(command_of("I like tomato"), None);
        assert_eq!(command_of("!confetti"), Some(Throw::Confetti));
        assert_eq!(vote_of("!up 12"), Some(12));
        assert_eq!(vote_of("!UP #7 please"), Some(7));
        assert_eq!(vote_of("!vote 3"), Some(3));
        assert_eq!(vote_of("!up"), None);
        assert_eq!(vote_of("up 3"), None);
        // moderators' bans and timeouts
        assert_eq!(
            parse_irc(
                "@room-id=1;target-user-id=2;tmi-sent-ts=3 :tmi.twitch.tv CLEARCHAT #show :BadGuy"
            ),
            IrcLine::Ban("badguy".into(), None)
        );
        assert_eq!(
            parse_irc(
                "@ban-duration=600;room-id=1;target-user-id=2 :tmi.twitch.tv CLEARCHAT #show :spammer"
            ),
            IrcLine::Ban("spammer".into(), Some(600))
        );
        assert_eq!(
            parse_irc("@room-id=1 :tmi.twitch.tv CLEARCHAT #show"),
            IrcLine::Other,
            "whole chat cleared"
        );
    }

    #[test]
    fn viewers_and_bans_across_platforms() {
        assert_eq!(ban_key("SomeLogin"), "tw:somelogin");
        assert_eq!(ban_key("kick:Bob"), "kick:bob");
        assert_eq!(ban_key("yt:Bob Smith"), "yt:bob smith");
        assert_eq!(
            ban_key(&format!("yt:{}", "Ann")),
            viewer_key(Platform::YouTube, "ann")
        );
        assert_eq!(ChatMsg::new("a", "b").platform, Platform::Twitch);
    }

    #[test]
    fn rate_gate_limits_floods() {
        let mut g = RateGate::new();
        assert!(g.allow("a", 0.0, 0));
        assert!(!g.allow("b", 0.3, 0), "global gap");
        assert!(g.allow("b", 0.7, 0));
        assert!(!g.allow("a", 5.0, 0), "per-viewer cooldown");
        assert!(!g.allow("c", 5.0, 3), "too many in the air");
        assert!(
            g.allow("A", 13.0, 0),
            "cooldown over, case-insensitive name"
        );
    }

    /// A stand-in Twitch server on localhost: checks the anonymous login and that chat arrives.
    #[test]
    fn reads_chat_from_an_irc_server() {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(&s);
            let mut hello = String::new();
            for _ in 0..4 {
                r.read_line(&mut hello).unwrap();
            }
            (&s).write_all(b":tmi.twitch.tv 001 justinfan :Welcome\r\nPING :tmi.twitch.tv\r\n:fan1!fan1@fan1.tmi.twitch.tv PRIVMSG #show :!tomato\r\n@ban-duration=60 :tmi.twitch.tv CLEARCHAT #show :fan2\r\n").unwrap();
            let mut pong = String::new();
            r.read_line(&mut pong).unwrap();
            (hello, pong)
        });
        let rx = start_twitch(
            "127.0.0.1".into(),
            port,
            "#Show".into(),
            Arc::new(Mutex::new(String::new())),
        );
        let m = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(m, ChatEvent::Msg(ChatMsg::new("fan1", "!tomato")));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ChatEvent::Ban("fan2".into(), Some(60))
        );
        let (hello, pong) = server.join().unwrap();
        assert!(
            hello.contains("CAP REQ :twitch.tv/commands")
                && hello.contains("NICK justinfan")
                && hello.contains("JOIN #show"),
            "{hello}"
        );
        assert_eq!(pong.trim(), "PONG :tmi.twitch.tv");
    }
}
