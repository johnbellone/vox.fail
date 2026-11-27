//! Props placed in a world (docs/design/world-editor.md, phase 4; docs/design/props.md,
//! "Placing"): a known prop an editor places reaches everyone; an unknown prop, one standing in a
//! solid block, and one overlapping another that collides are refused; a rug may share cells; a
//! move may cover its own old cells; a corrupted blob is left out and the rest still load; a
//! builder's plot holds 64 props and no more (a move into a full plot is refused, a move within
//! one is not); a builder removes its own prop; and an orphan prop (its hash left the catalogue)
//! takes no cells, cannot be moved, and can be removed.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use std::collections::HashMap;

use axum::{Json, extract::Query};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_art::{PropDef, VoxelGrid};
use vox_core::{
    auth::{self, Character, Claims, SigningKey},
    entity::{Component, EntityId, EntityWhy, Prop},
    wire::{self, EntityChange, PROTOCOL, ToClient, ToServer},
};
use vox_format::{
    Hash,
    prop::{Catalogue, CatalogueEntry, PropBlob},
};
use vox_server::{Config, Server};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A floor of grass at y = -1 and a spawn on it (as the other editor tests use).
const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] } ] }"#;

/// A crate: one floor cell, two blocks up, and it collides.
const CRATE: &str = r#"{ "id": "crate.box", "name": "Crate", "scale": "prop", "size": [4, 4, 4], "pivot": [1, 0, 1],
    "place": { "on": ["floor"], "footprint": [[0, 0]], "height": 2 }, "collide": "footprint", "palette_version": 1 }"#;

/// A long crate: two floor cells, and it collides.
const LONG: &str = r#"{ "id": "crate.long", "name": "Long crate", "scale": "prop", "size": [4, 4, 4], "pivot": [1, 0, 1],
    "place": { "on": ["floor"], "footprint": [[0, 0], [1, 0]], "height": 2 }, "collide": "footprint", "palette_version": 1 }"#;

/// A rug: one floor cell, and it does not collide.
const RUG: &str = r#"{ "id": "rug.round", "name": "Rug", "scale": "prop", "size": [4, 1, 4], "pivot": [1, 0, 1],
    "place": { "on": ["floor"], "footprint": [[0, 0]], "height": 1 }, "collide": "none", "palette_version": 1 }"#;

fn key() -> SigningKey {
    auth::key_from_seed([9; 32])
}

/// A world that takes sign-in from `key` and reads its props from `props`. Advanced rights are
/// asked of a directory (see `world`).
fn config(props: &std::path::Path) -> Config {
    let mut c = Config::new(":memory:", ["grass", "stone", "planks"].map(String::from).to_vec());
    c.name = "test".into();
    c.seed = Some(LEVEL.into());
    c.directory = Some(key().verifying_key());
    c.trust_moves = true;
    c.props = props.to_path_buf();
    c
}

/// A world with the stand-in directory at `fetches`' side: account 7 is an editor, 8 a builder.
async fn world(f: &Folder, fetches: Arc<AtomicUsize>) -> (Server, String) {
    let mut c = config(&f.dir);
    c.directory_url = Some(directory(fetches).await);
    c.world_secret = Some("s3cret".into());
    serve(c).await
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
}

/// Joins as `name` with `token`, and reads the world until it is ready.
async fn join(url: &str, name: &str, token: String) -> Who {
    join_as(url, name, Some(token)).await
}

/// Joins as `name`, signed in with `token` or as a guest (None), and reads the world until it is ready.
async fn join_as(url: &str, name: &str, token: Option<String>) -> Who {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token }).await;
    let mut spawn = None;
    let mut ready = false;
    while spawn.is_none() || !ready {
        match until(&mut ws, Some).await {
            ToClient::Welcome { spawn: s, .. } => spawn = Some(s),
            ToClient::Ready => ready = true,
            ToClient::Bye { why } => panic!("refused: {why}"),
            _ => {}
        }
    }
    while let Ok(Some(Ok(_))) = tokio::time::timeout(Duration::from_millis(200), ws.next()).await {}
    Who { ws, spawn: spawn.unwrap() }
}

fn pose(at: [f32; 3]) -> ToServer {
    ToServer::Pose { at, blob: vec![] }
}

