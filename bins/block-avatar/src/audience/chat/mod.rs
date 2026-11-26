//! Chat-thrown props. Viewers type `!tomato`, `!egg` or `!confetti` in Twitch chat and it flies at
//! the avatar: tomatoes and eggs splat on its face (and it flinches), confetti rains down.
//!
//! Twitch chat is read anonymously over IRC (no login, no OAuth: the read-only `justinfan` user),
//! so all you set is the channel name. YouTube, Kick or anything else: point a chat bot
//! (Streamer.bot, Nightbot, Mix It Up...) at `http://127.0.0.1:7777/cmd/throw:tomato` or
//! `throw:tomato:ViewerName`.

mod irc;
mod twitch;
mod throws;
#[allow(unused_imports)]
pub use irc::*;
#[allow(unused_imports)]
pub use twitch::*;
#[allow(unused_imports)]
pub use throws::*;

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
