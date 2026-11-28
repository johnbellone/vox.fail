//! Streaming by view radius (docs/design/chunk-streaming.md §3, vox-cti.7): a player who sends a
//! view before saying hello gets the chunks near them, nearest first, and only the edits they can
//! see; one who doesn't gets everything, as before.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::wire::{self, PROTOCOL, ToClient, ToServer, View};
use vox_core::world::ChunkKey;
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor round the spawn, and a tower far to the east (x 384, twelve chunks off).
const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [
    { "block": "grass", "from": [-40, -1, -40], "to": [40, -1, 40] },
    { "block": "stone", "from": [384, 0, 0], "to": [388, 20, 4] }
  ] }"#;

async fn serve() -> (Server, String) {
    let mut c = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    c.name = "void".into();
    c.seed = Some(LEVEL.into());
    c.trust_moves = true;
    // 32 m of full detail: 4 chunks
    c.view_radius_m = 32.0;
    let server = Server::start(c).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", l.local_addr().unwrap());
    let router = server.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (server, url)
}

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

/// Every message until `Ready` (the world as joining sends it).
async fn until_ready(ws: &mut Ws) -> Vec<ToClient> {
    let mut out = Vec::new();
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("no Ready").unwrap().unwrap();
        if let Message::Binary(b) = m {
            let m: ToClient = wire::decode(&b).unwrap();
            let ready = matches!(m, ToClient::Ready);
            out.push(m);
            if ready {
                return out;
            }
        }
    }
}

/// Messages for `secs` seconds.
async fn gather(ws: &mut Ws, secs: f32) -> Vec<ToClient> {
    let until = tokio::time::Instant::now() + Duration::from_secs_f32(secs);
    let mut out = Vec::new();
    while let Ok(Some(Ok(m))) = tokio::time::timeout_at(until, ws.next()).await {
        if let Message::Binary(b) = m {
            out.push(wire::decode(&b).unwrap());
        }
    }
    out
}

fn chunks(ms: &[ToClient]) -> Vec<ChunkKey> {
    ms.iter()
        .filter_map(|m| match m {
            ToClient::Chunk { key, .. } => Some(*key),
            _ => None,
        })
        .collect()
}

async fn join(url: &str, view: Option<View>) -> (Ws, Vec<ToClient>) {
    let (mut ws, _) = connect_async(url).await.unwrap();
    if let Some(v) = view {
        send(&mut ws, &ToServer::View(v)).await;
    }
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: "Ann".into(), avatar: vec![], token: None }).await;
    let got = until_ready(&mut ws).await;
    (ws, got)
}

const HERE: View = View { centre: ChunkKey([0, 0, 0]), radius: 16, rings: 0, budget_kbps: 0 };

#[tokio::test(flavor = "multi_thread")]
async fn a_far_player_gets_nothing_from_the_other_side() {
    let (_s, url) = serve().await;
    let (mut ann, got) = join(&url, Some(HERE)).await;
    let near = chunks(&got);
    assert!(!near.is_empty(), "the floor round the spawn");
    assert!(near.iter().all(|k| k.0[0].abs() <= 4 && k.0[2].abs() <= 4), "only within 4 chunks (the world's limit): {near:?}");
    assert!(!near.contains(&ChunkKey([12, 0, 0])), "not the tower");
    // someone without a view gets everything, the tower too
    let (mut bo, all) = join(&url, None).await;
    assert!(chunks(&all).contains(&ChunkKey([12, 0, 0])));
    // Bo builds on the tower: Ann, far off, isn't told; Bo is
    // (standing on the tower, building beside where they stand)
    send(&mut bo, &ToServer::Pose { at: [386.5, 21.0, 3.5], blob: vec![] }).await;
    send(&mut bo, &ToServer::Edit { id: 1, cell: [384, 21, 0], block: 2 }).await;
    let bo_saw = gather(&mut bo, 1.0).await;
    assert!(bo_saw.iter().any(|m| matches!(m, ToClient::Edits(e) if e.iter().any(|a| a.cell == [384, 21, 0]))), "{bo_saw:?}");
    let ann_saw = gather(&mut ann, 1.0).await;
    assert!(!ann_saw.iter().any(|m| matches!(m, ToClient::Edits(_))), "an edit far away isn't sent: {ann_saw:?}");
    assert!(chunks(&ann_saw).is_empty());
    // a build near Ann reaches her
    send(&mut bo, &ToServer::Pose { at: [3.5, 0.0, 3.5], blob: vec![] }).await;
    send(&mut bo, &ToServer::Edit { id: 2, cell: [1, 0, 1], block: 2 }).await;
    let ann_saw = gather(&mut ann, 1.0).await;
    assert!(ann_saw.iter().any(|m| matches!(m, ToClient::Edits(e) if e.iter().any(|a| a.cell == [1, 0, 1]))), "{ann_saw:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_nearest_chunk_comes_first_and_walking_brings_the_rest() {
    let (_s, url) = serve().await;
    let (mut ann, got) = join(&url, Some(View { centre: ChunkKey([1, 0, 1]), ..HERE })).await;
    let near = chunks(&got);
    assert_eq!(near.first(), Some(&ChunkKey([1, -1, 1])), "the chunk nearest [1, 0, 1] with blocks in it (the floor under it) comes first: {near:?}");
    let d = |k: &ChunkKey| (k.0[0] - 1).pow(2) + k.0[1].pow(2) + (k.0[2] - 1).pow(2);
    assert!(near.windows(2).all(|w| d(&w[0]) <= d(&w[1])), "nearest first: {near:?}");
    // walking east to the tower: it comes, paced, after the view moves
    send(&mut ann, &ToServer::View(View { centre: ChunkKey([11, 0, 0]), ..HERE })).await;
    let later = chunks(&gather(&mut ann, 1.5).await);
    assert!(later.contains(&ChunkKey([12, 0, 0])), "the tower, now in view: {later:?}");
    assert!(!later.iter().any(|k| near.contains(k)), "what was held isn't sent again");
}
