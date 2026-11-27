//! Structures and their entitlements over real HTTP: who may place, copy and edit, and what a
//! world server is told before it places one.

use std::path::PathBuf;

use reqwest::Client;
use serde_json::{Value, json};
use vox_core::auth;
use vox_directory::{Config, Directory, WorldInfo};

const SECRET: &str = "world-secret";
const V1: &str = "0123456789abcdef0123456789abcdef";
const V2: &str = "fedcba9876543210fedcba9876543210";
const V3: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn serve() -> String {
    let cfg = Config {
        db: ":memory:".into(),
        key: auth::key_from_seed([9; 32]),
        web: root().join("web"),
        assets: root().join("assets"),
        data: root().join("data"),
        worlds: vec![WorldInfo { id: "void".into(), name: "The Void".into(), url: "/w/7780/ws".into(), about: String::new() }],
        secure_cookies: false,
        downloads: vec![],
        behind_proxy: false,
        social: Default::default(),
        world_secret: Some(SECRET.into()),
        report_every: 0,
        instance_host: None,
    };
    let d = Directory::open(cfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = d.router().into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

struct P {
    c: Client,
    base: String,
    id: u64,
    name: String,
}

impl P {
    async fn new(base: &str, name: &str) -> P {
        let c = Client::builder().cookie_store(true).build().unwrap();
        let r = c.post(format!("{base}/api/signup")).json(&json!({"name": name, "password": "long enough"})).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        c.put(format!("{base}/api/character")).json(&json!({"model": "models/blue.glb", "wear": []})).send().await.unwrap();
        let t: Value = c.post(format!("{base}/api/token")).json(&json!({})).send().await.unwrap().json().await.unwrap();
        let id = auth::peek(t["token"].as_str().unwrap()).unwrap().account;
        P { c, base: base.into(), id, name: name.into() }
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

/// What a world server asks before placing: may `account` (none: a guest) place `id`?
async fn ask(base: &str, id: i64, account: Option<u64>, version: Option<&str>) -> (u16, Value) {
    let mut url = format!("{base}/api/world/structures/{id}?");
    if let Some(a) = account {
        url += &format!("account={a}&");
    }
    if let Some(v) = version {
        url += &format!("version={v}");
    }
    let r = Client::new().get(url).bearer_auth(SECRET).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_owner_shares_and_each_right_opens_what_it_should() {
    let base = serve().await;
    let (ann, bob, cy) = (P::new(&base, "Ann").await, P::new(&base, "Bob").await, P::new(&base, "Cyrus").await);
    // Ann makes a house
    let (st, v) = ann.post("/api/structures", json!({"name": "House", "version": V1})).await;
    assert_eq!(st, 200, "{v}");
    let id = v["id"].as_i64().unwrap();
    // nobody else can see it, or place it (404: they're not told it exists)
    assert_eq!(bob.get(&format!("/api/structures/{id}")).await.0, 404);
    assert_eq!(ask(&base, id, Some(bob.id), None).await.0, 404);
    assert_eq!(ask(&base, id, None, None).await.0, 404, "a guest has no rights");
    assert!(bob.get("/api/structures").await.1["structures"].as_array().unwrap().is_empty());
    // the owner may place it, and sees her own list
    let (st, v) = ask(&base, id, Some(ann.id), None).await;
    assert_eq!((st, v["right"].as_str(), v["version"].as_str()), (200, Some("edit"), Some(V1)));
    // she lets Bob use it: he can place it and sees it listed, but can't copy or change it
    assert_eq!(ann.post("/api/structures/grant", json!({"id": id, "to": "bob", "right": "use"})).await.0, 200);
    assert_eq!(ask(&base, id, Some(bob.id), None).await.1["right"], "use");
    let list = bob.get("/api/structures").await.1;
    assert_eq!(list["structures"][0]["right"], "use");
    assert_eq!(list["structures"][0]["owner"], "Ann");
    assert_eq!(bob.post("/api/structures/copy", json!({"id": id})).await.0, 404, "use isn't copy");
    assert_eq!(bob.post("/api/structures/version", json!({"id": id, "version": V2})).await.0, 404, "use isn't edit");
    // and he can't pass it on, or open it up
    assert_eq!(bob.post("/api/structures/grant", json!({"id": id, "to": "Cyrus", "right": "use"})).await.0, 400);
    assert_eq!(bob.post("/api/structures/public", json!({"id": id, "right": "use"})).await.0, 400);
    assert_eq!(bob.post("/api/structures/delete", json!({"id": id})).await.0, 400);
    // copy: Bob gets his own, derived from hers, and can change that without touching hers
    ann.post("/api/structures/grant", json!({"id": id, "to": "Bob", "right": "copy"})).await;
    let (st, c) = bob.post("/api/structures/copy", json!({"id": id, "name": "Bob's house"})).await;
    assert_eq!(st, 200, "{c}");
    let copy = c["id"].as_i64().unwrap();
    assert_eq!((c["version"].as_str(), c["derived_from"].as_i64()), (Some(V1), Some(id)));
    assert_eq!(bob.post("/api/structures/version", json!({"id": copy, "version": V3})).await.0, 200);
    assert_eq!(ask(&base, copy, Some(bob.id), None).await.1["version"], V3);
    assert_eq!(ask(&base, id, Some(ann.id), None).await.1["version"], V1, "hers is as it was");
    assert_eq!(ask(&base, copy, Some(ann.id), None).await.0, 404, "and she's no rights on his");
    // edit: Cy, granted edit, saves a new version of Ann's; the head moves
    ann.post("/api/structures/grant", json!({"id": id, "to": "Cyrus", "right": "edit"})).await;
    assert_eq!(cy.post("/api/structures/version", json!({"id": id, "version": V2})).await.0, 200);
    assert_eq!(ask(&base, id, Some(bob.id), None).await.1["version"], V2, "everyone's head moved");
    // Bob (use/copy) can place the head, not an older version; Cy (edit) can place either
    assert_eq!(ask(&base, id, Some(bob.id), Some(V2)).await.0, 200);
    assert_eq!(ask(&base, id, Some(bob.id), Some(V1)).await.0, 404);
    assert_eq!(ask(&base, id, Some(cy.id), Some(V1)).await.0, 200);
    assert_eq!(ask(&base, id, Some(cy.id), Some(V3)).await.0, 404, "a version that was never this structure's");
    // Cy sees the history; Bob doesn't; the owner sees who it's shared with
    assert_eq!(cy.get(&format!("/api/structures/{id}")).await.1["versions"].as_array().unwrap().len(), 2);
    assert!(bob.get(&format!("/api/structures/{id}")).await.1.get("versions").is_none());
    assert!(cy.get(&format!("/api/structures/{id}")).await.1.get("grants").is_none(), "an editor doesn't see the grants");
    let shown = ann.get(&format!("/api/structures/{id}")).await.1;
    assert_eq!(shown["grants"].as_array().unwrap().len(), 2);
    assert_eq!(shown["versions"][0]["by"], "Ann");
    // revoking takes it away at once
    assert_eq!(ann.post("/api/structures/grant", json!({"id": id, "to": "Cyrus", "right": null})).await.0, 200);
    assert_eq!(ask(&base, id, Some(cy.id), None).await.0, 404);
    assert_eq!(cy.post("/api/structures/version", json!({"id": id, "version": V1})).await.0, 404);
    // the owner can't be given a grant (she has everything), nor the public edit
    assert_eq!(ann.post("/api/structures/grant", json!({"id": id, "to": "Ann", "right": "use"})).await.0, 400);
    assert_eq!(ann.post("/api/structures/public", json!({"id": id, "right": "edit"})).await.0, 400);
    assert_eq!(ann.post("/api/structures/grant", json!({"id": id, "to": "Nobody", "right": "use"})).await.0, 404);
    assert_eq!(ann.post("/api/structures/grant", json!({"id": id, "to": "Bob", "right": "own"})).await.0, 400);
    let _ = bob.name.len() + cy.name.len();
}

#[tokio::test]
async fn open_to_everyone_is_for_signed_in_accounts_and_can_be_closed() {
    let base = serve().await;
    let (ann, bob) = (P::new(&base, "Ann").await, P::new(&base, "Bob").await);
    let id = ann.post("/api/structures", json!({"name": "Fountain", "version": V1})).await.1["id"].as_i64().unwrap();
    assert_eq!(ann.post("/api/structures/public", json!({"id": id, "right": "use"})).await.0, 200);
    assert_eq!(ask(&base, id, Some(bob.id), None).await.1["right"], "use");
    assert_eq!(ask(&base, id, None, None).await.0, 404, "guests still have none");
    assert_eq!(bob.post("/api/structures/copy", json!({"id": id})).await.0, 404, "open to use, not copy");
    ann.post("/api/structures/public", json!({"id": id, "right": "copy"})).await;
    assert_eq!(bob.post("/api/structures/copy", json!({"id": id})).await.0, 200);
    // closed again
    ann.post("/api/structures/public", json!({"id": id, "right": null})).await;
    assert_eq!(ask(&base, id, Some(bob.id), None).await.0, 404);
    // the copy Bob made is his, and stays
    let mine = bob.get("/api/structures").await.1;
    assert_eq!(mine["structures"].as_array().unwrap().len(), 1);
    assert_eq!(mine["structures"][0]["right"], "edit");
    // deleting is the owner's alone; it takes the grants with it and frees copies from it
    assert_eq!(ann.post("/api/structures/delete", json!({"id": id})).await.0, 200);
    assert_eq!(ask(&base, id, Some(ann.id), None).await.0, 404);
    assert!(bob.get("/api/structures").await.1["structures"][0]["derived_from"].is_null());
}

#[tokio::test]
async fn bad_input_and_who_may_ask() {
    let base = serve().await;
    let ann = P::new(&base, "Ann").await;
    // signed out: nothing
    let anon = Client::new();
    let r = anon.post(format!("{base}/api/structures")).json(&json!({"name": "X", "version": V1})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    // names and versions are checked
    for body in [json!({"name": "", "version": V1}), json!({"name": "x".repeat(49), "version": V1}), json!({"name": "A", "version": "nope"}), json!({"name": "A", "version": V1.to_uppercase()})] {
        assert_eq!(ann.post("/api/structures", body).await.0, 400);
    }
    let id = ann.post("/api/structures", json!({"name": "A", "version": V1})).await.1["id"].as_i64().unwrap();
    assert_eq!(ann.post("/api/structures/version", json!({"id": id, "version": "short"})).await.0, 400);
    assert_eq!(ann.get("/api/structures/9999").await.0, 404);
    // a world's check needs the world secret: not a player's cookie, not nothing, not a wrong one
    let url = format!("{base}/api/world/structures/{id}?account={}", ann.id);
    assert_eq!(anon.get(&url).send().await.unwrap().status().as_u16(), 401);
    assert_eq!(anon.get(&url).bearer_auth("wrong").send().await.unwrap().status().as_u16(), 401);
    assert_eq!(ann.c.get(&url).send().await.unwrap().status().as_u16(), 401);
    assert_eq!(anon.get(&url).bearer_auth(SECRET).send().await.unwrap().status().as_u16(), 200);
}
