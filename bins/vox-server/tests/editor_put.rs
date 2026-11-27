//! Entity edits over real WebSockets (docs/design/world-editor.md, "Who may edit what" and
//! "Security"): an editor's change reaches everyone in order, a stale or unknown entity is
//! `Changed`, rights are asked of the directory and fail closed, reach and the caps hold, a
//! builder's places are refused, and doorways to a dungeon or in place answer as they should.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    Why,
    auth::{self, Character, Claims, SigningKey},
    entity::{Axis, Component, Dest, EntityId, EntityWhy, Objective, Portal},
    wire::{self, EntityChange, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor of grass at y = -1 and a spawn on it (as world.rs's tests use).
const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] } ] }"#;

fn key() -> SigningKey {
    auth::key_from_seed([9; 32])
}

/// A world that takes sign-in from `key`. Positions are taken as players report them, so a
/// test stands where it needs to with a pose. Advanced rights are asked of a directory (see
/// `asking`): `editors_all` is for local play without one, and is refused with a directory.
fn config() -> Config {
    let mut c = Config::new(":memory:", ["grass", "stone", "planks"].map(String::from).to_vec());
    c.name = "test".into();
    c.seed = Some(LEVEL.into());
    c.directory = Some(key().verifying_key());
    c.trust_moves = true;
    c
}

/// The yard, which has a doorway with no key (a level's, not a player's) and settings.
fn yard_config() -> Config {
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/");
    let blocks = std::fs::read_to_string(format!("{data}blocks.json")).unwrap();
    let mut c = Config::new(":memory:", vox_core::palette_names(&blocks).unwrap());
    c.name = "test".into();
    c.seed = Some(std::fs::read_to_string(format!("{data}levels/yard.json")).unwrap());
    c.directory = Some(key().verifying_key());
    c.trust_moves = true;
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

async fn next_done(ws: &mut Ws) -> (u32, EntityId, u64) {
    until(ws, |m| match m {
        ToClient::EntityDone { id, entity, seq } => Some((id, entity, seq)),
        _ => None,
    })
    .await
}

struct Who {
    ws: Ws,
    spawn: [f32; 3],
    /// the welcome's settings
    settings: String,
    /// the entities a newcomer is sent
    entities: Vec<EntityChange>,
}

/// Joins as `name` (with `token`, or as a guest), and reads the world as it is sent.
async fn join(url: &str, name: &str, token: Option<String>) -> Who {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token }).await;
    let mut who = None;
    let mut ready = false;
    let mut entities = Vec::new();
    while who.is_none() || !ready {
        let m = until(&mut ws, Some).await;
        match m {
            ToClient::Welcome { spawn, settings, .. } => who = Some((spawn, settings)),
            // every entity comes before Ready
            ToClient::Entities(batch) => entities.extend(batch),
            ToClient::Ready => ready = true,
            ToClient::Bye { why } => panic!("refused: {why}"),
            _ => {}
        }
    }
    let (spawn, settings) = who.unwrap();
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout(Duration::from_millis(200), ws.next()).await {
        if let ToClient::Entities(batch) = wire::decode(&b).unwrap() {
            entities.extend(batch);
        }
    }
    Who { ws, spawn, settings, entities }
}

fn pose(at: [f32; 3]) -> ToServer {
    ToServer::Pose { at, blob: vec![] }
}

fn portal(name: &str, at: [f32; 3], to: Dest) -> Component {
    Component::Portal(Portal { name: name.into(), at, axis: Axis::X, size: [4.0, 6.0], yaw: 0.0, level: "yard".into(), to, key: None })
}

fn url_to(url: &str) -> Dest {
    Dest::Url { url: url.into(), arrive: [0.0; 3] }
}

