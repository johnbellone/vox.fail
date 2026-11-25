//! Signing in with Steam and Discord, against stand-ins for both.

use std::{collections::HashMap, path::PathBuf};

use axum::{
    Form, Json, Router,
    extract::Query,
    http::HeaderMap,
    routing::{get, post},
};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use vox_core::auth;
use vox_directory::{Config, Directory, Discord, Sites, Social, name_from};

const STEAM_ID: &str = "https://steamcommunity.com/openid/id/";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

async fn listen(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    format!("http://{addr}")
}

/// Steam and Discord as far as the directory sees them.
async fn sites() -> Sites {
    let steam = Router::new()
        // Steam vouches for what it signed (here: what says sig=good)
        .route(
            "/openid/login",
            post(|Form(f): Form<HashMap<String, String>>| async move {
                let ok = f.get("openid.mode").map(String::as_str) == Some("check_authentication")
                    && f.get("openid.sig").map(String::as_str) == Some("good");
                format!("ns:http://specs.openid.net/auth/2.0\nis_valid:{ok}\n")
            }),
        )
        .route(
            "/ISteamUser/GetPlayerSummaries/v2/",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                let name = if q.get("steamids").map(String::as_str) == Some("76561190000000001") { "Gabe <script> Player!!" } else { "" };
                Json(json!({ "response": { "players": [{ "personaname": name }] } }))
            }),
        );
    let steam = listen(steam).await;
    let discord = Router::new()
        .route(
            "/api/oauth2/token",
            post(|h: HeaderMap, Form(f): Form<HashMap<String, String>>| async move {
                // client id "app", secret "shh"
                let basic = h.get("authorization").and_then(|v| v.to_str().ok()) == Some("Basic YXBwOnNoaA==");
                match (basic, f.get("code").map(String::as_str)) {
                    (true, Some("abc")) => (axum::http::StatusCode::OK, Json(json!({ "access_token": "tok-abc" }))),
                    (true, Some("def")) => (axum::http::StatusCode::OK, Json(json!({ "access_token": "tok-def" }))),
                    _ => (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" }))),
                }
            }),
        )
        .route(
            "/api/users/@me",
            get(|h: HeaderMap| async move {
                match h.get("authorization").and_then(|v| v.to_str().ok()) {
                    Some("Bearer tok-abc") => Json(json!({ "id": "80351110224678912", "username": "samhill", "global_name": "Sam Hill" })),
                    Some("Bearer tok-def") => Json(json!({ "id": "80351110224678913", "username": "x", "global_name": null })),
                    _ => Json(json!({})),
                }
            }),
        );
    let discord = listen(discord).await;
    Sites {
        steam_openid: format!("{steam}/openid/login"),
        steam_api: steam,
        discord_authorize: format!("{discord}/oauth2/authorize"),
        discord_api: format!("{discord}/api"),
    }
}

async fn serve() -> (Directory, String, Sites) {
    let sites = sites().await;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let cfg = Config {
        db: ":memory:".into(),
        key: auth::key_from_seed([6; 32]),
        web: root().join("web"),
        assets: root().join("assets"),
        data: root().join("data"),
        worlds: vec![],
        secure_cookies: false,
        downloads: vec![],
        behind_proxy: false,
        social: Social {
            public_url: base.clone(),
            steam: true,
            steam_key: Some("k".into()),
            discord: Some(Discord { client_id: "app".into(), client_secret: "shh".into() }),
            sites: sites.clone(),
        },
    };
    let d = Directory::open(cfg).unwrap();
    let app = d.router().into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (d, base, sites)
}

/// A browser that stops at each redirect.
fn browser() -> Client {
    Client::builder().cookie_store(true).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

async fn go(c: &Client, url: &str) -> String {
    let r = c.get(url).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 303, "{url}");
    r.headers()["location"].to_str().unwrap().to_string()
}

fn param(url: &str, k: &str) -> String {
    Url::parse(url).unwrap().query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.to_string()).unwrap_or_default()
}

async fn me(c: &Client, base: &str) -> Value {
    c.get(format!("{base}/api/me")).send().await.unwrap().json().await.unwrap()
}

/// Off to Steam and back as `id`, Steam's signature `sig`: where the directory sends you.
async fn via_steam(c: &Client, base: &str, sites: &Sites, id: &str, sig: &str) -> String {
    let at = go(c, &format!("{base}/auth/steam")).await;
    assert!(at.starts_with(&sites.steam_openid), "{at}");
    assert_eq!(param(&at, "openid.mode"), "checkid_setup");
    assert_eq!(param(&at, "openid.realm"), format!("{base}/"));
    let back = param(&at, "openid.return_to");
    assert!(back.starts_with(&format!("{base}/auth/steam/return?state=")), "{back}");
    let claimed = format!("{STEAM_ID}{id}");
    let ret = Url::parse_with_params(
        &back,
        &[
            ("openid.ns", "http://specs.openid.net/auth/2.0"),
            ("openid.mode", "id_res"),
            ("openid.op_endpoint", sites.steam_openid.as_str()),
            ("openid.claimed_id", claimed.as_str()),
            ("openid.identity", claimed.as_str()),
            ("openid.return_to", back.as_str()),
            ("openid.response_nonce", "2026-11-25T00:00:00Zabc"),
            ("openid.assoc_handle", "1234567890"),
            ("openid.signed", "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle"),
            ("openid.sig", sig),
        ],
    )
    .unwrap();
    go(c, ret.as_str()).await
}

