//! A world server over real WebSockets: joining, edits in order, the checks, poses, and the
//! world surviving a restart.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    Chunk, Why, World,
    weather::{Climate, Weather},
    wire::{self, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] },
                                    { "block": "stone", "from": [40, 0, 0], "to": [40, 3, 0] } ] }"#;

fn palette() -> Vec<String> {
    ["grass", "stone", "planks"].map(String::from).to_vec()
}

fn config(db: &str) -> Config {
    let mut c = Config::new(db, palette());
    c.name = "test".into();
    c.seed = Some(LEVEL.into());
    c.flush_every = Duration::from_millis(200);
    c
}

async fn serve(cfg: Config) -> (Server, String) {
    let server = Server::start(cfg).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = server.router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (server, format!("ws://{addr}/ws"))
}

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> ToClient {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timed out").unwrap().unwrap();
        if let Message::Binary(b) = m {
            return wire::decode(&b).unwrap();
        }
    }
}

/// The next message that isn't a pose or the weather.
async fn recv_event(ws: &mut Ws) -> ToClient {
    loop {
        let m = recv(ws).await;
        if !matches!(m, ToClient::Pose { .. } | ToClient::Weather(_)) {
            return m;
        }
    }
}

struct Joined {
    ws: Ws,
    you: u32,
    world: World,
    spawn: [f32; 3],
    others: Vec<String>,
}

async fn join(url: &str, name: &str) -> Joined {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: name.as_bytes().to_vec(), token: None }).await;
    let ToClient::Welcome { you, palette: p, bounds, spawn, world: w, settings, .. } = recv(&mut ws).await else { panic!("no welcome") };
    assert!(settings.contains(r#""sky":"void""#), "{settings}");
    assert_eq!(w, "test");
    assert_eq!(p, palette());
    let mut world = World::new(bounds, p.len() as u16);
    loop {
        match recv(&mut ws).await {
            ToClient::Chunk { key, version, data } => world.put_chunk(key, Chunk::decode(&data, version).unwrap()),
            ToClient::Ready => break,
            m => panic!("{m:?}"),
        }
    }
    // the players already here
    let mut others = Vec::new();
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout(Duration::from_millis(150), ws.next()).await {
        match wire::decode(&b).unwrap() {
            ToClient::Joined { name, .. } => others.push(name),
            ToClient::Pose { .. } | ToClient::Weather(_) => {}
            m => panic!("{m:?}"),
        }
    }
    Joined { ws, you, world, spawn, others }
}

/// Stand at the spawn so edits near it are in reach.
async fn stand(j: &mut Joined) {
    let at = j.spawn;
    send(&mut j.ws, &ToServer::Pose { at, blob: vec![] }).await;
}

#[tokio::test]
async fn the_weather_comes_with_the_world_and_anyone_can_change_it() {
    let mut cfg = config(":memory:");
    cfg.seed = Some(r#"{ "world": { "sky": "void", "weather": "fog" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] } ] }"#.into());
    let (_server, url) = serve(cfg).await;
    // the keyframe comes straight after the blocks
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: "Ann".into(), avatar: vec![], token: None }).await;
    let mut kf = None;
    for _ in 0..10 {
        if let ToClient::Weather(k) = recv(&mut ws).await {
            kf = Some(k);
            break;
        }
    }
    let kf = kf.expect("a weather keyframe");
    let w = Weather::from_keyframe(&kf).unwrap();
    assert_eq!(w.climate, Climate::preset("fog", 0.0).unwrap());
    assert!(w.time >= 120.0, "the air has settled");
    assert!(w.air([0.0, 2.0, 0.0]).humidity > 0.95, "foggy");
    // Bob makes it windy; Ann hears about it
    let mut b = join(&url, "Bob").await;
    let windy = Climate::preset("windy", 0.0).unwrap();
    send(&mut b.ws, &ToServer::Climate(windy)).await;
    loop {
        if let ToClient::Weather(k) = recv(&mut ws).await {
            if k.climate.wind_speed == windy.wind_speed {
                break;
            }
        }
    }
}

