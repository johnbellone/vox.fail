//! World editors over real HTTP: the owner grants and revokes, others are refused, the list is
//! capped, and a world server asks (with the world secret) who may make advanced edits.

use std::path::PathBuf;

use reqwest::Client;
use serde_json::{Value, json};
use vox_core::auth;
use vox_directory::{Config, Directory, WorldInfo};

const SECRET: &str = "world-secret";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn serve() -> (Directory, String) {
    serve_with(false).await
}

/// `behind_proxy`: sign-ups are counted per `cf-connecting-ip`, so a test can make many accounts.
async fn serve_with(behind_proxy: bool) -> (Directory, String) {
    let cfg = Config {
        db: ":memory:".into(),
        key: auth::key_from_seed([5; 32]),
        web: root().join("web"),
        assets: root().join("assets"),
        data: root().join("data"),
        worlds: vec![WorldInfo { id: "yard".into(), name: "The Yard".into(), url: "/w/7781/ws".into(), about: String::new() }],
        secure_cookies: false,
        downloads: vec![],
        behind_proxy,
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
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self.c.get(format!("{}{path}", self.base)).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self.c.post(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

/// The world server's question: may `account` make advanced edits in `world`?
async fn ask(base: &str, world: &str, account: Option<u64>) -> (u16, Value) {
    let mut url = format!("{base}/api/world/{world}/editor");
    if let Some(a) = account {
        url += &format!("?account={a}");
    }
    let r = Client::new().get(url).bearer_auth(SECRET).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

fn change(world: &str, name: &str, grant: bool) -> Value {
    json!({ "world": world, "name": name, "grant": grant })
}

#[tokio::test]
async fn editors_owner_grants_and_revokes() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    d.own_world("yard", "Ann").unwrap();

    let (st, v) = ann.post("/api/worlds/editors", change("yard", "Bob", true)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["editors"], json!(["Bob"]));
    // granting again is harmless
    let (st, v) = ann.post("/api/worlds/editors", change("yard", "bob", true)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["editors"], json!(["Bob"]));

    let (st, v) = ann.get("/api/worlds/editors?world=yard").await;
    assert_eq!((st, &v["editors"]), (200, &json!(["Bob"])));
    // an editor sees the list too
    assert_eq!(bob.get("/api/worlds/editors?world=yard").await.0, 200);

    let (st, v) = ann.post("/api/worlds/editors", change("yard", "Bob", false)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["editors"], json!([]));
    // revoked: no longer sees the list, and no longer may edit
    assert_eq!(bob.get("/api/worlds/editors?world=yard").await.0, 403);
    assert_eq!(ask(&base, "yard", Some(bob.id)).await.1["editor"], false);
}

#[tokio::test]
async fn editors_non_owner_is_refused() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    let cat = P::new(&base, "Cat").await;
    d.own_world("yard", "Ann").unwrap();
    ann.post("/api/worlds/editors", change("yard", "Bob", true)).await;

    // an editor can't add editors, nor can anyone else
    assert_eq!(bob.post("/api/worlds/editors", change("yard", "Cat", true)).await.0, 403);
    assert_eq!(cat.post("/api/worlds/editors", change("yard", "Cat", true)).await.0, 403);
    // strangers don't see who edits
    assert_eq!(cat.get("/api/worlds/editors?world=yard").await.0, 403);
    // the owner can't add themself: they already may edit
    assert_eq!(ann.post("/api/worlds/editors", change("yard", "Ann", true)).await.0, 400);
    // signed out: nothing
    let anon = Client::new();
    let r = anon.post(format!("{base}/api/worlds/editors")).json(&change("yard", "Cat", true)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    // unknown names and worlds
    assert_eq!(ann.post("/api/worlds/editors", change("yard", "Nobody", true)).await.0, 404);
    assert_eq!(ann.post("/api/worlds/editors", change("nowhere", "Bob", true)).await.0, 400);
    // a form (not JSON) is refused like the other writes
    let r = ann.c.post(format!("{base}/api/worlds/editors")).body("world=yard&name=Cat&grant=true").send().await.unwrap();
    assert!(r.status().is_client_error());
}

#[tokio::test]
async fn editors_cap_is_thirty_two() {
    let (d, base) = serve_with(true).await;
    let ann = P::new(&base, "Ann").await;
    d.own_world("yard", "Ann").unwrap();
    // 33 accounts to name (the 33rd is the one refused); each signs up from its own address,
    // since sign-ups are limited per visitor
    for i in 0..33 {
        let r = Client::new()
            .post(format!("{base}/api/signup"))
            .header("cf-connecting-ip", format!("10.0.0.{i}"))
            .json(&json!({"name": format!("Ed{i}"), "password": "long enough"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
    }
    for i in 0..32 {
        let (st, v) = ann.post("/api/worlds/editors", change("yard", &format!("Ed{i}"), true)).await;
        assert_eq!(st, 200, "{v}");
    }
    // re-granting someone already listed doesn't count against the cap
    assert_eq!(ann.post("/api/worlds/editors", change("yard", "Ed0", true)).await.0, 200);
    // the 33rd is refused
    let (st, v) = ann.post("/api/worlds/editors", change("yard", "Ed32", true)).await;
    assert_eq!(st, 400, "{v}");
}

#[tokio::test]
async fn editors_world_secret_check() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    let cat = P::new(&base, "Cat").await;
    d.own_world("yard", "Ann").unwrap();
    ann.post("/api/worlds/editors", change("yard", "Bob", true)).await;

    assert_eq!(ask(&base, "yard", Some(ann.id)).await.1["editor"], true);
    assert_eq!(ask(&base, "yard", Some(bob.id)).await.1["editor"], true);
    assert_eq!(ask(&base, "yard", Some(cat.id)).await.1["editor"], false);
    // a guest may edit nothing
    assert_eq!(ask(&base, "yard", None).await.1["editor"], false);
    // unknown world: 404
    assert_eq!(ask(&base, "nowhere", Some(ann.id)).await.0, 404);

    // refused without the world secret, with a wrong one, or with a player's cookie
    let url = format!("{base}/api/world/yard/editor?account={}", ann.id);
    assert_eq!(Client::new().get(&url).send().await.unwrap().status().as_u16(), 401);
    assert_eq!(Client::new().get(&url).bearer_auth("wrong").send().await.unwrap().status().as_u16(), 401);
    assert_eq!(ann.c.get(&url).send().await.unwrap().status().as_u16(), 401);
}

async fn rules(base: &str, world: &str) -> Value {
    Client::new().get(format!("{base}/api/world/{world}/rules")).bearer_auth(SECRET).send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn editors_rules_include_owner_and_editors() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    // no owner yet: the rules say so
    assert!(rules(&base, "yard").await["owner"].is_null());
    d.own_world("yard", "Ann").unwrap();
    let r = rules(&base, "yard").await;
    assert_eq!((r["owner"].as_u64(), &r["editors"]), (Some(ann.id), &json!([])));

    ann.post("/api/worlds/editors", change("yard", "Bob", true)).await;
    let r = rules(&base, "yard").await;
    assert_eq!((r["owner"].as_u64(), &r["editors"]), (Some(ann.id), &json!([bob.id])));
}
