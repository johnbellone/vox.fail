//! The link to a world server: addresses (portals, proxies), and on the desktop a thread that
//! keeps a WebSocket open, reconnecting with backoff (the browser's is in `web_link`).

use super::*;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// `ws://host:port` as given, or with `/ws` added; `http(s)://` becomes `ws(s)://`.
/// A portal's address: `:7781` is the world on port 7781 of the same host as `current`, or,
/// behind a proxy that puts worlds at `/w/<port>/ws` (as vox.fail does), the same host's
/// `/w/7781/ws`. `/some/path` is a path on the same host. Anything else is a full address.
pub fn portal_url(current: Option<&str>, target: &str) -> String {
    let t = target.trim();
    let cur = current.unwrap_or("ws://127.0.0.1:7780/ws");
    let (scheme, rest) = cur.split_once("://").unwrap_or(("ws", cur));
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
    if t.starts_with('/') {
        return format!("{scheme}://{hostport}{t}");
    }
    let Some(port) = t.strip_prefix(':') else { return t.to_string() };
    // worlds behind a proxy, by port in the path
    if let Some(p) = path.strip_prefix("w/") {
        if p.split('/').next().is_some_and(|n| n.parse::<u16>().is_ok()) {
            return format!("{scheme}://{hostport}/w/{port}/ws");
        }
    }
    // the host without its port (an IPv6 address keeps its brackets)
    let host = match hostport.rfind(':') {
        Some(i) if !hostport[i..].contains(']') => &hostport[..i],
        _ => hostport,
    };
    format!("{scheme}://{host}:{port}/ws")
}

/// The host of a world address, without scheme, port or path.
pub(super) fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let hostport = rest.split('/').next().unwrap_or("");
    match hostport.rfind(':') {
        Some(i) if !hostport[i..].contains(']') => &hostport[..i],
        _ => hostport,
    }
    .to_ascii_lowercase()
}

pub fn ws_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = if let Some(r) = u.strip_prefix("http://") {
        format!("ws://{r}")
    } else if let Some(r) = u.strip_prefix("https://") {
        format!("wss://{r}")
    } else if u.starts_with("ws://") || u.starts_with("wss://") {
        u.to_string()
    } else {
        format!("ws://{u}")
    };
    if u.ends_with("/ws") { u } else { format!("{u}/ws") }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn run_link(url: String, out: mpsc::Receiver<Vec<u8>>, inbox: mpsc::Sender<Incoming>, stop: Arc<AtomicBool>) {
    let mut wait = Duration::from_millis(500);
    while !stop.load(Ordering::Relaxed) {
        let mut sock = match tungstenite::connect(&url) {
            Ok((s, _)) => s,
            Err(e) => {
                let _ = inbox.send(Incoming::Status(format!("Can't reach the world: {e}")));
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_secs(10));
                continue;
            }
        };
        wait = Duration::from_millis(500);
        let short = Some(Duration::from_millis(5));
        match sock.get_mut() {
            tungstenite::stream::MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(short);
                let _ = s.set_nodelay(true);
            }
            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                let _ = s.get_mut().set_read_timeout(short);
                let _ = s.get_mut().set_nodelay(true);
            }
            _ => {}
        }
        // anything queued before connecting (old edits, poses) is stale: the hello goes first
        while out.try_recv().is_ok() {}
        let _ = inbox.send(Incoming::Status("Connected".into()));
        let mut bye = false;
        'live: while !stop.load(Ordering::Relaxed) {
            while let Ok(bytes) = out.try_recv() {
                if sock.send(tungstenite::Message::binary(bytes)).is_err() {
                    break 'live;
                }
            }
            loop {
                match sock.read() {
                    Ok(tungstenite::Message::Binary(b)) => {
                        bye |= matches!(wire::decode::<ToClient>(&b), Ok(ToClient::Bye { .. }));
                        let _ = inbox.send(Incoming::Data(b.to_vec()));
                    }
                    Ok(tungstenite::Message::Close(_)) => break 'live,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
                    {
                        break;
                    }
                    Err(_) => break 'live,
                }
            }
        }
        let _ = sock.close(None);
        if bye {
            return; // the server said why; trying again won't help
        }
        if !stop.load(Ordering::Relaxed) {
            let _ = inbox.send(Incoming::Status("Reconnecting".into()));
            std::thread::sleep(wait);
        }
    }
}
