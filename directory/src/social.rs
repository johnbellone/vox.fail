//! Signing in with Steam (OpenID 2.0) and Discord (OAuth2).
//!
//! Both start at `/auth/<site>`, which sends the browser to the site with a random state kept
//! in a short-lived cookie, and come back to `/auth/<site>/return`. Steam's answer is checked
//! by asking Steam whether it said it (`check_authentication`); Discord's code is traded for a
//! token that reads who you are. Either way the site's id for you (a SteamID, a Discord user
//! id) finds your account, or makes one named after you there. Signed in already, a new site
//! is added to your account instead.

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{AppendHeaders, IntoResponse, Response},
};
use reqwest::Url;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use crate::{Directory, check_name, cookie, now, random_hex};

const STATE: &str = "vox_auth";
const OPENID_NS: &str = "http://specs.openid.net/auth/2.0";
const IDENTIFIER_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";
const STEAM_ID: &str = "https://steamcommunity.com/openid/id/";

/// Discord's application (from discord.com/developers).
#[derive(Clone, Debug)]
pub struct Discord {
    pub client_id: String,
    pub client_secret: String,
}

/// Where the sites are (tests point these at stand-ins).
#[derive(Clone, Debug)]
pub struct Sites {
    pub steam_openid: String,
    pub steam_api: String,
    pub discord_authorize: String,
    pub discord_api: String,
}

impl Default for Sites {
    fn default() -> Self {
        Sites {
            steam_openid: "https://steamcommunity.com/openid/login".into(),
            steam_api: "https://api.steampowered.com".into(),
            discord_authorize: "https://discord.com/oauth2/authorize".into(),
            discord_api: "https://discord.com/api/v10".into(),
        }
    }
}

/// How people can sign in besides a name and password.
#[derive(Clone, Debug, Default)]
pub struct Social {
    /// this site's address as visitors see it (`https://vox.fail`): where the sites send
    /// people back to
    pub public_url: String,
    /// Steam sign-in on; with a Steam Web API key, new accounts take your Steam name
    pub steam: bool,
    pub steam_key: Option<String>,
    pub discord: Option<Discord>,
    pub sites: Sites,
}

impl Social {
    /// The sites you can sign in with here.
    pub fn offered(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.steam {
            v.push("steam");
        }
        if self.discord.is_some() {
            v.push("discord");
        }
        v
    }

    fn back(&self, site: &str) -> String {
        format!("{}/auth/{site}/return", self.public_url.trim_end_matches('/'))
    }
}

pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("vox-directory")
        .build()
        .expect("http client")
}

/// Off to `url`, remembering `state` for `site`.
fn go(d: &Directory, url: Url, site: &str, state: &str) -> Response {
    let secure = if d.0.secure { "; Secure" } else { "" };
    let c = format!("{STATE}={site}.{state}; Path=/auth; HttpOnly; SameSite=Lax; Max-Age=600{secure}");
    (
        StatusCode::SEE_OTHER,
        AppendHeaders([
            (header::LOCATION, HeaderValue::from_str(url.as_str()).expect("url")),
            (header::SET_COOKIE, HeaderValue::from_str(&c).expect("cookie")),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ]),
    )
        .into_response()
}

/// Back to the start page, with what went wrong.
fn failed(why: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        AppendHeaders([
            (header::LOCATION, HeaderValue::from_str(&format!("/?error={why}")).expect("url")),
            (header::SET_COOKIE, HeaderValue::from_static("vox_auth=; Path=/auth; HttpOnly; SameSite=Lax; Max-Age=0")),
        ]),
    )
        .into_response()
}

/// The state this browser was sent off with, if it's for `site` and matches `given`.
fn state_ok(h: &HeaderMap, site: &str, given: Option<&str>) -> bool {
    let Some(kept) = cookie(h, STATE) else { return false };
    let Some((s, state)) = kept.split_once('.') else { return false };
    s == site && state.len() == 32 && given == Some(state)
}

