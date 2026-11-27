//! Voice end to end: a real recording (testdata/real, never synthetic) is cut into 20 ms
//! ADPCM packets the way the client does, sent through a real world server over real
//! WebSockets, and what each listener receives is decoded and compared with the source.
//!
//! Cases: someone near hears it (and the decoded audio matches the recording), someone far
//! away does not, a raid party member hears the party channel from any distance, a far player
//! outside the party does not, and abuse (a flood, bad packets, a watcher) is dropped. Run
//! with `--nocapture` to see the table of results.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use vox_core::{
    auth::{self, Character, Claims},
    wire::{self, PROTOCOL, ToClient, ToServer, VOICE_NEARBY, VOICE_PARTY, VoicePacket},
};
use vox_server::{Config, Server};
use vox_voice::{Packet, Packetizer, RATE, resample::Resampler};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const LEVEL: &str = r#"{ "world": { "sky": "void" }, "boxes": [ { "block": "grass", "from": [-8, -1, -8], "to": [8, -1, 8] } ] }"#;
/// Packets sent per case (1.8 s of speech): under the server's burst of 100.
const SENT: usize = 90;
/// Slack for a busy machine, not a delay the server has: relaying is immediate on localhost.
const WAIT: Duration = Duration::from_millis(400);

struct Player {
    ws: Ws,
    id: u32,
}

async fn serve(cfg: Config) -> String {
    let server = Server::start(cfg).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = server.router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    // keep the server alive for the whole test
    std::mem::forget(server);
    format!("ws://{addr}/ws")
}

fn config() -> Config {
    let mut c = Config::new(":memory:", ["grass", "stone"].map(String::from).to_vec());
    c.name = "test".into();
    c.seed = Some(LEVEL.into());
    c.trust_moves = true;
    c
}

async fn recv(ws: &mut Ws) -> ToClient {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timed out").unwrap().unwrap();
        if let Message::Binary(b) = m {
            return wire::decode(&b).unwrap();
        }
    }
}

async fn send(ws: &mut Ws, m: &ToServer) {
    ws.send(Message::Binary(wire::encode(m).into())).await.unwrap();
}

async fn join(url: &str, name: &str, token: Option<String>) -> Player {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(&mut ws, &ToServer::Hello { protocol: PROTOCOL, name: name.into(), avatar: vec![], token }).await;
    let ToClient::Welcome { you, .. } = recv(&mut ws).await else { panic!("no welcome") };
    loop {
        if matches!(recv(&mut ws).await, ToClient::Ready) {
            break;
        }
    }
    Player { ws, id: you }
}

/// Everyone sends a position, then waits to have heard everyone else's, so the server knows
/// where all of them are before any voice is sent.
async fn place(players: &mut [&mut Player], spots: &[[f32; 3]]) {
    for (p, at) in players.iter_mut().zip(spots) {
        send(&mut p.ws, &ToServer::Pose { at: *at, blob: vec![] }).await;
    }
    let n = players.len();
    for p in players.iter_mut() {
        let mut seen = Vec::new();
        while seen.len() < n - 1 {
            if let ToClient::Pose { player, .. } = recv(&mut p.ws).await
                && player != p.id
                && !seen.contains(&player)
            {
                seen.push(player);
            }
        }
    }
}

async fn heard(ws: &mut Ws) -> Vec<(u32, VoicePacket)> {
    let mut got = Vec::new();
    let end = tokio::time::Instant::now() + WAIT;
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout_at(end, ws.next()).await {
        if let Ok(ToClient::Voice { player, packet }) = wire::decode::<ToClient>(&b) {
            got.push((player, packet));
        }
    }
    got
}

/// The recording as 16 kHz mono samples and as the packets a client would send.
fn speech() -> (Vec<f32>, Vec<Packet>) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/real/LJ001-0004.wav");
    let (rate, channels, interleaved) = avatar_voice::wav::read_wav(path).expect("a real recording");
    let mono: Vec<f32> = interleaved.chunks(channels.max(1)).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect();
    let mut at16k = Vec::new();
    Resampler::new(rate, RATE).push(&mono, &mut at16k);
    let mut packets = Vec::new();
    let mut p = Packetizer::new();
    p.set_ptt(Some(true)); // always send: the recording has pauses, and we want all of it
    // in 20 ms pieces, as a microphone delivers it (the packetizer holds at most a second)
    for chunk in at16k.chunks(320) {
        p.push(chunk, &mut packets);
    }
    assert!(packets.len() >= SENT, "the recording is too short: {} packets", packets.len());
    packets.truncate(SENT);
    (at16k, packets)
}

