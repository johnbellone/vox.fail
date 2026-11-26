use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use vox_relay::{Created, DROPPABLE, Listed, RELIABLE, Relay, Sim};

async fn start(expire: Duration) -> (String, Relay) {
    let relay = Relay::new(expire);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let r = relay.router();
    tokio::spawn(async move { axum::serve(listener, r).await.unwrap() });
    (addr, relay)
}

/// A bare HTTP/1.1 request; returns (status, body).
async fn http(
    addr: &str,
    method: &str,
    path: &str,
    body: &str,
    auth: Option<&str>,
) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let auth = auth
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let status = out[9..12].parse().unwrap();
    let body = out
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

async fn new_show(addr: &str, title: &str, public: bool) -> Created {
    let (st, body) = http(
        addr,
        "POST",
        "/sessions",
        &format!(r#"{{"title":"{title}","character":"dog","public":{public}}}"#),
        None,
    )
    .await;
    assert_eq!(st, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn join(addr: &str, id: &str, query: &str) -> Ws {
    let (ws, _) = connect_async(format!("ws://{addr}/sessions/{id}/ws?{query}"))
        .await
        .unwrap();
    ws
}

/// The next data message, skipping relay notices.
async fn next_data(ws: &mut Ws) -> Message {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("a message in time")
            .unwrap()
            .unwrap();
        match &m {
            Message::Text(t) if t.as_str().contains("\"relay\"") => continue,
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => return m,
        }
    }
}

/// The next relay notice.
async fn next_notice(ws: &mut Ws) -> serde_json::Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("a notice in time")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = &m {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str()) {
                if v.get("relay").is_some() {
                    return v;
                }
            }
        }
    }
}

async fn nothing_for(ws: &mut Ws, ms: u64) -> bool {
    loop {
        match tokio::time::timeout(Duration::from_millis(ms), ws.next()).await {
            Err(_) => return true,
            Ok(Some(Ok(Message::Text(t)))) if t.as_str().contains("\"relay\"") => continue,
            Ok(_) => return false,
        }
    }
}

#[tokio::test]
async fn lists_public_shows_with_a_live_host() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let public = new_show(&addr, "Building night", true).await;
    let private = new_show(&addr, "Secret", false).await;
    // nothing is listed until its host connects
    let (_, body) = http(&addr, "GET", "/sessions", "", None).await;
    assert_eq!(serde_json::from_str::<Vec<Listed>>(&body).unwrap(), vec![]);
    let _h1 = join(
        &addr,
        &public.id,
        &format!("role=host&token={}", public.host_token),
    )
    .await;
    let _h2 = join(
        &addr,
        &private.id,
        &format!("role=host&token={}", private.host_token),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, body) = http(&addr, "GET", "/sessions", "", None).await;
    let listed: Vec<Listed> = serde_json::from_str(&body).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "Building night");
    assert_eq!(listed[0].character, "dog");
    let (st, page) = http(&addr, "GET", "/", "", None).await;
    assert_eq!(st, 200);
    assert!(page.contains("Building night") && !page.contains("Secret"));
}

#[tokio::test]
async fn titles_are_escaped_on_the_page() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let s = new_show(&addr, "<script>x</script>", true).await;
    let _h = join(&addr, &s.id, &format!("role=host&token={}", s.host_token)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, page) = http(&addr, "GET", "/", "", None).await;
    assert!(!page.contains("<script>x"));
    assert!(page.contains("&lt;script&gt;"));
}

#[tokio::test]
async fn host_reaches_viewers_and_guest_and_guest_reaches_host() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let s = new_show(&addr, "Show", true).await;
    let mut host = join(&addr, &s.id, &format!("role=host&token={}", s.host_token)).await;
    let mut v1 = join(&addr, &s.id, "role=viewer").await;
    let mut v2 = join(&addr, &s.id, "role=viewer").await;
    let mut guest = join(
        &addr,
        &s.id,
        &format!("role=guest&code={}&name=Sam", s.code),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    host.send(Message::Binary(vec![RELIABLE, 1, 2, 3].into()))
        .await
        .unwrap();
    host.send(Message::Binary(vec![DROPPABLE, 9].into()))
        .await
        .unwrap();
    for ws in [&mut v1, &mut v2, &mut guest] {
        assert_eq!(
            next_data(ws).await,
            Message::Binary(vec![RELIABLE, 1, 2, 3].into())
        );
        assert_eq!(
            next_data(ws).await,
            Message::Binary(vec![DROPPABLE, 9].into())
        );
    }
    // the guest's messages go to the host only
    guest
        .send(Message::Binary(vec![DROPPABLE, 7].into()))
        .await
        .unwrap();
    assert_eq!(
        next_data(&mut host).await,
        Message::Binary(vec![DROPPABLE, 7].into())
    );
    assert!(
        nothing_for(&mut v1, 150).await,
        "viewers don't get the guest's input"
    );
    // viewers can't send anything on
    v1.send(Message::Binary(vec![RELIABLE, 5].into()))
        .await
        .unwrap();
    assert!(nothing_for(&mut host, 150).await);
    assert!(nothing_for(&mut v2, 50).await);
}