async fn via_discord(c: &Client, base: &str, sites: &Sites, code: &str) -> String {
    let at = go(c, &format!("{base}/auth/discord")).await;
    assert!(at.starts_with(&sites.discord_authorize), "{at}");
    assert_eq!(param(&at, "client_id"), "app");
    assert_eq!(param(&at, "scope"), "identify");
    let back = param(&at, "redirect_uri");
    assert_eq!(back, format!("{base}/auth/discord/return"));
    let ret = Url::parse_with_params(&back, &[("code", code), ("state", param(&at, "state").as_str())]).unwrap();
    go(c, ret.as_str()).await
}

#[tokio::test]
async fn steam() {
    let (_d, base, sites) = serve().await;
    let c = browser();
    assert_eq!(me(&c, &base).await["sign_in_with"], json!(["steam", "discord"]));
    // first time: an account named after you on Steam, then make your character
    assert_eq!(via_steam(&c, &base, &sites, "76561190000000001", "good").await, "/create");
    let m = me(&c, &base).await;
    assert_eq!(m["name"], "Gabe script Player");
    assert_eq!(m["linked"], json!(["steam"]));
    // a character, then back in from another browser: same account, straight to the start page
    let r = c.put(format!("{base}/api/character")).json(&json!({"model": "models/dog.glb", "wear": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let c2 = browser();
    assert_eq!(via_steam(&c2, &base, &sites, "76561190000000001", "good").await, "/");
    assert_eq!(me(&c2, &base).await["name"], "Gabe script Player");
    // no Steam name to go by
    let c3 = browser();
    assert_eq!(via_steam(&c3, &base, &sites, "76561190000000002", "good").await, "/create");
    assert_eq!(me(&c3, &base).await["name"], "Steam player");
    // what Steam didn't sign
    let c4 = browser();
    assert_eq!(via_steam(&c4, &base, &sites, "76561190000000003", "forged").await, "/?error=steam");
    assert_eq!(me(&c4, &base).await["name"], Value::Null);
    // someone else's id in the answer
    let at = go(&c4, &format!("{base}/auth/steam")).await;
    let back = param(&at, "openid.return_to");
    let ret = Url::parse_with_params(
        &back,
        &[
            ("openid.ns", "http://specs.openid.net/auth/2.0"),
            ("openid.mode", "id_res"),
            ("openid.op_endpoint", "https://evil.example/openid/login"),
            ("openid.claimed_id", "https://steamcommunity.com/openid/id/76561190000000001"),
            ("openid.identity", "https://steamcommunity.com/openid/id/76561190000000001"),
            ("openid.return_to", back.as_str()),
            ("openid.sig", "good"),
        ],
    )
    .unwrap();
    assert_eq!(go(&c4, ret.as_str()).await, "/?error=steam");
    // an answer meant for another browser (no state cookie, or another state)
    let c5 = browser();
    let at = go(&c, &format!("{base}/auth/steam")).await;
    let stolen = Url::parse_with_params(&param(&at, "openid.return_to"), &[("openid.mode", "id_res")]).unwrap();
    assert_eq!(go(&c5, stolen.as_str()).await, "/?error=expired");
    go(&c5, &format!("{base}/auth/steam")).await;
    assert_eq!(go(&c5, stolen.as_str()).await, "/?error=expired");
    assert_eq!(me(&c5, &base).await["name"], Value::Null);
}

#[tokio::test]
async fn discord_and_linking() {
    let (_d, base, sites) = serve().await;
    // "Sam Hill" is taken by a password account
    let a = browser();
    let r = a.post(format!("{base}/api/signup")).json(&json!({"name": "Sam Hill", "password": "correct horse"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let b = browser();
    assert_eq!(via_discord(&b, &base, &sites, "abc").await, "/create");
    let m = me(&b, &base).await;
    assert_eq!(m["name"], "Sam Hill 2");
    assert_eq!(m["linked"], json!(["discord"]));
    // a code Discord won't trade
    let c = browser();
    assert_eq!(via_discord(&c, &base, &sites, "nope").await, "/?error=discord");
    // turned down at Discord
    let at = go(&c, &format!("{base}/auth/discord")).await;
    let ret = Url::parse_with_params(&format!("{base}/auth/discord/return"), &[("error", "access_denied"), ("state", param(&at, "state").as_str())]).unwrap();
    assert_eq!(go(&c, ret.as_str()).await, "/?error=cancelled");
    // the password account adds Discord (a new Discord user) and Steam
    assert_eq!(via_discord(&a, &base, &sites, "def").await, "/create");
    assert_eq!(via_steam(&a, &base, &sites, "76561190000000009", "good").await, "/create");
    let m = me(&a, &base).await;
    assert_eq!(m["name"], "Sam Hill");
    assert_eq!(m["linked"], json!(["discord", "steam"]));
    // so Steam signs you into it from anywhere
    let d = browser();
    via_steam(&d, &base, &sites, "76561190000000009", "good").await;
    assert_eq!(me(&d, &base).await["name"], "Sam Hill");
    // and a site-made account has no password to guess
    let r = d.post(format!("{base}/api/signin")).json(&json!({"name": "Sam Hill 2", "password": ""})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
}

#[test]
fn names_from_other_sites() {
    assert_eq!(name_from("Gabe <script> Player!!", "x"), "Gabe script Player");
    assert_eq!(name_from("  ~~cool   dude~~ ", "x"), "cool dude");
    assert_eq!(name_from("ab", "Steam player"), "Steam player");
    assert_eq!(name_from("★★★", "Steam player"), "Steam player");
    assert_eq!(name_from("a very long name that goes on", "x"), "a very long name tha");
    assert_eq!(name_from("Zoë Ångström", "x"), "Zoë Ångström");
}