fn wire_packet(channel: u8, p: &Packet) -> VoicePacket {
    VoicePacket { channel, seq: p.seq, samples: p.samples, pred: p.pred, index: p.index, data: p.data.clone() }
}

fn decode(v: &VoicePacket) -> Vec<f32> {
    Packet { seq: v.seq, samples: v.samples, pred: v.pred, index: v.index, data: v.data.clone() }.decode().expect("valid packet")
}

/// How alike two signals are: 1.0 for the same shape, near 0 for unrelated.
fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let (x, y) = (a[i] as f64, b[i] as f64);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    (ab / (aa.sqrt() * bb.sqrt()).max(1e-12)) as f32
}

async fn speak(who: &mut Player, channel: u8, packets: &[Packet]) {
    for p in packets {
        send(&mut who.ws, &ToServer::Voice(wire_packet(channel, p))).await;
    }
}

#[tokio::test]
async fn voice_reaches_who_it_should_and_nobody_else() {
    let (source, packets) = speech();
    let mut table: Vec<(&str, String, bool)> = Vec::new();

    // a stand-in directory: accounts 1 and 2 are in one raid party; 3 and 4 are in none
    let dir = axum::Router::new().route(
        "/api/world/test/rules",
        axum::routing::get(|h: axum::http::HeaderMap| async move {
            if h.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer s3cret") {
                return axum::Json(serde_json::json!({}));
            }
            axum::Json(serde_json::json!({ "plots": [], "portals": [], "parties": [[1, 2]] }))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, dir).await.unwrap() });
    let key = auth::key_from_seed([9; 32]);
    let mut cfg = config();
    cfg.directory = Some(key.verifying_key());
    cfg.directory_url = Some(dir_url);
    cfg.world_secret = Some("s3cret".into());
    cfg.rules_every = Duration::from_millis(100);
    cfg.report_every = Duration::from_secs(600);
    let url = serve(cfg).await;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let token = |account: u64, name: &str| {
        auth::sign(&Claims { account, name: name.into(), character: Character::default(), issued: now, expires: now + 60 }, &key)
    };

    // Ann (party), Ben (party, 200 blocks away), Cal (no party, next to Ann), Dee (no party, far)
    let mut ann = join(&url, "Ann", Some(token(1, "Ann"))).await;
    let mut ben = join(&url, "Ben", Some(token(2, "Ben"))).await;
    let mut cal = join(&url, "Cal", Some(token(3, "Cal"))).await;
    let mut dee = join(&url, "Dee", Some(token(4, "Dee"))).await;
    place(
        &mut [&mut ann, &mut ben, &mut cal, &mut dee],
        &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0], [5.0, 0.0, 0.0], [150.0, 0.0, 0.0]],
    )
    .await;

    // 1. nearby: Cal hears the recording; the decoded audio matches the source
    speak(&mut ann, VOICE_NEARBY, &packets).await;
    let got = heard(&mut cal.ws).await;
    let ok = got.len() == SENT && got.iter().all(|(p, v)| *p == ann.id && v.channel == VOICE_NEARBY);
    let decoded: Vec<f32> = got.iter().flat_map(|(_, v)| decode(v)).collect();
    let corr = correlation(&decoded, &source);
    table.push(("nearby: Cal (5 blocks) hears Ann", format!("{} packets, correlation {corr:.3} with the recording", got.len()), ok && corr > 0.9));
    // 2. nearby: Ben and Dee are far, so they hear nothing of it
    let (b, d) = (heard(&mut ben.ws).await, heard(&mut dee.ws).await);
    table.push(("nearby: Ben (200) and Dee (150) do not", format!("{} and {} packets", b.len(), d.len()), b.is_empty() && d.is_empty()));
    // the sender never hears herself
    let a = heard(&mut ann.ws).await;
    table.push(("nearby: Ann does not hear herself", format!("{} packets", a.len()), a.is_empty()));

    // 3. party: Ben hears Ann from 200 blocks (the rules reach the server on a timer, so the
    //    first packets may be dropped until they land: resend a probe until one gets through)
    let probe = wire_packet(VOICE_PARTY, &packets[0]);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "the party's rules never reached the server");
        send(&mut ann.ws, &ToServer::Voice(probe.clone())).await;
        if !heard(&mut ben.ws).await.is_empty() {
            break;
        }
    }
    let _ = heard(&mut ben.ws).await; // probes still in flight
    let (_, _, _) = (heard(&mut cal.ws).await, heard(&mut dee.ws).await, heard(&mut ann.ws).await);
    speak(&mut ann, VOICE_PARTY, &packets).await;
    let got = heard(&mut ben.ws).await;
    let ok = got.len() == SENT && got.iter().all(|(p, v)| *p == ann.id && v.channel == VOICE_PARTY);
    let decoded: Vec<f32> = got.iter().flat_map(|(_, v)| decode(v)).collect();
    let corr = correlation(&decoded, &source);
    table.push(("party: Ben (200 blocks, same party) hears Ann", format!("{} packets, correlation {corr:.3}", got.len()), ok && corr > 0.9));
    // 4. party: Cal is close but not in the party; Dee is far and not in it
    let (c, d) = (heard(&mut cal.ws).await, heard(&mut dee.ws).await);
    table.push(("party: Cal (near) and Dee (far), no party, do not", format!("{} and {} packets", c.len(), d.len()), c.is_empty() && d.is_empty()));

    // 5. abuse: bad packets, a flood and a watcher are dropped
    let mut bad = wire_packet(VOICE_NEARBY, &packets[0]);
    bad.data.truncate(10);
    let mut worse = wire_packet(VOICE_NEARBY, &packets[0]);
    worse.index = 200;
    let mut other = wire_packet(VOICE_NEARBY, &packets[0]);
    other.channel = 2;
    for v in [bad, worse, other] {
        send(&mut ann.ws, &ToServer::Voice(v)).await;
    }
    let got = heard(&mut cal.ws).await;
    table.push(("abuse: short, bad-index and bad-channel packets dropped", format!("{} packets", got.len()), got.is_empty()));

    let started = tokio::time::Instant::now();
    for i in 0..500 {
        send(&mut ann.ws, &ToServer::Voice(wire_packet(VOICE_NEARBY, &packets[i % SENT]))).await;
    }
    let secs = started.elapsed().as_secs_f64();
    let got = heard(&mut cal.ws).await;
    let allowed = 100.0 + 60.0 * (secs + WAIT.as_secs_f64()) + 10.0;
    table.push(("abuse: 500 packets in a burst are rate limited", format!("{} delivered of 500 (at most {allowed:.0})", got.len()), got.len() >= 50 && (got.len() as f64) <= allowed));

    let (mut watcher, _) = connect_async(&url).await.unwrap();
    send(&mut watcher, &ToServer::Watch { protocol: PROTOCOL }).await;
    loop {
        if matches!(recv(&mut watcher).await, ToClient::Ready) {
            break;
        }
    }
    // let the flood's tail finish, then speak once more and see that the watcher hears nothing
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let _ = heard(&mut cal.ws).await;
    send(&mut ann.ws, &ToServer::Voice(wire_packet(VOICE_NEARBY, &packets[1]))).await;
    let mut w = Vec::new();
    let end = tokio::time::Instant::now() + WAIT;
    while let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout_at(end, watcher.next()).await {
        if let Ok(ToClient::Voice { .. }) = wire::decode::<ToClient>(&b) {
            w.push(());
        }
    }
    table.push(("abuse: a watcher hears no voice", format!("{} packets", w.len()), w.is_empty()));

    println!("\nvoice end to end (recording: testdata/real/LJ001-0004.wav, {SENT} packets of 20 ms)");
    for (case, result, ok) in &table {
        println!("  [{}] {case}: {result}", if *ok { "ok" } else { "FAIL" });
    }
    assert!(table.iter().all(|(_, _, ok)| *ok), "some cases failed, see the table above");
}