pub(crate) async fn steam_start(State(d): State<Directory>) -> Response {
    let s = &d.0.social;
    if !s.steam {
        return StatusCode::NOT_FOUND.into_response();
    }
    let state = random_hex(16);
    let back = format!("{}?state={state}", s.back("steam"));
    let realm = format!("{}/", s.public_url.trim_end_matches('/'));
    let Ok(url) = Url::parse_with_params(
        &s.sites.steam_openid,
        &[
            ("openid.ns", OPENID_NS),
            ("openid.mode", "checkid_setup"),
            ("openid.return_to", &back),
            ("openid.realm", &realm),
            ("openid.identity", IDENTIFIER_SELECT),
            ("openid.claimed_id", IDENTIFIER_SELECT),
        ],
    ) else {
        return failed("steam");
    };
    go(&d, url, "steam", &state)
}

pub(crate) async fn steam_return(
    State(d): State<Directory>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let s = &d.0.social;
    if !s.steam {
        return StatusCode::NOT_FOUND.into_response();
    }
    if d.tried_up_to(&format!("auth:{}", d.visitor(addr, &h)), 30) {
        return failed("busy");
    }
    let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    if !state_ok(&h, "steam", get("state")) {
        return failed("expired");
    }
    if get("openid.mode") == Some("cancel") {
        return failed("cancelled");
    }
    // what Steam must have said: to us, about one of its ids
    let back = s.back("steam");
    let ok = get("openid.mode") == Some("id_res")
        && get("openid.ns") == Some(OPENID_NS)
        && get("openid.op_endpoint") == Some(s.sites.steam_openid.as_str())
        && get("openid.return_to").is_some_and(|r| r.split('?').next() == Some(back.as_str()))
        && get("openid.claimed_id") == get("openid.identity");
    let id = get("openid.claimed_id")
        .and_then(|c| c.strip_prefix(STEAM_ID))
        .filter(|id| !id.is_empty() && id.len() <= 20 && id.chars().all(|c| c.is_ascii_digit()))
        .map(String::from);
    let (true, Some(id)) = (ok, id) else { return failed("steam") };
    // and Steam says it said it
    let mut check: Vec<(String, String)> = q.iter().filter(|(k, _)| k.starts_with("openid.")).cloned().collect();
    for (k, v) in check.iter_mut() {
        if k == "openid.mode" {
            *v = "check_authentication".into();
        }
    }
    let valid = match d.0.http.post(&s.sites.steam_openid).form(&check).send().await {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default().lines().any(|l| l.trim() == "is_valid:true"),
        _ => false,
    };
    if !valid {
        return failed("steam");
    }
    // your Steam name, if we can ask
    let mut name = String::new();
    if let Some(key) = &s.steam_key {
        let url = format!("{}/ISteamUser/GetPlayerSummaries/v2/", s.sites.steam_api.trim_end_matches('/'));
        if let Ok(r) = d.0.http.get(url).query(&[("key", key.as_str()), ("steamids", id.as_str())]).send().await {
            if let Ok(v) = r.json::<Value>().await {
                name = v["response"]["players"][0]["personaname"].as_str().unwrap_or_default().to_string();
            }
        }
    }
    d.signed_in_with(&h, "steam", &id, &name, "Steam player")
}

pub(crate) async fn discord_start(State(d): State<Directory>) -> Response {
    let s = &d.0.social;
    let Some(app) = &s.discord else { return StatusCode::NOT_FOUND.into_response() };
    let state = random_hex(16);
    let back = s.back("discord");
    let Ok(url) = Url::parse_with_params(
        &s.sites.discord_authorize,
        &[
            ("response_type", "code"),
            ("client_id", app.client_id.as_str()),
            ("scope", "identify"),
            ("redirect_uri", back.as_str()),
            ("state", state.as_str()),
            ("prompt", "none"),
        ],
    ) else {
        return failed("discord");
    };
    go(&d, url, "discord", &state)
}

