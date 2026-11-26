//! The link to a world server: addresses (portals, proxies), and how the desktop dials one
//! (the connection itself is vox-link's; the browser's is in `web_link`).

use super::*;

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

/// How the desktop dials a world server (vox-link does the rest): the "Can't reach" status is
/// what a portal trip watches for to turn back, a goodbye ends it, and anything queued while
/// disconnected is stale because the hello must go first.
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct WorldDial {
    pub url: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl vox_link::Dial<()> for WorldDial {
    fn address(&mut self, _: &dyn Fn(())) -> Result<String, String> {
        Ok(self.url.clone())
    }
    fn refused(&mut self, _status: u16, error: &str) -> String {
        format!("Can't reach the world: {error}")
    }
    fn goodbye(&self, message: &[u8]) -> bool {
        matches!(wire::decode::<ToClient>(message), Ok(ToClient::Bye { .. }))
    }
    fn drop_stale(&self) -> bool {
        true
    }
}
