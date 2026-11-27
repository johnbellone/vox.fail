//! Going through a portal to another world: the world you leave checks you're in the doorway
//! and answers with where to go, a ticket from the directory and where you arrive; the world
//! you arrive in lets a ticket in once, for itself only, and puts you where it says.

use std::time::Duration;

use axum::{Json, Router, http::HeaderMap, routing::post};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    auth::{self, Character, Claims, Ticket},
    wire::{self, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor, and a doorway called "gate" a step and a half north of the spawn, to the yard.
const LEVEL: &str = r#"{ "world": { "sky": "void", "portals": [
    {"name": "gate", "at": [0.5, 0, 2.0], "axis": "z", "size": [4, 6], "level": "yard", "url": ":7781", "arrive": [0, 0, 11.5], "yaw": 0}
  ] }, "boxes": [ { "block": "grass", "from": [-16, -1, -16], "to": [16, -1, 16] } ] }"#;

fn config(name: &str) -> Config {
    let mut c = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    c.name = name.into();
    c.seed = Some(LEVEL.into());
    c
}

async fn serve(cfg: Config) -> (Server, String) {
    let server = Server::start(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (server, url)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

/// The next message `want` picks out, within `secs`; None if none came.
async fn wait_for<T>(ws: &mut Ws, secs: f32, want: impl Fn(ToClient) -> Option<T>) -> Option<T> {
    let until = tokio::time::Instant::now() + Duration::from_secs_f32(secs);
    loop {
        let m = tokio::time::timeout_at(until, ws.next()).await.ok()??.ok()?;
        if let Message::Binary(b) = m
            && let Some(t) = want(wire::decode(&b).unwrap())
        {
            return Some(t);
        }
    }
}

/// Say hello with `token` and wait until the world is all here. Err: the world's goodbye.
async fn join(url: &str, token: Option<String>) -> Result<Ws, String> {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: "Ann".into(), avatar: vec![], token }).await;
    let first = wait_for(&mut ws, 5.0, |m| match m {
        ToClient::Welcome { .. } => Some(Ok(())),
        ToClient::Bye { why } => Some(Err(why)),
        _ => None,
    })
    .await
    .expect("no answer");
    first?;
    wait_for(&mut ws, 5.0, |m| matches!(m, ToClient::Ready).then_some(())).await.expect("not ready");
    Ok(ws)
}

fn travel(m: ToClient) -> Option<(String, Option<String>, [f32; 3])> {
    match m {
        ToClient::Travel { url, ticket, arrive } => Some((url, ticket, arrive)),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn only_from_the_doorway() {
    // a world with no sign-in and no directory: the way is given, without a ticket
    let (_s, url) = serve(config("void")).await;
    let mut ws = join(&url, None).await.unwrap();
    // nowhere near the doorway: nothing happens
    send(&mut ws, &ToServer::Pose { at: [10.0, 0.0, 10.0], blob: vec![] }).await;
    send(&mut ws, &ToServer::Through { portal: "gate".into(), side: 1 }).await;
    assert!(wait_for(&mut ws, 0.5, travel).await.is_none());
    // a doorway that isn't there
    tokio::time::sleep(Duration::from_millis(1500)).await;
    send(&mut ws, &ToServer::Pose { at: [1.0, 0.0, 2.0], blob: vec![] }).await;
    send(&mut ws, &ToServer::Through { portal: "nowhere".into(), side: 1 }).await;
    assert!(wait_for(&mut ws, 0.5, travel).await.is_none());
    // in it: on to the yard, half a block east of its arrival point, as they were here
    send(&mut ws, &ToServer::Through { portal: "gate".into(), side: 1 }).await;
    let (to, ticket, arrive) = wait_for(&mut ws, 2.0, travel).await.expect("no way through");
    assert_eq!((to.as_str(), ticket, arrive), (":7781", None, [0.5, 0.0, 11.5]));
}

/// A doorway to a dungeon: its way is the dungeon's address, and its arrival is the level's own
/// `arrive` (the Void's dungeon doorways once arrived at the doorway's sill instead).
const DUNGEON_LEVEL: &str = r#"{ "world": { "sky": "void", "portals": [
    {"name": "down", "at": [0.5, 0, 2.0], "axis": "z", "size": [4, 6], "level": "crypt", "url": "dungeon:crypt", "arrive": [0, 0, 11.5], "yaw": 0}
  ] }, "boxes": [ { "block": "grass", "from": [-16, -1, -16], "to": [16, -1, 16] } ] }"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_dungeon_doorway_arrives_at_the_levels_arrival() {
    // no directory here, so the answer comes at once, with no ticket: the arrival is the level's
    // `arrive` carried through the doorway (the sill maps onto it exactly)
    let mut c = config("void");
    c.seed = Some(DUNGEON_LEVEL.into());
    let (_s, url) = serve(c).await;
    let mut ws = join(&url, None).await.unwrap();
    send(&mut ws, &ToServer::Pose { at: [0.5, 0.0, 2.0], blob: vec![] }).await;
    send(&mut ws, &ToServer::Through { portal: "down".into(), side: 1 }).await;
    let (to, ticket, arrive) = wait_for(&mut ws, 2.0, travel).await.expect("no way through");
    assert_eq!((to.as_str(), ticket), ("dungeon:crypt", None));
    assert_eq!(arrive, [0.0, 0.0, 11.5]);
}

