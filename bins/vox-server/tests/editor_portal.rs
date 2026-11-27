//! Player portals through the directory (docs/design/world-editor.md, phase 2): a builder's new
//! portal is charged once and placed when the rules still allow it, refunded when they no longer
//! do; a keyed portal's move and removal wait for the directory; an editor's unkeyed portal costs
//! nothing; and the rules' portal list is reconciled into the world (on each fetch, and at
//! startup, since the first fetch comes at once).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    auth::{self, Character, Claims, SigningKey},
    entity::{Axis, Component, Dest, EntityWhy, Portal},
    wire::{self, EntityChange, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor of grass at y = -1 and a spawn on it.
const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] } ] }"#;

fn key() -> SigningKey {
    auth::key_from_seed([9; 32])
}

/// A world that takes sign-in from the test key, asks the directory at `dir` (the stand-in) and
/// fetches its rules every 100 ms, so a change to them is seen at once.
fn asking(dir: &str) -> Config {
    let mut c = Config::new(":memory:", ["grass", "stone", "planks"].map(String::from).to_vec());
    c.name = "test".into();
    c.seed = Some(LEVEL.into());
    c.directory = Some(key().verifying_key());
    c.trust_moves = true;
    c.directory_url = Some(dir.into());
    c.world_secret = Some("s3cret".into());
    c.rules_every = Duration::from_millis(100);
    c
}

/// A world with no directory at all (local play): no rules, so no plots.
fn alone() -> Config {
    let mut c = asking("http://127.0.0.1:1");
    c.directory_url = None;
    c.world_secret = None;
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

/// A sign-in token for `account`, good for ten minutes.
fn token(account: u64, name: &str) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let claims = Claims { account, name: name.into(), character: Character::default(), issued: now, expires: now + 600 };
    auth::sign(&claims, &key())
}

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

/// The next message `pick` takes, within five seconds. Everything else is skipped.
async fn until<T>(ws: &mut Ws, mut pick: impl FnMut(ToClient) -> Option<T>) -> T {
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

async fn next_entities(ws: &mut Ws) -> Vec<EntityChange> {
    until(ws, |m| match m {
        ToClient::Entities(batch) => Some(batch),
        _ => None,
    })
    .await
}

async fn next_refusal(ws: &mut Ws) -> (u32, EntityWhy) {
    until(ws, |m| match m {
        ToClient::EntityRefused { id, why } => Some((id, why)),
        _ => None,
    })
    .await
}

/// Every entity change that arrives within `window`.
async fn quiet_for(ws: &mut Ws, window: Duration) -> Vec<EntityChange> {
    let mut out = Vec::new();
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout(window, ws.next()).await {
        if let ToClient::Entities(batch) = wire::decode(&b).unwrap() {
            out.extend(batch);
        }
    }
    out
}

struct Who {
    ws: Ws,
    /// the entities a newcomer is sent
    entities: Vec<EntityChange>,
}

/// Joins as `name` (signed in as `account`), and reads the world as it is sent.
async fn join(url: &str, name: &str, account: u64) -> Who {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token: Some(token(account, name)) }).await;
    let mut welcomed = false;
    let mut ready = false;
    let mut entities = Vec::new();
    while !welcomed || !ready {
        match until(&mut ws, Some).await {
            ToClient::Welcome { .. } => welcomed = true,
            // every entity comes before Ready
            ToClient::Entities(batch) => entities.extend(batch),
            ToClient::Ready => ready = true,
            ToClient::Bye { why } => panic!("refused: {why}"),
            _ => {}
        }
    }
    entities.extend(quiet_for(&mut ws, Duration::from_millis(300)).await);
    Who { ws, entities }
}

fn pose(at: [f32; 3]) -> ToServer {
    ToServer::Pose { at, blob: vec![] }
}

fn portal(name: &str, at: [f32; 3], to: Dest, key: Option<i64>) -> Component {
    Component::Portal(Portal { name: name.into(), at, axis: Axis::X, size: [4.0, 6.0], yaw: 0.0, level: "yard".into(), to, key })
}