#[tokio::test]
async fn joining_sends_the_world() {
    let (_server, url) = serve(config(":memory:")).await;
    let j = join(&url, "Ann").await;
    assert_eq!(j.world.count(), 17 * 17 + 4);
    assert_eq!(j.world.get([0, -1, 0]), 1);
    assert_eq!(j.world.get([40, 2, 0]), 2);
    assert_eq!(j.spawn, [0.5, 0.0, 0.5]);
}

#[tokio::test]
async fn reached_by_path_behind_a_tunnel() {
    // vox.fail/w/7780/ws reaches the world on port 7780 unchanged
    let (_server, url) = serve(config(":memory:")).await;
    let url = url.replace("/ws", "/w/7780/ws");
    let a = join(&url, "Ann").await;
    let b = join(&url, "Ben").await;
    assert_eq!(b.others, vec!["Ann"]);
    drop(a);
}

#[tokio::test]
async fn edits_reach_everyone_in_order() {
    let (server, url) = serve(config(":memory:")).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Bob").await;
    assert_eq!(b.others, vec!["Ann"]);
    assert!(matches!(recv_event(&mut a.ws).await, ToClient::Joined { ref name, .. } if name == "Bob"));
    stand(&mut a).await;
    stand(&mut b).await;
    send(&mut a.ws, &ToServer::Edit { id: 1, cell: [0, 0, 0], block: 3 }).await;
    send(&mut b.ws, &ToServer::Edit { id: 1, cell: [1, 0, 0], block: 2 }).await;
    send(&mut a.ws, &ToServer::Edit { id: 2, cell: [0, -1, 0], block: 0 }).await;
    let mut seen = [Vec::new(), Vec::new()];
    for (i, ws) in [&mut a.ws, &mut b.ws].into_iter().enumerate() {
        while seen[i].len() < 3 {
            match recv_event(ws).await {
                ToClient::Edits(e) => seen[i].extend(e),
                m => panic!("{m:?}"),
            }
        }
    }
    assert_eq!(seen[0], seen[1], "the same edits in the same order");
    let seqs: Vec<u64> = seen[0].iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3]);
    let ann: Vec<_> = seen[0].iter().filter(|e| e.player == a.you).map(|e| e.id).collect();
    assert_eq!(ann, vec![1, 2], "Ann's own edits come back with her ids");
    let s = server.status().await.unwrap();
    assert_eq!(s.seq, 3);
    assert_eq!(s.players, vec!["Ann", "Bob"]);
    assert_eq!(s.blocks, 17 * 17 + 4 + 2 - 1);
}

#[tokio::test]
async fn bad_edits_are_refused_to_their_sender_only() {
    let (_server, url) = serve(config(":memory:")).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Bob").await;
    let _ = recv_event(&mut a.ws).await; // Bob joined
    // no position yet
    send(&mut a.ws, &ToServer::Edit { id: 1, cell: [0, 0, 0], block: 1 }).await;
    assert_eq!(recv_event(&mut a.ws).await, ToClient::Refused { id: 1, why: Why::TooFar });
    stand(&mut a).await;
    let cases = [
        (2, [40, 0, 0], 0, Why::TooFar),
        (3, [0, -1, 0], 1, Why::NoChange),
        (4, [0, 0, 0], 9, Why::NoSuchBlock),
        (5, [0, -40, 0], 1, Why::Outside),
    ];
    for (id, cell, block, why) in cases {
        send(&mut a.ws, &ToServer::Edit { id, cell, block }).await;
        assert_eq!(recv_event(&mut a.ws).await, ToClient::Refused { id, why });
    }
    // Bob heard nothing about any of it (only Ann's poses)
    let quiet = tokio::time::timeout(Duration::from_millis(300), recv_event(&mut b.ws)).await;
    assert!(quiet.is_err(), "{quiet:?}");
}

