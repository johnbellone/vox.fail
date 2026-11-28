//! A tile of an atlas: its ground is generated, and walking off an edge goes on to the next
//! tile, at the same place on the map, standing on its ground.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use vox_core::travel::Motion;
use vox_core::{
    entity::{Component, EntityWhy, Settings},
    terrain::{self, Tile},
    wire::{self, EntityChange, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

#[tokio::test(flavor = "multi_thread")]
async fn over_the_edge() {
    let palette: Vec<String> = ["grass", "dirt", "stone", "sand", "water"].map(String::from).to_vec();
    let mut cfg = Config::new(":memory:", palette);
    let tile = Tile { atlas: "earth".into(), seed: 7, size: [2, 1], at: [0, 0] };
    cfg.name = tile.id();
    cfg.tile = Some(tile.clone());
    let server = Server::start(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });

    let (mut ws, _) = connect_async(&url).await.unwrap();
    let send = |m: ToServer| Message::Binary(wire::encode(&m).into());
    ws.send(send(ToServer::Hello { protocol: PROTOCOL, name: "Sam".into(), avatar: vec![], token: None })).await.unwrap();
    let mut settings = String::new();
    loop {
        let m = tokio::time::timeout(Duration::from_secs(20), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m {
            match wire::decode(&b).unwrap() {
                ToClient::Welcome { settings: s, .. } => settings = s,
                ToClient::Ready => break,
                _ => {}
            }
        }
    }
    // the world says it's a tile, and which ways lead on: only east, in a map two tiles wide
    let s: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(s["atlas"]["tile"], serde_json::json!([0, 0]));
    assert_eq!(s["atlas"]["edges"], serde_json::json!(["east"]));
    let travel = async |ws: &mut _| -> Option<(String, [f32; 3])> {
        let ws: &mut tokio_tungstenite::WebSocketStream<_> = ws;
        let until = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let m = tokio::time::timeout_at(until, ws.next()).await.ok()??.ok()?;
            if let Message::Binary(b) = m
                && let ToClient::Travel { url, arrive, .. } = wire::decode(&b).unwrap()
            {
                return Some((url, arrive));
            }
        }
    };
    // at the east edge: on to tile 1, 0, just inside its west edge, on its ground
    let y = tile.height(510, 10).max(terrain::SEA) as f32 + 1.0;
    ws.send(send(ToServer::Pose { at: [510.5, y, 10.5], blob: vec![] })).await.unwrap();
    ws.send(send(ToServer::Through { portal: "edge:east".into(), side: 1, motion: Motion::default() })).await.unwrap();
    let (to, arrive) = travel(&mut ws).await.expect("no way on");
    assert_eq!(to, "/w/atlas-earth-1-0/ws");
    let next = tile.next([1, 0]).unwrap();
    assert_eq!(arrive[0], -508.0, "clamped a step inside");
    assert_eq!(arrive[2], 10.5);
    assert_eq!(arrive[1], (next.height(-508, 10).max(terrain::SEA) + 1) as f32);
    // the west edge is the edge of the map; and the east one only from near it
    tokio::time::sleep(Duration::from_millis(1100)).await;
    ws.send(send(ToServer::Through { portal: "edge:west".into(), side: 1, motion: Motion::default() })).await.unwrap();
    assert!(travel(&mut ws).await.is_none());
}

/// A tile world that guests may edit (`editors_all`), with its tile's Settings entity.
async fn editable_tile() -> (Tile, String, tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, EntityChange, vox_server::Server) {
    let palette: Vec<String> = ["grass", "dirt", "stone", "sand", "water"].map(String::from).to_vec();
    let mut cfg = Config::new(":memory:", palette);
    let tile = Tile { atlas: "earth".into(), seed: 7, size: [2, 1], at: [0, 0] };
    cfg.name = tile.id();
    cfg.tile = Some(tile.clone());
    cfg.editors_all = true;
    let server = Server::start(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let (mut ws, _) = connect_async(&url).await.unwrap();
    ws.send(Message::Binary(wire::encode(&ToServer::Hello { protocol: PROTOCOL, name: "Gus".into(), avatar: vec![], token: None }).into())).await.unwrap();
    let mut settings = None;
    loop {
        let m = tokio::time::timeout(Duration::from_secs(20), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m {
            match wire::decode(&b).unwrap() {
                ToClient::Entities(batch) => {
                    settings = settings.or_else(|| batch.into_iter().find(|e| matches!(e.component, Some(Component::Settings(_)))));
                }
                ToClient::Ready => break,
                _ => {}
            }
        }
    }
    (tile, url, ws, settings.expect("a tile has its settings entity"), server)
}

/// The next message `pick` takes, within five seconds.
async fn next_with<T>(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, mut pick: impl FnMut(ToClient) -> Option<T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let m = ws.next().await.expect("closed").expect("socket error");
            if let Message::Binary(b) = m
                && let Some(t) = pick(wire::decode::<ToClient>(&b).unwrap())
            {
                return t;
            }
        }
    })
    .await
    .expect("timed out")
}

