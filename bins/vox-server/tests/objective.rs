//! A dungeon's objective: reaching it tells everyone there, once, and tells the directory who
//! was there (which is how a raid on the dungeon is won).

use std::time::Duration;

use axum::{Json, Router, routing::post};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    auth::{self, Character, Claims},
    wire::{self, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor, and the objective a step and a half north of the spawn.
const LEVEL: &str = r#"{ "world": { "sky": "void", "objective": {"name": "the altar", "at": [0.5, 0, 2.0], "radius": 1} },
  "boxes": [ { "block": "grass", "from": [-16, -1, -16], "to": [16, -1, 16] } ] }"#;

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

/// A notice within `secs`, if one comes.
async fn notice(ws: &mut Ws, secs: f32) -> Option<String> {
    let until = tokio::time::Instant::now() + Duration::from_secs_f32(secs);
    loop {
        let m = tokio::time::timeout_at(until, ws.next()).await.ok()??.ok()?;
        if let Message::Binary(b) = m
            && let ToClient::Notice { text } = wire::decode(&b).unwrap()
        {
            return Some(text);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reaching_the_objective() {
    // the directory: remembers what it's told
    let told = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let log = told.clone();
    let dir = Router::new().route(
        "/api/world/objective",
        post(move |Json(v): Json<serde_json::Value>| {
            let log = log.clone();
            async move {
                log.lock().unwrap().push(v);
                "{}"
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, dir).await.unwrap() });

    let key = auth::key_from_seed([9; 32]);
    let mut cfg = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    cfg.name = "crypt-a1".into();
    cfg.seed = Some(LEVEL.into());
    cfg.directory = Some(key.verifying_key());
    cfg.directory_url = Some(dir_url);
    cfg.world_secret = Some("s3cret".into());
    cfg.rules_every = Duration::from_secs(3600);
    cfg.report_every = Duration::from_secs(3600);
    let server = Server::start(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });

    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let join = async |account: u64, name: &str| {
        let token = auth::sign(&Claims { account, name: name.into(), character: Character::default(), issued: t, expires: t + 600 }, &key);
        let (mut ws, _) = connect_async(&url).await.unwrap();
        send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token: Some(token) }).await;
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
            if let Message::Binary(b) = m
                && matches!(wire::decode(&b).unwrap(), ToClient::Ready)
            {
                break;
            }
        }
        ws
    };
    let mut ann = join(7, "Ann").await;
    let mut bob = join(8, "Bob").await;
    // near it, not at it: nothing
    send(&mut ann, &ToServer::Pose { at: [0.5, 0.0, 0.5], blob: vec![] }).await;
    assert!(notice(&mut ann, 0.4).await.is_none());
    // Ann reaches it: both hear, and the directory is told who was there
    send(&mut ann, &ToServer::Pose { at: [0.5, 0.0, 1.8], blob: vec![] }).await;
    assert_eq!(notice(&mut ann, 2.0).await.as_deref(), Some("The altar: reached"));
    assert_eq!(notice(&mut bob, 2.0).await.as_deref(), Some("The altar: reached"));
    for _ in 0..50 {
        if !told.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let got = told.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["world"], "crypt-a1");
    let mut who: Vec<u64> = got[0]["players"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    who.sort();
    assert_eq!(who, [7, 8]);
    // only once
    send(&mut bob, &ToServer::Pose { at: [0.5, 0.0, 1.9], blob: vec![] }).await;
    assert!(notice(&mut bob, 0.5).await.is_none());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(told.lock().unwrap().len(), 1);
}
