//! A host running two worlds on one port: each answers at its id and its alias, joining
//! reaches the right one, unknown paths don't, the list says who's running, and two worlds
//! can't share a name; and with an idle time, worlds start when asked for and stop unused.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use vox_core::wire::{self, PROTOCOL, ToClient, ToServer};
use vox_host::{Host, Spec};
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
    let host = Host::start(vec![spec("void", Some("7780")), spec("yard", Some("7781"))], None).await.unwrap();
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
        assert!(Host::start(specs, None).await.is_err());
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
    let host = Host::start(vec![void, spec("yard", Some("7781"))], Some(Duration::from_secs(1))).await.unwrap();
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