#[tokio::test(flavor = "multi_thread")]
async fn tickets_from_the_directory() {
    let key = auth::key_from_seed([9; 32]);
    // the directory: a ticket for the yard, arriving where the world says
    let signer = key.clone();
    let dir = Router::new().route(
        "/api/world/travel",
        post(move |h: HeaderMap, Json(v): Json<serde_json::Value>| {
            let signer = signer.clone();
            async move {
                assert_eq!(h.get("authorization").unwrap(), "Bearer s3cret");
                assert_eq!((v["world"].as_str(), v["to"].as_str()), (Some("void"), Some(":7781")));
                let arrive: [f32; 3] = serde_json::from_value(v["arrive"].clone()).unwrap();
                let t = now();
                let claims = Claims { account: v["account"].as_u64().unwrap(), name: "Ann".into(), character: Character::default(), issued: t, expires: t + auth::TICKET_LIFETIME };
                let ticket = Ticket { claims, world: "yard".into(), arrive, nonce: 77 };
                Json(serde_json::json!({ "world": "yard", "url": "/w/7781/ws", "ticket": auth::sign_ticket(&ticket, &signer) }))
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, dir).await.unwrap() });

    let mut void = config("void");
    void.directory = Some(key.verifying_key());
    void.directory_url = Some(dir_url);
    void.world_secret = Some("s3cret".into());
    void.rules_every = Duration::from_secs(3600);
    let (_v, void_url) = serve(void).await;
    let mut yard = config("yard");
    yard.directory = Some(key.verifying_key());
    let (_y, yard_url) = serve(yard).await;

    let t = now();
    let token = auth::sign(&Claims { account: 7, name: "Ann".into(), character: Character::default(), issued: t, expires: t + 600 }, &key);
    let mut ws = join(&void_url, Some(token)).await.unwrap();
    send(&mut ws, &ToServer::Pose { at: [0.5, 0.0, 2.0], blob: vec![] }).await;
    send(&mut ws, &ToServer::Through { portal: "gate".into(), side: 1 }).await;
    let (to, ticket, arrive) = wait_for(&mut ws, 3.0, travel).await.expect("no way through");
    // in the middle of the doorway: the middle of the arrival point
    assert_eq!((to.as_str(), arrive), ("/w/7781/ws", [0.0, 0.0, 11.5]));
    let ticket = ticket.expect("no ticket");

    // a ticket for the yard doesn't open the Void
    assert!(join(&void_url, Some(ticket.clone())).await.unwrap_err().contains("another world"));
    // into the yard, at the arrival point: a first position anywhere else is sent back there
    let mut there = join(&yard_url, Some(ticket.clone())).await.unwrap();
    send(&mut there, &ToServer::Pose { at: [-14.0, 0.0, -14.0], blob: vec![] }).await;
    let back = wait_for(&mut there, 2.0, |m| match m {
        ToClient::Correct { at } => Some(at),
        _ => None,
    })
    .await;
    assert_eq!(back, Some([0.0, 0.0, 11.5]));
    // and only once
    assert!(join(&yard_url, Some(ticket)).await.unwrap_err().contains("used"));
}
