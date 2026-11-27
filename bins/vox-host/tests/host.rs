//! A host running two worlds on one port: each answers at its id and its alias, joining
//! reaches the right one, unknown paths don't, the list says who's running, and two worlds
//! can't share a name; and with an idle time, worlds start when asked for and stop unused.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use vox_core::wire::{self, PROTOCOL, ToClient, ToServer};
use vox_host::{Host, Instances, Options, Spec};
use vox_server::Config;

const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-4, -1, -4], "to": [4, -1, 4] } ] }"#;

fn spec(id: &str, alias: Option<&str>) -> Spec {
    let mut cfg = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    cfg.name = id.into();
    cfg.seed = Some(LEVEL.into());
    Spec { id: id.into(), aliases: alias.into_iter().map(String::from).collect(), cfg, keep: false }
}

async fn serve(host: &Host) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = host.router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr.to_string()
}

/// Join `url` and say which world welcomed us.
async fn welcomed_by(url: &str) -> String {
    let (mut ws, _) = connect_async(url).await.unwrap();
    let hello = ToServer::Hello { protocol: PROTOCOL, name: "Sam".into(), avatar: vec![], token: None };
    ws.send(Message::Binary(wire::encode(&hello).into())).await.unwrap();
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timed out").unwrap().unwrap();
        if let Message::Binary(b) = m
            && let ToClient::Welcome { world, .. } = wire::decode(&b).unwrap()
        {
            return world;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn worlds_by_id_and_alias() {
    let host = Host::start(vec![spec("void", Some("7780")), spec("yard", Some("7781"))], Options::default()).await.unwrap();
    assert_eq!(host.ids(), ["void", "yard"]);
    let addr = serve(&host).await;

    assert_eq!(welcomed_by(&format!("ws://{addr}/w/void/ws")).await, "void");
    assert_eq!(welcomed_by(&format!("ws://{addr}/w/yard/ws")).await, "yard");
    // the old ports still lead to the same worlds
    assert_eq!(welcomed_by(&format!("ws://{addr}/w/7781/ws")).await, "yard");
    assert_eq!(welcomed_by(&format!("ws://{addr}/w/7780/ws")).await, "void");
    // nothing else answers
    assert!(connect_async(format!("ws://{addr}/w/orbit/ws")).await.is_err());
    assert!(connect_async(format!("ws://{addr}/ws")).await.is_err());

    // a world's status, and the list
    let status: serde_json::Value = serde_json::from_str(&http_get(&addr, "/w/yard/").await).unwrap();
    assert_eq!(status["world"], "yard");
    let list: serde_json::Value = serde_json::from_str(&http_get(&addr, "/").await).unwrap();
    let ids: Vec<_> = list["worlds"].as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["void", "yard"]);
    assert_eq!(list["worlds"][1]["aliases"][0], "7781");
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn names_must_be_unique_and_plain() {
    // an id twice, an alias that's another world's id, a name that isn't a path segment
    for specs in [
        vec![spec("void", None), spec("void", None)],
        vec![spec("void", None), spec("yard", Some("void"))],
        vec![spec("../void", None)],
    ] {
        assert!(Host::start(specs, Options::default()).await.is_err());
    }
}

/// A plain HTTP GET's body (no client library needed for two requests).
async fn http_get(addr: &str, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{path}: {out}");
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn on_demand() {
    let mut void = spec("void", None);
    void.keep = true;
    let host = Host::start(vec![void, spec("yard", Some("7781"))], Options { idle: Some(Duration::from_secs(1)), ..Default::default() }).await.unwrap();
    let addr = serve(&host).await;
    // the Void is kept; the yard waits until someone comes
    assert_eq!(host.running().await, ["void"]);
    let status: serde_json::Value = serde_json::from_str(&http_get(&addr, "/w/yard/").await).unwrap();
    assert_eq!(status["running"], false, "asking how it is doesn't start it");

    // someone comes (by its old port, too): it starts
    let (mut ws, _) = connect_async(format!("ws://{addr}/w/7781/ws")).await.unwrap();
    let hello = ToServer::Hello { protocol: PROTOCOL, name: "Sam".into(), avatar: vec![], token: None };
    ws.send(Message::Binary(wire::encode(&hello).into())).await.unwrap();
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timed out").unwrap().unwrap();
        if let Message::Binary(b) = m
            && let ToClient::Welcome { world, .. } = wire::decode(&b).unwrap()
        {
            assert_eq!(world, "yard");
            break;
        }
    }
    assert_eq!(host.running().await, ["void", "yard"]);
    // while they're there it stays, well past the idle time
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(host.running().await, ["void", "yard"]);

    // they leave: after the idle time it's saved and stopped; the Void stays
    drop(ws);
    let mut stopped = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if host.running().await == ["void"] {
            stopped = true;
            break;
        }
    }
    assert!(stopped, "the yard should stop once empty");
    // and comes back for the next visitor
    assert_eq!(welcomed_by(&format!("ws://{addr}/w/yard/ws")).await, "yard");
    host.shutdown().await;
}

/// A POST with a JSON body; the status and the body.
async fn http_post(addr: &str, path: &str, secret: Option<&str>, body: serde_json::Value) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = body.to_string();
    let auth = secret.map_or(String::new(), |s| format!("Authorization: Bearer {s}\r\n"));
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let code = out.get(9..12).and_then(|c| c.parse().ok()).unwrap_or(0);
    (code, out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread")]
async fn instances_of_dungeons() {
    let dir = std::env::temp_dir().join(format!("vox-host-dungeons-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("crypt.json"), LEVEL).unwrap();
    let base = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    let instances = Instances { base, templates: dir.clone(), secret: "s3cret".into(), idle: Duration::from_secs(3), most: 2, players: 8 };
    let host = Host::start(vec![spec("void", None)], Options { idle: None, instances: Some(instances) }).await.unwrap();
    let addr = serve(&host).await;
    let make = |id: &str, template: &str| serde_json::json!({ "id": id, "template": template, "minutes": 60 });

    // only the directory may ask
    assert_eq!(http_post(&addr, "/api/host/instance", None, make("crypt-a1", "crypt")).await.0, 401);
    assert_eq!(http_post(&addr, "/api/host/instance", Some("guess"), make("crypt-a1", "crypt")).await.0, 401);
    // made once; asking again is fine and makes nothing more
    let (code, body) = http_post(&addr, "/api/host/instance", Some("s3cret"), make("crypt-a1", "crypt")).await;
    assert_eq!(code, 200, "{body}");
    assert!(body.contains(r#""made":true"#));
    assert!(http_post(&addr, "/api/host/instance", Some("s3cret"), make("crypt-a1", "crypt")).await.1.contains(r#""made":false"#));
    // not a world's name, not a dungeon that isn't there, not a bad name, not past the limit
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), make("void", "crypt")).await.0, 409);
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), make("tomb-1", "tomb")).await.0, 404);
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), make("../x", "crypt")).await.0, 400);
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), make("crypt-b2", "crypt")).await.0, 200);
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), make("crypt-c3", "crypt")).await.0, 503);

    // asked only whether it's there: yes for one it has, Gone for one it hasn't (and none made)
    let ask = |id: &str| serde_json::json!({ "id": id, "template": "crypt", "minutes": 60, "make": false });
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), ask("crypt-a1")).await.0, 200);
    assert_eq!(http_post(&addr, "/api/host/instance", Some("s3cret"), ask("crypt-zz")).await.0, 410);
    assert!(!host.ids().contains(&"crypt-zz".to_string()));
    // in, under its own name
    assert_eq!(welcomed_by(&format!("ws://{addr}/w/crypt-a1/ws")).await, "crypt-a1");
    // empty for a while: thrown away, and never started again
    let mut gone = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if !host.ids().contains(&"crypt-a1".to_string()) {
            gone = true;
            break;
        }
    }
    assert!(gone, "an empty instance should go");
    assert!(connect_async(format!("ws://{addr}/w/crypt-a1/ws")).await.is_err());
    // the kept world stays
    assert_eq!(host.running().await, ["void"]);
    host.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
