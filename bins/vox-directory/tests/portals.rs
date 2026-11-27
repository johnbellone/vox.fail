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

/// A world server's request for a player portal that pairs with another of the player's portals.
fn pair_req(world: &str, account: u64, at: [f64; 3], pair: &Value, idem: &str) -> Value {
    json!({ "world": world, "account": account, "at": at, "axis": "x", "pair": pair, "idem": idem })
}

/// A world server's request to re-point a portal (`pair` none unlinks it).
async fn link(base: &str, body: Value) -> (u16, Value) {
    let r = Client::new().post(format!("{base}/api/world/portal/link")).bearer_auth(SECRET).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

/// The rules' entry for one portal key.
fn entry(rules: &Value, key: &Value) -> Value {
    rules["portals"].as_array().unwrap().iter().find(|p| p["key"] == *key).cloned().unwrap_or(Value::Null)
}

/// Ann claims a plot in the Yard as well (the Yard is reported with Ann standing in it first).
async fn yard_plot(base: &str, ann: &P) {
    report(base, "yard", json!([{ "account": ann.id, "at": [40.5, 1.0, 40.5] }])).await;
    let (st, v) = ann.post("/api/feats/plot", json!({"world": "yard"})).await;
    assert_eq!(st, 200, "{v}");
}

#[tokio::test]
async fn pair_travel_arrives_at_the_other_end() {
    let (_d, base, ann, _bob) = plotted().await;
    yard_plot(&base, &ann).await;
    let (st, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "pa")).await;
    assert_eq!(st, 200, "{a}");
    // a portal in the Yard paired with A: A is turned to lead back to it
    let (st, b) = world_portal(&base, pair_req("yard", ann.id, [44.0, 1.0, 44.0], &a["key"], "pb")).await;
    assert_eq!(st, 200, "{b}");

    // A (in the Void) now arrives at B's place in the Yard, and names B as its pair
    let v = rules(&base, "void").await;
    let pa = entry(&v, &a["key"]);
    assert_eq!(pa["pair"], b["key"]);
    assert_eq!(pa["arrive"], json!([44.0, 1.0, 44.0]));
    assert_eq!(pa["url"], "/w/7781/ws");
    // B (in the Yard) arrives at A's place in the Void
    let y = rules(&base, "yard").await;
    let pb = entry(&y, &b["key"]);
    assert_eq!(pb["pair"], a["key"]);
    assert_eq!(pb["arrive"], json!([40.0, 1.0, 40.0]));
    assert_eq!(pb["url"], "/w/7780/ws");

    // the travel into the Yard goes to the Yard, by its address as the level gives it
    let r = Client::new()
        .post(format!("{base}/api/world/travel"))
        .bearer_auth(SECRET)
        .json(&json!({ "world": "void", "account": ann.id, "to": "/w/7781/ws", "arrive": [44.0, 1.0, 44.0] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let t: Value = r.json().await.unwrap();
    assert_eq!(t["world"], "yard");
    assert_eq!(t["url"], "/w/7781/ws");
}

#[tokio::test]
async fn pair_refused_for_another_owner_and_missing_keys() {
    let (d, base, ann, bob) = plotted().await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    // Bob has a plot and a portal of his own
    report(&base, "void", json!([{ "account": bob.id, "at": [100.5, 1.0, 100.5] }])).await;
    assert_eq!(bob.post("/api/feats/plot", json!({"world": "void"})).await.0, 200);
    d.grant("Bob", 100, "test").unwrap();
    let (st, bobs) = world_portal(&base, req("void", bob.id, [100.0, 1.0, 100.0], "yard", "b")).await;
    assert_eq!(st, 200, "{bobs}");
    let bob_before = bob.authority().await;

    // pairing with Ann's portal: refused, nothing charged
    let (st, v) = world_portal(&base, pair_req("void", bob.id, [101.0, 1.0, 101.0], &a["key"], "steal")).await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(bob.authority().await, bob_before);
    // a key that does not exist (or was refunded): 404
    let (st, _) = world_portal(&base, pair_req("void", bob.id, [101.0, 1.0, 101.0], &json!(999_999), "gone")).await;
    assert_eq!(st, 404);
    // Ann's portal is unchanged: still leads to the Yard's spawn
    let pa = entry(&rules(&base, "void").await, &a["key"]);
    assert!(pa.get("pair").is_none(), "{pa}");

    // re-pointing Ann's portal at Bob's: refused too (403), and Ann's stays as it was
    let (st, v) = link(&base, json!({ "world": "void", "account": ann.id, "key": a["key"], "pair": bobs["key"] })).await;
    assert_eq!(st, 403, "{v}");
    assert!(entry(&rules(&base, "void").await, &a["key"]).get("pair").is_none());
    // a link of a key that is not Ann's (Bob's, here) is 404
    let (st, _) = link(&base, json!({ "world": "void", "account": ann.id, "key": bobs["key"], "pair": null })).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn pair_gone_end_falls_back_to_spawn() {
    let (_d, base, ann, _bob) = plotted().await;
    yard_plot(&base, &ann).await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    let (st, b) = world_portal(&base, pair_req("yard", ann.id, [44.0, 1.0, 44.0], &a["key"], "b")).await;
    assert_eq!(st, 200, "{b}");
    assert_eq!(entry(&rules(&base, "void").await, &a["key"])["arrive"], json!([44.0, 1.0, 44.0]));

    // the Yard end is taken back: A has nothing to arrive at but the Yard's spawn
    let (st, v) = refund(&base, "yard", ann.id, b["key"].as_i64().unwrap()).await;
    assert_eq!((st, v), (200, json!({"refunded": true})));
    let pa = entry(&rules(&base, "void").await, &a["key"]);
    assert!(pa.get("pair").is_none(), "{pa}");
    assert_eq!(pa["arrive"], json!([0.5, 1.0, 0.5]));
    assert_eq!(pa["url"], "/w/7781/ws");
}

#[tokio::test]
async fn pair_within_one_world_works() {
    let (_d, base, ann, _bob) = plotted().await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    // a second portal in the same plot pairs with the first: both lead into the Void
    let (st, c) = world_portal(&base, pair_req("void", ann.id, [41.0, 1.0, 41.0], &a["key"], "c")).await;
    assert_eq!(st, 200, "{c}");
    let v = rules(&base, "void").await;
    let pc = entry(&v, &c["key"]);
    assert_eq!(pc["pair"], a["key"]);
    assert_eq!(pc["arrive"], json!([40.0, 1.0, 40.0]));
    assert_eq!(pc["url"], "/w/7780/ws");
    let pa = entry(&v, &a["key"]);
    assert_eq!(pa["pair"], c["key"]);
    assert_eq!(pa["arrive"], json!([41.0, 1.0, 41.0]));
    assert_eq!(pa["url"], "/w/7780/ws");
}

#[tokio::test]
async fn pair_link_repoints_and_unlinks() {
    let (_d, base, ann, _bob) = plotted().await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    let (_, c) = world_portal(&base, req("void", ann.id, [41.0, 1.0, 41.0], "yard", "c")).await;
    // A pairs with C: both ends name each other (C was free, so it is turned to lead back)
    let (st, v) = link(&base, json!({ "world": "void", "account": ann.id, "key": a["key"], "pair": c["key"] })).await;
    assert_eq!(st, 200, "{v}");
    let r = rules(&base, "void").await;
    assert_eq!(entry(&r, &a["key"])["pair"], c["key"]);
    assert_eq!(entry(&r, &a["key"])["arrive"], json!([41.0, 1.0, 41.0]));
    assert_eq!(entry(&r, &c["key"])["pair"], a["key"]);

    // A unlinks: it leads to its destination world's spawn, which is the Void now that it was
    // turned to lead to C; C loses its pointer and leads to the Void's spawn too
    let (st, v) = link(&base, json!({ "world": "void", "account": ann.id, "key": a["key"], "pair": null })).await;
    assert_eq!(st, 200, "{v}");
    let r = rules(&base, "void").await;
    let pa = entry(&r, &a["key"]);
    assert!(pa.get("pair").is_none(), "{pa}");
    assert_eq!(pa["arrive"], json!([0.5, 1.0, 0.5]));
    assert_eq!(pa["url"], "/w/7780/ws");
    let pc = entry(&r, &c["key"]);
    assert!(pc.get("pair").is_none(), "{pc}");
    assert_eq!(pc["url"], "/w/7780/ws");

    // a portal can't pair with itself, and a key that is gone is 404
    let (st, _) = link(&base, json!({ "world": "void", "account": ann.id, "key": a["key"], "pair": a["key"] })).await;
    assert_eq!(st, 400);
    assert_eq!(refund(&base, "void", ann.id, c["key"].as_i64().unwrap()).await.1, json!({"refunded": true}));
    let (st, _) = link(&base, json!({ "world": "void", "account": ann.id, "key": a["key"], "pair": c["key"] })).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn pair_a_paired_end_is_not_taken_over() {
    let (d, base, ann, _bob) = plotted().await;
    yard_plot(&base, &ann).await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    let (_, b) = world_portal(&base, pair_req("yard", ann.id, [44.0, 1.0, 44.0], &a["key"], "b")).await;
    // a third portal pairs with A, which already has B: A keeps B, the new one still arrives at A
    d.grant("Ann", 100, "test").unwrap();
    let (_, c) = world_portal(&base, req("void", ann.id, [41.0, 1.0, 41.0], "yard", "c")).await;
    let (st, v) = link(&base, json!({ "world": "void", "account": ann.id, "key": c["key"], "pair": a["key"] })).await;
    assert_eq!(st, 200, "{v}");
    let r = rules(&base, "void").await;
    assert_eq!(entry(&r, &a["key"])["pair"], b["key"]);
    assert_eq!(entry(&r, &c["key"])["pair"], a["key"]);
    let yard = rules(&base, "yard").await;
    assert_eq!(entry(&yard, &b["key"])["pair"], a["key"]);
}

#[tokio::test]
async fn pair_refund_clears_the_partner_and_a_new_pair_turns_it_again() {
    let (d, base, ann, _bob) = plotted().await;
    yard_plot(&base, &ann).await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    let (_, b) = world_portal(&base, pair_req("yard", ann.id, [44.0, 1.0, 44.0], &a["key"], "b")).await;
    assert_eq!(entry(&rules(&base, "yard").await, &b["key"])["pair"], a["key"]);

    // A is refunded: B's pointer goes with it, and B leads to the Void's spawn
    assert_eq!(refund(&base, "void", ann.id, a["key"].as_i64().unwrap()).await.1, json!({"refunded": true}));
    let pb = entry(&rules(&base, "yard").await, &b["key"]);
    assert!(pb.get("pair").is_none(), "{pb}");
    assert_eq!(pb["arrive"], json!([0.5, 1.0, 0.5]));
    assert_eq!(pb["url"], "/w/7780/ws");

    // C in the Void pairs with B: B has no live pair, so it is turned to lead back to C
    d.grant("Ann", 100, "test").unwrap();
    let (st, c) = world_portal(&base, pair_req("void", ann.id, [41.0, 1.0, 41.0], &b["key"], "c")).await;
    assert_eq!(st, 200, "{c}");
    let pb = entry(&rules(&base, "yard").await, &b["key"]);
    assert_eq!(pb["pair"], c["key"]);
    assert_eq!(pb["arrive"], json!([41.0, 1.0, 41.0]));
    assert_eq!(pb["url"], "/w/7780/ws");
}

#[tokio::test]
async fn pair_release_clears_the_partner() {
    let (_d, base, ann, _bob) = plotted().await;
    yard_plot(&base, &ann).await;
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    let (_, _b) = world_portal(&base, pair_req("yard", ann.id, [44.0, 1.0, 44.0], &a["key"], "b")).await;
    // the Yard plot is released, taking B with it: A leads to the Yard's spawn, with no pair
    let plot = rules(&base, "yard").await["plots"][0]["id"].clone();
    assert_eq!(ann.post("/api/plots/release", json!({ "plot": plot })).await.0, 200);
    let pa = entry(&rules(&base, "void").await, &a["key"]);
    assert!(pa.get("pair").is_none(), "{pa}");
    assert_eq!(pa["arrive"], json!([0.5, 1.0, 0.5]));
    assert_eq!(pa["url"], "/w/7781/ws");
}

#[tokio::test]
async fn pair_plain_to_portal_is_turned_around_when_paired_to() {
    let (_d, base, ann, _bob) = plotted().await;
    yard_plot(&base, &ann).await;
    // A leads to the Yard's spawn and is paired with nothing
    let (_, a) = world_portal(&base, req("void", ann.id, [40.0, 1.0, 40.0], "yard", "a")).await;
    assert!(entry(&rules(&base, "void").await, &a["key"]).get("pair").is_none());
    // B in the Yard pairs with A: A is turned to lead back to B
    let (st, b) = world_portal(&base, pair_req("yard", ann.id, [44.0, 1.0, 44.0], &a["key"], "b")).await;
    assert_eq!(st, 200, "{b}");
    let pa = entry(&rules(&base, "void").await, &a["key"]);
    assert_eq!(pa["pair"], b["key"]);
    assert_eq!(pa["arrive"], json!([44.0, 1.0, 44.0]));
}