#[tokio::test]
async fn ping_gets_pong() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let s = new_show(&addr, "Show", true).await;
    let mut v = join(&addr, &s.id, "role=viewer").await;
    v.send(Message::Text("ping:42".into())).await.unwrap();
    assert_eq!(next_data(&mut v).await, Message::Text("pong:42".into()));
}

#[tokio::test]
async fn host_hears_about_the_guest_and_viewers() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let s = new_show(&addr, "Show", true).await;
    let mut host = join(&addr, &s.id, &format!("role=host&token={}", s.host_token)).await;
    assert_eq!(next_notice(&mut host).await["n"], 0);
    let v = join(&addr, &s.id, "role=viewer").await;
    assert_eq!(next_notice(&mut host).await["n"], 1);
    let guest = join(
        &addr,
        &s.id,
        &format!("role=guest&code={}&name=Sam", s.code),
    )
    .await;
    let n = next_notice(&mut host).await;
    assert_eq!(n["relay"], "guest_joined");
    assert_eq!(n["name"], "Sam");
    drop(guest);
    assert_eq!(next_notice(&mut host).await["relay"], "guest_left");
    drop(v);
    assert_eq!(next_notice(&mut host).await["n"], 0);
}

#[tokio::test]
async fn wrong_token_or_code_is_refused() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let s = new_show(&addr, "Show", false).await;
    let url = |q: &str| format!("ws://{addr}/sessions/{}/ws?{q}", s.id);
    assert!(connect_async(url("role=host&token=nope")).await.is_err());
    assert!(connect_async(url("role=guest&code=AAAAAA")).await.is_err());
    assert!(
        connect_async(url("role=viewer")).await.is_err(),
        "private show needs the code"
    );
    assert!(
        connect_async(url(&format!("role=viewer&code={}", s.code.to_lowercase())))
            .await
            .is_ok()
    );
    assert!(
        connect_async(format!("ws://{addr}/sessions/nope/ws?role=viewer"))
            .await
            .is_err()
    );
    // one guest at a time
    let _g = join(&addr, &s.id, &format!("role=guest&code={}", s.code)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        connect_async(url(&format!("role=guest&code={}", s.code)))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn ending_needs_the_token_and_closes_viewers() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    let s = new_show(&addr, "Show", true).await;
    let mut v = join(&addr, &s.id, "role=viewer").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        http(
            &addr,
            "DELETE",
            &format!("/sessions/{}", s.id),
            "",
            Some("bad")
        )
        .await
        .0,
        401
    );
    assert_eq!(
        http(
            &addr,
            "DELETE",
            &format!("/sessions/{}", s.id),
            "",
            Some(&s.host_token)
        )
        .await
        .0,
        204
    );
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match v.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return true,
                _ => continue,
            }
        }
    })
    .await;
    assert_eq!(closed, Ok(true));
}

#[tokio::test]
async fn shows_expire_after_the_host_leaves() {
    let (addr, relay) = start(Duration::from_millis(200)).await;
    let s = new_show(&addr, "Show", true).await;
    let host = join(&addr, &s.id, &format!("role=host&token={}", s.host_token)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    relay.reap();
    assert!(
        connect_async(format!("ws://{addr}/sessions/{}/ws?role=viewer", s.id))
            .await
            .is_ok(),
        "host still there"
    );
    drop(host);
    tokio::time::sleep(Duration::from_millis(300)).await;
    relay.reap();
    assert!(
        connect_async(format!("ws://{addr}/sessions/{}/ws?role=viewer", s.id))
            .await
            .is_err(),
        "gone"
    );
}

#[tokio::test]
async fn bad_requests() {
    let (addr, _) = start(Duration::from_secs(30)).await;
    assert_eq!(
        http(&addr, "POST", "/sessions", r#"{"title":"   "}"#, None)
            .await
            .0,
        400
    );
    let s = new_show(&addr, "Show", true).await;
    assert!(
        connect_async(format!("ws://{addr}/sessions/{}/ws?role=boss", s.id))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn simulated_bad_network_delays_everything_and_loses_only_droppables() {
    let relay = Relay::new(Duration::from_secs(30)).simulate(Sim {
        delay: Duration::from_millis(200),
        loss: 1.0,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let r = relay.router();
    tokio::spawn(async move { axum::serve(listener, r).await.unwrap() });
    let s = new_show(&addr, "Show", true).await;
    let mut host = join(&addr, &s.id, &format!("role=host&token={}", s.host_token)).await;
    let mut v = join(&addr, &s.id, "role=viewer").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let t0 = std::time::Instant::now();
    host.send(Message::Binary(vec![DROPPABLE, 1].into()))
        .await
        .unwrap();
    host.send(Message::Binary(vec![RELIABLE, 2].into()))
        .await
        .unwrap();
    assert_eq!(
        next_data(&mut v).await,
        Message::Binary(vec![RELIABLE, 2].into())
    );
    let waited = t0.elapsed();
    assert!(waited >= Duration::from_millis(100), "delayed: {waited:?}");
    assert!(nothing_for(&mut v, 400).await, "the droppable one was lost");
}
