//! The directory over real HTTP: signing up and in, making a character, and the token a world
//! server accepts.

use std::path::PathBuf;

use serde_json::{Value, json};
use vox_core::auth;
use vox_directory::{Config, Directory, WorldInfo, check_name};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn serve() -> (Directory, String) {
    let cfg = Config {
        db: ":memory:".into(),
        key: auth::key_from_seed([5; 32]),
        web: root().join("web"),
        assets: root().join("assets"),
        data: root().join("data"),
        worlds: vec![WorldInfo { id: "void".into(), name: "The Void".into(), url: ":7780".into(), about: String::new() }],
        secure_cookies: false,
        downloads: vec![],
        behind_proxy: false,
        social: Default::default(),
        world_secret: None,
        report_every: 0,
        instance_host: None,
        atlases: vec![("earth".into(), [3, 2])],
    };
    let d = Directory::open(cfg).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = d.router().into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (d, format!("http://{addr}"))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().cookie_store(true).build().unwrap()
}

async fn post(c: &reqwest::Client, url: String, body: Value) -> (u16, Value) {
    let r = c.post(url).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn sign_up_make_a_character_and_get_in() {
    let (d, base) = serve().await;
    let c = client();
    // nobody yet
    let me: Value = c.get(format!("{base}/api/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["name"], Value::Null);
    // bad names and passwords
    assert_eq!(post(&c, format!("{base}/api/signup"), json!({"name": "x", "password": "longenough"})).await.0, 400);
    assert_eq!(post(&c, format!("{base}/api/signup"), json!({"name": "Sam", "password": "short"})).await.0, 400);
    // sign up: signed in at once
    let (st, v) = post(&c, format!("{base}/api/signup"), json!({"name": "Sam Hill", "password": "correct horse"})).await;
    assert_eq!(st, 200, "{v}");
    let me: Value = c.get(format!("{base}/api/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["name"], "Sam Hill");
    assert_eq!(me["character"], Value::Null);
    // the name is taken, whatever the case
    assert_eq!(post(&client(), format!("{base}/api/signup"), json!({"name": "sam hill", "password": "another one"})).await.0, 409);
    // no token without a character
    assert_eq!(post(&c, format!("{base}/api/token"), json!({})).await.0, 400);
    // the catalogue, and a character from it
    let cat: Value = c.get(format!("{base}/api/catalogue")).send().await.unwrap().json().await.unwrap();
    let model = cat["characters"][3]["model"].as_str().unwrap().to_string();
    assert!(cat["wardrobe"].as_array().unwrap().len() > 10);
    let r = c.put(format!("{base}/api/character")).json(&json!({"model": "models/../../etc/passwd", "wear": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let r = c.put(format!("{base}/api/character")).json(&json!({"model": model, "wear": ["cowboy", "crown"]})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "two hats");
    let r = c.put(format!("{base}/api/character")).json(&json!({"model": model, "wear": ["cowboy", "shades"]})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    // the token: who you are and what you look like, signed so a world server believes it
    let (st, v) = post(&c, format!("{base}/api/token"), json!({})).await;
    assert_eq!(st, 200, "{v}");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let key = auth::public_from_hex(&d.public_key()).unwrap();
    let claims = auth::verify(v["token"].as_str().unwrap(), &key, now).unwrap();
    assert_eq!(claims.name, "Sam Hill");
    assert_eq!(claims.character.model, model);
    assert_eq!(claims.character.wear, vec!["cowboy", "shades"]);
    assert_eq!(v["worlds"][0]["url"], ":7780");
    let served = c.get(format!("{base}/api/key")).send().await.unwrap().text().await.unwrap();
    assert_eq!(served, d.public_key());
    // sign out, then in again
    post(&c, format!("{base}/api/signout"), json!({})).await;
    let me: Value = c.get(format!("{base}/api/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["name"], Value::Null);
    assert_eq!(post(&c, format!("{base}/api/signin"), json!({"name": "Sam Hill", "password": "wrong"})).await.0, 401);
    assert_eq!(post(&c, format!("{base}/api/signin"), json!({"name": "nobody", "password": "whatever1"})).await.0, 401);
    let (st, _) = post(&c, format!("{base}/api/signin"), json!({"name": "SAM HILL", "password": "correct horse"})).await;
    assert_eq!(st, 200);
    let me: Value = c.get(format!("{base}/api/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["character"]["model"], model.as_str());
}

#[tokio::test]
async fn guarded() {
    let (_d, base) = serve().await;
    let c = client();
    // no token or character without signing in
    assert_eq!(post(&c, format!("{base}/api/token"), json!({})).await.0, 401);
    let r = c.put(format!("{base}/api/character")).json(&json!({"model": "models/dog.glb", "wear": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    // writes must be JSON (a form on another site can't send that without asking)
    let r = c.post(format!("{base}/api/signin")).header("content-type", "application/x-www-form-urlencoded").body("name=a&password=b").send().await.unwrap();
    assert!(r.status().is_client_error());
    // too many tries
    let mut last = 0;
    for _ in 0..12 {
        last = post(&c, format!("{base}/api/signin"), json!({"name": "who", "password": "guessing"})).await.0;
    }
    assert_eq!(last, 429);
    // the pages come with a content policy
    let r = c.get(format!("{base}/")).send().await.unwrap();
    assert!(r.headers().get("content-security-policy").is_some());
    assert_eq!(r.headers()["x-frame-options"], "DENY");
    // game files are served for the browser client; nothing outside them
    assert_eq!(c.get(format!("{base}/data/blocks.json")).send().await.unwrap().status().as_u16(), 200);
    assert_eq!(c.get(format!("{base}/assets/portraits/dog.png")).send().await.unwrap().status().as_u16(), 200);
    assert!(c.get(format!("{base}/assets/../Cargo.toml")).send().await.unwrap().status().is_client_error());
}

#[test]
fn names() {
    assert_eq!(check_name("  Sam  ").unwrap(), "Sam");
    assert!(check_name("Sam Hill-2.0").is_ok());
    for bad in ["ab", "_sam", "sam  hill", "sam<script>", "a very long name indeed yes", ""] {
        assert!(check_name(bad).is_err(), "{bad}");
    }
}

/// Signs `c` up as `name` and gives it a character (a token needs one).
async fn signed_up_with_character(base: &str, c: &reqwest::Client, name: &str) {
    let (st, v) = post(c, format!("{base}/api/signup"), json!({"name": name, "password": "correct horse"})).await;
    assert_eq!(st, 200, "{v}");
    let cat: Value = c.get(format!("{base}/api/catalogue")).send().await.unwrap().json().await.unwrap();
    let model = cat["characters"][3]["model"].as_str().unwrap().to_string();
    let r = c.put(format!("{base}/api/character")).json(&json!({"model": model, "wear": ["cowboy", "shades"]})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
}

async fn me(c: &reqwest::Client, base: &str) -> Value {
    c.get(format!("{base}/api/me")).send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn rename_valid() {
    let (d, base) = serve().await;
    let c = client();
    signed_up_with_character(&base, &c, "alice").await;
    let (st, v) = post(&c, format!("{base}/api/name"), json!({"name": "Alicia"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["name"], "Alicia");
    assert!(v["rename_after"].as_u64().unwrap() > 0);
    let m = me(&c, &base).await;
    assert_eq!(m["name"], "Alicia");
    assert!(m["rename_after"].as_u64().unwrap() > std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
    // a new token carries the new name
    let (st, v) = post(&c, format!("{base}/api/token"), json!({})).await;
    assert_eq!(st, 200, "{v}");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let key = auth::public_from_hex(&d.public_key()).unwrap();
    let claims = auth::verify(v["token"].as_str().unwrap(), &key, now).unwrap();
    assert_eq!(claims.name, "Alicia");
}

#[tokio::test]
async fn rename_taken() {
    let (_d, base) = serve().await;
    let bob = client();
    signed_up_with_character(&base, &bob, "bob").await;
    let carol = client();
    signed_up_with_character(&base, &carol, "carol").await;
    assert_eq!(post(&carol, format!("{base}/api/name"), json!({"name": "bob"})).await.0, 409);
    assert_eq!(post(&carol, format!("{base}/api/name"), json!({"name": "BOB"})).await.0, 409);
    // and carol is still carol
    assert_eq!(me(&carol, &base).await["name"], "carol");
}

#[tokio::test]
async fn rename_invalid() {
    let (_d, base) = serve().await;
    let c = client();
    signed_up_with_character(&base, &c, "dave").await;
    for bad in ["ab", "  _x", "a  b"] {
        let (st, v) = post(&c, format!("{base}/api/name"), json!({"name": bad})).await;
        assert_eq!(st, 400, "{bad:?}: {v}");
        assert!(v["error"].is_string(), "{bad:?}: {v}");
    }
    assert_eq!(me(&c, &base).await["name"], "dave");
}

#[tokio::test]
async fn rename_needs_sign_in() {
    let (_d, base) = serve().await;
    let c = client();
    assert_eq!(post(&c, format!("{base}/api/name"), json!({"name": "eve"})).await.0, 401);
}

#[tokio::test]
async fn rename_limit() {
    let (d, base) = serve().await;
    let c = client();
    signed_up_with_character(&base, &c, "alice").await;
    assert_eq!(post(&c, format!("{base}/api/name"), json!({"name": "alice2"})).await.0, 200);
    let (st, v) = post(&c, format!("{base}/api/name"), json!({"name": "alice3"})).await;
    assert_eq!(st, 429, "{v}");
    assert!(v["error"].as_str().unwrap().contains("days"), "{v}");
    // 30 days later the change is allowed
    d.backdate_rename("alice2", 2592000 + 1).unwrap();
    assert_eq!(post(&c, format!("{base}/api/name"), json!({"name": "alice3"})).await.0, 200);
}

#[tokio::test]
async fn rename_capitals_only() {
    let (_d, base) = serve().await;
    let c = client();
    signed_up_with_character(&base, &c, "alice").await;
    assert_eq!(post(&c, format!("{base}/api/name"), json!({"name": "alice2"})).await.0, 200);
    // capitals inside the 30 days are free
    let (st, v) = post(&c, format!("{base}/api/name"), json!({"name": "Alice2"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(me(&c, &base).await["name"], "Alice2");
}

#[tokio::test]
async fn rename_old_name_held() {
    let (d, base) = serve().await;
    let alice = client();
    signed_up_with_character(&base, &alice, "alice").await;
    assert_eq!(post(&alice, format!("{base}/api/name"), json!({"name": "alice2"})).await.0, 200);
    // nobody else can sign up as the old name while it is held
    let newcomer = client();
    let (st, _) = post(&newcomer, format!("{base}/api/signup"), json!({"name": "alice", "password": "correct horse"})).await;
    assert_eq!(st, 409);
    // nor change to it
    let bob = client();
    signed_up_with_character(&base, &bob, "bob").await;
    assert_eq!(post(&bob, format!("{base}/api/name"), json!({"name": "Alice"})).await.0, 409);
    // once the 30 days are up the old name is still held for a week, and alice may take it back
    d.backdate_rename("alice2", 2592000 + 1).unwrap();
    assert_eq!(post(&bob, format!("{base}/api/name"), json!({"name": "alice"})).await.0, 409);
    assert_eq!(post(&alice, format!("{base}/api/name"), json!({"name": "alice"})).await.0, 200);
    assert_eq!(me(&alice, &base).await["name"], "alice");
}

#[tokio::test]
async fn rename_hold_ends() {
    let (d, base) = serve().await;
    let alice = client();
    signed_up_with_character(&base, &alice, "alice").await;
    assert_eq!(post(&alice, format!("{base}/api/name"), json!({"name": "alice2"})).await.0, 200);
    let bob = client();
    signed_up_with_character(&base, &bob, "bob").await;
    // 30 days and the week of grace: the old name is free for anyone
    d.backdate_rename("alice2", 2592000 + 7 * 86400 + 1).unwrap();
    let (st, v) = post(&bob, format!("{base}/api/name"), json!({"name": "alice"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(me(&alice, &base).await["rename_after"], Value::Null);
}

#[tokio::test]
async fn rename_non_ascii_capitals_count_in_full() {
    // SQLite's NOCASE folds ASCII only, so É to é is a different name to it: a full change
    let (_d, base) = serve().await;
    let c = client();
    signed_up_with_character(&base, &c, "Émile").await;
    let (st, v) = post(&c, format!("{base}/api/name"), json!({"name": "émile"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(post(&c, format!("{base}/api/name"), json!({"name": "Émile"})).await.0, 429);
}

#[tokio::test]
async fn rename_same_name() {
    let (_d, base) = serve().await;
    let c = client();
    signed_up_with_character(&base, &c, "frank").await;
    let (st, v) = post(&c, format!("{base}/api/name"), json!({"name": "frank"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["name"], "frank");
    assert_eq!(v["rename_after"], Value::Null);
    assert_eq!(me(&c, &base).await["rename_after"], Value::Null);
}
