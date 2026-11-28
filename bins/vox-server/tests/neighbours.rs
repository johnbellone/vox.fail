//! Links between neighbouring worlds: two atlas tiles side by side with the same world secret
//! link, the link goes down when one stops and comes back when it starts again; a dialler that
//! can't prove the secret, or isn't a neighbour, is refused.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use vox_core::{
    neighbour::{self, Answer, Hail, LINK_PROTOCOL},
    terrain::Tile,
};
use vox_server::{Config, Server};

fn tile(i: i32) -> Tile {
    Tile { atlas: "earth".into(), seed: 7, size: [2, 1], at: [i, 0] }
}

/// Tile `i` of a map two tiles wide, with `secret`, dialling its neighbours at `base`, served on
/// `listener`.
async fn world(i: i32, secret: &str, base: String, listener: TcpListener) -> (Server, JoinHandle<()>) {
    let palette: Vec<String> = ["grass", "dirt", "stone", "sand", "water"].map(String::from).to_vec();
    let mut cfg = Config::new(":memory:", palette);
    let t = tile(i);
    cfg.name = t.id();
    cfg.tile = Some(t);
    cfg.world_secret = Some(secret.into());
    cfg.link_base = Some(base);
    let server = Server::start(cfg).unwrap();
    let router = server.router();
    let serving = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (server, serving)
}

/// Is `server` linked with `other`?
fn linked(server: &Server, other: &str) -> bool {
    server.neighbours().iter().any(|n| n.world == other && n.linked)
}

/// Wait up to `secs` for `want`.
async fn until(secs: u64, mut want: impl FnMut() -> bool) -> bool {
    let end = tokio::time::Instant::now() + Duration::from_secs(secs);
    while tokio::time::Instant::now() < end {
        if want() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    want()
}

#[tokio::test(flavor = "multi_thread")]
async fn neighbours_link_drop_and_relink() {
    let (a_id, b_id) = (tile(0).id(), tile(1).id());
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a_addr, b_addr) = (la.local_addr().unwrap(), lb.local_addr().unwrap());
    // tile 0-0 sorts first, so it dials 1-0: its base is where 1-0 answers
    let (a, _a_serving) = world(0, "s3cret", format!("ws://{b_addr}"), la).await;
    let (b, b_serving) = world(1, "s3cret", format!("ws://{a_addr}"), lb).await;
    let n = a.neighbours();
    assert_eq!(n.len(), 1, "a map two tiles wide: one neighbour");
    assert!(n[0].dials && n[0].world == b_id);
    assert!(until(10, || linked(&a, &b_id) && linked(&b, &a_id)).await, "never linked: {:?} / {:?}", a.neighbours(), b.neighbours());
    // the status page says so too
    let page: serde_json::Value = reqwest::get(format!("http://{b_addr}/")).await.unwrap().json().await.unwrap();
    assert_eq!(page["neighbours"], serde_json::json!([{ "world": a_id, "linked": true }]));
    // heartbeats keep it up past the silence limit's worth of pings
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(linked(&a, &b_id) && linked(&b, &a_id), "the link fell over while both ran");

    // 1-0 stops: 0-0 sees the link go
    b_serving.abort();
    b.shutdown().await;
    assert!(until(10, || !linked(&a, &b_id)).await, "still linked to a world that stopped");

    // and comes back on the same address: they link again
    let lb = TcpListener::bind(b_addr).await.unwrap();
    let (b, _b_serving) = world(1, "s3cret", format!("ws://{a_addr}"), lb).await;
    assert!(until(20, || linked(&a, &b_id) && linked(&b, &a_id)).await, "never relinked: {:?} / {:?}", a.neighbours(), b.neighbours());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_different_secret_never_links() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a_addr, b_addr) = (la.local_addr().unwrap(), lb.local_addr().unwrap());
    let (a, _sa) = world(0, "s3cret", format!("ws://{b_addr}"), la).await;
    let (b, _sb) = world(1, "another", format!("ws://{a_addr}"), lb).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!linked(&a, &tile(1).id()) && !linked(&b, &tile(0).id()));
}

/// Dial `url`, say `hello`, and return the first answer.
async fn hail(url: &str, hello: Hail) -> (tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Answer) {
    let (mut ws, _) = connect_async(url).await.unwrap();
    ws.send(Message::Binary(neighbour::encode(&hello).into())).await.unwrap();
    let answer = next(&mut ws).await;
    (ws, answer)
}

async fn next(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) -> Answer {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m {
            return neighbour::decode_answer(&b).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listener_refuses_bad_hails() {
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let b_addr = lb.local_addr().unwrap();
    // nothing for 1-0 to dial (its only neighbour sorts first), so its base doesn't matter
    let (b, _sb) = world(1, "s3cret", "ws://127.0.0.1:9".into(), lb).await;
    // its link task has taken in its neighbours
    assert!(until(5, || !b.neighbours().is_empty()).await);
    let url = format!("ws://{b_addr}/w/{}/link", tile(1).id());
    let hello = |from: &str, protocol| Hail::Hello { protocol, from: from.into(), nonce: [4; 32] };
    let a_id = tile(0).id();

    // a neighbour that sends the wrong proof: the listener proved itself, then says goodbye
    let (mut ws, answer) = hail(&url, hello(&a_id, LINK_PROTOCOL)).await;
    let Answer::Challenge { world, proof, .. } = answer else { panic!("{answer:?}") };
    assert_eq!(world, tile(1).id());
    assert!(neighbour::same(&proof, &neighbour::proof("s3cret", neighbour::Side::Listener, &[4; 32], &world, &a_id)));
    ws.send(Message::Binary(neighbour::encode(&Hail::Proof { proof: [0; 32] }).into())).await.unwrap();
    assert_eq!(next(&mut ws).await, Answer::Bye { why: "wrong proof".into() });

    // a world that isn't a neighbour, and another protocol
    let (_, answer) = hail(&url, hello("yard", LINK_PROTOCOL)).await;
    assert!(matches!(answer, Answer::Bye { .. }), "{answer:?}");
    let (_, answer) = hail(&url, hello(&a_id, LINK_PROTOCOL + 1)).await;
    assert!(matches!(answer, Answer::Bye { .. }), "{answer:?}");

    // a world with no secret keeps no links at all
    let palette: Vec<String> = ["grass", "dirt", "stone", "sand", "water"].map(String::from).to_vec();
    let lone = Server::start(Config::new(":memory:", palette)).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lone_url = format!("ws://{}/link", l.local_addr().unwrap());
    let router = lone.router();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    let (_, answer) = hail(&lone_url, hello(&a_id, LINK_PROTOCOL)).await;
    assert_eq!(answer, Answer::Bye { why: "this world keeps no links".into() });
}
