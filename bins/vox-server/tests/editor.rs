//! Joining a world sends its entities in batches (none over the wire's size cap, none past
//! its count), the yard's settings and doorway arrive as entities, and walking through the
//! yard's doorway still answers with the way on.

use std::{collections::HashSet, time::Duration};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::travel::Motion;
use vox_core::{
    entity::{Axis, Component, Dest, Portal},
    wire::{self, ENTITIES_PER_MESSAGE, EntityChange, MAX_MESSAGE, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn db_path(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("vox-editor-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("world.db");
    let _ = std::fs::remove_file(&db);
    db.to_str().unwrap().to_string()
}

async fn serve(cfg: Config) -> (Server, String) {
    let server = Server::start(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (server, url)
}

/// Says hello as a guest, then reads until `Ready` and `want` entity changes have come.
/// Returns the connection, the Welcome's settings and each `Entities` batch in turn.
async fn join(url: &str, want: usize) -> (Ws, String, Vec<Vec<EntityChange>>) {
    let (mut ws, _) = connect_async(url).await.unwrap();
    ws.send(Message::Binary(wire::encode(&ToServer::Hello { protocol: PROTOCOL, name: "Ann".into(), avatar: vec![], token: None }).into())).await.unwrap();
    let mut settings = String::new();
    let mut batches = Vec::new();
    let (mut ready, mut total) = (false, 0);
    while !ready || total < want {
        let m = tokio::time::timeout(Duration::from_secs(20), ws.next()).await.expect("timed out").unwrap().unwrap();
        let Message::Binary(b) = m else { continue };
        match wire::decode::<ToClient>(&b).unwrap() {
            ToClient::Welcome { settings: s, .. } => settings = s,
            ToClient::Ready => ready = true,
            ToClient::Entities(batch) => {
                // the message as it went over the wire (a chunk or the weather may be larger)
                assert!(b.len() <= MAX_MESSAGE, "an entities message is {} bytes", b.len());
                assert!(batch.len() <= ENTITIES_PER_MESSAGE, "a batch has {} changes", batch.len());
                total += batch.len();
                batches.push(batch);
            }
            ToClient::Bye { why } => panic!("refused: {why}"),
            _ => {}
        }
    }
    (ws, settings, batches)
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_join_batches() {
    let db = db_path("batches");
    let cfg = || {
        let mut c = Config::new(&db, ["grass", "stone"].map(String::from).to_vec());
        c.name = "editor".into();
        c
    };
    // a new world writes its tables and its spawn; then 20,000 doorways are added to it
    Server::start(cfg()).unwrap().shutdown().await;
    let rows = {
        let mut c = rusqlite::Connection::open(&db).unwrap();
        let t = c.transaction().unwrap();
        for i in 0..20_000u32 {
            // long names and urls, so the byte cap (not only the count) splits the batches
            let portal = Component::Portal(Portal {
                name: format!("portal {i:0>24}"),
                at: [i as f32 * 8.0, 0.0, 0.0],
                axis: Axis::Z,
                size: [4.0, 6.0],
                yaw: 0.0,
                level: "yard".into(),
                to: Dest::Url { url: format!("/w/7781/{i:0>110}"), arrive: [0.0, 0.0, 0.0] },
                key: None,
            });
            t.execute(
                "INSERT INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (?1, 0, 0, 'vox::Portal', 1, 0, ?2)",
                rusqlite::params![1000 + i, wire::encode(&portal)],
            )
            .unwrap();
        }
        t.commit().unwrap();
        c.query_row("SELECT count(*) FROM entities", [], |r| r.get::<_, i64>(0)).unwrap() as usize
    };
    assert_eq!(rows, 20_001);

    let (_server, url) = serve(cfg()).await;
    let (_ws, _settings, batches) = join(&url, rows).await;
    assert!(batches.len() > 40, "{} batches", batches.len());
    let ids: HashSet<u32> = batches.iter().flatten().map(|c| c.entity.0).collect();
    assert_eq!(ids.len(), rows, "every entity arrives once");
    let _ = std::fs::remove_dir_all(std::path::Path::new(&db).parent().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_join_yard() {
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/");
    let blocks = std::fs::read_to_string(format!("{data}blocks.json")).unwrap();
    let mut cfg = Config::new(db_path("yard"), vox_core::palette_names(&blocks).unwrap());
    cfg.name = "yard".into();
    cfg.seed = Some(std::fs::read_to_string(format!("{data}levels/yard.json")).unwrap());
    // positions are taken as they come, so the walk to the doorway needs no path checks
    cfg.trust_moves = true;
    let (_server, url) = serve(cfg).await;

    // the yard has three entities: its doorway, its spawn and its settings
    let (mut ws, settings, batches) = join(&url, 3).await;
    assert!(settings.contains(r#""sky":"earth""#), "{settings}");
    assert!(!settings.contains("portals"), "{settings}");
    let all: Vec<EntityChange> = batches.into_iter().flatten().collect();
    assert_eq!(all.len(), 3);
    let portal = all
        .iter()
        .find_map(|c| match &c.component {
            Some(Component::Portal(p)) if p.name == "void" => Some(p),
            _ => None,
        })
        .expect("no void doorway");
    assert_eq!(portal.to, Dest::Url { url: ":7780".into(), arrive: [0.0, 0.0, -79.0] });
    assert!(all.iter().any(|c| matches!(c.component, Some(Component::Spawn(_)))));
    assert!(all.iter().any(|c| matches!(c.component, Some(Component::Settings(_)))));

    // standing in the doorway and going through still answers with the yard's way on
    let send = |m: ToServer| Message::Binary(wire::encode(&m).into());
    ws.send(send(ToServer::Pose { at: [0.0, 0.0, 23.0], blob: vec![] })).await.unwrap();
    ws.send(send(ToServer::Through { portal: "void".into(), side: 1, motion: Motion::default() })).await.unwrap();
    let travel = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Binary(b) = ws.next().await.unwrap().unwrap()
                && let ToClient::Travel { url, ticket, arrive, .. } = wire::decode(&b).unwrap()
            {
                return (url, ticket, arrive);
            }
        }
    })
    .await
    .expect("no way through");
    assert_eq!(travel, (":7780".to_string(), None, [0.0, 0.0, -79.0]));
}
