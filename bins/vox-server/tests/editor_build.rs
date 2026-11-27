//! Building a node over real WebSockets (docs/design/world-editor.md, "Graph nodes in the
//! editor" and "Security"): an editor's build lands for everyone as ordinary edits, a builder may
//! not build, a recipe over the cap is refused, one build starts per ten seconds, another player's
//! edit lands while a big build is still going, and the edit log holds the built blocks.

use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    Chunk, ChunkKey,
    auth::{self, Character, Claims, SigningKey},
    entity::{Component, EntityId, EntityWhy, Node},
    wire::{self, EntityChange, PROTOCOL, ToClient, ToServer},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor of grass at y = -1, as the other editor tests use.
const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] } ] }"#;

/// A hollow house of brick-and-wood-sized boxes (vox-art's house test shape: 482 voxels), with a
/// gable roof and a door. Its materials are block names, as a world-scale recipe's must be.
const HOUSE: &str = r#"{
    "version": 1, "scale": "world", "seed": 1, "size": [10, 12, 8],
    "ops": [
        { "box": { "from": [0, 0, 0], "to": [9, 5, 7], "mat": "stone", "tone": 0, "hollow": true } },
        { "roof": { "over": [0, 0, 9, 7], "at_y": 6, "style": "gable", "along_x": true, "mat": "planks", "tone": 0 } },
        { "cut": { "from": [4, 0, 0], "to": [5, 2, 0] } }
    ]
}"#;

/// The house's footprint, and how many of its cells are blocks.
const HOUSE_AT: [i32; 3] = [0, 0, 0];
const HOUSE_SIZE: [u16; 3] = [10, 12, 8];
const HOUSE_BLOCKS: usize = 482;

/// A solid cube of 22 by 22 by 22: 10,648 blocks, more than one tick applies.
const CUBE_SIZE: [u16; 3] = [22, 22, 22];
const CUBE_BLOCKS: usize = 10_648;

fn cube_recipe(side: u16) -> String {
    let last = side - 1;
    format!(
        r#"{{ "version": 1, "scale": "world", "seed": 1, "size": [{side}, {side}, {side}],
            "ops": [ {{ "box": {{ "from": [0, 0, 0], "to": [{last}, {last}, {last}], "mat": "stone", "tone": 0, "hollow": false }} }} ] }}"#
    )
}

fn key() -> SigningKey {
    auth::key_from_seed([9; 32])
}

