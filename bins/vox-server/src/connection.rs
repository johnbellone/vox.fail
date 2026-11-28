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

/// Who may come in: the directory's key (None: anyone, under any name), whether guests may too,
/// this world's id (what tickets must name), and the tickets already used.
pub(super) struct Gate {
    pub key: Option<vox_core::auth::VerifyingKey>,
    pub guests: bool,
    pub world: String,
    /// ticket nonces seen, with when each ticket runs out (forgotten after that)
    pub used: Mutex<HashMap<u64, u64>>,
}

/// Tickets remembered at most; more than this many unexpired at once and new ones are refused
/// (a minute's worth of arrivals is far fewer).
const MAX_USED_TICKETS: usize = 100_000;

/// Someone let in: the name they go by, their account, and where a ticket says they arrive.
#[derive(Debug, PartialEq)]
pub(super) struct Admitted {
    pub name: String,
    pub account: Option<u64>,
    pub arrive: Option<[f32; 3]>,
}

/// May this player in, and under what name? With a directory key, a good token gives their
/// name and account, and a ticket also where they arrive (once: a ticket used before is
/// refused); without either they're a guest (if guests are let in). Without a key, any name
/// goes.
pub(super) fn admit(gate: &Gate, name: String, token: Option<&str>, now: u64) -> Result<Admitted, String> {
    let Some(key) = &gate.key else { return Ok(Admitted { name, account: None, arrive: None }) };
    match token {
        Some(t) if vox_core::auth::is_ticket(t) => {
            let t = vox_core::auth::verify_ticket(t, key, &gate.world, now)?;
            let mut used = gate.used.lock().unwrap_or_else(|e| e.into_inner());
            used.retain(|_, until| *until > now);
            if used.len() >= MAX_USED_TICKETS {
                return Err("too many arrivals at once: try again in a minute".into());
            }
            if used.insert(t.nonce, t.claims.expires).is_some() {
                return Err("that ticket has been used: go through again".into());
            }
            Ok(Admitted { name: t.claims.name, account: Some(t.claims.account), arrive: Some(t.arrive) })
        }
        Some(t) => vox_core::auth::verify(t, key, now).map(|c| Admitted { name: c.name, account: Some(c.account), arrive: None }),
        None if gate.guests => {
            let n = clean(&name, 24);
            Ok(Admitted { name: if n.is_empty() { "guest".into() } else { format!("{n} (guest)") }, account: None, arrive: None })
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
    let Admitted { name, account, arrive } = match admit(&server.gate, name, token.as_deref(), unix_now()) {
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
    if server.tx.send(Event::Join { conn, tx, protocol, name, avatar, account, arrive }).await.is_err() {
        return;
    }
    // poses and voice have no limit further on: cap them here, before they queue. Each has its
    // own count in the same one-second window.
    let (mut window, mut poses, mut voices) = (Instant::now(), 0u32, 0u32);
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
                            (window, poses, voices) = (Instant::now(), 0, 0);
                        }
                        poses += 1;
                        if poses > MAX_POSES_PER_SECOND {
                            continue;
                        }
                    }
                    if let ToServer::Voice(packet) = &msg {
                        // a packet of any other size is dropped, not a reason to disconnect
                        if packet.data.len() > wire::VOICE_BYTES {
                            continue;
                        }
                        if window.elapsed() >= Duration::from_secs(1) {
                            (window, poses, voices) = (Instant::now(), 0, 0);
                        }
                        voices += 1;
                        if voices > MAX_VOICE_PER_SECOND {
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

/// Voice packets per connection per second. Over this, packets are dropped here, before they
/// reach the world thread.
const MAX_VOICE_PER_SECOND: u32 = 120;

/// Raid parties from the directory's rules: an array of arrays of account ids. A missing or
/// malformed key is no parties; non-numeric members are skipped. Capped, because the list
/// comes from over the network: at most 500 parties of 24 members each.
fn parse_parties(v: &serde_json::Value) -> Vec<Vec<u64>> {
    const MAX_PARTIES: usize = 500;
    const MAX_MEMBERS: usize = 24;
    let Some(parties) = v.as_array() else { return Vec::new() };
    parties
        .iter()
        .take(MAX_PARTIES)
        .filter_map(|p| {
            let members: Vec<u64> = p.as_array()?.iter().filter_map(|m| m.as_u64()).take(MAX_MEMBERS).collect();
            (!members.is_empty()).then_some(members)
        })
        .collect()
}

/// Talking to the directory: rules every `rules_every`, a report every `report_every`, and the
/// rules at once when `rules_now` is woken (the directory's portals changed). A directory that's
/// down is tried again next time; the world goes on meanwhile.
pub(super) async fn directory_link(
    tx: mpsc::Sender<Event>,
    (url, secret, world, report_every, rules_every): (String, String, String, Duration, Duration),
    rules_now: Arc<Notify>,
) {
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
            let started = Instant::now();
            let got = http.get(format!("{url}/api/world/{world}/rules")).bearer_auth(&secret).send().await;
            match got {
                Ok(r) if r.status().is_success() => {
                    if let Ok(v) = r.json::<serde_json::Value>().await {
                        let plots = v["plots"].as_array().map(|a| a.iter().filter_map(Plot::from_json).collect()).unwrap_or_default();
                        let portals = v["portals"].as_array().cloned().unwrap_or_default();
                        let parties = parse_parties(&v["parties"]);
                        if tx.send(Event::Rules(plots, portals, parties, started)).await.is_err() {
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
        let nap = tokio::time::sleep(wake.saturating_duration_since(Instant::now()).max(Duration::from_millis(20)));
        tokio::select! {
            _ = nap => {}
            _ = rules_now.notified() => next_rules = Instant::now(),
        }
    }
}

/// Telling the directory a world's objective was reached, and by whom (it decides what that
/// wins: a raid on the dungeon). Tried a few times; a directory that stays down misses it.
pub(super) async fn objective_reached(mut goals: mpsc::Receiver<Vec<u64>>, url: String, secret: String, world: String) {
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("no link to the directory for objectives: {e}"),
    };
    while let Some(players) = goals.recv().await {
        let body = serde_json::json!({ "world": world, "players": players });
        for attempt in 0..3u64 {
            match http.post(format!("{url}/api/world/objective")).bearer_auth(&secret).json(&body).send().await {
                Ok(r) if r.status().is_success() || r.status().is_client_error() => break,
                _ => tokio::time::sleep(Duration::from_secs(2 << attempt)).await,
            }
        }
    }
}

/// Asking the directory for tickets: the world thread hands over trips ([`Trip`]) and this asks
/// the directory for each, then tells the world how it went ([`Event::Ticket`]). A directory
/// that's down or doesn't know the way gives no ticket; the player is then sent on without one.
pub(super) async fn ticket_office(tx: mpsc::Sender<Event>, mut trips: mpsc::Receiver<Trip>, url: String, secret: String, world: String) {
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("no link to the directory for tickets: {e}"),
    };
    while let Some(trip) = trips.recv().await {
        let http = http.clone();
        let (tx, url, secret, world) = (tx.clone(), url.clone(), secret.clone(), world.clone());
        // one at a time would hold everyone up behind a slow answer
        tokio::spawn(async move {
            let body = serde_json::json!({ "world": world, "account": trip.account, "to": trip.to, "arrive": trip.arrive, "motion": trip.motion });
            let got = http.post(format!("{url}/api/world/travel")).bearer_auth(&secret).json(&body).send().await;
            let answer = match got {
                Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
                Ok(r) => {
                    eprintln!("the directory gave no ticket to {} ({})", trip.to, r.status());
                    None
                }
                Err(e) => {
                    eprintln!("can't reach the directory for a ticket: {e}");
                    None
                }
            };
            let ticket = answer.as_ref().and_then(|a| a["ticket"].as_str()).map(String::from);
            let to = answer.as_ref().and_then(|a| a["url"].as_str()).map_or(trip.to, String::from);
            let _ = tx.send(Event::Ticket { conn: trip.conn, url: to, ticket, arrive: trip.arrive, motion: trip.motion, frame: trip.frame }).await;
        });
    }
}

/// Asking the directory whether an account may make advanced edits here. Each question runs on
/// its own task, so one slow answer holds nobody else up. Any failure (no answer in five
/// seconds, an error, a non-200) is `None`, which the world refuses the edit for: advanced
/// rights fail closed.
pub(super) async fn editor_office(tx: mpsc::Sender<Event>, mut asks: mpsc::Receiver<Asking>, url: String, secret: String, world: String) {
    let http = match reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(h) => h,
        Err(e) => return eprintln!("no link to the directory for editors: {e}"),
    };
    while let Some(ask) = asks.recv().await {
        let (http, tx, url, secret, world) = (http.clone(), tx.clone(), url.clone(), secret.clone(), world.clone());
        tokio::spawn(async move {
            // the account is a number the world holds, not text from a player
            let got = http.get(format!("{url}/api/world/{world}/editor?account={}", ask.account)).bearer_auth(&secret).send().await;
            let editor = match got {
                Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok().and_then(|v| v["editor"].as_bool()),
                Ok(r) => {
                    eprintln!("the directory gave no editor answer ({})", r.status());
                    None
                }
                Err(e) => {
                    eprintln!("can't reach the directory for an editor check: {e}");
                    None
                }
            };
            let _ = tx.send(Event::Editor { conn: ask.conn, account: ask.account, editor }).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::parse_parties;
    use serde_json::json;

    #[test]
    fn parses_parties() {
        let v = json!([[1, 2, 3], [4]]);
        assert_eq!(parse_parties(&v), vec![vec![1, 2, 3], vec![4]]);
    }

    #[test]
    fn missing_or_wrong_shape_is_empty() {
        assert!(parse_parties(&serde_json::Value::Null).is_empty());
        assert!(parse_parties(&json!({ "a": 1 })).is_empty());
        assert!(parse_parties(&json!("[1,2]")).is_empty());
        assert!(parse_parties(&json!([])).is_empty());
    }

    #[test]
    fn skips_non_numeric_members_and_empty_parties() {
        let v = json!([[1, "x", -5, 2.5, null, 7], ["a"], [], 9, [8]]);
        assert_eq!(parse_parties(&v), vec![vec![1, 7], vec![8]]);
    }

    #[test]
    fn caps_parties_and_members() {
        let many: Vec<Vec<u64>> = (0..600u64).map(|i| vec![i]).collect();
        assert_eq!(parse_parties(&json!(many)).len(), 500);
        let big = json!([(0..100u64).collect::<Vec<_>>()]);
        let got = parse_parties(&big);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len(), 24);
    }
}
