//! Small HTTP helpers: headers, JSON answers, the asker's address, reading a body with a limit.

#[allow(unused_imports)]
use super::*;

pub(super) fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ascii header")
}

pub(super) fn with_common<R: Read>(r: Response<R>, ctype: &str) -> Response<R> {
    r.with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
        .with_header(header("Referrer-Policy", "no-referrer"))
        .with_header(header("X-Frame-Options", "DENY"))
        .with_header(header(
            "Content-Security-Policy",
            "default-src 'self'; media-src 'self' blob:; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ))
        .with_header(header("Permissions-Policy", "microphone=(self), camera=()"))
}

pub(super) fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    with_common(
        Response::from_data(v.to_string().into_bytes()).with_status_code(code),
        "application/json",
    )
}

pub(super) fn err(code: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_resp(code, json!({ "error": msg }))
}

pub(super) fn req_header(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

/// The viewer's address: Cloudflare's header when the request came through the local tunnel.
pub(super) fn client_ip(req: &Request) -> String {
    let peer = req.remote_addr().map(|a| a.ip());
    if peer.is_some_and(|p| p.is_loopback()) {
        if let Some(ip) = req_header(req, "cf-connecting-ip").or_else(|| {
            req_header(req, "x-forwarded-for")
                .and_then(|v| v.split(',').next().map(|s| s.trim().to_string()))
        }) {
            if ip.parse::<std::net::IpAddr>().is_ok() {
                return ip;
            }
        }
    }
    peer.map(|p| p.to_string()).unwrap_or_default()
}

pub(super) fn read_body(req: &mut Request, limit: usize) -> Result<Vec<u8>, &'static str> {
    if req.body_length().is_some_and(|n| n > limit) {
        return Err("too big");
    }
    let mut b = Vec::new();
    req.as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut b)
        .map_err(|_| "read error")?;
    if b.len() > limit {
        return Err("too big");
    }
    Ok(b)
}
