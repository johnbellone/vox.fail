//! What doorways show about where they lead (ADR 0004 decisions 4, 11 and 12): the world asks
//! its host, tells players near a doorway its state, wakes a stopped destination, holds a
//! player at a waking doorway's sill until it is up, and turns them back from one that leads
//! nowhere.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::travel::{DoorState, Motion};
use vox_core::wire::{self, PROTOCOL, ToClient, ToServer};
use vox_server::doors::{Way, Ways};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Two doorways a step and a half north of the spawn, side by side: "sea" to a world on this
/// host, "lost" to one that isn't.
const LEVEL: &str = r#"{ "world": { "sky": "void", "portals": [
    {"name": "sea", "at": [0.5, 0, 2.0], "axis": "z", "size": [4, 6], "level": "sea", "url": "/w/sea/ws", "arrive": [0, 0, 11.5], "yaw": 0},
    {"name": "lost", "at": [8.5, 0, 2.0], "axis": "z", "size": [4, 6], "level": "sea", "url": "/w/lost/ws", "arrive": [0, 0, 11.5], "yaw": 0}
  ] }, "boxes": [ { "block": "grass", "from": [-16, -1, -16], "to": [16, -1, 16] } ] }"#;

/// A host for the test: worlds by name, and which were woken.
#[derive(Default)]
struct Host {
    worlds: Mutex<HashMap<String, Way>>,
    woken: Mutex<Vec<String>>,
}

impl Ways for Host {
    fn way(&self, name: &str) -> Way {
        *self.worlds.lock().unwrap().get(name).unwrap_or(&Way::Absent)
    }
    fn wake(&self, name: &str) {
        self.woken.lock().unwrap().push(name.to_string());
        self.worlds.lock().unwrap().insert(name.to_string(), Way::Starting);
    }
}

async fn serve(host: Arc<Host>) -> (Server, String) {
    let mut c = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    c.name = "void".into();
    c.seed = Some(LEVEL.into());
    c.ways = Some(host);
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

async fn join(url: &str) -> Ws {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: "Ann".into(), avatar: vec![], token: None }).await;
    wait_for(&mut ws, 5.0, |m| matches!(m, ToClient::Ready).then_some(())).await.expect("not ready");
    ws
}

fn door(name: &'static str) -> impl Fn(ToClient) -> Option<DoorState> {
    move |m| match m {
        ToClient::DoorState { portal, state } if portal == name => Some(state),
        _ => None,
    }
}

fn notice(m: ToClient) -> Option<String> {
    match m {
        ToClient::Notice { text } => Some(text),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_world_is_woken_and_its_doorway_opens_when_it_is_up() {
    let host = Arc::new(Host::default());
    host.worlds.lock().unwrap().insert("sea".into(), Way::Stopped);
    let (_s, url) = serve(host.clone()).await;
    let mut ws = join(&url).await;
    // near the doorway: it is checked, which wakes the sea, so it shows as waking
    send(&mut ws, &ToServer::Pose { at: [0.5, 0.0, 1.0], blob: vec![] }).await;
    assert_eq!(wait_for(&mut ws, 3.0, door("sea")).await, Some(DoorState::Waking));
    assert_eq!(host.woken.lock().unwrap().as_slice(), ["sea".to_string()]);
    // through while it wakes: held at the sill, told why, no way yet
    send(&mut ws, &ToServer::Pose { at: [0.5, 0.0, 2.0], blob: vec![] }).await;
    send(&mut ws, &ToServer::Through { portal: "sea".into(), side: 1, motion: Motion::default() }).await;
    let told = wait_for(&mut ws, 2.0, notice).await.expect("told nothing");
    assert!(told.contains("opening"), "{told}");
    // up now: the doorway opens and the held player goes on
    host.worlds.lock().unwrap().insert("sea".into(), Way::Running);
    let travel = wait_for(&mut ws, 3.0, |m| match m {
        ToClient::Travel { url, .. } => Some(url),
        _ => None,
    })
    .await;
    assert_eq!(travel.as_deref(), Some("/w/sea/ws"), "sent on once it was up");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_doorway_to_nowhere_shows_unlinked_and_turns_you_back() {
    let host = Arc::new(Host::default());
    let (_s, url) = serve(host.clone()).await;
    let mut ws = join(&url).await;
    send(&mut ws, &ToServer::Pose { at: [8.5, 0.0, 1.0], blob: vec![] }).await;
    assert_eq!(wait_for(&mut ws, 3.0, door("lost")).await, Some(DoorState::Unlinked));
    send(&mut ws, &ToServer::Pose { at: [8.5, 0.0, 2.0], blob: vec![] }).await;
    send(&mut ws, &ToServer::Through { portal: "lost".into(), side: 1, motion: Motion::default() }).await;
    let told = wait_for(&mut ws, 2.0, notice).await.expect("told nothing");
    assert!(told.contains("closed"), "{told}");
    let travel = wait_for(&mut ws, 1.5, |m| matches!(m, ToClient::Travel { .. }).then_some(())).await;
    assert!(travel.is_none(), "not sent anywhere");
    assert!(host.woken.lock().unwrap().is_empty(), "nothing to wake");
}