#[tokio::test]
async fn too_many_edits_too_fast() {
    let (_server, url) = serve(config(":memory:")).await;
    let mut a = join(&url, "Ann").await;
    stand(&mut a).await;
    for i in 0..30 {
        let cell = [i % 6 - 3, 0, i / 6 - 3];
        send(&mut a.ws, &ToServer::Edit { id: i as u32, cell, block: 2 }).await;
    }
    let (mut made, mut slowed) = (0, 0);
    for _ in 0..30 {
        match recv_event(&mut a.ws).await {
            ToClient::Edits(e) => made += e.len(),
            ToClient::Refused { why: Why::TooFast, .. } => slowed += 1,
            m => panic!("{m:?}"),
        }
    }
    assert!(made >= 20 && made < 30, "{made}");
    assert_eq!(made + slowed, 30);
}

#[tokio::test]
async fn poses_go_to_the_others_and_leaving_is_told() {
    let (_server, url) = serve(config(":memory:")).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Bob").await;
    let _ = recv_event(&mut a.ws).await; // Bob joined
    send(&mut a.ws, &ToServer::Pose { at: [1.0, 0.0, 1.0], blob: vec![7, 7] }).await;
    assert_eq!(recv(&mut b.ws).await, ToClient::Pose { player: a.you, blob: vec![7, 7] });
    let bob = b.you;
    b.ws.close(None).await.unwrap();
    assert_eq!(recv_event(&mut a.ws).await, ToClient::Left { player: bob });
}

#[tokio::test]
async fn wrong_version_full_and_no_hello() {
    let mut cfg = config(":memory:");
    cfg.max_players = 1;
    let (_server, url) = serve(cfg).await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL + 1, name: "Old".into(), avatar: vec![], token: None }).await;
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { why } if why.contains("version")));

    let _a = join(&url, "Ann").await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: "Bob".into(), avatar: vec![], token: None }).await;
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { why } if why.contains("full")));

    let (mut ws, _) = connect_async(&url).await.unwrap();
    ws.send(Message::Binary(vec![255, 255, 255].into())).await.unwrap();
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { .. }));
}

#[tokio::test]
async fn the_world_survives_a_restart() {
    let dir = std::env::temp_dir().join(format!("vox-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("world.db");
    let _ = std::fs::remove_file(&db);
    let db = db.to_str().unwrap().to_string();
    {
        let (server, url) = serve(config(&db)).await;
        let mut a = join(&url, "Ann").await;
        stand(&mut a).await;
        send(&mut a.ws, &ToServer::Edit { id: 1, cell: [2, 0, 2], block: 3 }).await;
        send(&mut a.ws, &ToServer::Edit { id: 2, cell: [0, -1, 0], block: 0 }).await;
        for _ in 0..2 {
            assert!(matches!(recv_event(&mut a.ws).await, ToClient::Edits(_)));
        }
        server.shutdown().await;
    }
    // a different seed doesn't matter now: the world is what was built
    let mut cfg = config(&db);
    cfg.seed = Some(r#"{"boxes":[]}"#.into());
    let (server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    assert_eq!(a.world.get([2, 0, 2]), 3);
    assert_eq!(a.world.get([0, -1, 0]), 0);
    assert_eq!(a.world.get([1, -1, 0]), 1);
    assert_eq!(a.world.count(), 17 * 17 + 4);
    // edit numbers carry on
    stand(&mut a).await;
    send(&mut a.ws, &ToServer::Edit { id: 1, cell: [3, 0, 3], block: 2 }).await;
    let ToClient::Edits(e) = recv_event(&mut a.ws).await else { panic!() };
    assert_eq!(e[0].seq, 3);
    server.shutdown().await;
    // the edit log has all three
    let conn = rusqlite_count(&db);
    assert_eq!(conn, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

fn rusqlite_count(db: &str) -> i64 {
    let c = rusqlite::Connection::open(db).unwrap();
    c.query_row("SELECT count(*) FROM edits", [], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn a_world_that_needs_sign_in() {
    use vox_core::auth::{self, Character, Claims};
    let key = auth::key_from_seed([9; 32]);
    let mut cfg = config(":memory:");
    cfg.directory = Some(key.verifying_key());
    let (_server, url) = serve(cfg).await;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let hello = |name: &str, token: Option<String>| ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token };
    // no token: turned away
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &hello("Sam", None)).await;
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { why } if why.contains("sign in")));
    // someone else's signature: turned away
    let claims = Claims { account: 4, name: "Samantha".into(), character: Character::default(), issued: now, expires: now + 60 };
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &hello("Sam", Some(auth::sign(&claims, &auth::key_from_seed([1; 32]))))).await;
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { .. }));
    // a good token: in, under the name it gives (whatever the hello says)
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &hello("Admin", Some(auth::sign(&claims, &key)))).await;
    let ToClient::Welcome { name, .. } = recv(&mut ws).await else { panic!("no welcome") };
    assert_eq!(name, "Samantha");
    // guests, when the world lets them in, are marked as guests
    let mut cfg = config(":memory:");
    cfg.directory = Some(key.verifying_key());
    cfg.guests = true;
    let (_server, url) = serve(cfg).await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &hello("Bob", None)).await;
    let ToClient::Welcome { name, .. } = recv(&mut ws).await else { panic!("no welcome") };
    assert_eq!(name, "Bob (guest)");
}

