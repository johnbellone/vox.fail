//! A world server over real WebSockets: joining, edits in order, the checks, poses, and the
//! world surviving a restart.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    Chunk, Why, World,
    weather::{Climate, Weather},
    wire::{self, PROTOCOL, ToClient, ToServer, VOICE_BYTES, VOICE_NEARBY, VOICE_PARTY, VOICE_SAMPLES, VoicePacket},
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

/// The next message that isn't a pose, the weather or the entities a newcomer is sent.
async fn recv_event(ws: &mut Ws) -> ToClient {
    loop {
        let m = recv(ws).await;
        if !matches!(m, ToClient::Pose { .. } | ToClient::Weather(_) | ToClient::Entities(_)) {
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
    join_with(url, name, None).await
}

async fn join_with(url: &str, name: &str, token: Option<String>) -> Joined {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: name.as_bytes().to_vec(), token }).await;
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
            ToClient::Pose { .. } | ToClient::Weather(_) | ToClient::Entities(_) => {}
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
async fn a_world_without_air_has_no_weather() {
    let mut cfg = config(":memory:");
    cfg.seed = Some(r#"{ "world": { "sky": "orbit", "weather": "none" }, "boxes": [ { "block": "stone", "from": [-4, -1, -4], "to": [4, -1, 4] } ] }"#.into());
    let (_server, url) = serve(cfg).await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: "Ann".into(), avatar: vec![], token: None }).await;
    // asking for wind changes nothing, and nothing about the weather ever comes (on joining,
    // where a world with air sends it straight after the blocks, or later)
    send(&mut ws, &ToServer::Climate(Climate::preset("windy", 0.0).unwrap())).await;
    let quiet = tokio::time::timeout(Duration::from_millis(1500), async {
        loop {
            if let ToClient::Weather(_) = recv(&mut ws).await {
                return;
            }
        }
    })
    .await;
    assert!(quiet.is_err(), "a weather keyframe in a world without air");
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
async fn older_versions_are_refused_for_hello_and_watch() {
    let (_server, url) = serve(config(":memory:")).await;
    let old = PROTOCOL - 1;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: old, name: "Old".into(), avatar: vec![], token: None }).await;
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { why } if why.contains("version")));

    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, &ToServer::Watch { protocol: old }).await;
    assert!(matches!(recv(&mut ws).await, ToClient::Bye { why } if why.contains("version")));
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