pub(crate) async fn discord_return(
    State(d): State<Directory>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let s = &d.0.social;
    let Some(app) = &s.discord else { return StatusCode::NOT_FOUND.into_response() };
    if d.tried_up_to(&format!("auth:{}", d.visitor(addr, &h)), 30) {
        return failed("busy");
    }
    if !state_ok(&h, "discord", q.get("state").map(String::as_str)) {
        return failed("expired");
    }
    if q.contains_key("error") {
        return failed("cancelled");
    }
    let Some(code) = q.get("code") else { return failed("discord") };
    let api = s.sites.discord_api.trim_end_matches('/');
    let back = s.back("discord");
    let form = [("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", back.as_str())];
    let token = match d.0.http.post(format!("{api}/oauth2/token")).basic_auth(&app.client_id, Some(&app.client_secret)).form(&form).send().await {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok().and_then(|v| v["access_token"].as_str().map(String::from)),
        _ => None,
    };
    let Some(token) = token else { return failed("discord") };
    let me = match d.0.http.get(format!("{api}/users/@me")).bearer_auth(token).send().await {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
        _ => None,
    };
    let Some(me) = me else { return failed("discord") };
    let Some(id) = me["id"].as_str().filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())) else {
        return failed("discord");
    };
    let name = me["global_name"].as_str().or(me["username"].as_str()).unwrap_or_default();
    d.signed_in_with(&h, "discord", id, name, "Discord player")
}

/// A name people can go by, made from a name on another site: the characters names may use,
/// at most 20; `fallback` if too little is left.
pub fn name_from(raw: &str, fallback: &str) -> String {
    let mut n = String::new();
    for c in raw.chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if !(c.is_alphanumeric() || " _-.".contains(c)) || (c == ' ' && n.ends_with(' ')) {
            continue;
        }
        if n.is_empty() && !c.is_alphanumeric() {
            continue;
        }
        n.push(c);
    }
    let n: String = n.chars().take(20).collect::<String>().trim_end().to_string();
    check_name(&n).unwrap_or_else(|_| fallback.to_string())
}

impl Directory {
    /// `site` vouches that this browser is its user `subject` (who goes by `name` there).
    fn signed_in_with(&self, h: &HeaderMap, site: &str, subject: &str, name: &str, fallback: &str) -> Response {
        let current = self.account(h).map(|(id, _)| id);
        let account = {
            let mut db = self.0.db.lock().unwrap();
            let Ok(tx) = db.transaction() else { return failed("busy") };
            let known: Option<i64> = tx
                .query_row("SELECT account FROM identities WHERE site = ?1 AND subject = ?2", params![site, subject], |r| r.get(0))
                .optional()
                .unwrap_or(None);
            let account = match (known, current) {
                (Some(a), _) => a as u64,
                // signed in already: add this site to your account
                (None, Some(a)) => a,
                // someone new: an account named after you there (or the nearest free name)
                (None, None) => {
                    let base = name_from(name, fallback);
                    let mut made = None;
                    for n in 1..1000 {
                        let try_name = if n == 1 {
                            base.clone()
                        } else {
                            let tail = format!(" {n}");
                            let head: String = base.chars().take(20 - tail.len()).collect();
                            format!("{}{tail}", head.trim_end())
                        };
                        if tx
                            .execute("INSERT INTO accounts (name, pass, created) VALUES (?1, '', ?2)", params![try_name, now() as i64])
                            .is_ok()
                        {
                            made = Some(tx.last_insert_rowid() as u64);
                            break;
                        }
                    }
                    let Some(a) = made else { return failed("busy") };
                    a
                }
            };
            if known.is_none()
                && tx
                    .execute(
                        "INSERT INTO identities (site, subject, account, created) VALUES (?1, ?2, ?3, ?4)",
                        params![site, subject, account as i64, now() as i64],
                    )
                    .is_err()
            {
                return failed("busy");
            }
            if tx.commit().is_err() {
                return failed("busy");
            }
            account
        };
        let next = if self.character(account).is_some() { "/" } else { "/create" };
        let session = if current == Some(account) { None } else { Some(self.session(account)) };
        let mut r = (StatusCode::SEE_OTHER, [(header::LOCATION, HeaderValue::from_static(next))]).into_response();
        let hs = r.headers_mut();
        hs.append(header::SET_COOKIE, HeaderValue::from_static("vox_auth=; Path=/auth; HttpOnly; SameSite=Lax; Max-Age=0"));
        if let Some(c) = session {
            hs.append(header::SET_COOKIE, c);
        }
        hs.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        r
    }

    /// The sites an account can sign in with.
    pub(crate) fn linked(&self, account: u64) -> Vec<String> {
        let db = self.0.db.lock().unwrap();
        let Ok(mut st) = db.prepare("SELECT site FROM identities WHERE account = ?1 ORDER BY site") else { return vec![] };
        st.query_map(params![account as i64], |r| r.get(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }
}