#[tokio::test]
async fn plots_portals_and_reports_with_a_directory() {
    use std::sync::{Arc, Mutex};
    use vox_core::auth::{self, Character, Claims};
    // a stand-in directory: one plot (Ann's, account 7), one portal, and the reports it gets
    let reports: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let got = reports.clone();
    let dir = axum::Router::new()
        .route(
            "/api/world/test/rules",
            axum::routing::get(|h: axum::http::HeaderMap| async move {
                if h.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer s3cret") {
                    return axum::Json(serde_json::json!({}));
                }
                axum::Json(serde_json::json!({
                    "plots": [{ "id": 1, "x0": 2, "z0": -2, "x1": 6, "z1": 2, "builders": [7] }],
                    "portals": [{ "name": "The Yard (1)", "at": [0, 0, -6], "axis": "z", "size": [4, 6], "level": "yard",
                                  "url": "/w/7781/ws", "arrive": [0.5, 0, 0.5], "yaw": 0 }],
                }))
            }),
        )
        .route(
            "/api/world/report",
            axum::routing::post(move |axum::Json(v): axum::Json<serde_json::Value>| {
                let got = got.clone();
                async move {
                    got.lock().unwrap_or_else(|e| e.into_inner()).push(v);
                    "ok"
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, dir).await.unwrap() });

    let key = auth::key_from_seed([9; 32]);
    let mut cfg = config(":memory:");
    cfg.directory = Some(key.verifying_key());
    cfg.level = "void".into();
    cfg.directory_url = Some(dir_url);
    cfg.world_secret = Some("s3cret".into());
    cfg.report_every = Duration::from_millis(600);
    cfg.rules_every = Duration::from_millis(100);
    let (_server, url) = serve(cfg).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let enter = |account: u64, name: &'static str| {
        let token = auth::sign(&Claims { account, name: name.into(), character: Character::default(), issued: now, expires: now + 60 }, &key);
        let url = url.clone();
        async move {
            let (mut ws, _) = connect_async(&url).await.unwrap();
            send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token: Some(token) }).await;
            let ToClient::Welcome { settings, spawn, .. } = recv(&mut ws).await else { panic!("no welcome") };
            loop {
                if let ToClient::Ready = recv(&mut ws).await {
                    break;
                }
            }
            send(&mut ws, &ToServer::Pose { at: spawn, blob: vec![] }).await;
            (ws, settings)
        }
    };
    let (mut ann, settings) = enter(7, "Ann").await;
    let (mut bob, _) = enter(8, "Bob").await;
    // the portal Ann opened at the directory is in the settings new arrivals get
    let s: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(s["sky"], "void");
    assert_eq!(s["portals"][0]["url"], "/w/7781/ws");
    // Ann builds in her plot; Bob can't, but can outside it
    send(&mut ann, &ToServer::Edit { id: 1, cell: [3, 0, 0], block: 2 }).await;
    send(&mut bob, &ToServer::Edit { id: 1, cell: [4, 0, 0], block: 2 }).await;
    send(&mut bob, &ToServer::Edit { id: 2, cell: [-3, 0, 0], block: 2 }).await;
    let mut refused = None;
    let mut applied = 0;
    while refused.is_none() || applied < 2 {
        match recv_event(&mut bob).await {
            ToClient::Refused { id, why } => refused = Some((id, why)),
            ToClient::Edits(e) => applied += e.len(),
            _ => {}
        }
    }
    assert_eq!(refused, Some((1, Why::Claimed)));
    // the report: who's here, where, and the blocks each placed
    tokio::time::sleep(Duration::from_millis(900)).await;
    let r = reports.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|r| r["players"].as_array().is_some_and(|p| !p.is_empty())).cloned().expect("a report");
    assert_eq!(r["world"], "test");
    assert_eq!(r["level"], "void");
    let mut placed: Vec<(u64, u64)> = r["players"].as_array().unwrap().iter().map(|p| (p["account"].as_u64().unwrap(), p["edits"].as_u64().unwrap())).collect();
    placed.sort();
    assert_eq!(placed, vec![(7, 1), (8, 1)]);
}

