//! YouTube and Kick chat, read alongside Twitch. Their messages land in the same `Chat` as
//! Twitch's, so throws, dress-up, polls, votes and the viewer crowd all work the same.
//!
//! - YouTube: the YouTube Data API v3 (`liveChat/messages`), with your own API key and the live
//!   video's id. Every poll costs quota (the default is 10,000 units a day and a poll is about 5),
//!   so polls are spaced at least `youtube_poll_s` seconds apart (default 8; that is roughly a
//!   4-hour stream a day). Super Chats count as bits (1 dollar = 100 bits) and new members as subs.
//! - Kick: Kick's own chat page listens on a Pusher websocket; this does the same, read-only and
//!   without signing in. It needs the chatroom id (the app looks it up from the channel name, but
//!   Kick's site often blocks that, so the control page also takes the number). This route is
//!   unofficial and may change without notice; `kick_pusher` overrides the websocket address.

use crate::chat::{ChatEvent, ChatMsg};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

pub const YT_API: &str = "https://www.googleapis.com/youtube/v3";
pub const KICK_WS: &str = "wss://ws-us2.pusher.com/app/32cbd69e4b950bf97679?protocol=7&client=js&version=8.4.0&flash=false";

/// One other platform's reader: messages come in on `rx`; dropping it stops the thread.
pub struct Feed {
    pub rx: Mutex<Receiver<ChatEvent>>,
    pub status: Arc<Mutex<String>>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let mut r = agent().get(url).call().map_err(|e| e.to_string())?;
    let code = r.status().as_u16();
    let body = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let j: serde_json::Value = serde_json::from_str(&body).map_err(|_| format!("HTTP {code}"))?;
    if code >= 400 {
        let why = j["error"]["message"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| format!("HTTP {code}"));
        return Err(why);
    }
    Ok(j)
}

// ------------------------------------------------------------------------------------------
// YouTube
// ------------------------------------------------------------------------------------------

/// A video id from a watch / live / youtu.be link, or the id itself.
pub fn youtube_id(s: &str) -> Option<String> {
    let s = s.trim();
    let id = if let Some((_, q)) = s.split_once("v=") {
        q.split(['&', '#']).next().unwrap_or("")
    } else if s.contains('/') {
        s.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .split(['?', '#'])
            .next()
            .unwrap_or("")
    } else {
        s
    };
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| id.to_string())
}

/// `videos?part=liveStreamingDetails` -> the live chat id.
pub fn youtube_chat_id(j: &serde_json::Value) -> Option<String> {
    j["items"][0]["liveStreamingDetails"]["activeLiveChatId"]
        .as_str()
        .map(String::from)
}

