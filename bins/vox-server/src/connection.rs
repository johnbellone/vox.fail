//! Connections: the status page, the WebSocket upgrade, admitting players (their sign-in), the
//! connection tasks for players and watchers, and the link to the directory.

#[allow(unused_imports)]
use super::*;

pub(super) async fn status_page(State(server): State<Server>) -> Json<serde_json::Value> {
    Json(server.status().await.map(|s| s.json()).unwrap_or(serde_json::json!({ "error": "stopped" })))
}

pub(super) async fn connect(State(server): State<Server>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(MAX_MESSAGE).on_upgrade(move |socket| run_connection(server, socket))
}

pub(super) fn clean(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(max).collect()
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// May this player in, and under what name? With a directory key, a good token gives their
/// name and account; without one they're a guest (if guests are let in). Without a key, any
/// name goes.
pub(super) fn admit(
    gate: &(Option<vox_core::auth::VerifyingKey>, bool),
    name: String,
    token: Option<&str>,
    now: u64,
) -> Result<(String, Option<u64>), String> {
    let Some(key) = &gate.0 else { return Ok((name, None)) };
    match token {
        Some(t) => vox_core::auth::verify(t, key, now).map(|c| (c.name, Some(c.account))),
        None if gate.1 => {
            let n = clean(&name, 24);
            Ok((if n.is_empty() { "guest".into() } else { format!("{n} (guest)") }, None))
        }
        None => Err("this world needs you to sign in first".into()),
    }
}

/// A watcher's connection: the world goes out, nothing comes in. Anything they send ends it.
pub(super) async fn watch_connection(
    server: Server,
    conn: u64,
    protocol: u32,
    mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
    mut stream: futures_util::stream::SplitStream<WebSocket>,
) {
    let (tx, mut rx) = mpsc::channel::<Message>(QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    if server.tx.send(Event::Watch { conn, tx, protocol }).await.is_err() {
        return;
    }
    while let Some(Ok(m)) = stream.next().await {
        match m {
            Message::Binary(_) | Message::Text(_) | Message::Close(_) => break,
            _ => {}
        }
        if writer.is_finished() {
            break;
        }
    }
    let _ = server.tx.send(Event::Leave { conn }).await;
    writer.abort();
}

pub(super) async fn run_connection(server: Server, socket: WebSocket) {
    let conn = server.next_conn.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    // the first message must be a hello
    let hello = tokio::time::timeout(HELLO_WITHIN, async {
        while let Some(Ok(m)) = stream.next().await {
            if let Message::Binary(b) = m {
                return wire::decode::<ToServer>(&b).ok();
            }
        }
        None
    })
    .await;
    let (protocol, name, avatar, token) = match hello {
        Ok(Some(ToServer::Hello { protocol, name, avatar, token })) => (protocol, name, avatar, token),
        Ok(Some(ToServer::Watch { protocol })) => return watch_connection(server, conn, protocol, sink, stream).await,
        _ => {
            let bye = wire::encode(&ToClient::Bye { why: "say hello first".into() });
            let _ = sink.send(Message::Binary(bye.into())).await;
            return;
        }
    };
    // who they are: from their sign-in, when this world checks it
    let (name, account) = match admit(&server.gate, name, token.as_deref(), unix_now()) {
        Ok(x) => x,
        Err(why) => {
            let bye = wire::encode(&ToClient::Bye { why });
            let _ = sink.send(Message::Binary(bye.into())).await;
            return;
        }
    };
    let (tx, mut rx) = mpsc::channel::<Message>(QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let name = clean(&name, 32);
    if server.tx.send(Event::Join { conn, tx, protocol, name, avatar, account }).await.is_err() {
        return;
    }
    // poses are the one message with no limit further on: cap them here, before they queue
    let (mut window, mut poses) = (Instant::now(), 0u32);
    while let Some(Ok(m)) = stream.next().await {
        match m {
            Message::Binary(b) => match wire::decode::<ToServer>(&b) {
                Ok(ToServer::Hello { .. }) | Err(_) => break,
                Ok(msg) => {
                    if let ToServer::Pose { blob, .. } = &msg {
                        if blob.len() > MAX_POSE_BLOB {
                            break;
                        }
                        if window.elapsed() >= Duration::from_secs(1) {
                            (window, poses) = (Instant::now(), 0);
                        }
                        poses += 1;
                        if poses > MAX_POSES_PER_SECOND {
                            continue;
                        }
                    }
                    if server.tx.send(Event::Msg { conn, msg }).await.is_err() {
                        break;
                    }
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
        if writer.is_finished() {
            break;
        }
    }
    let _ = server.tx.send(Event::Leave { conn }).await;
    let _ = writer.await;
}

/// Talking to the directory: rules every `rules_every`, a report every `report_every`. A
/// directory that's down is tried again next time; the world goes on meanwhile.
pub(super) async fn directory_link(tx: mpsc::Sender<Event>, (url, secret, world, report_every, rules_every): (String, String, String, Duration, Duration)) {
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("no link to the directory: {e}"),
    };
    let mut next_report = Instant::now() + report_every;
    let mut next_rules = Instant::now();
    let mut said = false;
    loop {
        if Instant::now() >= next_rules {
            next_rules = Instant::now() + rules_every;
            let got = http.get(format!("{url}/api/world/{world}/rules")).bearer_auth(&secret).send().await;
            match got {
                Ok(r) if r.status().is_success() => {
                    if let Ok(v) = r.json::<serde_json::Value>().await {
                        let plots = v["plots"].as_array().map(|a| a.iter().filter_map(Plot::from_json).collect()).unwrap_or_default();
                        let portals = v["portals"].as_array().cloned().unwrap_or_default();
                        if tx.send(Event::Rules(plots, portals)).await.is_err() {
                            return;
                        }
                        said = false;
                    }
                }
                Ok(r) if !said => {
                    eprintln!("the directory said {} to this world ({world}): check --directory-url, --name and VOX_WORLD_SECRET", r.status());
                    said = true;
                }
                Err(e) if !said => {
                    eprintln!("can't reach the directory at {url}: {e}");
                    said = true;
                }
                _ => {}
            }
        }
        if Instant::now() >= next_report {
            next_report = Instant::now() + report_every;
            let (rtx, rrx) = oneshot::channel();
            if tx.send(Event::Report(rtx)).await.is_err() {
                return;
            }
            if let Ok(body) = rrx.await {
                let _ = http.post(format!("{url}/api/world/report")).bearer_auth(&secret).json(&body).send().await;
            }
        }
        let wake = next_rules.min(next_report);
        tokio::time::sleep(wake.saturating_duration_since(Instant::now()).max(Duration::from_millis(20))).await;
    }
}
