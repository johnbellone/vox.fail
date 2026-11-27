//! Player portals placed by a world server (`/api/world/portal`, charged once per request id, with
//! a refund for a portal taken back within a minute), and the feat that makes one from the
//! inventory page (`/api/feats/portal`), which goes through the same code.

use std::path::PathBuf;

use reqwest::Client;
use serde_json::{Value, json};
use vox_core::auth;
use vox_directory::{Config, Directory, WorldInfo, game};

const SECRET: &str = "world-secret";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn serve() -> (Directory, String) {
    let w = |id: &str, name: &str, port: u16| WorldInfo { id: id.into(), name: name.into(), url: format!("/w/{port}/ws"), about: String::new() };
    let cfg = Config {
        db: ":memory:".into(),
        key: auth::key_from_seed([8; 32]),
        web: root().join("web"),
        assets: root().join("assets"),
        data: root().join("data"),
        worlds: vec![w("void", "The Void", 7780), w("yard", "The Yard", 7781)],
        secure_cookies: false,
        downloads: vec![],
        behind_proxy: false,
        social: Default::default(),
        world_secret: Some(SECRET.into()),
        report_every: 0,
        instance_host: None,
        atlases: vec![("earth".into(), [3, 2])],
    };
    let d = Directory::open(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = d.router().into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (d, base)
}

struct P {
    c: Client,
    base: String,
    id: u64,
}

impl P {
    async fn new(base: &str, name: &str) -> P {
        let c = Client::builder().cookie_store(true).build().unwrap();
        let r = c.post(format!("{base}/api/signup")).json(&json!({"name": name, "password": "long enough"})).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        c.put(format!("{base}/api/character")).json(&json!({"model": "models/blue.glb", "wear": []})).send().await.unwrap();
        let t: Value = c.post(format!("{base}/api/token")).json(&json!({})).send().await.unwrap().json().await.unwrap();
        let id = auth::peek(t["token"].as_str().unwrap()).unwrap().account;
        P { c, base: base.into(), id }
    }
    async fn get(&self, path: &str) -> Value {
        self.c.get(format!("{}{path}", self.base)).send().await.unwrap().json().await.unwrap()
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self.c.post(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn authority(&self) -> i64 {
        self.get("/api/wallet").await["balances"][0]["amount"].as_i64().unwrap()
    }
}

/// A world server's report: who is where (this also sets where a feat happens).
async fn report(base: &str, world: &str, players: Value) {
    let r = Client::new()
        .post(format!("{base}/api/world/report"))
        .bearer_auth(SECRET)
        .json(&json!({ "world": world, "level": world, "spawn": [0.5, 1.0, 0.5], "players": players }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
}

async fn rules(base: &str, world: &str) -> Value {
    Client::new().get(format!("{base}/api/world/{world}/rules")).bearer_auth(SECRET).send().await.unwrap().json().await.unwrap()
}

/// A world server's request for a player portal.
async fn world_portal(base: &str, body: Value) -> (u16, Value) {
    let r = Client::new().post(format!("{base}/api/world/portal")).bearer_auth(SECRET).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

async fn refund(base: &str, world: &str, account: u64, key: i64) -> (u16, Value) {
    let r = Client::new()
        .post(format!("{base}/api/world/portal/refund"))
        .bearer_auth(SECRET)
        .json(&json!({ "world": world, "account": account, "key": key }))
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

fn req(world: &str, account: u64, at: [f64; 3], to: &str, idem: &str) -> Value {
    json!({ "world": world, "account": account, "at": at, "axis": "x", "to": to, "idem": idem })
}

/// Ann has a plot in the Void (32 to 48 on x and z) and the Yard has reported; she holds
/// authority for three portals. Bob has nothing.
async fn plotted() -> (Directory, String, P, P) {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    report(&base, "void", json!([{ "account": ann.id, "at": [40.5, 1.0, 40.5] }])).await;
    let (st, v) = ann.post("/api/feats/plot", json!({"world": "void"})).await;
    assert_eq!(st, 200, "{v}");
    report(&base, "yard", json!([])).await;
    d.grant("Ann", 100, "test").unwrap();
    (d, base, ann, bob)
}

#[tokio::test]
async fn portal_charged_once_for_a_repeated_request() {
    let (_d, base, ann, bob) = plotted().await;
    let before = ann.authority().await;
    let (st, first) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "edit-7")).await;
    assert_eq!(st, 200, "{first}");
    assert_eq!(first["name"], "The Yard (".to_string() + &first["key"].to_string() + ")");
    assert_eq!(first["at"], json!([40.0, 1.0, 40.0]));
    assert_eq!(ann.authority().await, before - game::COST_PORTAL);
    // the same request again: the same answer, and nothing charged
    let (st, again) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "edit-7")).await;
    assert_eq!(st, 200, "{again}");
    assert_eq!(again, first);
    assert_eq!(ann.authority().await, before - game::COST_PORTAL);
    // another world's or player's use of the same id is refused, and charges nothing
    let (st, _) = world_portal(&base, req("void", bob.id, [40.0, 1.0, 40.0], "yard", "edit-7")).await;
    assert_eq!(st, 409);
    // the rules list the portal once, under its key
    let r = rules(&base, "void").await;
    assert_eq!(r["portals"].as_array().unwrap().len(), 1);
    assert_eq!(r["portals"][0]["key"], first["key"]);
    assert_eq!(r["portals"][0]["axis"], "x");
}

#[tokio::test]
async fn portal_cap_is_two_per_plot() {
    let (_d, base, ann, _bob) = plotted().await;
    assert_eq!(world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await.0, 200);
    assert_eq!(world_portal(&base, req("void", ann.id, [41.0, 1.0, 41.0], "yard", "b")).await.0, 200);
    let before = ann.authority().await;
    let (st, v) = world_portal(&base, req("void", ann.id, [42.0, 1.0, 42.0], "yard", "c")).await;
    assert_eq!(st, 400, "{v}");
    assert_eq!(ann.authority().await, before);
    assert_eq!(rules(&base, "void").await["portals"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn portal_must_be_in_a_plot_of_the_player() {
    let (_d, base, ann, bob) = plotted().await;
    // outside Ann's plot
    assert_eq!(world_portal(&base, req("void", ann.id, [10.0, 1.0, 10.0], "yard", "out")).await.0, 400);
    // inside her plot, but Bob has no plot there
    assert_eq!(world_portal(&base, req("void", bob.id, [40.0, 1.0, 40.0], "yard", "bob")).await.0, 400);
    // a wall along neither x nor z
    let mut bad_axis = req("void", ann.id, [40.0, 1.0, 40.0], "yard", "axis");
    bad_axis["axis"] = json!("y");
    assert_eq!(world_portal(&base, bad_axis).await.0, 400);
    assert_eq!(ann.authority().await, game::WELCOME + 100 - game::COST_PLOT);
}

#[tokio::test]
async fn portal_destination_must_be_another_known_world() {
    let (_d, base, ann, _bob) = plotted().await;
    // the same world
    assert_eq!(world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "void", "same")).await.0, 400);
    // a world the directory doesn't know
    assert_eq!(world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "nowhere", "unknown")).await.0, 400);
    // a request id that is not a plain id
    assert_eq!(world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "has space")).await.0, 400);
    assert_eq!(world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "")).await.0, 400);
}