/// A portal row as the directory's rules give it: its destination world (`to`) and that world's
/// url and arrival.
fn row(key: i64, at: [f64; 3], size: [f64; 2], yaw: f64) -> Value {
    json!({
        "key": key, "name": format!("yard ({key})"), "at": at, "axis": "x", "size": size, "yaw": yaw,
        "level": "yard", "url": "/w/yard/ws", "arrive": [0.5, 0.0, 0.5], "to": "yard",
    })
}

/// A paired portal's row: it leads to its other end (`pair`), which arrives at `arrive`.
fn paired(key: i64, at: [f64; 3], pair: i64, arrive: [f64; 3]) -> Value {
    let mut r = row(key, at, [4.0, 6.0], 0.0);
    r["pair"] = json!(pair);
    r["arrive"] = json!(arrive);
    r
}

/// A builder's plot round the origin, for `builders`.
fn plot(builders: &[u64]) -> Value {
    json!({ "id": 1, "x0": 0, "z0": 0, "x1": 10, "z1": 10, "builders": builders })
}

/// The stand-in directory's state: its rules (changed by a test), and every portal call it got
/// (path and body, in order).
#[derive(Default)]
struct Stand {
    rules: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    /// a charge that is answered also adds its row to the rules (as the directory's list would)
    on_make: Mutex<bool>,
    /// how many times the world fetched the rules
    fetches: Mutex<usize>,
    /// the next rules fetch is answered this late (with the list as it was when the fetch came)
    slow_rules: Mutex<Option<Duration>>,
    /// how many refunds are answered with an error first
    refund_fails: Mutex<usize>,
}

impl Stand {
    fn calls_to(&self, path: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(p, _)| p == path).map(|(_, b)| b.clone()).collect()
    }

    fn set_rules(&self, plots: Vec<Value>, portals: Vec<Value>) {
        *self.rules.lock().unwrap() = json!({ "plots": plots, "portals": portals, "parties": [] });
    }

    fn fetched(&self) -> usize {
        *self.fetches.lock().unwrap()
    }
}

