//! A tile of an atlas: its ground is generated, and walking off an edge goes on to the next
//! tile, at the same place on the map, standing on its ground.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use vox_core::{
    terrain::{self, Tile},
    wire::{self, PROTOCOL, ToClient, ToServer},
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
    let y = tile.height(254, 10).max(terrain::SEA) as f32 + 1.0;
    ws.send(send(ToServer::Pose { at: [254.5, y, 10.5], blob: vec![] })).await.unwrap();
    ws.send(send(ToServer::Through { portal: "edge:east".into(), side: 1 })).await.unwrap();
    let (to, arrive) = travel(&mut ws).await.expect("no way on");
    assert_eq!(to, "/w/atlas-earth-1-0/ws");
    let next = tile.next([1, 0]).unwrap();
    assert_eq!(arrive[0], -254.0, "clamped a step inside");
    assert_eq!(arrive[2], 10.5);
    assert_eq!(arrive[1], (next.height(-254, 10).max(terrain::SEA) + 1) as f32);
    // the west edge is the edge of the map; and the east one only from near it
    tokio::time::sleep(Duration::from_millis(1100)).await;
    ws.send(send(ToServer::Through { portal: "edge:west".into(), side: 1 })).await.unwrap();
    assert!(travel(&mut ws).await.is_none());
}