#[tokio::test]
async fn portal_needs_authority() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    report(&base, "void", json!([{ "account": ann.id, "at": [40.5, 1.0, 40.5] }])).await;
    assert_eq!(ann.post("/api/feats/plot", json!({"world": "void"})).await.0, 200);
    report(&base, "yard", json!([])).await;
    // a plot leaves 5 authority, a portal costs 30
    assert_eq!(ann.authority().await, game::WELCOME - game::COST_PLOT);
    let (st, v) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "poor")).await;
    assert_eq!(st, 402, "{v}");
    assert!(rules(&base, "void").await["portals"].as_array().unwrap().is_empty());
    drop(d);
}

#[tokio::test]
async fn portal_refund_within_a_minute_credits_once() {
    let (_d, base, ann, bob) = plotted().await;
    let before = ann.authority().await;
    let (_, made) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "refund-me")).await;
    let key = made["key"].as_i64().unwrap();
    assert_eq!(ann.authority().await, before - game::COST_PORTAL);
    // someone else's key, or the wrong world: nothing happens
    assert_eq!(refund(&base, "void", bob.id, key).await.1, json!({"refunded": false}));
    assert_eq!(refund(&base, "yard", ann.id, key).await.1, json!({"refunded": false}));
    assert_eq!(rules(&base, "void").await["portals"].as_array().unwrap().len(), 1);
    // ours: credited, row and key gone
    assert_eq!(refund(&base, "void", ann.id, key).await.1, json!({"refunded": true}));
    assert_eq!(ann.authority().await, before);
    assert!(rules(&base, "void").await["portals"].as_array().unwrap().is_empty());
    let keys = ann.get("/api/inventory").await["items"].as_array().unwrap().iter().filter(|i| i["kind"] == "key").count();
    assert_eq!(keys, 0);
    // a second refund does nothing
    assert_eq!(refund(&base, "void", ann.id, key).await.1, json!({"refunded": false}));
    assert_eq!(ann.authority().await, before);
    // a repeat of the original request now says the portal was refunded, and charges nothing
    assert_eq!(world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "refund-me")).await.0, 409);
    assert_eq!(ann.authority().await, before);
}