/// A prop of `hash` at `at` (5 cm units) owned by `owner`.
fn prop(hash: [u8; 16], at: [i32; 3], yaw: u8, owner: u64) -> Component {
    Component::Prop(Prop { prop: hash, at, yaw, variant: 0, state: 0, channel: None, text: None, owner: Some(owner) })
}

/// A prop's blob and the name it is kept under, from its definition and its voxels.
fn make(json: &str, voxels: &[([u16; 3], u8)]) -> (Vec<u8>, Hash) {
    let def: PropDef = serde_json::from_str(json).unwrap();
    let mut grid = VoxelGrid::new(def.scale.scale(), def.size).unwrap();
    for &(at, index) in voxels {
        grid.set(at, index).unwrap();
    }
    PropBlob { def, grid }.encode().unwrap()
}

/// A props folder for one world: the catalogue, and a blob for each prop. With `corrupt_long`,
/// the long crate's blob has its magic changed, so it is not a prop.
struct Folder {
    dir: PathBuf,
    crate_box: [u8; 16],
    long: [u8; 16],
    rug: [u8; 16],
}

fn folder(tag: &str, corrupt_long: bool) -> Folder {
    let dir = std::env::temp_dir().join(format!("vox-editor-prop-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (crate_bytes, crate_hash) = make(CRATE, &[([1, 0, 1], 1), ([2, 2, 2], 3)]);
    let (mut long_bytes, long_hash) = make(LONG, &[([1, 0, 1], 1), ([2, 0, 1], 2)]);
    let (rug_bytes, rug_hash) = make(RUG, &[([1, 0, 1], 1)]);
    if corrupt_long {
        long_bytes[0] = b'X';
    }
    let mut cat = Catalogue::default();
    for (id, name, hash, bytes) in [
        ("crate.box", "Crate", crate_hash, crate_bytes),
        ("crate.long", "Long crate", long_hash, long_bytes),
        ("rug.round", "Rug", rug_hash, rug_bytes),
    ] {
        std::fs::write(dir.join(format!("{}.voxp", hash.hex())), bytes).unwrap();
        cat.props.insert(id.into(), CatalogueEntry { hash, name: name.into(), tags: vec![] });
    }
    std::fs::write(dir.join("catalogue.json"), cat.encode().unwrap()).unwrap();
    Folder { dir, crate_box: crate_hash.0, long: long_hash.0, rug: rug_hash.0 }
}

/// A stand-in directory with a plot round the origin whose builder is account 8; account 7 is
/// an editor and no one else is. Counts the rules fetches.
async fn directory(fetches: Arc<AtomicUsize>) -> String {
    directory_with(fetches, json!([{ "id": 1, "x0": 0, "z0": 0, "x1": 10, "z1": 10, "builders": [8] }])).await
}

/// The stand-in directory with `plots` as its rules' plots (see `directory`).
async fn directory_with(fetches: Arc<AtomicUsize>, plots: serde_json::Value) -> String {
    let app = axum::Router::new()
        .route(
            "/api/world/test/rules",
            axum::routing::get(move || {
                let fetches = fetches.clone();
                let plots = plots.clone();
                async move {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    Json(json!({
                        "plots": plots,
                        "portals": [],
                        "parties": [],
                    }))
                }
            }),
        )
        .route(
            "/api/world/test/editor",
            axum::routing::get(|Query(q): Query<HashMap<String, String>>| async move {
                Json(json!({ "editor": q.get("account").map(String::as_str) == Some("7") }))
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

async fn wait_for(mut done: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out")
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_known_prop_reaches_a_second_player() {
    let f = folder("reach", false);
    let (_server, url) = world(&f, Arc::default()).await;
    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    let mut b = join(&url, "Bob", token(8, "Bob")).await;
    send(&mut a.ws, &pose(a.spawn)).await;

    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.crate_box, [25, 0, 25], 0, 7) }).await;
    let made = next_entities(&mut a.ws).await;
    assert_eq!(made.len(), 1);
    assert_eq!(next_entities(&mut b.ws).await, made, "the second player is told the same change");
    assert_eq!(next_done(&mut a.ws).await.0, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_an_unknown_prop_is_no_such_prop() {
    let f = folder("unknown", false);
    let (_server, url) = world(&f, Arc::default()).await;
    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop([0xab; 16], [25, 0, 25], 0, 7) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NoSuchProp));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_footprint_in_a_solid_block_is_blocked() {
    let f = folder("solid", false);
    let (_server, url) = world(&f, Arc::default()).await;
    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    // at height -10 (block -1) the floor cell is the grass
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.crate_box, [25, -10, 25], 0, 7) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::Blocked));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_colliding_props_may_not_overlap_but_a_rug_may() {
    let f = folder("overlap", false);
    let (_server, url) = world(&f, Arc::default()).await;
    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    let cell = [25, 0, 25];

    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.crate_box, cell, 0, 7) }).await;
    assert_eq!(next_done(&mut a.ws).await.0, 1);
    // a crate on a crate
    send(&mut a.ws, &ToServer::Put { id: 2, entity: None, expect: None, component: prop(f.crate_box, cell, 0, 7) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::Blocked));
    // a rug blocks nothing, so it may share the crate's cell
    send(&mut a.ws, &ToServer::Put { id: 3, entity: None, expect: None, component: prop(f.rug, cell, 0, 7) }).await;
    assert_eq!(next_done(&mut a.ws).await.0, 3);
    // a crate still may not go where the first crate stands, rug or no rug
    send(&mut a.ws, &ToServer::Put { id: 4, entity: None, expect: None, component: prop(f.crate_box, cell, 0, 7) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (4, EntityWhy::Blocked));
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_move_may_cover_its_own_old_cells() {
    let f = folder("move", false);
    let (_server, url) = world(&f, Arc::default()).await;
    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    // the long crate covers (2, 0, 2) and (3, 0, 2)
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.long, [25, 0, 25], 0, 7) }).await;
    let (_, entity, seq) = next_done(&mut a.ws).await;
    // moved one cell along: (3, 0, 2) and (4, 0, 2), sharing (3, 0, 2) with where it was
    send(&mut a.ws, &ToServer::Put { id: 2, entity: Some(entity), expect: Some(seq), component: prop(f.long, [35, 0, 25], 0, 7) }).await;
    assert_eq!(next_done(&mut a.ws).await.0, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_corrupt_blob_is_left_out_and_the_rest_load() {
    let f = folder("corrupt", true);
    let (_server, url) = world(&f, Arc::default()).await;
    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.long, [25, 0, 25], 0, 7) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (1, EntityWhy::NoSuchProp));
    send(&mut a.ws, &ToServer::Put { id: 2, entity: None, expect: None, component: prop(f.crate_box, [45, 0, 25], 0, 7) }).await;
    assert_eq!(next_done(&mut a.ws).await.0, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_plot_holds_64_props_and_no_more() {
    let f = folder("plot", false);
    let fetches = Arc::new(AtomicUsize::new(0));
    let dir = directory(fetches.clone()).await;
    let mut cfg = config(&f.dir);
    cfg.directory_url = Some(dir);
    cfg.world_secret = Some("s3cret".into());
    let (_server, url) = serve(cfg).await;
    // the plot must be known before the builder places: the first rules fetch comes at once
    wait_for(|| fetches.load(Ordering::SeqCst) > 0).await;

    // Cy is account 8, a builder in the plot
    let mut c = join(&url, "Cy", token(8, "Cy")).await;
    send(&mut c.ws, &pose([5.0, 0.0, 5.0])).await;
    // a builder's edits are rate-limited to ten a second; one every 110 ms keeps under it
    for i in 0..65u32 {
        let (x, z) = ((i % 9) as i32, (i / 9) as i32);
        let at = [x * 10 + 5, 0, z * 10 + 5];
        send(&mut c.ws, &ToServer::Put { id: i + 1, entity: None, expect: None, component: prop(f.crate_box, at, 0, 8) }).await;
        tokio::time::sleep(Duration::from_millis(110)).await;
    }
    for i in 1..=64u32 {
        assert_eq!(next_done(&mut c.ws).await.0, i);
    }
    assert_eq!(next_refusal(&mut c.ws).await, (65, EntityWhy::TooMany));
}

/// A hash no catalogue names: a prop placed under it is an orphan once the world opens.
const ORPHAN: [u8; 16] = [0xcd; 16];
/// The entity number the orphan row is written under.
const ORPHAN_ID: u32 = 900;

/// Writes the orphan prop's row into the closed world at `db` (as the world itself would, with
/// the next edit number), and returns that number.
fn orphan_row(db: &std::path::Path) -> u64 {
    let mut c = rusqlite::Connection::open(db).unwrap();
    let seq = c
        .query_row("SELECT value FROM meta WHERE key = 'seq'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    let comp = prop(ORPHAN, [25, 0, 25], 0, 7);
    let t = c.transaction().unwrap();
    t.execute(
        "INSERT INTO entities(id, zone_x, zone_z, name, version, seq, data) VALUES (?1, 0, 0, ?2, ?3, ?4, ?5)",
        rusqlite::params![ORPHAN_ID, comp.name(), comp.version(), seq as i64, wire::encode(&comp)],
    )
    .unwrap();
    t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('seq', ?1)", [seq.to_string()]).unwrap();
    t.commit().unwrap();
    seq
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_builder_removes_its_own_prop_and_places_another_in_its_cells() {
    let f = folder("remove", false);
    let fetches: Arc<AtomicUsize> = Arc::default();
    let (_server, url) = world(&f, fetches.clone()).await;
    wait_for(|| fetches.load(Ordering::SeqCst) > 0).await;

    let mut c = join(&url, "Cy", token(8, "Cy")).await;
    send(&mut c.ws, &pose([5.0, 0.0, 5.0])).await;
    send(&mut c.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.crate_box, [25, 0, 25], 0, 8) }).await;
    let (_, entity, seq) = next_done(&mut c.ws).await;
    // a builder's edits are rate-limited to ten a second
    tokio::time::sleep(Duration::from_millis(110)).await;
    send(&mut c.ws, &ToServer::Remove { id: 2, entity, expect: Some(seq) }).await;
    assert_eq!(next_done(&mut c.ws).await.0, 2);
    tokio::time::sleep(Duration::from_millis(110)).await;
    // the cells it stood in are free again
    send(&mut c.ws, &ToServer::Put { id: 3, entity: None, expect: None, component: prop(f.crate_box, [25, 0, 25], 0, 8) }).await;
    assert_eq!(next_done(&mut c.ws).await.0, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_an_orphan_takes_no_cells_cannot_move_and_can_be_removed() {
    let f = folder("orphan", false);
    let db = std::env::temp_dir().join(format!("vox-editor-prop-orphan-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let mut cfg = config(&f.dir);
    cfg.db = db.clone();
    cfg.directory_url = Some(directory(Arc::default()).await);
    cfg.world_secret = Some("s3cret".into());
    // a first run makes the tables; the orphan is written while the world is closed
    let (first, _) = serve(cfg.clone()).await;
    first.shutdown().await;
    let seq = orphan_row(&db);
    let (_server, url) = serve(cfg).await;

    let mut a = join(&url, "Ann", token(7, "Ann")).await;
    send(&mut a.ws, &pose(a.spawn)).await;
    // it takes no cells: a crate may stand where it is
    send(&mut a.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.crate_box, [25, 0, 25], 0, 7) }).await;
    assert_eq!(next_done(&mut a.ws).await.0, 1);
    // it cannot be moved
    send(&mut a.ws, &ToServer::Put { id: 2, entity: Some(EntityId(ORPHAN_ID)), expect: Some(seq), component: prop(ORPHAN, [45, 0, 25], 0, 7) }).await;
    assert_eq!(next_refusal(&mut a.ws).await, (2, EntityWhy::NoSuchProp));
    // an editor may remove it
    send(&mut a.ws, &ToServer::Remove { id: 3, entity: EntityId(ORPHAN_ID), expect: Some(seq) }).await;
    assert_eq!(next_done(&mut a.ws).await.0, 3);
    let _ = std::fs::remove_file(&db);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_builder_may_not_move_into_a_full_plot_but_may_move_within_it() {
    let f = folder("full", false);
    let fetches: Arc<AtomicUsize> = Arc::default();
    // plot 1 (full, below) and plot 2 are both account 8's
    let dir = directory_with(
        fetches.clone(),
        json!([
            { "id": 1, "x0": 0, "z0": 0, "x1": 10, "z1": 10, "builders": [8] },
            { "id": 2, "x0": 20, "z0": 0, "x1": 30, "z1": 10, "builders": [8] },
        ]),
    )
    .await;
    let mut cfg = config(&f.dir);
    cfg.directory_url = Some(dir);
    cfg.world_secret = Some("s3cret".into());
    let (_server, url) = serve(cfg).await;
    wait_for(|| fetches.load(Ordering::SeqCst) > 0).await;

    let mut c = join(&url, "Cy", token(8, "Cy")).await;
    send(&mut c.ws, &pose([8.0, 0.0, 5.0])).await;
    // fill plot 1 with 64 crates, one a cell, one every 110 ms to stay under the builder's rate
    for i in 0..64u32 {
        let (x, z) = ((i % 9) as i32, (i / 9) as i32);
        let at = [x * 10 + 5, 0, z * 10 + 5];
        send(&mut c.ws, &ToServer::Put { id: i + 1, entity: None, expect: None, component: prop(f.crate_box, at, 0, 8) }).await;
        tokio::time::sleep(Duration::from_millis(110)).await;
    }
    let mut placed = Vec::new();
    for i in 1..=64u32 {
        let (id, entity, seq) = next_done(&mut c.ws).await;
        assert_eq!(id, i);
        placed.push((entity, seq));
    }
    // a crate in plot 2, which has room
    tokio::time::sleep(Duration::from_millis(110)).await;
    send(&mut c.ws, &ToServer::Put { id: 65, entity: None, expect: None, component: prop(f.crate_box, [215, 0, 25], 0, 8) }).await;
    let (_, moved, moved_seq) = next_done(&mut c.ws).await;
    // moving it into full plot 1 is refused, though the cell is free
    tokio::time::sleep(Duration::from_millis(110)).await;
    send(&mut c.ws, &ToServer::Put { id: 66, entity: Some(moved), expect: Some(moved_seq), component: prop(f.crate_box, [95, 0, 95], 0, 8) }).await;
    assert_eq!(next_refusal(&mut c.ws).await, (66, EntityWhy::TooMany));
    // but a move within full plot 1 is allowed
    tokio::time::sleep(Duration::from_millis(110)).await;
    let (first, first_seq) = placed[0];
    send(&mut c.ws, &ToServer::Put { id: 67, entity: Some(first), expect: Some(first_seq), component: prop(f.crate_box, [95, 0, 95], 0, 8) }).await;
    assert_eq!(next_done(&mut c.ws).await.0, 67);
}

/// The owner of the prop the first change of `made` places.
fn owner_of(made: &[EntityChange]) -> Option<u64> {
    match &made[0].component {
        Some(Component::Prop(p)) => p.owner,
        _ => panic!("expected a prop"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_guest_on_an_editors_all_world_places_a_prop() {
    let f = folder("guest", false);
    // a local world with no directory: every guest is an editor (`--editors-all`)
    let mut c = config(&f.dir);
    c.directory = None;
    c.editors_all = true;
    let (_server, url) = serve(c).await;
    let mut g = join_as(&url, "Gus", None).await;
    send(&mut g.ws, &pose(g.spawn)).await;
    // the owner the client names is replaced by the guest's local account
    send(&mut g.ws, &ToServer::Put { id: 1, entity: None, expect: None, component: prop(f.crate_box, [25, 0, 25], 0, 99) }).await;
    let made = next_entities(&mut g.ws).await;
    assert_eq!(made.len(), 1);
    assert!(owner_of(&made).is_some_and(|o| o != 99), "the server stamps the guest's own owner");
    assert_eq!(next_done(&mut g.ws).await.0, 1);
}

/// A builder places a prop that names `sent` as its owner (or none): it comes back owned by the
/// builder, account 8, since the owner is stamped by the server.
async fn builder_places_as_its_own(tag: &str, sent: Option<u64>) {
    let f = folder(tag, false);
    let fetches: Arc<AtomicUsize> = Arc::default();
    let (_server, url) = world(&f, fetches.clone()).await;
    wait_for(|| fetches.load(Ordering::SeqCst) > 0).await;
    let mut c = join(&url, "Cy", token(8, "Cy")).await;
    send(&mut c.ws, &pose([5.0, 0.0, 5.0])).await;
    let component = Component::Prop(Prop {
        prop: f.crate_box,
        at: [25, 0, 25],
        yaw: 0,
        variant: 0,
        state: 0,
        channel: None,
        text: None,
        owner: sent,
    });
    send(&mut c.ws, &ToServer::Put { id: 1, entity: None, expect: None, component }).await;
    assert_eq!(owner_of(&next_entities(&mut c.ws).await), Some(8));
    assert_eq!(next_done(&mut c.ws).await.0, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_builder_places_a_prop_with_no_owner_as_its_own() {
    builder_places_as_its_own("stamp-none", None).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_prop_a_builder_places_a_prop_named_for_someone_else_as_its_own() {
    builder_places_as_its_own("stamp-other", Some(7)).await;
}
