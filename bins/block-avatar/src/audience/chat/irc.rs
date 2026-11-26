//! Reading Twitch's IRC lines: tags, notices (bans, raids, subs, bits), messages, votes and throw commands.

#[allow(unused_imports)]
use super::*;

pub(super) fn tags_of(line: &str) -> HashMap<&str, String> {
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