/// One `liveChat/messages` page: the events, the next page token and how long to wait.
pub fn youtube_page(j: &serde_json::Value) -> (Vec<ChatEvent>, Option<String>, u64) {
    let mut out = vec![];
    for it in j["items"].as_array().into_iter().flatten() {
        let sn = &it["snippet"];
        let au = &it["authorDetails"];
        let user = au["displayName"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('@')
            .to_string();
        // a ban has no author worth reading
        if user.is_empty() && sn["type"].as_str() != Some("userBannedEvent") {
            continue;
        }
        match sn["type"].as_str().unwrap_or("") {
            "textMessageEvent" | "superChatEvent" => {
                let micros = sn["superChatDetails"]["amountMicros"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or(sn["superChatDetails"]["amountMicros"].as_u64())
                    .unwrap_or(0);
                out.push(ChatEvent::Msg(ChatMsg {
                    platform: crate::chat::Platform::YouTube,
                    user,
                    text: sn["displayMessage"]
                        .as_str()
                        .or(sn["superChatDetails"]["userComment"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    sub: au["isChatSponsor"].as_bool().unwrap_or(false),
                    vip: false,
                    moderator: au["isChatModerator"].as_bool().unwrap_or(false),
                    broadcaster: au["isChatOwner"].as_bool().unwrap_or(false),
                    bits: (micros / 10_000) as u32,
                }));
            }
            "userBannedEvent" => {
                let d = &sn["userBannedDetails"];
                if let Some(who) = d["bannedUserDetails"]["displayName"].as_str() {
                    let secs = (d["banType"].as_str() == Some("temporary")).then(|| {
                        d["banDurationSeconds"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .or(d["banDurationSeconds"].as_u64())
                            .unwrap_or(300)
                    });
                    out.push(ChatEvent::Ban(
                        format!("yt:{}", who.trim_start_matches('@')),
                        secs,
                    ));
                }
            }
            "newSponsorEvent" | "memberMilestoneChatEvent" => {
                let months = sn["memberMilestoneChatDetails"]["memberMonth"]
                    .as_u64()
                    .unwrap_or(1) as u32;
                out.push(ChatEvent::Sub {
                    user,
                    months,
                    gifts: 0,
                });
            }
            _ => {}
        }
    }
    let next = j["nextPageToken"].as_str().map(String::from);
    let wait = j["pollingIntervalMillis"].as_u64().unwrap_or(5000);
    (out, next, wait)
}

pub fn start_youtube(base: String, key: String, video: String, min_poll_s: u64) -> Feed {
    let (tx, rx) = mpsc::channel();
    let status = Arc::new(Mutex::new(format!("YouTube: connecting to {video}")));
    let st = status.clone();
    std::thread::Builder::new()
        .name("youtube".into())
        .spawn(move || youtube_loop(&base, &key, &video, min_poll_s, &tx, &st))
        .expect("spawn youtube thread");
    Feed {
        rx: Mutex::new(rx),
        status,
    }
}

fn youtube_loop(
    base: &str,
    key: &str,
    video: &str,
    min_poll_s: u64,
    tx: &Sender<ChatEvent>,
    st: &Mutex<String>,
) {
    let set = |s: String| *st.lock().unwrap() = s;
    let chat_id = loop {
        match get_json(&format!(
            "{base}/videos?part=liveStreamingDetails&id={video}&key={key}"
        )) {
            Ok(j) => match youtube_chat_id(&j) {
                Some(c) => break c,
                None => set(
                    "YouTube: that video has no live chat right now (retrying every 30 s)".into(),
                ),
            },
            Err(e) => set(format!("YouTube: {e} (retrying every 30 s)")),
        }
        std::thread::sleep(Duration::from_secs(30));
        if tx.send(ChatEvent::Ban(String::new(), Some(0))).is_err() {
            return; // app closed or feed replaced
        }
    };
    let mut page: Option<String> = None;
    let mut first = true;
    loop {
        let url = format!(
            "{base}/liveChat/messages?liveChatId={chat_id}&part=snippet,authorDetails&maxResults=200&key={key}{}",
            page.as_ref()
                .map(|p| format!("&pageToken={p}"))
                .unwrap_or_default()
        );
        let wait = match get_json(&url) {
            Ok(j) => {
                let (evs, next, wait) = youtube_page(&j);
                set(format!("YouTube: reading {video}"));
                // the first page is the chat's history: skip it, as Twitch would
                if !first {
                    for e in evs {
                        if tx.send(e).is_err() {
                            return;
                        }
                    }
                }
                first = false;
                if next.is_none() && j["offlineAt"].is_string() {
                    set("YouTube: the stream has ended".into());
                    return;
                }
                page = next.or(page);
                wait
            }
            Err(e) => {
                let quota = e.to_ascii_lowercase().contains("quota");
                set(format!(
                    "YouTube: {e}{}",
                    if quota {
                        " (daily quota used up: raise youtube_poll_s)"
                    } else {
                        ""
                    }
                ));
                if quota { 600_000 } else { 15_000 }
            }
        };
        std::thread::sleep(Duration::from_millis(wait.max(min_poll_s * 1000)));
    }
}

// ------------------------------------------------------------------------------------------
// Kick
// ------------------------------------------------------------------------------------------

/// `api/v2/channels/<slug>` -> the chatroom id.
pub fn kick_chatroom(j: &serde_json::Value) -> Option<u64> {
    j["chatroom"]["id"].as_u64()
}

/// The subscribe message for a chatroom.
pub fn kick_subscribe(room: u64) -> String {
    serde_json::json!({"event": "pusher:subscribe", "data": {"auth": "", "channel": format!("chatrooms.{room}.v2")}}).to_string()
}

/// One Pusher frame -> a chat event. Pusher double-encodes: `data` is itself a JSON string.
pub fn kick_frame(frame: &str) -> Option<ChatEvent> {
    let f: serde_json::Value = serde_json::from_str(frame).ok()?;
    let data: serde_json::Value = match &f["data"] {
        serde_json::Value::String(s) => serde_json::from_str(s).ok()?,
        v => v.clone(),
    };
    match f["event"].as_str()? {
        "App\\Events\\ChatMessageEvent" => {
            let s = &data["sender"];
            let badges: Vec<&str> = s["identity"]["badges"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["type"].as_str())
                .collect();
            let has = |t: &str| badges.contains(&t);
            Some(ChatEvent::Msg(ChatMsg {
                platform: crate::chat::Platform::Kick,
                user: s["username"].as_str()?.to_string(),
                text: data["content"].as_str().unwrap_or("").to_string(),
                sub: has("subscriber") || has("founder") || has("og"),
                vip: has("vip"),
                moderator: has("moderator"),
                broadcaster: has("broadcaster"),
                bits: 0,
            }))
        }
        "App\\Events\\SubscriptionEvent" => Some(ChatEvent::Sub {
            user: data["username"].as_str()?.to_string(),
            months: data["months"].as_u64().unwrap_or(1) as u32,
            gifts: 0,
        }),
        "App\\Events\\GiftedSubscriptionsEvent" => {
            let n = data["gifted_usernames"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(1) as u32;
            Some(ChatEvent::Sub {
                user: data["gifter_username"]
                    .as_str()
                    .unwrap_or("someone")
                    .to_string(),
                months: 1,
                gifts: n,
            })
        }
        "App\\Events\\StreamHostEvent" => Some(ChatEvent::Raid {
            from: data["host_username"].as_str()?.to_string(),
            viewers: data["number_viewers"].as_u64().unwrap_or(0) as u32,
        }),
        "App\\Events\\UserBannedEvent" => {
            let secs = data["duration"].as_u64().map(|m| m * 60);
            Some(ChatEvent::Ban(
                format!("kick:{}", data["user"]["username"].as_str()?),
                if data["permanent"].as_bool().unwrap_or(false) {
                    None
                } else {
                    secs
                },
            ))
        }
        _ => None,
    }
}

/// `channel` is a channel name or a chatroom number.
pub fn start_kick(ws: String, channel: String) -> Feed {
    let (tx, rx) = mpsc::channel();
    let status = Arc::new(Mutex::new(format!("Kick: connecting to {channel}")));
    let st = status.clone();
    std::thread::Builder::new()
        .name("kick".into())
        .spawn(move || kick_loop(&ws, &channel, &tx, &st))
        .expect("spawn kick thread");
    Feed {
        rx: Mutex::new(rx),
        status,
    }
}

fn kick_loop(ws: &str, channel: &str, tx: &Sender<ChatEvent>, st: &Mutex<String>) {
    let set = |s: String| *st.lock().unwrap() = s;
    let room = match channel.parse::<u64>() {
        Ok(n) => n,
        Err(_) => match get_json(&format!("https://kick.com/api/v2/channels/{channel}"))
            .ok()
            .as_ref()
            .and_then(kick_chatroom)
        {
            Some(n) => n,
            None => {
                set(format!(
                    "Kick: couldn't look up {channel}'s chatroom (Kick blocks apps); enter its chatroom number instead"
                ));
                return;
            }
        },
    };
    loop {
        match tungstenite::connect(ws) {
            Ok((mut sock, _)) => {
                if let tungstenite::stream::MaybeTlsStream::Rustls(s) = sock.get_mut() {
                    let _ = s.get_mut().set_read_timeout(Some(Duration::from_secs(150)));
                }
                if sock
                    .send(tungstenite::Message::text(kick_subscribe(room)))
                    .is_err()
                {
                    continue;
                }
                set(format!("Kick: reading {channel}"));
                loop {
                    match sock.read() {
                        Ok(tungstenite::Message::Text(t)) => {
                            if t.contains("pusher:ping") {
                                let _ = sock.send(tungstenite::Message::text(
                                    r#"{"event":"pusher:pong","data":{}}"#,
                                ));
                            } else if let Some(ev) = kick_frame(&t) {
                                if tx.send(ev).is_err() {
                                    return;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                set(format!("Kick: disconnected from {channel}, reconnecting"));
            }
            Err(e) => set(format!("Kick: can't connect ({e}), retrying")),
        }
        std::thread::sleep(Duration::from_secs(5));
        // stop when the app has dropped this feed
        if tx.send(ChatEvent::Ban(String::new(), Some(0))).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_ids_from_links() {
        for s in [
            "dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ?si=x",
        ] {
            assert_eq!(youtube_id(s).as_deref(), Some("dQw4w9WgXcQ"), "{s}");
        }
        assert_eq!(youtube_id("not a video"), None);
    }

    #[test]
    fn reads_a_youtube_page() {
        let j: serde_json::Value = serde_json::from_str(
            r#"{"nextPageToken":"p2","pollingIntervalMillis":3000,"items":[
            {"snippet":{"type":"textMessageEvent","displayMessage":"!throw tomato"},"authorDetails":{"displayName":"@Fan","isChatModerator":true,"isChatOwner":false,"isChatSponsor":false}},
            {"snippet":{"type":"superChatEvent","superChatDetails":{"amountMicros":"5000000","userComment":"love it"}},"authorDetails":{"displayName":"Big","isChatSponsor":true}},
            {"snippet":{"type":"newSponsorEvent"},"authorDetails":{"displayName":"Newbie"}},
            {"snippet":{"type":"chatEndedEvent"},"authorDetails":{"displayName":"x"}},
            {"snippet":{"type":"userBannedEvent","userBannedDetails":{"bannedUserDetails":{"displayName":"Troll"},"banType":"temporary","banDurationSeconds":"300"}},"authorDetails":{"displayName":"Mod"}}]}"#,
        )
        .unwrap();
        let (evs, next, wait) = youtube_page(&j);
        assert_eq!((next.as_deref(), wait, evs.len()), (Some("p2"), 3000, 4));
        assert_eq!(evs[3], ChatEvent::Ban("yt:Troll".into(), Some(300)));
        let ChatEvent::Msg(m) = &evs[0] else { panic!() };
        assert_eq!(
            (m.user.as_str(), m.text.as_str(), m.moderator, m.platform),
            ("Fan", "!throw tomato", true, crate::chat::Platform::YouTube)
        );
        let ChatEvent::Msg(m) = &evs[1] else { panic!() };
        assert_eq!((m.bits, m.sub, m.text.as_str()), (500, true, "love it"));
        assert_eq!(
            evs[2],
            ChatEvent::Sub {
                user: "Newbie".into(),
                months: 1,
                gifts: 0
            }
        );
        let v: serde_json::Value = serde_json::from_str(
            r#"{"items":[{"liveStreamingDetails":{"activeLiveChatId":"Cg0"}}]}"#,
        )
        .unwrap();
        assert_eq!(youtube_chat_id(&v).as_deref(), Some("Cg0"));
    }

    #[test]
    fn reads_kick_frames() {
        let msg = r##"{"event":"App\\Events\\ChatMessageEvent","data":"{\"id\":\"1\",\"chatroom_id\":7,\"content\":\"!hat cowboy\",\"type\":\"message\",\"sender\":{\"id\":3,\"username\":\"KickFan\",\"identity\":{\"color\":\"#fff\",\"badges\":[{\"type\":\"subscriber\",\"text\":\"Sub\"},{\"type\":\"moderator\",\"text\":\"Mod\"}]}}}","channel":"chatrooms.7.v2"}"##;
        let Some(ChatEvent::Msg(m)) = kick_frame(msg) else {
            panic!()
        };
        assert_eq!(
            (m.user.as_str(), m.text.as_str(), m.sub, m.moderator, m.vip),
            ("KickFan", "!hat cowboy", true, true, false)
        );
        let host = r#"{"event":"App\\Events\\StreamHostEvent","data":"{\"host_username\":\"Pal\",\"number_viewers\":42}"}"#;
        assert_eq!(
            kick_frame(host),
            Some(ChatEvent::Raid {
                from: "Pal".into(),
                viewers: 42
            })
        );
        assert_eq!(
            kick_frame(r#"{"event":"pusher:connection_established","data":"{}"}"#),
            None
        );
        let ban = r#"{"event":"App\\Events\\UserBannedEvent","data":"{\"user\":{\"username\":\"Troll\"},\"permanent\":true}"}"#;
        assert_eq!(
            kick_frame(ban),
            Some(ChatEvent::Ban("kick:Troll".into(), None))
        );
        assert!(kick_subscribe(7).contains("chatrooms.7.v2"));
        let ch: serde_json::Value = serde_json::from_str(r#"{"chatroom":{"id":668}}"#).unwrap();
        assert_eq!(kick_chatroom(&ch), Some(668));
    }

    /// A stand-in for the YouTube API: the history page is skipped, the next one comes through.
    #[test]
    fn polls_a_youtube_server() {
        let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = srv.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            let mut n = 0;
            for rq in srv.incoming_requests() {
                let body = if rq.url().contains("/videos?") {
                    r#"{"items":[{"liveStreamingDetails":{"activeLiveChatId":"C1"}}]}"#.to_string()
                } else {
                    n += 1;
                    let who = if n == 1 { "Old" } else { "New" };
                    format!(
                        r#"{{"nextPageToken":"p{n}","pollingIntervalMillis":10,"items":[{{"snippet":{{"type":"textMessageEvent","displayMessage":"hi"}},"authorDetails":{{"displayName":"{who}"}}}}]}}"#
                    )
                };
                let _ = rq.respond(tiny_http::Response::from_string(body));
            }
        });
        let f = start_youtube(
            format!("http://127.0.0.1:{port}"),
            "k".into(),
            "dQw4w9WgXcQ".into(),
            0,
        );
        let ev =
            f.rx.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        let ChatEvent::Msg(m) = ev else { panic!() };
        assert_eq!(m.user, "New");
        assert!(f.status.lock().unwrap().contains("reading"));
    }
}