/// The stand-in directory. Editors are account 7 alone. A portal charge is answered by account:
/// 6 has no Authority (402), 9's directory fails (500), 10's answer is slow (1.5 s, so a test can
/// change the plots meanwhile); anyone else is charged and placed. Calls are recorded before
/// they are answered.
async fn directory(st: Arc<Stand>) -> String {
    let app = axum::Router::new()
        .route(
            "/api/world/test/rules",
            get({
                let st = st.clone();
                move || {
                    let st = st.clone();
                    async move {
                        *st.fetches.lock().unwrap() += 1;
                        let rules = st.rules.lock().unwrap().clone();
                        let slow = st.slow_rules.lock().unwrap().take();
                        if let Some(d) = slow {
                            tokio::time::sleep(d).await;
                        }
                        Json(rules)
                    }
                }
            }),
        )
        .route(
            "/api/world/test/editor",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                Json(json!({ "editor": q.get("account").map(String::as_str) == Some("7") }))
            }),
        )
        .route(
            "/api/world/portal",
            post({
                let st = st.clone();
                move |Json(body): Json<Value>| {
                    let st = st.clone();
                    async move {
                        st.calls.lock().unwrap().push(("portal".into(), body.clone()));
                        match body["account"].as_u64() {
                            Some(6) => (StatusCode::PAYMENT_REQUIRED, Json(json!({ "error": "that takes 40 authority and you have 0" }))).into_response(),
                            Some(9) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                            account => {
                                // the first charge is key 100 (this call is already recorded, so count from 99)
                                let key = 99 + st.calls_to("portal").len() as i64;
                                if *st.on_make.lock().unwrap() {
                                    // the directory's list has the new row before it answers
                                    st.rules.lock().unwrap()["portals"].as_array_mut().unwrap().push(json!({
                                        "key": key, "name": format!("yard ({key})"), "at": body["at"], "axis": body["axis"],
                                        "size": body["size"], "yaw": body["yaw"], "level": "yard", "url": "/w/yard/ws",
                                        "arrive": [0.5, 0.0, 0.5], "to": "yard",
                                    }));
                                }
                                if account == Some(10) {
                                    tokio::time::sleep(Duration::from_millis(1500)).await;
                                }
                                Json(json!({ "key": key, "at": body["at"], "name": format!("yard ({key})") })).into_response()
                            }
                        }
                    }
                }
            }),
        )
        .route(
            "/api/world/portal/refund",
            post({
                let st = st.clone();
                move |Json(body): Json<Value>| {
                    let st = st.clone();
                    async move {
                        st.calls.lock().unwrap().push(("portal/refund".into(), body));
                        let fail = {
                            let mut left = st.refund_fails.lock().unwrap();
                            let fail = *left > 0;
                            *left = left.saturating_sub(1);
                            fail
                        };
                        if fail {
                            return StatusCode::SERVICE_UNAVAILABLE.into_response();
                        }
                        Json(json!({ "refunded": true })).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/world/portal/update",
            post({
                let st = st.clone();
                move |Json(body): Json<Value>| {
                    st.calls.lock().unwrap().push(("portal/update".into(), body.clone()));
                    async move { Json(json!({ "key": body["key"], "at": body["at"] })) }
                }
            }),
        )
        .route(
            "/api/world/portal/link",
            post({
                let st = st.clone();
                move |Json(body): Json<Value>| {
                    st.calls.lock().unwrap().push(("portal/link".into(), body.clone()));
                    // a row given a pair leads to the other end: here, a world called far
                    if let (Some(key), Some(pair)) = (body["key"].as_i64(), body["pair"].as_i64()) {
                        let mut rules = st.rules.lock().unwrap();
                        for r in rules["portals"].as_array_mut().unwrap() {
                            if r["key"].as_i64() == Some(key) {
                                r["pair"] = json!(pair);
                                r["to"] = json!("far");
                                r["url"] = json!("/w/far/ws");
                                r["arrive"] = json!([7.5, 0.0, 2.5]);
                            }
                        }
                    }
                    async { StatusCode::OK }
                }
            }),
        )
        .route(
            "/api/world/portal/remove",
            post({
                let st = st.clone();
                move |Json(body): Json<Value>| {
                    st.calls.lock().unwrap().push(("portal/remove".into(), body));
                    async { Json(json!({ "removed": true })) }
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

/// Waits (up to five seconds) until `done` holds.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

/// The keyed portal with `key` in a list of changes, if there is one.
fn keyed(list: &[EntityChange], key: i64) -> Option<&EntityChange> {
    list.iter().find(|e| matches!(&e.component, Some(Component::Portal(p)) if p.key == Some(key)))
}

fn portal_of(c: &EntityChange) -> &Portal {
    match &c.component {
        Some(Component::Portal(p)) => p,
        other => panic!("not a portal: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_builder_is_charged_once_and_everyone_sees_it() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[8])], vec![]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    let mut a = join(&url, "Cy", 8).await;
    let mut b = join(&url, "Di", 7).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;

    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    let made = next_entities(&mut a.ws).await;
    assert_eq!(made.len(), 1);
    let p = portal_of(&made[0]);
    assert_eq!(p.key, Some(100), "the directory's key is kept on the portal");
    assert_eq!(p.name, "yard (100)", "the directory's name");
    assert_eq!(next_entities(&mut b.ws).await, made, "the second player is told the same change");

    let asks = st.calls_to("portal");
    assert_eq!(asks.len(), 1, "one charge for one edit");
    assert_eq!(asks[0]["to"], json!("yard"));
    assert_eq!(asks[0]["axis"], json!("x"));
    assert_eq!(asks[0]["size"], json!([4.0, 6.0]));
    // a hash of the world, account, connection and edit id: always 32 hex characters, which
    // the directory accepts whatever the world is called
    assert!(
        asks[0]["idem"].as_str().is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())),
        "the request id is 32 hex characters"
    );
    assert!(st.calls_to("portal/refund").is_empty(), "a placed portal is not refunded");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_plot_released_while_asked_is_refunded_and_refused() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[10])], vec![]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    // account 10's answer is slow: the plot goes meanwhile
    let mut a = join(&url, "Ed", 10).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    wait_until("the charge to be asked", || !st.calls_to("portal").is_empty()).await;
    st.set_rules(vec![], vec![]);
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NotYours));
    wait_until("the refund to be asked", || !st.calls_to("portal/refund").is_empty()).await;
    let refunds = st.calls_to("portal/refund");
    assert_eq!(refunds.len(), 1, "the charge is given back");
    assert_eq!(refunds[0]["key"], json!(100));
    assert!(quiet_for(&mut a.ws, Duration::from_millis(200)).await.is_empty(), "nothing was placed");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_cant_afford_is_refused_and_nothing_is_placed() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[6])], vec![]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    let mut a = join(&url, "Al", 6).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::CantAfford));
    assert!(st.calls_to("portal/refund").is_empty(), "nothing was charged, so nothing is refunded");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_directory_failure_is_unavailable() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[9])], vec![]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    let mut a = join(&url, "Nia", 9).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::Unavailable));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_without_a_directory_a_builder_has_no_plot() {
    // local play: no rules, so no plot is anyone's and a builder's portal is refused before any
    // charge could be asked (a directory is the only thing that could charge one)
    let (_server, url) = serve(alone()).await;
    let mut a = join(&url, "Cy", 8).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NotYours));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_rules_fetch_begun_before_a_make_does_not_undo_it() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[8])], vec![]);
    *st.on_make.lock().unwrap() = true;
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    let mut a = join(&url, "Cy", 8).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    // the next rules fetch is answered late, with a list that has no portal in it: it begins
    // now, before the charge and the Make
    *st.slow_rules.lock().unwrap() = Some(Duration::from_millis(1500));
    wait_until("the slow fetch to begin", || st.slow_rules.lock().unwrap().is_none()).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    let made = next_entities(&mut a.ws).await;
    assert_eq!(portal_of(&made[0]).key, Some(100));
    // the late answer comes back and must not take the new portal away
    let later = quiet_for(&mut a.ws, Duration::from_millis(2500)).await;
    assert!(later.iter().all(|c| c.component.is_some()), "a portal was removed by a stale list: {later:?}");
    let newcomer = join(&url, "Di", 7).await;
    assert!(keyed(&newcomer.entities, 100).is_some(), "the new portal is still placed");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_refund_that_fails_is_asked_once_more() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[10])], vec![]);
    *st.refund_fails.lock().unwrap() = 1;
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    // as the plot-released test: the plot goes while the charge is asked, so the charge is refunded
    let mut a = join(&url, "Ed", 10).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    wait_until("the charge to be asked", || !st.calls_to("portal").is_empty()).await;
    st.set_rules(vec![], vec![]);
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NotYours));
    wait_until("the refund to be asked twice", || st.calls_to("portal/refund").len() >= 2).await;
    let refunds = st.calls_to("portal/refund");
    assert_eq!(refunds.len(), 2, "the refused refund is asked once more, and no more");
    assert_eq!(refunds[1]["key"], json!(100));
}