/// A stand-in directory: a world named `test` with no plots, and the editor check, which says
/// yes for account 7 (and, slowly, for account 5), no for everyone else, and fails (500) for
/// account 9. Each check is counted.
async fn directory(asks: Arc<AtomicUsize>) -> String {
    let app = axum::Router::new()
        .route(
            "/api/world/test/rules",
            axum::routing::get(|| async { Json(serde_json::json!({ "plots": [], "portals": [] })) }),
        )
        .route(
            "/api/world/test/editor",
            axum::routing::get(move |Query(q): Query<HashMap<String, String>>| {
                let asks = asks.clone();
                async move {
                    asks.fetch_add(1, Ordering::SeqCst);
                    match q.get("account").map(String::as_str) {
                        Some("9") => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                        // a slow yes: an edit made meanwhile is held until it comes
                        Some("5") => {
                            tokio::time::sleep(Duration::from_millis(1500)).await;
                            Json(serde_json::json!({ "editor": true })).into_response()
                        }
                        account => Json(serde_json::json!({ "editor": account == Some("7") })).into_response(),
                    }
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

/// A world that asks `dir` about advanced rights (not every player is an editor).
fn asking(dir: &str) -> Config {
    let mut c = config();
    c.directory_url = Some(dir.into());
    c.world_secret = Some("s3cret".into());
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_an_editor_moves_and_removes_a_portal_for_everyone() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    let mut b = join(&url, "Bob", Some(token(8, "Bob"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;

    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    let made = next_entities(&mut a.ws).await;
    assert_eq!(made.len(), 1);
    let (id, seq) = (made[0].entity, made[0].seq);
    let seen = next_entities(&mut b.ws).await;
    assert_eq!(seen, made, "the second player is told the same change");

    send(&mut a.ws, &ToServer::Put { id: 2, entity: Some(id), expect: Some(seq), component: portal("door", [3.5, 0.0, 2.5], url_to(":7781")) }).await;
    let moved = next_entities(&mut b.ws).await;
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].entity, id);
    assert!(moved[0].seq > seq, "a change takes a later edit number");
    match &moved[0].component {
        Some(Component::Portal(p)) => assert_eq!(p.at, [3.5, 0.0, 2.5]),
        other => panic!("{other:?}"),
    }

    send(&mut a.ws, &ToServer::Remove { id: 3, entity: id, expect: Some(moved[0].seq) }).await;
    let gone = next_entities(&mut b.ws).await;
    assert_eq!(gone, vec![EntityChange { seq: gone[0].seq, entity: id, component: None }]);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_stale_or_unknown_entity_is_changed() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    let made = next_entities(&mut a.ws).await;
    let (id, seq) = (made[0].entity, made[0].seq);

    // an expect that is not the entity's last change, and none at all
    send(&mut a.ws, &ToServer::Put { id: 2, entity: Some(id), expect: Some(seq + 100), component: portal("door", [3.5, 0.0, 2.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::Changed));
    send(&mut a.ws, &ToServer::Put { id: 3, entity: Some(id), expect: None, component: portal("door", [3.5, 0.0, 2.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (3, EntityWhy::Changed));
    // an entity that doesn't exist
    send(&mut a.ws, &ToServer::Put { id: 4, entity: Some(EntityId(999_999)), expect: Some(0), component: portal("door", [3.5, 0.0, 2.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (4, EntityWhy::Changed));
    send(&mut a.ws, &ToServer::Remove { id: 5, entity: EntityId(999_999), expect: Some(0) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (5, EntityWhy::Changed));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_guest_is_not_yours() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let mut cfg = asking(&dir);
    cfg.guests = true;
    let (_server, url) = serve(cfg).await;
    let mut g = join(&url, "Gus", None).await;
    send(&mut g.ws, &pose(g.spawn)).await;
    send(&mut g.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut g.ws).await, (1, EntityWhy::NotYours));
}

/// A local world with no directory (so guests may join), with or without `--editors-all`.
fn local_config(editors_all: bool) -> Config {
    let mut c = config();
    c.directory = None;
    c.editors_all = editors_all;
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_guest_edits_on_an_editors_all_world() {
    let (_server, url) = serve(local_config(true)).await;
    let mut g = join(&url, "Gus", None).await;
    send(&mut g.ws, &pose(g.spawn)).await;
    send(&mut g.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    let (id, _, _) = next_done(&mut g.ws).await;
    assert_eq!(id, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_guest_is_not_yours_without_editors_all() {
    let (_server, url) = serve(local_config(false)).await;
    let mut g = join(&url, "Gus", None).await;
    send(&mut g.ws, &pose(g.spawn)).await;
    send(&mut g.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("door", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut g.ws).await, (1, EntityWhy::NotYours));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_builder_cannot_move_a_level_portal_or_place_one() {
    let dir = directory(Arc::default()).await;
    let (_server, url) = serve(yard_config_asking(&dir)).await;
    // Cy is account 8: the directory says no, so a builder
    let mut c = join(&url, "Cy", Some(token(8, "Cy"))).await;
    let void = c
        .entities
        .iter()
        .find_map(|e| match &e.component {
            Some(Component::Portal(p)) if p.name == "void" && p.key.is_none() => Some((e.entity, e.seq, p.at)),
            _ => None,
        })
        .expect("the yard's void doorway");
    send(&mut c.ws, &pose(void.2)).await;
    let moved = portal("void", [void.2[0] + 1.0, void.2[1], void.2[2]], url_to(":7780"));
    send(&mut c.ws, &ToServer::Put { id: 1, entity: Some(void.0), expect: Some(void.1), component: moved }).await;
    assert_eq!(next_refusal(&mut c.ws).await, (1, EntityWhy::NotYours));
    // a builder's new portal: the rules are judged first (this stand-in has no plot of theirs),
    // so it is refused before any charge is asked for
    send(&mut c.ws, &ToServer::Put { id: 2, entity: None, expect: None, component: portal("door", [void.2[0], void.2[1], void.2[2] + 1.0], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut c.ws).await, (2, EntityWhy::NotYours));
}

/// The yard, asking the directory at `dir` about advanced rights.
fn yard_config_asking(dir: &str) -> Config {
    let mut c = yard_config();
    c.directory_url = Some(dir.into());
    c.world_secret = Some("s3cret".into());
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_far_entity_cannot_be_moved_or_placed_out_of_reach() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    // a portal far off, placed while standing there
    send(&mut a.ws, &pose([100.0, 0.0, 0.0])).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("far", [100.5, 0.0, 0.5], url_to(":7781")) }).await;
    let far = next_entities(&mut a.ws).await;
    let (id, seq) = (far[0].entity, far[0].seq);
    // back at the spawn: placing one there is in reach, but this one's old place is not
    send(&mut a.ws, &pose(a.spawn)).await;
    send(&mut a.ws, &ToServer::Put { id: 2, entity: Some(id), expect: Some(seq), component: portal("far", [5.5, 0.0, 0.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::TooFar));
    send(&mut a.ws, &ToServer::Remove { id: 3, entity: id, expect: Some(seq) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (3, EntityWhy::TooFar));
    // and a new one out of reach is refused too
    send(&mut a.ws, &ToServer::Put { id: 4, entity: None, expect: None, component: portal("near", [100.5, 0.0, 2.5], url_to(":7781")) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (4, EntityWhy::TooFar));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_eleven_in_a_second_are_too_fast() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    for i in 0..11u32 {
        let at = [1.5 + i as f32, 0.0, 2.5];
        send(&mut a.ws, &ToServer::Put { id: i, entity: None, expect: None, component: portal("p", at, url_to(":7781")) }).await;
    }
    let (mut made, mut slowed) = (0, 0);
    while made + slowed < 11 {
        let got = until(&mut a.ws, |m| match m {
            ToClient::Entities(batch) => Some(Ok(batch.len())),
            ToClient::EntityRefused { why, .. } => Some(Err(why)),
            _ => None,
        })
        .await;
        match got {
            Ok(n) => made += n,
            Err(why) => {
                assert_eq!(why, EntityWhy::TooFast);
                slowed += 1;
            }
        }
    }
    assert!(slowed >= 1, "{made} made, none slowed");
    assert_eq!(made + slowed, 11);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_second_objective_or_settings_is_too_many() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    let altar = Component::Objective(Objective { name: "altar".into(), at: [2.5, 0.0, 2.5], radius: 3.0 });
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: altar.clone() }).await;
    assert_eq!(next_entities(&mut a.ws).await.len(), 1);
    let second = Component::Objective(Objective { name: "tomb".into(), at: [3.5, 0.0, 2.5], radius: 3.0 });
    send(&mut a.ws, &ToServer::Put { id: 2, entity: None, expect: None, component: second }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::TooMany));
    // the one world has one Settings too: a second is refused
    let settings = Component::Settings(vox_core::entity::Settings { json: "{}".into() });
    send(&mut a.ws, &ToServer::Put { id: 3, entity: None, expect: None, component: settings }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (3, EntityWhy::TooMany));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_changing_the_settings_sky_reaches_a_second_player() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(yard_config_asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    let mut b = join(&url, "Bob", Some(token(8, "Bob"))).await;
    let (id, seq, mut json) = a
        .entities
        .iter()
        .find_map(|e| match &e.component {
            Some(Component::Settings(s)) => Some((e.entity, e.seq, serde_json::from_str::<serde_json::Value>(&s.json).unwrap())),
            _ => None,
        })
        .expect("the yard's settings");
    json["sky"] = serde_json::json!("dusk");
    let changed = Component::Settings(vox_core::entity::Settings { json: json.to_string() });
    send(&mut a.ws, &ToServer::Put { id: 1, entity: Some(id), expect: Some(seq), component: changed }).await;
    let seen = next_entities(&mut b.ws).await;
    assert_eq!(seen.len(), 1);
    match &seen[0].component {
        Some(Component::Settings(s)) => assert!(s.json.contains("dusk"), "{}", s.json),
        other => panic!("{other:?}"),
    }
    // and whoever arrives next is welcomed with it
    let late = join(&url, "Cat", Some(token(10, "Cat"))).await;
    assert!(late.settings.contains("dusk"), "{}", late.settings);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_dungeon_doorway_travels_by_name() {
    // no directory here: a trip is answered at once, without a ticket
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    let to = Dest::Dungeon { name: "crypt".into(), arrive: [1.0, 0.0, 1.5] };
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("crypt", [2.5, 0.0, 2.5], to) }).await;
    assert_eq!(next_entities(&mut a.ws).await.len(), 1);
    send(&mut a.ws, &ToServer::Through { portal: "crypt".into(), side: 1 }).await;
    let (url_got, ticket, _arrive) = until(&mut a.ws, |m| match m {
        ToClient::Travel { url, ticket, arrive } => Some((url, ticket, arrive)),
        _ => None,
    })
    .await;
    assert_eq!(url_got, "dungeon:crypt");
    assert_eq!(ticket, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_doorway_in_place_answers_no_travel() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    let here = Dest::Here { arrive: [1.0, 0.0, 1.5] };
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("inner", [2.5, 0.0, 2.5], here) }).await;
    assert_eq!(next_entities(&mut a.ws).await.len(), 1);
    send(&mut a.ws, &ToServer::Through { portal: "inner".into(), side: 1 }).await;
    // a refused edit after it is answered in order, so anything the walk sent came first
    send(&mut a.ws, &ToServer::Edit { id: 9, cell: [0, -70, 0], block: 1 }).await;
    let mut travelled = false;
    until(&mut a.ws, |m| match m {
        ToClient::Travel { .. } => {
            travelled = true;
            None
        }
        ToClient::Refused { id: 9, why: Why::Outside } => Some(()),
        _ => None,
    })
    .await;
    assert!(!travelled, "a doorway within the world sent a traveller away");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_rights_are_asked_once_and_refused_without_an_answer() {
    let asks = Arc::new(AtomicUsize::new(0));
    let dir = directory(asks.clone()).await;
    let (_server, url) = serve(asking(&dir)).await;
    // account 7 is an editor (the directory says so): asked once, then kept for a few seconds
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    for id in 1..=2u32 {
        let at = [1.5 + id as f32, 0.0, 2.5];
        send(&mut a.ws, &ToServer::Put { id, entity: None, expect: None, component: portal("p", at, url_to(":7781")) }).await;
        assert_eq!(next_entities(&mut a.ws).await.len(), 1);
    }
    assert_eq!(asks.load(Ordering::SeqCst), 1, "the answer is kept, not asked again");
    // account 9's check fails: the edit is refused, and each try asks again (nothing is kept)
    let mut n = join(&url, "Nia", Some(token(9, "Nia"))).await;
    send(&mut n.ws, &pose(n.spawn)).await;
    for id in 1..=2u32 {
        send(&mut n.ws, &ToServer::Put { id, entity: None, expect: None, component: portal("q", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
        assert_eq!(next_refusal(&mut n.ws).await, (id, EntityWhy::Unavailable));
    }
    assert_eq!(asks.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_settings_entity_cannot_become_a_spawn() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(yard_config_asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    let (id, seq) = a
        .entities
        .iter()
        .find_map(|e| matches!(e.component, Some(Component::Settings(_))).then_some((e.entity, e.seq)))
        .expect("the yard's settings");
    let spawn = Component::Spawn(vox_core::entity::Spawn { at: a.spawn, yaw: 0.0 });
    send(&mut a.ws, &ToServer::Put { id: 1, entity: Some(id), expect: Some(seq), component: spawn }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NotYours));
    // the settings entity is still settings, at the same edit number
    let b = join(&url, "Bob", Some(token(8, "Bob"))).await;
    let kept = b.entities.iter().find(|e| e.entity == id).expect("the settings entity is still there");
    assert!(matches!(kept.component, Some(Component::Settings(_))), "{:?}", kept.component);
    assert_eq!(kept.seq, seq);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_second_edit_while_one_is_held_is_too_fast() {
    let dir = directory(Arc::default()).await;
    let (_server, url) = serve(asking(&dir)).await;
    // account 5's answer is slow, so the first edit is held while the second arrives
    let mut a = join(&url, "Abe", Some(token(5, "Abe"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("p", [1.5, 0.0, 2.5], url_to(":7781")) }).await;
    send(&mut a.ws, &ToServer::Put { id: 2, entity: None, expect: None, component: portal("q", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    // the second is refused at once; the first is made when the answer comes
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::TooFast));
    assert_eq!(next_entities(&mut a.ws).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_a_held_edit_is_dropped_when_the_player_leaves() {
    let dir = directory(Arc::default()).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Abe", Some(token(5, "Abe"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: portal("left", [1.5, 0.0, 2.5], url_to(":7781")) }).await;
    // leave while the directory is still answering (the answer is slow): the Put was taken
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(a);
    // the answer arrives for a player who is gone: nothing is applied, and the world carries on
    tokio::time::sleep(Duration::from_secs(2)).await;
    let b = join(&url, "Bob", Some(token(8, "Bob"))).await;
    assert!(
        b.entities.iter().all(|e| !matches!(&e.component, Some(Component::Portal(p)) if p.name == "left")),
        "a held edit was applied after its player left"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_of_a_missing_node_is_changed() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    // no entity has this id, so there is no node to build (editor_build.rs builds real ones);
    // an entity that is not a node would be Invalid instead
    send(&mut a.ws, &ToServer::Build { id: 6, node: EntityId(999_999) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (6, EntityWhy::Changed));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_put_the_sender_alone_is_told_it_is_done() {
    let dir = directory(Arc::new(AtomicUsize::new(0))).await;
    let (_server, url) = serve(asking(&dir)).await;
    let mut a = join(&url, "Ann", Some(token(7, "Ann"))).await;
    let mut b = join(&url, "Bob", Some(token(8, "Bob"))).await;
    send(&mut a.ws, &pose(a.spawn)).await;

    send(&mut a.ws, &ToServer::Put { id: 11, entity: None, expect: None, component: portal("door", [2.5, 0.0, 2.5], url_to(":7781")) }).await;
    let made = next_entities(&mut a.ws).await;
    assert_eq!(next_done(&mut a.ws).await, (11, made[0].entity, made[0].seq), "the echo names the broadcast change");

    send(&mut a.ws, &ToServer::Remove { id: 12, entity: made[0].entity, expect: Some(made[0].seq) }).await;
    let gone = next_entities(&mut a.ws).await;
    assert_eq!(next_done(&mut a.ws).await, (12, made[0].entity, gone[0].seq));

    // a refused edit gets a refusal and no Done; the other player never gets one
    send(&mut a.ws, &ToServer::Remove { id: 13, entity: EntityId(999_999), expect: Some(0) }).await;
    let mut saw_done = false;
    let refused = until(&mut a.ws, |m| match m {
        ToClient::EntityDone { .. } => {
            saw_done = true;
            None
        }
        ToClient::EntityRefused { id, why } => Some((id, why)),
        _ => None,
    })
    .await;
    assert_eq!(refused, (13, EntityWhy::Changed));
    assert!(!saw_done, "a refused edit is not done");
    while let Ok(Some(Ok(Message::Binary(bytes)))) = tokio::time::timeout(Duration::from_millis(200), b.ws.next()).await {
        assert!(!matches!(wire::decode::<ToClient>(&bytes).unwrap(), ToClient::EntityDone { .. }), "Done goes to the sender only");
    }
}