/// A world that takes sign-in from `key`, asking the directory about advanced rights.
fn config(dir: &str) -> Config {
    let mut c = Config::new(":memory:", ["grass", "stone", "planks"].map(String::from).to_vec());
    c.name = "test".into();
    c.seed = Some(LEVEL.into());
    c.directory = Some(key().verifying_key());
    c.trust_moves = true;
    c.directory_url = Some(dir.into());
    c.world_secret = Some("s3cret".into());
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

/// A stand-in directory: a world named `test` with no plots, and an editor check that says yes
/// for account 7 and no for everyone else.
async fn directory() -> String {
    let app = axum::Router::new()
        .route(
            "/api/world/test/rules",
            axum::routing::get(|| async { Json(serde_json::json!({ "plots": [], "portals": [] })) }),
        )
        .route(
            "/api/world/test/editor",
            axum::routing::get(move |Query(q): Query<HashMap<String, String>>| async move {
                match q.get("account").map(String::as_str) {
                    Some("9") => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                    account => Json(serde_json::json!({ "editor": account == Some("7") })).into_response(),
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

async fn next_entities(ws: &mut Ws) -> Vec<EntityChange> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let m = ws.next().await.expect("closed").expect("socket error");
            if let Message::Binary(b) = m
                && let ToClient::Entities(batch) = wire::decode::<ToClient>(&b).unwrap()
            {
                return batch;
            }
        }
    })
    .await
    .expect("timed out")
}

/// What a player has been told of the blocks: single edits by cell, and whole chunks as they
/// were sent. Every block is set once by a build, so the two never disagree about one cell.
#[derive(Default)]
struct Seen {
    edits: HashMap<[i32; 3], (u64, u16)>,
    chunks: HashMap<ChunkKey, Chunk>,
}

impl Seen {
    fn fold(&mut self, m: &ToClient) {
        match m {
            ToClient::Edits(list) => {
                for a in list {
                    self.edits.insert(a.cell, (a.seq, a.block));
                }
            }
            ToClient::Chunk { key, version, data } => {
                if let Ok(c) = Chunk::decode(data, *version) {
                    // a whole chunk is the state of every cell in it, so older single edits are stale
                    self.edits.retain(|cell, _| ChunkKey::of(*cell) != *key);
                    self.chunks.insert(*key, c);
                }
            }
            _ => {}
        }
    }

    fn at(&self, cell: [i32; 3]) -> u16 {
        if let Some((_, block)) = self.edits.get(&cell) {
            return *block;
        }
        self.chunks.get(&ChunkKey::of(cell)).map_or(0, |c| c.get(cell))
    }

    /// How many cells in the box at `min` of `size` are blocks.
    fn solid_in(&self, min: [i32; 3], size: [u16; 3]) -> usize {
        let mut n = 0;
        for x in min[0]..min[0] + i32::from(size[0]) {
            for y in min[1]..min[1] + i32::from(size[1]) {
                for z in min[2]..min[2] + i32::from(size[2]) {
                    if self.at([x, y, z]) != 0 {
                        n += 1;
                    }
                }
            }
        }
        n
    }
}

/// Reads `ws`, folding every block change into `seen`, until the answer to request `id` (its
/// `EntityDone` or `EntityRefused`) comes. That answer is returned.
async fn until_answer(ws: &mut Ws, seen: &mut Seen, id: u32) -> ToClient {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let m = ws.next().await.expect("closed").expect("socket error");
            let Message::Binary(b) = m else { continue };
            let m = wire::decode::<ToClient>(&b).unwrap();
            seen.fold(&m);
            match &m {
                ToClient::EntityDone { id: i, .. } | ToClient::EntityRefused { id: i, .. } if *i == id => return m,
                _ => {}
            }
        }
    })
    .await
    .expect("timed out")
}

/// Reads `ws`, folding block changes into `seen`, until the server's next `Notice`; returns its text.
async fn next_notice(ws: &mut Ws, seen: &mut Seen) -> String {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let m = ws.next().await.expect("closed").expect("socket error");
            let Message::Binary(b) = m else { continue };
            let m = wire::decode::<ToClient>(&b).unwrap();
            seen.fold(&m);
            if let ToClient::Notice { text } = m {
                return text;
            }
        }
    })
    .await
    .expect("timed out")
}

/// Reads `ws` into `seen` until `done` holds of it (within five seconds).
async fn read_until(ws: &mut Ws, seen: &mut Seen, done: impl Fn(&Seen) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done(seen) {
            let m = ws.next().await.expect("closed").expect("socket error");
            if let Message::Binary(b) = m {
                seen.fold(&wire::decode::<ToClient>(&b).unwrap());
            }
        }
    })
    .await
    .expect("timed out");
}

/// Joins as `name` (with `token`) and reads the world as it is sent, up to Ready.
async fn join(url: &str, name: &str, token: String) -> Ws {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token: Some(token) }).await;
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timed out").expect("closed").expect("socket error");
        let Message::Binary(b) = m else { continue };
        match wire::decode::<ToClient>(&b).unwrap() {
            ToClient::Ready => break,
            ToClient::Bye { why } => panic!("refused: {why}"),
            _ => {}
        }
    }
    ws
}

fn pose(at: [f32; 3]) -> ToServer {
    ToServer::Pose { at, blob: vec![] }
}

fn node(kind: &str, at: [i32; 3], size: [u16; 3], recipe: String) -> Component {
    Component::Node(Node { kind: kind.into(), detail: 4, at, size, parent: None, recipe: Some(recipe), seed: 1 })
}