/// Sets the rules to one plot (accounts 7 and 8 build there) and `portals`.
fn with_keyed_portal(st: &Stand, portals: Vec<Value>) {
    st.set_rules(vec![plot(&[7, 8])], portals);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_builder_moves_its_own_keyed_portal_through_the_directory() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // the builder moves it: the directory moves it first, then the world applies the change
    let mut a = join(&url, "Cy", 8).await;
    let (id, seq) = {
        let c = keyed(&a.entities, 42).expect("the directory's portal is placed at startup");
        (c.entity, c.seq)
    };
    let moved = portal_of(keyed(&a.entities, 42).unwrap()).clone();
    let moved = Component::Portal(Portal { at: [4.5, 0.0, 2.5], ..moved });
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: Some(id), expect: Some(seq), component: moved }).await;
    let changed = next_entities(&mut a.ws).await;
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].entity, id, "the same entity");
    assert!(changed[0].seq > seq);
    assert_eq!(portal_of(&changed[0]).at, [4.5, 0.0, 2.5]);
    assert_eq!(portal_of(&changed[0]).key, Some(42), "still the directory's portal");
    let updates = st.calls_to("portal/update");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0]["key"], json!(42));
    assert_eq!(updates[0]["at"], json!([4.5, 0.0, 2.5]));
    assert!(st.calls_to("portal/link").is_empty(), "the same destination is not linked again");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_an_editor_cannot_move_or_remove_a_players_keyed_portal() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // a player's portal belongs to its owner: refused here, with no call to the directory
    let mut a = join(&url, "Ann", 7).await;
    let c = keyed(&a.entities, 42).expect("placed at startup").clone();
    let moved = Component::Portal(Portal { at: [4.5, 0.0, 2.5], ..portal_of(&c).clone() });
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: Some(c.entity), expect: Some(c.seq), component: moved }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NotYours));
    send(&mut a.ws, &ToServer::Remove { id: 2, entity: c.entity, expect: Some(c.seq) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::NotYours));
    assert!(st.calls_to("portal/update").is_empty());
    assert!(st.calls_to("portal/remove").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_builder_repoints_its_portal_to_another_pair_through_link() {
    let st = Arc::new(Stand::default());
    // 42 leads to the world; 43 is another of the builder's portals
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0), row(43, [7.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut a = join(&url, "Cy", 8).await;
    let c = keyed(&a.entities, 42).expect("placed at startup").clone();
    let mut p = portal_of(&c).clone();
    p.to = Dest::Pair { key: 43 };
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: Some(c.entity), expect: Some(c.seq), component: Component::Portal(p) }).await;
    let changed = next_entities(&mut a.ws).await;
    assert_eq!(changed.len(), 1);
    assert!(matches!(&portal_of(&changed[0]).to, Dest::Pair { key: 43 }));
    assert_eq!(st.calls_to("portal/link"), vec![json!({ "key": 42, "pair": 43, "world": "test", "account": 8 })]);
    assert!(st.calls_to("portal/update").is_empty(), "nothing moved");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_walking_into_a_world_portal_travels_to_its_rows_url() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut a = join(&url, "Di", 7).await;
    send(&mut a.ws, &pose([3.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Through { portal: "yard (42)".into(), side: 1 }).await;
    let (to, arrive) = until(&mut a.ws, |m| match m {
        ToClient::Travel { url, arrive, .. } => Some((url, arrive)),
        _ => None,
    })
    .await;
    assert_eq!(to, "/w/yard/ws", "the url of the row's world");
    assert_eq!(arrive, [0.5, 0.0, 0.5], "where the row says it arrives");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_walking_into_a_pair_travels_to_the_other_end() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![paired(42, [3.5, 0.0, 2.5], 43, [7.5, 0.0, 2.5])]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut a = join(&url, "Di", 7).await;
    send(&mut a.ws, &pose([3.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Through { portal: "yard (42)".into(), side: 1 }).await;
    let arrive = until(&mut a.ws, |m| match m {
        ToClient::Travel { arrive, .. } => Some(arrive),
        _ => None,
    })
    .await;
    assert_eq!(arrive, [7.5, 0.0, 2.5], "the paired end's place");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_remove_of_a_keyed_portal_waits_for_the_directory() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // builder 8 removes its own keyed portal (no refund: the directory does not give Authority back here)
    let mut a = join(&url, "Cy", 8).await;
    let c = keyed(&a.entities, 42).expect("placed at startup").clone();
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Remove { id: 1, entity: c.entity, expect: Some(c.seq) }).await;
    let gone = next_entities(&mut a.ws).await;
    assert_eq!(gone, vec![EntityChange { seq: gone[0].seq, entity: c.entity, component: None }]);
    assert_eq!(st.calls_to("portal/remove"), vec![json!({ "key": 42, "world": "test", "account": 8 })]);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_an_editors_unkeyed_portal_needs_no_directory_call() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![], vec![]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    let mut a = join(&url, "Ann", 7).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("free", [3.5, 0.0, 2.5], Dest::Url { url: ":7781".into(), arrive: [0.0; 3] }, None) }).await;
    let made = next_entities(&mut a.ws).await;
    assert_eq!(portal_of(&made[0]).key, None, "an editor's portal is not keyed");
    assert!(st.calls_to("portal").is_empty(), "an editor's portal costs no call");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_new_portal_is_walkable_once_the_directory_has_it() {
    // the rules fetch is every 60 s, so only the answer to the charge can fetch them in time
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[8])], vec![]);
    *st.on_make.lock().unwrap() = true;
    let mut cfg = asking(&directory(st.clone()).await);
    cfg.rules_every = Duration::from_secs(60);
    let (_server, url) = serve(cfg).await;
    wait_until("the startup fetch", || st.fetched() >= 1).await;
    let mut a = join(&url, "Cy", 8).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    let made = next_entities(&mut a.ws).await;
    let name = portal_of(&made[0]).name.clone();
    wait_until("the rules to be fetched again, at once", || st.fetched() >= 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    send(&mut a.ws, &pose([3.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Through { portal: name, side: 1 }).await;
    let to = until(&mut a.ws, |m| match m {
        ToClient::Travel { url, .. } => Some(url),
        _ => None,
    })
    .await;
    assert_eq!(to, "/w/yard/ws", "the new portal's row gives its url");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_repointed_portal_travels_to_its_new_pair_at_once() {
    let st = Arc::new(Stand::default());
    // 42 leads to the world; 43 is another of the builder's portals
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0), row(43, [7.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let mut cfg = asking(&directory(st.clone()).await);
    cfg.rules_every = Duration::from_secs(60);
    let (_server, url) = serve(cfg).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut a = join(&url, "Cy", 8).await;
    let c = keyed(&a.entities, 42).expect("placed at startup").clone();
    let mut p = portal_of(&c).clone();
    p.to = Dest::Pair { key: 43 };
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: Some(c.entity), expect: Some(c.seq), component: Component::Portal(p) }).await;
    next_entities(&mut a.ws).await;
    wait_until("the rules to be fetched again, at once", || st.fetched() >= 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    send(&mut a.ws, &pose([3.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Through { portal: "yard (42)".into(), side: 1 }).await;
    let (to, arrive) = until(&mut a.ws, |m| match m {
        ToClient::Travel { url, arrive, .. } => Some((url, arrive)),
        _ => None,
    })
    .await;
    assert_eq!(to, "/w/far/ws", "the new pair's world");
    assert_eq!(arrive, [7.5, 0.0, 2.5], "the new pair's arrival");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_player_who_leaves_while_asked_gets_the_charge_back() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[10])], vec![]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    // account 10's answer is slow (1.5 s): the player leaves before it comes
    let mut a = join(&url, "Ed", 10).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    wait_until("the charge to be asked", || !st.calls_to("portal").is_empty()).await;
    drop(a);
    wait_until("the refund to be asked", || !st.calls_to("portal/refund").is_empty()).await;
    let refunds = st.calls_to("portal/refund");
    assert_eq!(refunds.len(), 1, "the charge is given back once");
    assert_eq!(refunds[0]["key"], json!(100));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_a_portal_the_rules_placed_first_is_not_placed_twice() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[10])], vec![]);
    // the new row is in the rules before the answer, and the rules are fetched (every 100 ms)
    // before the 1.5 s answer comes: the portal is placed by the fetch
    *st.on_make.lock().unwrap() = true;
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    let mut a = join(&url, "Ed", 10).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    let placed = next_entities(&mut a.ws).await;
    assert!(keyed(&placed, 100).is_some(), "the fetch placed the portal");
    // the answer comes after: nothing more is refused, and nothing is placed a second time
    let mut refusals = Vec::new();
    let mut again = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(2500);
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout_at(deadline, a.ws.next()).await {
        match wire::decode::<ToClient>(&b).unwrap() {
            ToClient::EntityRefused { id, why } => refusals.push((id, why)),
            ToClient::Entities(batch) => again.extend(batch.into_iter().filter(|e| keyed(std::slice::from_ref(e), 100).is_some())),
            _ => {}
        }
    }
    assert!(refusals.is_empty(), "no refusal: {refusals:?}");
    assert!(again.is_empty(), "no second entity with key 100: {again:?}");
    assert!(st.calls_to("portal/refund").is_empty(), "a placed portal is not refunded");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_with_no_id_left_is_refused_and_the_charge_refunded() {
    let st = Arc::new(Stand::default());
    st.set_rules(vec![plot(&[8])], vec![]);
    let db = std::env::temp_dir().join(format!("vox-no-ids-{}-{}.db", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let mut cfg = asking(&directory(st.clone()).await);
    cfg.db = db.clone();
    // a first run makes the tables; then an entity at the last id is stored, so the counter is spent
    let (first, _) = serve(cfg.clone()).await;
    first.shutdown().await;
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let data = wire::encode(&portal("last", [30.5, 0.0, 30.5], Dest::World { world: "yard".into() }, None));
        conn.execute(
            "INSERT INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (?1, 1, 1, 'last', 1, 1, ?2)",
            rusqlite::params![i64::from(u32::MAX), data],
        )
        .unwrap();
    }
    let (_server, url) = serve(cfg).await;
    let mut a = join(&url, "Cy", 8).await;
    send(&mut a.ws, &pose([2.5, 0.0, 2.5])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [3.5, 0.0, 2.5], Dest::World { world: "yard".into() }, None) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::TooMany));
    wait_until("the refund to be asked", || !st.calls_to("portal/refund").is_empty()).await;
    assert_eq!(st.calls_to("portal/refund")[0]["key"], json!(100), "the charge is given back");
    assert!(quiet_for(&mut a.ws, Duration::from_millis(200)).await.is_empty(), "nothing was placed");
    let _ = std::fs::remove_file(&db);
}