#[tokio::test]
async fn a_fresh_yard_world_has_its_entities() {
    let dir = std::env::temp_dir().join(format!("vox-test-yard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("yard.db");
    let _ = std::fs::remove_file(&db);
    let db = db.to_str().unwrap().to_string();
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/");
    let blocks = std::fs::read_to_string(format!("{data}blocks.json")).unwrap();
    let mut cfg = config(&db);
    cfg.palette = vox_core::palette_names(&blocks).unwrap();
    cfg.seed = Some(std::fs::read_to_string(format!("{data}levels/yard.json")).unwrap());
    let (server, _url) = serve(cfg).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;
    let c = rusqlite::Connection::open(&db).unwrap();
    let kind = |name: &str| -> i64 {
        c.query_row("SELECT count(*) FROM entities WHERE name = ?1", [name], |r| r.get(0)).unwrap()
    };
    assert_eq!(kind("vox::Portal"), 1);
    assert_eq!(kind("vox::Spawn"), 1);
    assert_eq!(kind("vox::Settings"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Opens the yard world on `db` (seeding it if it is new), then shuts it down.
async fn open_yard(db: &str) {
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/");
    let blocks = std::fs::read_to_string(format!("{data}blocks.json")).unwrap();
    let mut cfg = config(db);
    cfg.palette = vox_core::palette_names(&blocks).unwrap();
    cfg.seed = Some(std::fs::read_to_string(format!("{data}levels/yard.json")).unwrap());
    let (server, _url) = serve(cfg).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;
}

fn entity_rows(db: &str) -> i64 {
    let c = rusqlite::Connection::open(db).unwrap();
    c.query_row("SELECT count(*) FROM entities", [], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn the_yard_entities_seed_once_and_stay_gone() {
    let dir = std::env::temp_dir().join(format!("vox-test-entities-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("yard.db");
    let _ = std::fs::remove_file(&db);
    let db = db.to_str().unwrap().to_string();

    open_yard(&db).await;
    let seeded = entity_rows(&db);
    assert_eq!(seeded, 3);
    // reopening a seeded world adds nothing
    open_yard(&db).await;
    assert_eq!(entity_rows(&db), seeded);

    // a world from before entities: no entity rows and no seed mark. It is seeded once,
    // from its stored settings, and a second reopen leaves it as it is.
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute("DELETE FROM entities", []).unwrap();
        c.execute("DELETE FROM meta WHERE key = 'entities_seeded'", []).unwrap();
    }
    open_yard(&db).await;
    assert_eq!(entity_rows(&db), seeded);
    open_yard(&db).await;
    assert_eq!(entity_rows(&db), seeded);

    // the mark stays but the rows are gone (an emptied world): nothing comes back
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute("DELETE FROM entities", []).unwrap();
    }
    open_yard(&db).await;
    assert_eq!(entity_rows(&db), 0);
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
                    "portals": [{ "key": 1, "name": "The Yard (1)", "at": [0, 0, -6], "axis": "z", "size": [4, 6], "level": "yard",
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
            // the world's entities follow Ready: collected until they stop coming
            let mut entities = Vec::new();
            while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout(Duration::from_millis(150), ws.next()).await {
                if let ToClient::Entities(batch) = wire::decode(&b).unwrap() {
                    entities.extend(batch);
                }
            }
            send(&mut ws, &ToServer::Pose { at: spawn, blob: vec![] }).await;
            (ws, settings, entities)
        }
    };
    let (mut ann, settings, entities) = enter(7, "Ann").await;
    let (mut bob, _, _) = enter(8, "Bob").await;
    // the settings carry no portals: the portal Ann opened at the directory is an entity, keyed
    // by the directory's id, and it leads where the directory said
    let s: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(s["sky"], "void");
    assert!(s.get("portals").is_none(), "{settings}");
    let opened = entities.iter().find_map(|c| match &c.component {
        Some(vox_core::entity::Component::Portal(p)) if p.key == Some(1) => Some(p.to.clone()),
        _ => None,
    });
    assert_eq!(opened, Some(vox_core::entity::Dest::Url { url: "/w/7781/ws".into(), arrive: [0.5, 0.0, 0.5] }));
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
    // each report counts the edits since the one before it, so the reports are added up
    let all = reports.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let first = all.iter().find(|r| r["players"].as_array().is_some_and(|p| !p.is_empty())).expect("a report");
    assert_eq!(first["world"], "test");
    assert_eq!(first["level"], "void");
    let mut placed = std::collections::BTreeMap::<u64, u64>::new();
    for r in &all {
        for p in r["players"].as_array().into_iter().flatten() {
            *placed.entry(p["account"].as_u64().unwrap()).or_default() += p["edits"].as_u64().unwrap();
        }
    }
    assert_eq!(placed.into_iter().collect::<Vec<_>>(), vec![(7, 1), (8, 1)]);
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

#[tokio::test]
async fn teleports_are_sent_back_and_not_shown() {
    let (_server, url) = serve(config(":memory:")).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Bob").await;
    assert!(matches!(recv_event(&mut a.ws).await, ToClient::Joined { ref name, .. } if name == "Bob"));
    stand(&mut a).await;
    stand(&mut b).await;
    // Bob says he's suddenly far away: he's told to go back to the spawn
    send(&mut b.ws, &ToServer::Pose { at: [400.0, 0.0, 400.0], blob: vec![7] }).await;
    let back = loop {
        if let ToClient::Correct { at } = recv(&mut b.ws).await {
            break at;
        }
    };
    assert_eq!(back, b.spawn);
    // and an edit there is out of his reach: the server still has him at the spawn
    send(&mut b.ws, &ToServer::Edit { id: 9, cell: [40, 2, 0], block: 0 }).await;
    loop {
        match recv_event(&mut b.ws).await {
            ToClient::Refused { id: 9, why } => {
                assert_eq!(why, Why::TooFar);
                break;
            }
            ToClient::Edits(e) => assert!(e.iter().all(|e| e.id != 9), "the far edit went through"),
            _ => {}
        }
    }
    // Ann never saw the teleported pose
    while let Ok(Some(Ok(Message::Binary(m)))) = tokio::time::timeout(Duration::from_millis(200), a.ws.next()).await {
        if let Ok(ToClient::Pose { blob, .. }) = wire::decode::<ToClient>(&m) {
            assert_ne!(blob, vec![7], "the teleport was shown");
        }
    }
}

#[tokio::test]
async fn a_trusting_world_takes_positions_as_they_come() {
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    let (_server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    stand(&mut a).await;
    send(&mut a.ws, &ToServer::Pose { at: [38.0, 0.0, 0.0], blob: vec![] }).await;
    send(&mut a.ws, &ToServer::Edit { id: 1, cell: [40, 2, 0], block: 0 }).await;
    loop {
        match recv_event(&mut a.ws).await {
            ToClient::Edits(e) if e.iter().any(|e| e.id == 1) => break,
            ToClient::Refused { id: 1, why } => panic!("refused: {why:?}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn snapshots_restore_to_a_moment_and_the_restore_can_be_undone() {
    snapshot_cycle("plain", None).await;
}

/// The `oxen` to test with (`OXEN`, or one on the PATH); None, and the test skips, without.
fn oxen() -> Option<std::path::PathBuf> {
    let from_env = std::env::var_os("OXEN").map(std::path::PathBuf::from);
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).map(|d| d.join("oxen")).collect::<Vec<_>>());
    let found = from_env.into_iter().chain(on_path).find(|p| p.is_file());
    if found.is_none() {
        eprintln!("skipped: no oxen on the PATH (set OXEN)");
    }
    found
}

#[tokio::test]
async fn the_same_with_oxen_keeping_the_snapshots_as_commits() {
    let Some(oxen) = oxen() else { return };
    snapshot_cycle("oxen", Some(oxen)).await;
}

async fn snapshot_cycle(name: &str, oxen: Option<std::path::PathBuf>) {
    let dir = std::env::temp_dir().join(format!("vox-server-snap-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("world.db");
    let cfg = || {
        let mut c = config(db.to_str().unwrap());
        c.snapshots = Some(dir.join("snaps"));
        c.snapshot_oxen = oxen.clone();
        c.snapshot_every = Duration::ZERO;
        c
    };
    let unix = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let wait_for = |id: u32| {
        move |m: &ToClient| matches!(m, ToClient::Edits(e) if e.iter().any(|e| e.id == id))
    };
    // build: one block, a moment passes, another block, then close
    let (server, url) = serve(cfg()).await;
    let mut a = join(&url, "Ann").await;
    stand(&mut a).await;
    send(&mut a.ws, &ToServer::Edit { id: 1, cell: [0, 0, 0], block: 3 }).await;
    loop {
        if wait_for(1)(&recv_event(&mut a.ws).await) {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let between = unix();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    send(&mut a.ws, &ToServer::Edit { id: 2, cell: [1, 0, 0], block: 2 }).await;
    loop {
        if wait_for(2)(&recv_event(&mut a.ws).await) {
            break;
        }
    }
    server.shutdown().await;
    drop(a);
    let before = vox_server::list_snapshots(&dir.join("snaps"), oxen.as_deref()).unwrap();
    assert!(before.len() >= 2, "{before:?}");
    // back to the moment between the two edits
    let what = vox_server::restore(cfg(), vox_server::Restore::At(between)).unwrap();
    assert!(what.contains("restored"), "{what}");
    let (server, url) = serve(cfg()).await;
    let j = join(&url, "Ann").await;
    assert_eq!(j.world.get([0, 0, 0]), 3, "the first block is there");
    assert_eq!(j.world.get([1, 0, 0]), 0, "the second is gone");
    server.shutdown().await;
    drop(j);
    // the restore took a snapshot of what it replaced: going back to it undoes the restore
    let after = vox_server::list_snapshots(&dir.join("snaps"), oxen.as_deref()).unwrap();
    let undo = after.iter().rev().nth(1).unwrap().0.clone();
    vox_server::restore(cfg(), vox_server::Restore::Snapshot(undo)).unwrap();
    let (server, url) = serve(cfg()).await;
    let j = join(&url, "Ann").await;
    assert_eq!(j.world.get([1, 0, 0]), 2, "undone: the second block is back");
    server.shutdown().await;
    // with Oxen every snapshot is a commit in the repository beside the plain files
    if let Some(bin) = &oxen {
        let store = vox_format::OxenStore::open(dir.join("snaps"), bin).unwrap();
        let commits = store.log().unwrap();
        assert!(commits.len() >= after.len(), "a commit per snapshot: {} commits, {} snapshots", commits.len(), after.len());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A voice packet that passes the checks; `seq` tells packets apart.
fn packet(channel: u8, seq: u32) -> VoicePacket {
    VoicePacket { channel, seq, samples: VOICE_SAMPLES, pred: 120, index: 7, data: vec![(seq % 256) as u8; VOICE_BYTES] }
}

/// How long to wait before believing that nothing more is coming. Relaying is immediate on
/// localhost; this is slack for a busy CI machine, not a delay the server has.
const QUIET: Duration = Duration::from_millis(300);

/// Every voice packet that arrives within `for_`, with the player it came from. Anything else
/// that arrives is skipped.
async fn voice_within(ws: &mut Ws, for_: Duration) -> Vec<(u32, VoicePacket)> {
    let mut got = Vec::new();
    let end = tokio::time::Instant::now() + for_;
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout_at(end, ws.next()).await {
        if let Ok(ToClient::Voice { player, packet }) = wire::decode::<ToClient>(&b) {
            got.push((player, packet));
        }
    }
    got
}

/// Each player sends a position, then waits until every other player has been told of it. So
/// the server holds every position before any voice is sent.
async fn place(players: &mut [&mut Joined], spots: &[[f32; 3]]) {
    for (j, at) in players.iter_mut().zip(spots) {
        send(&mut j.ws, &ToServer::Pose { at: *at, blob: vec![] }).await;
    }
    let ids: Vec<u32> = players.iter().map(|j| j.you).collect();
    for j in players.iter_mut() {
        let mut heard = Vec::new();
        while heard.len() < ids.len() - 1 {
            if let ToClient::Pose { player, .. } = recv(&mut j.ws).await
                && player != j.you
                && !heard.contains(&player)
            {
                heard.push(player);
            }
        }
    }
}

#[tokio::test]
async fn nearby_speech_reaches_the_near_and_no_one_else() {
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    let (_server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Ben").await;
    let mut c = join(&url, "Cal").await;
    // Ben is 10 blocks away (inside the 32 radius), Cal 100 (outside it)
    place(&mut [&mut a, &mut b, &mut c], &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [100.0, 0.0, 0.0]]).await;
    let sent = packet(VOICE_NEARBY, 1);
    send(&mut a.ws, &ToServer::Voice(sent.clone())).await;
    assert_eq!(voice_within(&mut b.ws, QUIET).await, vec![(a.you, sent)], "Ben hears Ann, unchanged");
    assert!(voice_within(&mut c.ws, QUIET).await.is_empty(), "Cal is too far away");
    assert!(voice_within(&mut a.ws, QUIET).await.is_empty(), "Ann doesn't hear herself");
}

#[tokio::test]
async fn invalid_voice_packets_are_dropped() {
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    let (_server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Ben").await;
    place(&mut [&mut a, &mut b], &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]).await;
    let mut bad = Vec::new();
    let mut p = packet(VOICE_NEARBY, 100);
    p.channel = 2;
    bad.push(p);
    let mut p = packet(VOICE_NEARBY, 101);
    p.samples = VOICE_SAMPLES - 1;
    bad.push(p);
    let mut p = packet(VOICE_NEARBY, 102);
    p.data.pop();
    bad.push(p);
    let mut p = packet(VOICE_NEARBY, 103);
    p.data.push(0);
    bad.push(p);
    let mut p = packet(VOICE_NEARBY, 104);
    p.index = 200;
    bad.push(p);
    for p in bad {
        send(&mut a.ws, &ToServer::Voice(p)).await;
    }
    // a good one after them: the connection is still fine, and only it gets through
    send(&mut a.ws, &ToServer::Voice(packet(VOICE_NEARBY, 1))).await;
    let heard: Vec<u32> = voice_within(&mut b.ws, QUIET).await.iter().map(|(_, p)| p.seq).collect();
    assert_eq!(heard, vec![1]);
}

#[tokio::test]
async fn watchers_never_hear_voice_and_cannot_send_it() {
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    let (server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Ben").await;
    place(&mut [&mut a, &mut b], &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]).await;
    let (mut w, _) = connect_async(&url).await.unwrap();
    send(&mut w, &ToServer::Watch { protocol: PROTOCOL }).await;
    assert!(matches!(recv(&mut w).await, ToClient::Welcome { .. }));
    loop {
        if let ToClient::Ready = recv(&mut w).await {
            break;
        }
    }
    // Ann speaks; Ben hears her, the watcher doesn't
    send(&mut a.ws, &ToServer::Voice(packet(VOICE_NEARBY, 1))).await;
    assert_eq!(voice_within(&mut b.ws, QUIET).await.len(), 1);
    assert!(voice_within(&mut w, QUIET).await.is_empty(), "a watcher heard voice");
    // and a watcher that tries to speak is let go, as with any other message
    send(&mut w, &ToServer::Voice(packet(VOICE_NEARBY, 2))).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(m)) = w.next().await {
            if matches!(m, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the watcher was let go");
    assert_eq!(server.status().await.unwrap().players, vec!["Ann", "Ben"]);
    assert!(voice_within(&mut b.ws, QUIET).await.is_empty(), "the watcher's packet got through");
}

#[tokio::test]
async fn a_flood_of_voice_is_rate_limited() {
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    let (_server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Ben").await;
    place(&mut [&mut a, &mut b], &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]]).await;
    let start = std::time::Instant::now();
    for seq in 0..500 {
        send(&mut a.ws, &ToServer::Voice(packet(VOICE_NEARBY, seq))).await;
    }
    let sent = start.elapsed();
    // the bucket keeps refilling while we listen, so the bound counts that time as well
    let heard = voice_within(&mut b.ws, Duration::from_millis(1000)).await;
    let elapsed = start.elapsed().as_secs_f64();
    // a burst of 100, then 60 a second: no more than that gets through (with slack), and some must
    let max = 100.0 + 60.0 * elapsed + 10.0;
    assert!((heard.len() as f64) <= max, "{} got through in {elapsed:.2} s (sending took {sent:?})", heard.len());
    assert!(heard.len() >= 50, "only {} got through", heard.len());
    assert!(heard.iter().all(|(who, _)| *who == a.you));
}

#[tokio::test]
async fn party_speech_with_no_party_reaches_nobody() {
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    let (_server, url) = serve(cfg).await;
    let mut a = join(&url, "Ann").await;
    let mut b = join(&url, "Ben").await;
    let mut c = join(&url, "Cal").await;
    place(&mut [&mut a, &mut b, &mut c], &[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [100.0, 0.0, 0.0]]).await;
    // guests have no party, so the party channel goes nowhere, near or far
    send(&mut a.ws, &ToServer::Voice(packet(VOICE_PARTY, 1))).await;
    assert!(voice_within(&mut b.ws, QUIET).await.is_empty());
    assert!(voice_within(&mut c.ws, QUIET).await.is_empty());
}

#[tokio::test]
async fn party_speech_reaches_the_party_at_any_distance() {
    use vox_core::auth::{self, Character, Claims};
    // a stand-in directory: accounts 1 and 2 are in one raid party, account 3 is in none
    let dir = axum::Router::new().route(
        "/api/world/test/rules",
        axum::routing::get(|h: axum::http::HeaderMap| async move {
            if h.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer s3cret") {
                return axum::Json(serde_json::json!({}));
            }
            axum::Json(serde_json::json!({ "plots": [], "portals": [], "parties": [[1, 2]] }))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, dir).await.unwrap() });

    let key = auth::key_from_seed([9; 32]);
    let mut cfg = config(":memory:");
    cfg.trust_moves = true;
    cfg.directory = Some(key.verifying_key());
    cfg.directory_url = Some(dir_url);
    cfg.world_secret = Some("s3cret".into());
    cfg.rules_every = Duration::from_millis(100);
    cfg.report_every = Duration::from_secs(600);
    let (_server, url) = serve(cfg).await;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let token = |account: u64, name: &str| {
        auth::sign(&Claims { account, name: name.into(), character: Character::default(), issued: now, expires: now + 60 }, &key)
    };
    let mut a = join_with(&url, "Ann", Some(token(1, "Ann"))).await;
    let mut b = join_with(&url, "Ben", Some(token(2, "Ben"))).await;
    let mut c = join_with(&url, "Cal", Some(token(3, "Cal"))).await;
    // Ben is 200 blocks away; Cal is next to Ann
    place(&mut [&mut a, &mut b, &mut c], &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0], [4.0, 0.0, 0.0]]).await;

    // party speech: Ben hears it from 200 blocks off; Cal, who is not in the party, doesn't.
    // The rules are fetched on a timer and until they land the server drops party speech, so
    // resend every 100 ms until Ben gets one (5 s at most).
    let party = packet(VOICE_PARTY, 1);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut heard = Vec::new();
    while heard.is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "the party's rules never reached the server");
        send(&mut a.ws, &ToServer::Voice(party.clone())).await;
        heard = voice_within(&mut b.ws, Duration::from_millis(100)).await;
    }
    assert!(heard.iter().all(|h| *h == (a.you, party.clone())), "{heard:?}");
    // resends still in flight are the same packet; clear them before the nearby check
    let extra = voice_within(&mut b.ws, QUIET).await;
    assert!(extra.iter().all(|h| *h == (a.you, party.clone())), "{extra:?}");
    assert!(voice_within(&mut c.ws, QUIET).await.is_empty(), "a non-member heard the party");
    assert!(voice_within(&mut a.ws, QUIET).await.is_empty());

    // nearby speech: Cal hears it, Ben (far off, and not what was asked) doesn't
    let near = packet(VOICE_NEARBY, 2);
    send(&mut a.ws, &ToServer::Voice(near.clone())).await;
    assert_eq!(voice_within(&mut c.ws, QUIET).await, vec![(a.you, near)]);
    assert!(voice_within(&mut b.ws, QUIET).await.is_empty(), "far nearby speech reached Ben");
}