/// An editor places a node and returns its entity id; the node is in reach of `at`.
async fn place_node(ws: &mut Ws, request: u32, component: Component) -> EntityId {
    send(ws, &ToServer::Put { id: request, entity: None, expect: None, component }).await;
    let made = next_entities(ws).await;
    assert_eq!(made.len(), 1);
    made[0].entity
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_a_house_is_built_and_a_second_player_sees_it() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    let mut viewer = join(&url, "Vi", token(8, "Vi")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    send(&mut viewer, &pose([0.5, 0.0, 0.5])).await;

    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    send(&mut ed, &ToServer::Build { id: 2, node: house }).await;
    let mut seen_ed = Seen::default();
    let answer = until_answer(&mut ed, &mut seen_ed, 2).await;
    assert!(matches!(answer, ToClient::EntityDone { entity, .. } if entity == house), "{answer:?}");
    assert_eq!(seen_ed.solid_in(HOUSE_AT, HOUSE_SIZE), HOUSE_BLOCKS);

    let mut seen_vi = Seen::default();
    read_until(&mut viewer, &mut seen_vi, |s| s.solid_in(HOUSE_AT, HOUSE_SIZE) == HOUSE_BLOCKS).await;
    assert_eq!(seen_vi.solid_in(HOUSE_AT, HOUSE_SIZE), HOUSE_BLOCKS);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_a_recipe_over_the_cap_is_too_big() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    // 40 cubed is 64,000 blocks, past the 32,768 a build may change
    let big = place_node(&mut ed, 1, node("tower", [0, 0, 0], [40, 40, 40], cube_recipe(40))).await;
    send(&mut ed, &ToServer::Build { id: 2, node: big }).await;
    let mut seen = Seen::default();
    assert_eq!(until_answer(&mut ed, &mut seen, 2).await, ToClient::EntityRefused { id: 2, why: EntityWhy::TooBig });
    assert_eq!(seen.solid_in([0, 0, 0], [40, 40, 40]), 0, "nothing is built");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_a_second_build_within_ten_seconds_is_too_fast() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    let mut seen = Seen::default();
    send(&mut ed, &ToServer::Build { id: 2, node: house }).await;
    assert!(matches!(until_answer(&mut ed, &mut seen, 2).await, ToClient::EntityDone { .. }));
    send(&mut ed, &ToServer::Build { id: 3, node: house }).await;
    assert_eq!(until_answer(&mut ed, &mut seen, 3).await, ToClient::EntityRefused { id: 3, why: EntityWhy::TooFast });
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_a_builder_may_not_build() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    let mut builder = join(&url, "Bo", token(8, "Bo")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    send(&mut builder, &pose([0.5, 0.0, 0.5])).await;
    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    send(&mut builder, &ToServer::Build { id: 2, node: house }).await;
    let mut seen = Seen::default();
    assert_eq!(until_answer(&mut builder, &mut seen, 2).await, ToClient::EntityRefused { id: 2, why: EntityWhy::NotYours });
    assert_eq!(seen.solid_in(HOUSE_AT, HOUSE_SIZE), 0, "nothing is built");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_another_players_edit_lands_while_a_big_build_goes_on() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    let mut other = join(&url, "Ot", token(8, "Ot")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    send(&mut other, &pose([0.5, 0.0, 0.5])).await;
    let cube = place_node(&mut ed, 1, node("tower", [0, 0, 0], CUBE_SIZE, cube_recipe(22))).await;

    send(&mut ed, &ToServer::Build { id: 2, node: cube }).await;
    // The other player's edit is sent once the build's first block has been seen (an Edits or a
    // Chunk message for it), so it is made while the build is still going.
    let mut seen = Seen::default();
    tokio::time::timeout(Duration::from_secs(5), async {
        while seen.edits.is_empty() && seen.chunks.is_empty() {
            let m = ed.next().await.expect("closed").expect("socket error");
            if let Message::Binary(b) = m {
                seen.fold(&wire::decode::<ToClient>(&b).unwrap());
            }
        }
    })
    .await
    .expect("the build's first block never came");
    // one block, outside the cube, by the other player during the build
    let edit_at = [-2, 0, -2];
    send(&mut other, &ToServer::Edit { id: 9, cell: edit_at, block: 3 }).await;

    let answer = until_answer(&mut ed, &mut seen, 2).await;
    let ToClient::EntityDone { seq: done_seq, .. } = answer else { panic!("{answer:?}") };
    // The build's blocks take consecutive edit numbers, with the other player's one edit
    // among them, so the build's first block is CUBE_BLOCKS numbers before the last one.
    let first_build_seq = done_seq - CUBE_BLOCKS as u64;
    let (edit_seq, block) = *seen.edits.get(&edit_at).expect("the other player's edit was not seen");
    assert_eq!(block, 3);
    assert!(
        first_build_seq < edit_seq && edit_seq < done_seq,
        "the edit (seq {edit_seq}) falls between the build's first block ({first_build_seq}) and its last ({done_seq})"
    );
    assert_eq!(seen.at(edit_at), 3, "the other player's edit is in before the build is done");
    assert_eq!(seen.solid_in([0, 0, 0], CUBE_SIZE), CUBE_BLOCKS);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_build_the_built_blocks_are_in_the_edit_log() {
    let path = std::env::temp_dir().join(format!(
        "editor_build-{}-{}.db",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ));
    let mut cfg = config(&directory().await);
    cfg.db = path.clone();
    let (server, url) = serve(cfg).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    send(&mut ed, &ToServer::Build { id: 2, node: house }).await;
    let mut seen = Seen::default();
    assert!(matches!(until_answer(&mut ed, &mut seen, 2).await, ToClient::EntityDone { .. }));
    server.shutdown().await;

    let db = rusqlite::Connection::open(&path).unwrap();
    let (rows, named): (i64, i64) = db
        .query_row(
            "SELECT COUNT(*), SUM(player = 'Ed' AND new != 0 AND x BETWEEN 0 AND 9 AND y BETWEEN 0 AND 11 AND z BETWEEN 0 AND 7)
             FROM edits WHERE player = 'Ed'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    drop(db);
    let _ = std::fs::remove_file(&path);
    assert_eq!(rows as usize, HOUSE_BLOCKS);
    assert_eq!(named as usize, HOUSE_BLOCKS);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_unbuild_restores_the_blocks_a_build_put_there() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    send(&mut ed, &ToServer::Build { id: 2, node: house }).await;
    let mut seen = Seen::default();
    assert!(matches!(until_answer(&mut ed, &mut seen, 2).await, ToClient::EntityDone { .. }));
    assert_eq!(seen.solid_in(HOUSE_AT, HOUSE_SIZE), HOUSE_BLOCKS);

    send(&mut ed, &ToServer::Unbuild { id: 3, build: 2 }).await;
    assert!(matches!(until_answer(&mut ed, &mut seen, 3).await, ToClient::EntityDone { id: 3, .. }));
    assert_eq!(next_notice(&mut ed, &mut seen).await, format!("Undid the build: {HOUSE_BLOCKS} blocks back; 0 changed since were left"));
    assert_eq!(seen.solid_in(HOUSE_AT, HOUSE_SIZE), 0, "every block is back to what it was");

    // the kept build was used up by its undo
    send(&mut ed, &ToServer::Unbuild { id: 4, build: 2 }).await;
    assert_eq!(until_answer(&mut ed, &mut seen, 4).await, ToClient::EntityRefused { id: 4, why: EntityWhy::Changed });
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_unbuild_leaves_a_cell_another_player_changed_and_counts_it() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    let mut other = join(&url, "Ot", token(8, "Ot")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    send(&mut other, &pose([-1.5, 0.0, 3.5])).await;
    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    send(&mut ed, &ToServer::Build { id: 2, node: house }).await;
    let mut seen = Seen::default();
    assert!(matches!(until_answer(&mut ed, &mut seen, 2).await, ToClient::EntityDone { .. }));

    // a wall cell of the house, replaced by planks (block 3) by someone else
    let wall = [0, 1, 3];
    assert_ne!(seen.at(wall), 0);
    send(&mut other, &ToServer::Edit { id: 9, cell: wall, block: 3 }).await;
    read_until(&mut ed, &mut seen, |s| s.at(wall) == 3).await;

    send(&mut ed, &ToServer::Unbuild { id: 3, build: 2 }).await;
    assert!(matches!(until_answer(&mut ed, &mut seen, 3).await, ToClient::EntityDone { .. }));
    let notice = next_notice(&mut ed, &mut seen).await;
    assert_eq!(notice, format!("Undid the build: {} blocks back; 1 changed since were left", HOUSE_BLOCKS - 1));
    assert_eq!(seen.at(wall), 3, "the other player's block is still there");
    assert_eq!(seen.solid_in(HOUSE_AT, HOUSE_SIZE), 1, "nothing else of the house is left");
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_unbuild_of_an_unknown_build_is_refused() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    let mut seen = Seen::default();
    send(&mut ed, &ToServer::Unbuild { id: 1, build: 99 }).await;
    assert_eq!(until_answer(&mut ed, &mut seen, 1).await, ToClient::EntityRefused { id: 1, why: EntityWhy::Changed });
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_unbuild_of_another_players_build_is_refused() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    let mut other = join(&url, "Ot", token(7, "Ot")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    let house = place_node(&mut ed, 1, node("house", HOUSE_AT, HOUSE_SIZE, HOUSE.into())).await;
    send(&mut ed, &ToServer::Build { id: 2, node: house }).await;
    let mut seen = Seen::default();
    assert!(matches!(until_answer(&mut ed, &mut seen, 2).await, ToClient::EntityDone { .. }));
    // the same build number on another connection (even of the same account) is not theirs
    send(&mut other, &ToServer::Unbuild { id: 2, build: 2 }).await;
    let mut seen_ot = Seen::default();
    assert_eq!(until_answer(&mut other, &mut seen_ot, 2).await, ToClient::EntityRefused { id: 2, why: EntityWhy::Changed });
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_unbuild_is_refused_while_a_build_is_running() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    let cube = place_node(&mut ed, 1, node("tower", [0, 0, 0], CUBE_SIZE, cube_recipe(22))).await;
    send(&mut ed, &ToServer::Build { id: 2, node: cube }).await;
    send(&mut ed, &ToServer::Unbuild { id: 3, build: 2 }).await;
    let mut seen = Seen::default();
    assert_eq!(until_answer(&mut ed, &mut seen, 3).await, ToClient::EntityRefused { id: 3, why: EntityWhy::Unavailable });
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_unbuild_another_players_edit_lands_while_it_goes_on() {
    let (_server, url) = serve(config(&directory().await)).await;
    let mut ed = join(&url, "Ed", token(7, "Ed")).await;
    let mut other = join(&url, "Ot", token(8, "Ot")).await;
    send(&mut ed, &pose([0.5, 0.0, 0.5])).await;
    send(&mut other, &pose([0.5, 0.0, 0.5])).await;
    let cube = place_node(&mut ed, 1, node("tower", [0, 0, 0], CUBE_SIZE, cube_recipe(22))).await;
    send(&mut ed, &ToServer::Build { id: 2, node: cube }).await;
    let mut seen = Seen::default();
    let built = until_answer(&mut ed, &mut seen, 2).await;
    let ToClient::EntityDone { seq: built_seq, .. } = built else { panic!("{built:?}") };
    assert_eq!(seen.solid_in([0, 0, 0], CUBE_SIZE), CUBE_BLOCKS);

    send(&mut ed, &ToServer::Unbuild { id: 3, build: 2 }).await;
    // once the undo has begun (some block of the cube is gone), the other player edits outside it
    read_until(&mut ed, &mut seen, |s| s.solid_in([0, 0, 0], CUBE_SIZE) < CUBE_BLOCKS).await;
    let edit_at = [-2, 0, -2];
    send(&mut other, &ToServer::Edit { id: 9, cell: edit_at, block: 3 }).await;

    let answer = until_answer(&mut ed, &mut seen, 3).await;
    let ToClient::EntityDone { seq: done_seq, .. } = answer else { panic!("{answer:?}") };
    let (edit_seq, block) = *seen.edits.get(&edit_at).expect("the other player's edit was not seen");
    assert_eq!(block, 3);
    assert!(
        built_seq < edit_seq && edit_seq < done_seq,
        "the edit (seq {edit_seq}) falls within the undo ({built_seq}..{done_seq})"
    );
    assert_eq!(seen.solid_in([0, 0, 0], CUBE_SIZE), 0);
}