// ---- reconciliation of the rules' portals

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_reconcile_an_unchanged_list_broadcasts_nothing() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut w = join(&url, "Di", 7).await;
    // several rules fetches (every 100 ms) with the same list
    assert!(quiet_for(&mut w.ws, Duration::from_millis(600)).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_reconcile_a_changed_value_replaces_it_at_the_same_id() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut w = join(&url, "Di", 7).await;
    let before = keyed(&w.entities, 42).unwrap().clone();
    // the directory moves it and gives it a new size and yaw
    with_keyed_portal(&st, vec![row(42, [5.5, 0.0, 2.5], [5.0, 7.0], 90.0)]);
    let changed = next_entities(&mut w.ws).await;
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].entity, before.entity, "the same entity");
    assert!(changed[0].seq > before.seq);
    let p = portal_of(&changed[0]);
    assert_eq!(p.at, [5.5, 0.0, 2.5]);
    assert_eq!(p.size, [5.0, 7.0], "size comes from the rules row");
    assert_eq!(p.yaw, 90.0, "yaw comes from the rules row");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_reconcile_a_dropped_key_is_removed_live() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut w = join(&url, "Di", 7).await;
    let before = keyed(&w.entities, 42).unwrap().clone();
    with_keyed_portal(&st, vec![]);
    let gone = next_entities(&mut w.ws).await;
    assert_eq!(gone, vec![EntityChange { seq: gone[0].seq, entity: before.entity, component: None }]);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_reconcile_a_duplicate_key_keeps_the_first() {
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0), row(42, [7.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let w = join(&url, "Di", 7).await;
    let with_42: Vec<&EntityChange> = w.entities.iter().filter(|e| matches!(&e.component, Some(Component::Portal(p)) if p.key == Some(42))).collect();
    assert_eq!(with_42.len(), 1, "one portal per key");
    assert_eq!(portal_of(with_42[0]).at, [3.5, 0.0, 2.5], "the first row wins");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_reconcile_a_row_without_a_key_is_skipped() {
    let st = Arc::new(Stand::default());
    let mut keyless = row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0);
    keyless.as_object_mut().unwrap().remove("key");
    with_keyed_portal(&st, vec![keyless]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let w = join(&url, "Di", 7).await;
    let portals = w.entities.iter().filter(|e| matches!(&e.component, Some(Component::Portal(_)))).count();
    assert_eq!(portals, 0, "a row without a key is left out");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_reconcile_a_malformed_row_removes_its_portal() {
    // deliberately: the directory is the record of keyed portals, so a row it sends that does not
    // read as a doorway means the portal is gone from its list, and it is removed here too
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut w = join(&url, "Di", 7).await;
    let before = keyed(&w.entities, 42).unwrap().clone();
    let mut bad = row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0);
    bad.as_object_mut().unwrap().remove("level");
    with_keyed_portal(&st, vec![bad]);
    let gone = next_entities(&mut w.ws).await;
    assert_eq!(gone, vec![EntityChange { seq: gone[0].seq, entity: before.entity, component: None }]);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_portal_startup_reconciles_before_anyone_joins() {
    // the first rules fetch comes at once when the world starts (restore runs before it), so the
    // portals are placed without any further change
    let st = Arc::new(Stand::default());
    with_keyed_portal(&st, vec![row(42, [3.5, 0.0, 2.5], [4.0, 6.0], 0.0)]);
    let (_server, url) = serve(asking(&directory(st.clone()).await)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let w = join(&url, "Di", 7).await;
    assert!(keyed(&w.entities, 42).is_some());
}
