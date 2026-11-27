//! Authority, world currencies, feats, the inventory, friends, groups and raids, with world
//! servers' reports played by the test.

use std::path::PathBuf;

use reqwest::Client;
use serde_json::{Value, json};
use vox_core::auth;
use vox_directory::{Config, Directory, WorldInfo, game};

const SECRET: &str = "world-secret";

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn serve() -> (Directory, String) {
    serve_with(None).await
}

async fn serve_with(instance_host: Option<String>) -> (Directory, String) {
    let w = |id: &str, name: &str, port: u16| WorldInfo { id: id.into(), name: name.into(), url: format!("/w/{port}/ws"), about: String::new() };
    let cfg = Config {
        db: ":memory:".into(),
        key: auth::key_from_seed([7; 32]),
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
        instance_host,
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

async fn report(base: &str, world: &str, players: Value) -> (u16, Value) {
    let r = Client::new()
        .post(format!("{base}/api/world/report"))
        .bearer_auth(SECRET)
        .json(&json!({ "world": world, "level": world, "spawn": [0.5, 1.0, 0.5], "players": players }))
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

async fn rules(base: &str, world: &str) -> Value {
    Client::new().get(format!("{base}/api/world/{world}/rules")).bearer_auth(SECRET).send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn authority_is_earned_and_spent() {
    let (_d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    // a welcome
    assert_eq!(ann.authority().await, game::WELCOME);
    // only world servers report
    let r = Client::new().post(format!("{base}/api/world/report")).bearer_auth("guess").json(&json!({"world": "void", "spawn": [0, 0, 0], "players": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    // ten minutes in the Void and 120 blocks placed: 1 + 2
    for i in 0..10 {
        let (st, _) = report(&base, "void", json!([{ "account": ann.id, "at": [40.2, 1.0, 40.7], "edits": if i == 0 { 120 } else { 0 } }])).await;
        assert_eq!(st, 200);
    }
    assert_eq!(ann.authority().await, game::WELCOME + 3);
    // capped per day
    for _ in 0..20 {
        report(&base, "void", json!([{ "account": ann.id, "at": [40.2, 1.0, 40.7], "edits": 1200 }])).await;
    }
    let w = ann.get("/api/wallet").await;
    assert_eq!(w["earning"]["today"], game::DAILY);
    assert_eq!(ann.authority().await, game::WELCOME + game::DAILY);
    assert!(w["ledger"].as_array().unwrap().iter().any(|l| l["memo"] == "playing in The Void"));
    // authority can't be sent
    let bob = P::new(&base, "Bob").await;
    assert_eq!(ann.post("/api/send", json!({"to": "Bob", "code": "AUTH", "amount": 5})).await.0, 400);
    assert_eq!(bob.authority().await, game::WELCOME);
}

#[tokio::test]
async fn plots_portals_and_what_worlds_enforce() {
    let (_d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    // feats happen where you last stood
    assert_eq!(ann.post("/api/feats/plot", json!({"world": "void"})).await.0, 400);
    // too near the spawn
    report(&base, "void", json!([{ "account": ann.id, "at": [10.0, 1.0, 3.0] }, { "account": bob.id, "at": [50.0, 1.0, 50.0] }])).await;
    let (st, v) = ann.post("/api/feats/plot", json!({"world": "void"})).await;
    assert_eq!(st, 400, "{v}");
    // further out
    report(&base, "void", json!([{ "account": ann.id, "at": [40.5, 1.0, 40.5] }, { "account": bob.id, "at": [50.0, 1.0, 50.0] }])).await;
    let (st, v) = ann.post("/api/feats/plot", json!({"world": "void"})).await;
    assert_eq!(st, 200, "{v}");
    let plot = v["plot"].as_i64().unwrap();
    assert_eq!(v["area"], json!({"x0": 32, "z0": 32, "x1": 48, "z1": 48}));
    assert_eq!(ann.authority().await, game::WELCOME - game::COST_PLOT);
    // Bob's would overlap
    let (st, v) = bob.post("/api/feats/plot", json!({"world": "void"})).await;
    assert_eq!(st, 409, "{v}");
    // the world learns who builds there
    let r = rules(&base, "void").await;
    assert_eq!(r["plots"][0]["builders"], json!([ann.id]));
    // a portal: from the plot, to a world that's running
    let (st, v) = ann.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await;
    assert_eq!(st, 400, "the yard hasn't reported: {v}");
    report(&base, "yard", json!([])).await;
    let (st, v) = ann.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await;
    assert_eq!(st, 402, "not enough authority left: {v}");
    let (st, v) = bob.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await;
    assert_eq!(st, 400, "not in a plot of his: {v}");
    _d.grant("Ann", 100, "test").unwrap();
    let (st, v) = ann.post("/api/feats/portal", json!({"world": "void", "to": "yard"})).await;
    assert_eq!(st, 200, "{v}");
    let r = rules(&base, "void").await;
    let p = &r["portals"][0];
    assert_eq!(p["url"], "/w/7781/ws");
    assert_eq!(p["level"], "yard");
    assert_eq!(p["at"], json!([40.5, 1.0, 40.5]));
    assert_eq!(p["arrive"], json!([0.5, 1.0, 0.5]));
    // the inventory: the deed and the key, coins, the wardrobe
    let inv = ann.get("/api/inventory").await;
    let items = inv["items"].as_array().unwrap();
    let kinds = |k: &str| items.iter().filter(|i| i["kind"] == k).count();
    assert_eq!((kinds("deed"), kinds("key"), kinds("coin")), (1, 1, 1));
    assert!(kinds("wear") > 10);
    // releasing the plot closes its portal
    assert_eq!(ann.post("/api/plots/release", json!({"plot": plot})).await.0, 200);
    let r = rules(&base, "void").await;
    assert_eq!((r["plots"].as_array().unwrap().len(), r["portals"].as_array().unwrap().len()), (0, 0));
    assert_eq!(bob.post("/api/plots/release", json!({"plot": plot})).await.0, 400);
}

#[tokio::test]
async fn world_currencies() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    let issue = json!({"world": "yard", "code": "ACRN", "name": "Acorns", "supply": 10000});
    // only the world's owner, and it costs 100 authority
    assert_eq!(ann.post("/api/feats/currency", issue.clone()).await.0, 403);
    d.own_world("yard", "ann").unwrap();
    assert_eq!(ann.post("/api/feats/currency", issue.clone()).await.0, 402);
    d.grant("Ann", 100, "test").unwrap();
    assert_eq!(ann.post("/api/feats/currency", json!({"world": "yard", "code": "AUTH", "name": "x", "supply": 1})).await.0, 400);
    let (st, v) = ann.post("/api/feats/currency", issue.clone()).await;
    assert_eq!(st, 200, "{v}");
    // one per world
    d.grant("Ann", 100, "test").unwrap();
    assert_eq!(ann.post("/api/feats/currency", json!({"world": "yard", "code": "PINE", "name": "Pine cones", "supply": 5})).await.0, 409);
    // sent between players
    let (st, v) = ann.post("/api/send", json!({"to": "bob", "code": "ACRN", "amount": 250, "memo": "for the fence"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ann.post("/api/send", json!({"to": "Bob", "code": "ACRN", "amount": 20000})).await.0, 400);
    assert_eq!(ann.post("/api/send", json!({"to": "Nobody", "code": "ACRN", "amount": 1})).await.0, 404);
    let w = bob.get("/api/wallet").await;
    let acrn = w["balances"].as_array().unwrap().iter().find(|b| b["code"] == "ACRN").unwrap().clone();
    assert_eq!((acrn["amount"].as_i64(), acrn["world_name"].as_str()), (Some(250), Some("The Yard")));
    assert_eq!(w["ledger"][0]["memo"], "for the fence");
    assert_eq!(w["ledger"][0]["with"], "Ann");
    let w = ann.get("/api/wallet").await;
    assert_eq!(w["ledger"][0]["amount"], -250);
}

#[tokio::test]
async fn friends_groups_and_raids() {
    let (d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    let cy = P::new(&base, "Cyd").await;
    // friends: asked, then accepted (or asked both ways)
    assert_eq!(ann.post("/api/friends", json!({"name": "bob"})).await.1["asked"], "Bob");
    assert_eq!(bob.get("/api/people").await["asking"], json!(["Ann"]));
    assert_eq!(bob.post("/api/friends/answer", json!({"name": "Ann", "accept": true})).await.0, 200);
    ann.post("/api/friends", json!({"name": "Cyd"})).await;
    assert_eq!(cy.post("/api/friends", json!({"name": "Ann"})).await.1["friends"], "Ann");
    let p = ann.get("/api/people").await;
    assert_eq!(p["friends"].as_array().unwrap().len(), 2);
    // Bob and Cy aren't friends, so Bob can't invite Cy
    let (_, g) = bob.post("/api/crews", json!({"kind": "group", "name": "Builders"})).await;
    assert_eq!(bob.post("/api/crews/invite", json!({"crew": g["id"], "name": "Cyd"})).await.0, 400);
    // a group, and a plot shared with it
    assert_eq!(bob.post("/api/crews/invite", json!({"crew": g["id"], "name": "Ann"})).await.0, 200);
    assert_eq!(ann.get("/api/people").await["invites"][0]["name"], "Builders");
    assert_eq!(ann.post("/api/crews/answer", json!({"crew": g["id"], "accept": true})).await.0, 200);
    report(&base, "void", json!([{ "account": bob.id, "at": [-60.0, 1.0, -60.0] }])).await;
    let (_, plot) = bob.post("/api/feats/plot", json!({"world": "void"})).await;
    let (st, v) = bob.post("/api/plots/share", json!({"plot": plot["plot"], "group": g["id"]})).await;
    assert_eq!(st, 200, "{v}");
    let r = rules(&base, "void").await;
    let mut b: Vec<u64> = r["plots"][0]["builders"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    b.sort();
    assert_eq!(b, vec![ann.id, bob.id]);
    // a raid party: Ann leads, Bob and Cy join
    let (_, party) = ann.post("/api/crews", json!({"kind": "party", "name": "Night Owls"})).await;
    for (who, name) in [(&bob, "Bob"), (&cy, "Cyd")] {
        assert_eq!(ann.post("/api/crews/invite", json!({"crew": party["id"], "name": name})).await.0, 200);
        assert_eq!(who.post("/api/crews/answer", json!({"crew": party["id"], "accept": true})).await.0, 200);
    }
    // only the leader starts the raid
    assert_eq!(bob.post("/api/feats/raid", json!({"party": party["id"], "world": "yard"})).await.0, 403);
    let (st, v) = ann.post("/api/feats/raid", json!({"party": party["id"], "world": "yard"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ann.post("/api/feats/raid", json!({"party": party["id"], "world": "yard"})).await.0, 409);
    let raid = |p: &Value| p["crews"].as_array().unwrap().iter().find(|c| c["kind"] == "party").unwrap()["raid"].clone();
    assert_eq!(raid(&cy.get("/api/people").await)["state"], "on");
    // one of them alone doesn't count; two together for ten minutes does
    report(&base, "yard", json!([{ "account": ann.id, "at": [0.0, 1.0, 0.0] }])).await;
    assert_eq!(raid(&ann.get("/api/people").await)["together"], 0);
    let before = cy.authority().await;
    let mut finished = 0;
    for _ in 0..game::RAID_TOGETHER {
        let (_, v) = report(&base, "yard", json!([{ "account": ann.id, "at": [0.0, 1.0, 0.0] }, { "account": cy.id, "at": [1.0, 1.0, 0.0] }])).await;
        finished += v["raids_finished"].as_u64().unwrap();
    }
    assert_eq!(finished, 1);
    assert_eq!(raid(&ann.get("/api/people").await)["state"], "won");
    // Cy was there: authority (the raid plus ten minutes' play) and a trophy; Bob wasn't
    assert_eq!(cy.authority().await, before + game::RAID_REWARD + 1);
    let has_trophy = |inv: Value| inv["items"].as_array().unwrap().iter().any(|i| i["kind"] == "trophy" && i["name"] == "Raided The Yard");
    assert!(has_trophy(cy.get("/api/inventory").await));
    assert!(!has_trophy(bob.get("/api/inventory").await));
    // leaving passes the lead on; the last one out ends it
    assert_eq!(ann.post("/api/crews/leave", json!({"crew": party["id"]})).await.0, 200);
    let c = bob.get("/api/people").await["crews"].as_array().unwrap().iter().find(|c| c["kind"] == "party").unwrap().clone();
    assert_eq!(c["you_lead"], true);
    bob.post("/api/crews/leave", json!({"crew": party["id"]})).await;
    cy.post("/api/crews/leave", json!({"crew": party["id"]})).await;
    assert!(cy.get("/api/people").await["crews"].as_array().unwrap().iter().all(|c| c["kind"] != "party"));
    // the group's plot goes back to Bob alone when the group ends
    ann.post("/api/crews/leave", json!({"crew": g["id"]})).await;
    bob.post("/api/crews/leave", json!({"crew": g["id"]})).await;
    assert_eq!(rules(&base, "void").await["plots"][0]["builders"], json!([bob.id]));
    // unfriending
    assert_eq!(bob.post("/api/friends/remove", json!({"name": "Ann"})).await.0, 200);
    assert_eq!(ann.get("/api/people").await["friends"].as_array().unwrap().len(), 1);
    let _ = d;
}

#[tokio::test]
async fn raid_parties_in_rules() {
    let (_d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    let cy = P::new(&base, "Cyd").await;
    let dan = P::new(&base, "Dan").await;
    let eve = P::new(&base, "Eve").await;
    // invites need friends first
    for (asker, who, asker_name, who_name) in [(&ann, &bob, "Ann", "Bob"), (&cy, &dan, "Cyd", "Dan"), (&ann, &cy, "Ann", "Cyd")] {
        asker.post("/api/friends", json!({"name": who_name})).await;
        assert_eq!(who.post("/api/friends/answer", json!({"name": asker_name, "accept": true})).await.0, 200);
    }
    // two parties of two, and one of only its leader, which the rules leave out
    let (_, a) = ann.post("/api/crews", json!({"kind": "party", "name": "Night Owls"})).await;
    ann.post("/api/crews/invite", json!({"crew": a["id"], "name": "Bob"})).await;
    assert_eq!(bob.post("/api/crews/answer", json!({"crew": a["id"], "accept": true})).await.0, 200);
    let (_, b) = cy.post("/api/crews", json!({"kind": "party", "name": "Dawn Patrol"})).await;
    cy.post("/api/crews/invite", json!({"crew": b["id"], "name": "Dan"})).await;
    assert_eq!(dan.post("/api/crews/answer", json!({"crew": b["id"], "accept": true})).await.0, 200);
    eve.post("/api/crews", json!({"kind": "party", "name": "Alone"})).await;
    // a group is not a party, even with two members
    let (_, g) = ann.post("/api/crews", json!({"kind": "group", "name": "Builders"})).await;
    ann.post("/api/crews/invite", json!({"crew": g["id"], "name": "Cyd"})).await;
    cy.post("/api/crews/answer", json!({"crew": g["id"], "accept": true})).await;
    let r = rules(&base, "void").await;
    let mut parties: Vec<Vec<u64>> = r["parties"].as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()).collect();
    parties.sort();
    let mut want = vec![vec![ann.id, bob.id], vec![cy.id, dan.id]];
    want.sort();
    assert_eq!(parties, want);
}

#[test]
fn world_ids() {
    assert_eq!(vox_directory::world_id("The Void"), "void");
    assert_eq!(vox_directory::world_id("Ember Isles!"), "ember-isles");
    let w = WorldInfo::parse("yard=The Yard=/w/7781/ws").unwrap();
    assert_eq!((w.id.as_str(), w.name.as_str(), w.url.as_str()), ("yard", "The Yard", "/w/7781/ws"));
    assert_eq!(WorldInfo::parse("The Void=:7780").unwrap().id, "void");
    assert!(WorldInfo::parse("nothing").is_none());
}

async fn travel(base: &str, secret: &str, body: Value) -> (u16, Value) {
    let r = Client::new().post(format!("{base}/api/world/travel")).bearer_auth(secret).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

#[test]
fn ways_into_worlds() {
    assert_eq!(game::way(":7781").as_deref(), Some("7781"));
    assert_eq!(game::way("/w/7781/ws").as_deref(), Some("7781"));
    assert_eq!(game::way("wss://vox.fail/w/yard/ws").as_deref(), Some("yard"));
    assert_eq!(game::way("ws://127.0.0.1:7781/ws").as_deref(), Some("7781"));
    assert_eq!(game::way("ws://[::1]:7781/ws").as_deref(), Some("7781"));
    assert_eq!(game::way("/yard/ws"), None);
    assert_eq!(game::way("wss://vox.fail/ws"), None);
}

#[tokio::test]
async fn tickets_for_portals() {
    let (_d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let key = auth::key_from_seed([7; 32]).verifying_key();
    let trip = |to: &str| json!({ "world": "void", "account": ann.id, "to": to, "arrive": [0.0, 0.0, 11.5] });
    // a level's portal (":7781") and the directory's list ("/w/7781/ws") name the same world
    let (st, v) = travel(&base, SECRET, trip(":7781")).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["world"], "yard");
    assert_eq!(v["url"], "/w/7781/ws");
    let t = auth::verify_ticket(v["ticket"].as_str().unwrap(), &key, "yard", now()).unwrap();
    assert_eq!((t.claims.account, t.claims.name.as_str(), t.arrive), (ann.id, "Ann", [0.0, 0.0, 11.5]));
    assert_eq!(t.claims.character.model, "models/blue.glb");
    // good for the yard only
    assert!(auth::verify_ticket(v["ticket"].as_str().unwrap(), &key, "void", now()).is_err());
    // two tickets are two trips
    let (_, w) = travel(&base, SECRET, trip("yard")).await;
    let t2 = auth::verify_ticket(w["ticket"].as_str().unwrap(), &key, "yard", now()).unwrap();
    assert_ne!(t.nonce, t2.nonce);
    // only worlds ask; only to known worlds; only for real accounts and real places
    assert_eq!(travel(&base, "guess", trip(":7781")).await.0, 401);
    assert_eq!(travel(&base, SECRET, trip(":9999")).await.0, 404);
    assert_eq!(travel(&base, SECRET, json!({ "world": "void", "account": 999_999, "to": ":7781", "arrive": [0, 0, 0] })).await.0, 400);
    assert_ne!(travel(&base, SECRET, json!({ "world": "nowhere", "account": ann.id, "to": ":7781", "arrive": [0, 0, 0] })).await.0, 200);
}

#[tokio::test]
async fn dungeons_for_parties() {
    // a stand-in host: it makes any dungeon but the tomb, holds what it made, and can be told
    // to let one go (as the real one does when an instance has been empty a while)
    let held = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::<String>::new()));
    let made = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let (h2, m2) = (held.clone(), made.clone());
    let host = axum::Router::new().route(
        "/api/host/instance",
        axum::routing::post(move |h: axum::http::HeaderMap, axum::Json(v): axum::Json<Value>| {
            let (held, made) = (h2.clone(), m2.clone());
            async move {
                use axum::http::StatusCode;
                assert_eq!(h.get("authorization").unwrap(), &format!("Bearer {SECRET}"));
                let id = v["id"].as_str().unwrap().to_string();
                let mut held = held.lock().unwrap();
                if held.contains(&id) {
                    return (StatusCode::OK, "{}");
                }
                if v["make"] == false {
                    return (StatusCode::GONE, "not here");
                }
                if v["template"] == "tomb" {
                    return (StatusCode::NOT_FOUND, "no such dungeon");
                }
                held.insert(id);
                *made.lock().unwrap() += 1;
                (StatusCode::OK, "{}")
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, host).await.unwrap() });
    let (_d, base) = serve_with(Some(host_url)).await;
    let key = auth::key_from_seed([7; 32]).verifying_key();

    let ann = P::new(&base, "Ann").await;
    let bob = P::new(&base, "Bob").await;
    let cy = P::new(&base, "Cyd").await;
    let trip = |who: &P, to: &str| json!({ "world": "void", "account": who.id, "to": to, "arrive": [0.0, 0.0, 4.0] });
    // no party, no dungeon
    assert_eq!(travel(&base, SECRET, trip(&ann, "dungeon:crypt")).await.0, 403);
    ann.post("/api/friends", json!({"name": "Bob"})).await;
    assert_eq!(bob.post("/api/friends/answer", json!({"name": "Ann", "accept": true})).await.0, 200);
    let (_, party) = ann.post("/api/crews", json!({"kind": "party", "name": "Night Owls"})).await;
    assert_eq!(ann.post("/api/crews/invite", json!({"crew": party["id"], "name": "Bob"})).await.0, 200);
    assert_eq!(bob.post("/api/crews/answer", json!({"crew": party["id"], "accept": true})).await.0, 200);

    // Ann goes in: a new instance, made on the host, the ticket for it
    let (st, v) = travel(&base, SECRET, trip(&ann, "dungeon:crypt")).await;
    assert_eq!(st, 200, "{v}");
    let id = v["world"].as_str().unwrap().to_string();
    assert!(id.starts_with("crypt-"), "{id}");
    assert_eq!(v["url"], format!("/w/{id}/ws"));
    let t = auth::verify_ticket(v["ticket"].as_str().unwrap(), &key, &id, now()).unwrap();
    assert_eq!(t.claims.account, ann.id);
    assert_eq!(*made.lock().unwrap(), 1);
    // Bob, in her party, goes to the same one (nothing more made); Cy, not in it, can't
    let (_, v) = travel(&base, SECRET, trip(&bob, "dungeon:crypt")).await;
    assert_eq!(v["world"], id.as_str());
    assert_eq!(*made.lock().unwrap(), 1);
    assert_eq!(travel(&base, SECRET, trip(&cy, "dungeon:crypt")).await.0, 403);
    // the instance may ask for tickets out again (and report) like a world
    let (st, v) = travel(&base, SECRET, json!({ "world": id, "account": ann.id, "to": ":7780", "arrive": [0.0, 0.0, 0.0] })).await;
    assert_eq!((st, v["world"].as_str()), (200, Some("void")));
    // a dungeon that isn't there, a bad name
    assert_eq!(travel(&base, SECRET, trip(&ann, "dungeon:tomb")).await.0, 404);
    assert_eq!(travel(&base, SECRET, trip(&ann, "dungeon:../x")).await.0, 400);

    // they all left and the host let it go: going back in makes it afresh, under the same name,
    // and that counts as one of the hour's three
    held.lock().unwrap().clear();
    let (st, v) = travel(&base, SECRET, trip(&bob, "dungeon:crypt")).await;
    assert_eq!((st, v["world"].as_str()), (200, Some(id.as_str())));
    assert_eq!(*made.lock().unwrap(), 2);
    assert_eq!(travel(&base, SECRET, trip(&ann, "dungeon:cave")).await.0, 200);
    assert_eq!(travel(&base, SECRET, trip(&ann, "dungeon:mine")).await.0, 429);
    // the ones it has still open
    assert_eq!(travel(&base, SECRET, trip(&ann, "dungeon:crypt")).await.1["world"], id.as_str());

    // a raid on the Crypt: won when the instance says its objective was reached
    let (st, v) = ann.post("/api/feats/raid", json!({"party": party["id"], "world": "dungeon:crypt"})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(ann.post("/api/feats/raid", json!({"party": party["id"], "world": "dungeon:../x"})).await.0, 400);
    let before = bob.authority().await;
    let reached = |world: &str, players: Value| {
        let base = base.clone();
        let body = json!({ "world": world, "players": players });
        async move {
            let r = Client::new().post(format!("{base}/api/world/objective")).bearer_auth(SECRET).json(&body).send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    // only worlds say so, and only instances have objectives
    let r = Client::new().post(format!("{base}/api/world/objective")).bearer_auth("guess").json(&json!({"world": id, "players": []})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    assert_eq!(reached("void", json!([ann.id])).await.0, 400);
    // Cy was never in the party: only Ann and Bob win it
    let (st, v) = reached(&id, json!([ann.id, bob.id, cy.id])).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["raid"], true);
    let mut won: Vec<u64> = v["won_by"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    won.sort();
    let mut want = vec![ann.id, bob.id];
    want.sort();
    assert_eq!(won, want);
    assert_eq!(bob.authority().await, before + game::RAID_REWARD);
    let inv = bob.get("/api/inventory").await;
    assert!(inv.to_string().contains("Raided the Crypt"), "{inv}");
    // once won it's done: reaching it again pays nothing more
    assert_eq!(reached(&id, json!([ann.id, bob.id])).await.1["raid"], false);
    assert_eq!(bob.authority().await, before + game::RAID_REWARD);
}

#[tokio::test]
async fn tickets_between_tiles() {
    let (_d, base) = serve().await;
    let ann = P::new(&base, "Ann").await;
    let key = auth::key_from_seed([7; 32]).verifying_key();
    let trip = |from: &str, to: &str| json!({ "world": from, "account": ann.id, "to": to, "arrive": [-254.0, 9.0, 10.5] });
    // over an edge: a ticket for the next tile, which isn't listed but is a tile of the atlas
    let (st, v) = travel(&base, SECRET, trip("atlas-earth-0-0", "/w/atlas-earth-1-0/ws")).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["world"].as_str(), v["url"].as_str()), (Some("atlas-earth-1-0"), Some("/w/atlas-earth-1-0/ws")));
    let t = auth::verify_ticket(v["ticket"].as_str().unwrap(), &key, "atlas-earth-1-0", now()).unwrap();
    assert_eq!(t.arrive, [-254.0, 9.0, 10.5]);
    // into the atlas from a world, and back out
    assert_eq!(travel(&base, SECRET, trip("void", "/w/atlas-earth-0-0/ws")).await.0, 200);
    assert_eq!(travel(&base, SECRET, trip("atlas-earth-2-1", ":7780")).await.1["world"], "void");
    // off the map, or not a tile
    assert_eq!(travel(&base, SECRET, trip("atlas-earth-2-1", "/w/atlas-earth-3-1/ws")).await.0, 404);
    assert_eq!(travel(&base, SECRET, trip("atlas-earth-9-9", ":7780")).await.0, 400);
}