/// Puts the tile's Settings with `edges` as its atlas's open edges, expecting `expect`.
fn put_edges(id: u32, entity: &EntityChange, expect: u64, tile: &Tile, edges: &[&str]) -> ToServer {
    let mut s = tile.settings();
    s["atlas"]["edges"] = serde_json::json!(edges);
    ToServer::Put { id, entity: Some(entity.entity), expect: Some(expect), component: Component::Settings(Settings { json: s.to_string() }) }
}

#[tokio::test(flavor = "multi_thread")]
async fn atlas_closed_edge_refuses_travel_until_reopened() {
    let (tile, _url, mut ws, settings, _server) = editable_tile().await;
    let send = |m: ToServer| Message::Binary(wire::encode(&m).into());
    let travel = async |ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>| -> Option<String> {
        let until = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let m = tokio::time::timeout_at(until, ws.next()).await.ok()??.ok()?;
            if let Message::Binary(b) = m
                && let ToClient::Travel { url, .. } = wire::decode(&b).unwrap()
            {
                return Some(url);
            }
        }
    };
    let y = tile.height(510, 10).max(terrain::SEA) as f32 + 1.0;
    ws.send(send(ToServer::Pose { at: [510.5, y, 10.5], blob: vec![] })).await.unwrap();

    // close east: the Settings entity no longer lists it
    let expect = settings.seq;
    ws.send(send(put_edges(1, &settings, expect, &tile, &[]))).await.unwrap();
    let (id, _, seq) = next_with(&mut ws, |m| match m {
        ToClient::EntityDone { id, entity, seq } => Some((id, entity, seq)),
        _ => None,
    })
    .await;
    assert_eq!(id, 1);

    // at the east edge, but it is closed: no way on
    ws.send(send(ToServer::Through { portal: "edge:east".into(), side: 1, motion: Motion::default() })).await.unwrap();
    assert!(travel(&mut ws).await.is_none(), "a closed edge led on");

    // reopen it: the way on is back
    tokio::time::sleep(Duration::from_millis(1100)).await;
    ws.send(send(put_edges(2, &settings, seq, &tile, &["east"]))).await.unwrap();
    let (id, _, _) = next_with(&mut ws, |m| match m {
        ToClient::EntityDone { id, entity, seq } => Some((id, entity, seq)),
        _ => None,
    })
    .await;
    assert_eq!(id, 2);
    ws.send(send(ToServer::Through { portal: "edge:east".into(), side: 1, motion: Motion::default() })).await.unwrap();
    assert_eq!(travel(&mut ws).await.as_deref(), Some("/w/atlas-earth-1-0/ws"));
}

#[tokio::test(flavor = "multi_thread")]
async fn atlas_settings_cannot_open_an_edge_the_tile_lacks() {
    let (tile, _url, mut ws, settings, _server) = editable_tile().await;
    let send = |m: ToServer| Message::Binary(wire::encode(&m).into());
    // this tile is at 0-0 of a two-wide map: only east leads on, so "west" is refused
    ws.send(send(put_edges(3, &settings, settings.seq, &tile, &["east", "west"]))).await.unwrap();
    let refused = next_with(&mut ws, |m| match m {
        ToClient::EntityRefused { id, why } => Some((id, why)),
        _ => None,
    })
    .await;
    assert_eq!(refused, (3, EntityWhy::Invalid));
}