#[tokio::test]
async fn portal_feat_still_opens_one_through_the_same_code() {
    let (_d, base, ann, _bob) = plotted().await;
    let before = ann.authority().await;
    let (st, v) = ann.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["at"], json!([40.5, 1.0, 40.5]));
    assert_eq!(ann.authority().await, before - game::COST_PORTAL);
    let r = rules(&base, "void").await;
    assert_eq!(r["portals"][0]["key"], v["portal"]);
    assert_eq!(r["portals"][0]["axis"], "z");
    // the feat has no request id, so a second one is charged too; the third is over the cap
    let (st, v) = ann.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ann.authority().await, before - 2 * game::COST_PORTAL);
    assert_eq!(ann.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await.0, 400);
    assert_eq!(ann.authority().await, before - 2 * game::COST_PORTAL);
}

#[tokio::test]
async fn portal_keys_are_never_reused() {
    let (_d, base, ann, _bob) = plotted().await;
    let (_, first) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "k1")).await;
    let k1 = first["key"].as_i64().unwrap();
    assert_eq!(refund(&base, "void", ann.id, k1).await.1, json!({"refunded": true}));
    // the highest key was just deleted; the next one must still be above it
    let (st, second) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "k2")).await;
    assert_eq!(st, 200, "{second}");
    let k2 = second["key"].as_i64().unwrap();
    assert!(k2 > k1, "{k2} reuses or undercuts {k1}");
    assert_eq!(refund(&base, "void", ann.id, k2).await.1, json!({"refunded": true}));
    let (st, third) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "k3")).await;
    assert_eq!(st, 200, "{third}");
    assert!(third["key"].as_i64().unwrap() > k2);
}

#[tokio::test]
async fn portal_refunded_request_does_not_leak_the_next_portal() {
    let (d, base, ann, bob) = plotted().await;
    report(&base, "void", json!([{ "account": bob.id, "at": [100.5, 1.0, 100.5] }])).await;
    assert_eq!(bob.post("/api/feats/plot", json!({"world": "void"})).await.0, 200);
    d.grant("Bob", 100, "test").unwrap();
    let bob_before = bob.authority().await;
    // Ann places and takes it back
    let (_, made) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "ann-1")).await;
    let ann_key = made["key"].as_i64().unwrap();
    assert_eq!(refund(&base, "void", ann.id, ann_key).await.1, json!({"refunded": true}));
    // Bob places his own portal in his plot
    let (st, bobs) = world_portal(&base, req("void", bob.id, [100.0, 1.0, 100.0], "yard", "bob-1")).await;
    assert_eq!(st, 200, "{bobs}");
    assert_ne!(bobs["key"], made["key"]);
    assert_eq!(bob.authority().await, bob_before - game::COST_PORTAL);
    // Ann repeats her request: refused, nothing charged, and Bob's portal is not shown to her
    let (st, again) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "ann-1")).await;
    assert_eq!(st, 409, "{again}");
    assert!(again.get("key").is_none() && again.get("name").is_none(), "{again}");
    assert!(!again.to_string().contains(&bobs["key"].to_string()));
    let portals = rules(&base, "void").await["portals"].as_array().unwrap().clone();
    assert_eq!(portals.len(), 1);
    assert_eq!(portals[0]["key"], bobs["key"]);
}

#[tokio::test]
async fn portal_older_than_a_minute_is_not_refunded() {
    let (d, base, ann, _bob) = plotted().await;
    let before = ann.authority().await;
    let (_, made) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "old")).await;
    let key = made["key"].as_i64().unwrap();
    d.backdate_portal(key, 61).unwrap();
    assert_eq!(refund(&base, "void", ann.id, key).await.1, json!({"refunded": false}));
    assert_eq!(rules(&base, "void").await["portals"].as_array().unwrap().len(), 1);
    assert_eq!(ann.authority().await, before - game::COST_PORTAL);
}
