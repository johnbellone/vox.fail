//! A dungeon's objective: reaching it tells everyone there, once, and tells the directory who
//! was there (which is how a raid on the dungeon is won).

use std::time::Duration;

use axum::{Json, Router, routing::post};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    ChunkKey,
    auth::{self, Character, Claims},
    entity::{Component, Objective, Settings},
    weather::{Climate, Weather},
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

/// The objective is the world's Objective entity, not the settings it was seeded from. Changed
/// while the world is closed, the next opening has the new one; a restore to a snapshot from
/// before the change brings the old one back.
#[tokio::test(flavor = "multi_thread")]
async fn objective_follows_its_entity() {
    let dir = std::env::temp_dir().join(format!("vox-server-objective-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("crypt.db").to_str().unwrap().to_string();
    let snaps = dir.join("snaps");
    let config = || {
        let mut cfg = Config::new(&db, ["grass", "stone"].map(String::from).to_vec());
        cfg.name = "crypt-a1".into();
        cfg.seed = Some(LEVEL.into());
        cfg.trust_moves = true;
        cfg.snapshots = Some(snaps.clone());
        cfg.snapshot_every = Duration::ZERO;
        cfg
    };
    let altar = [0.5, 0.0, 2.0];
    let crypt = [0.5, 0.0, -6.0];

    // the world as it is seeded: its objective is the altar, and a snapshot holds that
    run_world(config()).await;
    let before = vox_format::Snapshots::new(vox_format::DirStore::new(&snaps)).list().unwrap();
    let old = before.last().expect("a snapshot of the seeded world").id.clone();
    assert_eq!(objective_row(&db).at, altar);

    // the objective moves while the world is closed
    edit_entity_while_closed(&db, Component::Objective(Objective { name: "the crypt".into(), at: crypt, radius: 1.0 }));
    let (server, url) = open_world(config()).await;
    let mut ann = join_guest(&url, "Ann").await;
    // the old place does nothing now
    send(&mut ann, &ToServer::Pose { at: [0.5, 0.0, 1.8], blob: vec![] }).await;
    assert!(notice(&mut ann, 0.4).await.is_none());
    // the new one is reached
    send(&mut ann, &ToServer::Pose { at: [0.5, 0.0, -5.9], blob: vec![] }).await;
    assert_eq!(notice(&mut ann, 2.0).await.as_deref(), Some("The crypt: reached"));
    drop(ann);
    server.shutdown().await;

    // restored to the snapshot from before the change: the altar again
    vox_server::restore(config(), vox_server::Restore::Snapshot(old)).unwrap();
    let (server, url) = open_world(config()).await;
    let mut ann = join_guest(&url, "Ann").await;
    send(&mut ann, &ToServer::Pose { at: [0.5, 0.0, 1.8], blob: vec![] }).await;
    assert_eq!(notice(&mut ann, 2.0).await.as_deref(), Some("The altar: reached"));
    drop(ann);
    server.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// The weather a world's Settings entity sets is what it runs, and a player's climate (changed
/// while it runs) is kept across openings: a settings change made while the world is closed is
/// taken once, not again on every opening, which would overwrite the player's climate.
#[tokio::test(flavor = "multi_thread")]
async fn weather_follows_its_entity_and_a_players_climate_survives() {
    let dir = std::env::temp_dir().join(format!("vox-server-weather-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("crypt.db").to_str().unwrap().to_string();
    let config = || {
        let mut cfg = Config::new(&db, ["grass", "stone"].map(String::from).to_vec());
        cfg.name = "crypt-a1".into();
        cfg.seed = Some(LEVEL.into());
        cfg.trust_moves = true;
        cfg.snapshot_every = Duration::from_secs(3600);
        cfg
    };
    let windy = Climate::preset("windy", 0.0).unwrap();

    // the Settings entity now says fog, while the world is closed
    run_world(config()).await;
    edit_entity_while_closed(&db, Component::Settings(Settings { json: r#"{"sky":"void","weather":"fog"}"#.into() }));

    // opening takes the entity's weather
    let (server, url) = open_world(config()).await;
    let mut ann = join_guest(&url, "Ann").await;
    assert_eq!(next_weather(&mut ann).await.climate, Climate::preset("fog", 0.0).unwrap());
    // a player makes it windy; that is stored
    send(&mut ann, &ToServer::Climate(windy)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(ann);
    server.shutdown().await;

    // opened again, closed and opened again: the windy climate is still there each time
    for _ in 0..2 {
        let (server, url) = open_world(config()).await;
        let mut ann = join_guest(&url, "Ann").await;
        assert_eq!(next_weather(&mut ann).await.climate.wind_speed, windy.wind_speed);
        drop(ann);
        server.shutdown().await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The next weather keyframe the server sends this client.
async fn next_weather(ws: &mut Ws) -> Weather {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m
            && let ToClient::Weather(k) = wire::decode(&b).unwrap()
        {
            return Weather::from_keyframe(&k).unwrap();
        }
    }
}

/// Opens a world with no directory (guests are let in) and serves it on a local port.
async fn open_world(cfg: Config) -> (Server, String) {
    let server = Server::start(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (server, url)
}

/// Opens a world for a moment, then shuts it down (which saves it and takes a snapshot).
async fn run_world(cfg: Config) {
    let (server, _url) = open_world(cfg).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;
}

async fn join_guest(url: &str, name: &str) -> Ws {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token: None }).await;
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m
            && matches!(wire::decode(&b).unwrap(), ToClient::Ready)
        {
            return ws;
        }
    }
}

/// An edit of one entity row while the world is closed: the entity row, the edit log, and the
/// world's edit number (as the world itself would write them). The row is the one with this
/// component's name.
fn edit_entity_while_closed(db: &str, to: Component) {
    let mut c = rusqlite::Connection::open(db).unwrap();
    let name = to.name();
    let (id, old_bytes): (u32, Vec<u8>) =
        c.query_row("SELECT id, data FROM entities WHERE name = ?1", [name], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let new_bytes = wire::encode(&to);
    let seq: u64 = c
        .query_row("SELECT value FROM meta WHERE key = 'seq'", [], |r| r.get::<_, String>(0))
        .unwrap()
        .parse::<u64>()
        .unwrap()
        + 1;
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let spot = match &to {
        Component::Objective(o) => o.at,
        _ => [0.5, 0.0, 0.5],
    };
    let cell = [spot[0].floor() as i32, spot[1].floor() as i32, spot[2].floor() as i32];
    let zone = vox_format::zone_of(ChunkKey::of(cell));
    let t = c.transaction().unwrap();
    t.execute(
        "UPDATE entities SET data = ?1, seq = ?2, zone_x = ?3, zone_z = ?4 WHERE id = ?5",
        rusqlite::params![new_bytes, seq as i64, zone.x, zone.z, id],
    )
    .unwrap();
    t.execute(
        "INSERT INTO entity_edits(seq, at, player, entity, old, new) VALUES (?1, ?2, 'test', ?3, ?4, ?5)",
        rusqlite::params![seq as i64, at as i64, id, old_bytes, new_bytes],
    )
    .unwrap();
    t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('seq', ?1)", [seq.to_string()]).unwrap();
    t.commit().unwrap();
}

/// The Objective entity as the entities table holds it.
fn objective_row(db: &str) -> Objective {
    let c = rusqlite::Connection::open(db).unwrap();
    let data: Vec<u8> = c.query_row("SELECT data FROM entities WHERE name = 'vox::Objective'", [], |r| r.get(0)).unwrap();
    match wire::decode::<Component>(&data).unwrap() {
        Component::Objective(o) => o,
        _ => panic!("the objective row isn't an objective"),
    }
}