#[tokio::test]
async fn watchers_see_the_world_without_names_and_are_not_players() {
    let (server, url) = serve(config(":memory:")).await;
    let mut a = join(&url, "Ann").await;
    // a watcher gets the world, then Ann, with no name
    let (mut w, _) = connect_async(&url).await.unwrap();
    send(&mut w, &ToServer::Watch { protocol: PROTOCOL }).await;
    let ToClient::Welcome { you, name, .. } = recv(&mut w).await else { panic!("no welcome") };
    assert_eq!(you, u32::MAX);
    assert!(name.is_empty());
    let mut chunks = 0;
    loop {
        match recv(&mut w).await {
            ToClient::Chunk { .. } => chunks += 1,
            ToClient::Ready => break,
            m => panic!("{m:?}"),
        }
    }
    assert!(chunks > 0);
    assert!(matches!(recv_event(&mut w).await, ToClient::Joined { ref name, .. } if name.is_empty()));
    // the watcher is not a player: nobody hears of them, and the status counts only Ann
    assert_eq!(server.status().await.unwrap().players, vec!["Ann".to_string()]);
    // Ann's edits and Bob's arrival reach the watcher, Bob without his name
    stand(&mut a).await;
    send(&mut a.ws, &ToServer::Edit { id: 1, cell: [0, 0, 0], block: 3 }).await;
    assert!(matches!(recv_event(&mut w).await, ToClient::Edits(ref e) if e.iter().any(|e| e.cell == [0, 0, 0] && e.block == 3)));
    let b = join(&url, "Bob").await;
    assert_eq!(b.others, vec!["Ann"], "Bob doesn't see the watcher");
    assert!(matches!(recv_event(&mut w).await, ToClient::Joined { ref name, .. } if name.is_empty()));
    // a watcher may not send anything: the server ends the connection
    send(&mut w, &ToServer::Edit { id: 1, cell: [1, 0, 0], block: 2 }).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(m)) = w.next().await {
            if matches!(m, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the watcher was let go");
}

#[tokio::test]
async fn watchers_are_capped() {
    let mut cfg = config(":memory:");
    cfg.max_watchers = 1;
    let (_server, url) = serve(cfg).await;
    let (mut w1, _) = connect_async(&url).await.unwrap();
    send(&mut w1, &ToServer::Watch { protocol: PROTOCOL }).await;
    assert!(matches!(recv(&mut w1).await, ToClient::Welcome { .. }));
    let (mut w2, _) = connect_async(&url).await.unwrap();
    send(&mut w2, &ToServer::Watch { protocol: PROTOCOL }).await;
    assert!(matches!(recv(&mut w2).await, ToClient::Bye { ref why } if why.contains("too many")));
}
