//! What each address does: the page and its files, portraits, the session's state, asking, your questions, withdrawing one.

#[allow(unused_imports)]
use super::*;

pub(super) fn handle(sh: &AskShared, mut req: Request) {
    let ip = client_ip(&req);
    let path = req.url().split('?').next().unwrap_or("/").to_string();
    let query = req
        .url()
        .split_once('?')
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    let method = req.method().clone();
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(&ip, "any", 120, Duration::from_secs(60))
    {
        let _ = req.respond(err(429, "Too many requests. Slow down a little."));
        return;
    }
    let resp = match (&method, path.as_str()) {
        (Method::Get, "/") => with_common(
            Response::from_data(PAGE.as_bytes().to_vec()),
            "text/html; charset=utf-8",
        ),
        (Method::Get, "/app.js") => with_common(
            Response::from_data(APP_JS.as_bytes().to_vec()),
            "text/javascript; charset=utf-8",
        ),
        (Method::Get, "/app.css") => with_common(
            Response::from_data(APP_CSS.as_bytes().to_vec()),
            "text/css; charset=utf-8",
        ),
        (Method::Get, "/font.ttf") => match sh.cfg.lock().unwrap_or_else(|e| e.into_inner()).font.clone() {
            Some(f) => with_common(Response::from_data(f.to_vec()), "font/ttf"),
            None => err(404, "no font"),
        },
        (Method::Get, "/api/info") => info(sh, &req, &ip),
        (Method::Get, p) if p.starts_with("/portrait/") => portrait(sh, p),
        (Method::Post, "/api/login/twitch") => login_twitch(sh, &ip),
        (Method::Post, "/api/login/poll") => login_poll(sh, &req),
        (Method::Post, "/api/login/name") => match read_body(&mut req, 2048) {
            Ok(b) => login_name(sh, &b, &ip),
            Err(_) => err(413, "Too big."),
        },
        (Method::Post, "/api/logout") => {
            if let Some(t) = req_header(&req, SESSION_HEADER) {
                sh.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&t);
            }
            json_resp(200, json!({ "ok": true }))
        }
        (Method::Post, "/api/ask") => ask(sh, &mut req, &query, &ip),
        (Method::Get, "/api/mine") => mine(sh, &req),
        (Method::Post, "/api/withdraw") => withdraw(sh, &req, &query),
        _ => err(404, "Not found."),
    };
    let _ = req.respond(resp);
}

/// /portrait/<n>.png: the picture of character n (made by tools/assets/make_portraits.py).
pub(super) fn portrait(sh: &AskShared, p: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let n = p
        .strip_prefix("/portrait/")
        .and_then(|x| x.strip_suffix(".png"))
        .and_then(|x| x.parse::<usize>().ok());
    let model = n.and_then(|i| {
        sh.cfg
            .lock()
            .unwrap()
            .characters
            .get(i)
            .map(|c| c.1.clone())
    });
    let stem = model
        .as_deref()
        .and_then(|m| std::path::Path::new(m).file_stem())
        .map(|s| s.to_string_lossy().into_owned());
    match stem.and_then(|s| {
        std::fs::read(
            crate::asset_base()
                .join("assets/portraits")
                .join(format!("{s}.png")),
        )
        .ok()
    }) {
        Some(b) => with_common(Response::from_data(b), "image/png"),
        None => err(404, "No picture."),
    }
}

pub(super) fn session_login(sh: &AskShared, req: &Request) -> Option<Login> {
    let t = req_header(req, SESSION_HEADER)?;
    let mut s = sh.sessions.lock().unwrap_or_else(|e| e.into_inner());
    let sess = s.get_mut(&t)?;
    sess.last = Instant::now();
    sess.login.clone()
}

pub(super) fn new_session(sh: &AskShared, login: Option<Login>, device: Option<Device>) -> Option<String> {
    let mut s = sh.sessions.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if s.len() >= MAX_SESSIONS {
        s.retain(|_, v| {
            now.duration_since(v.last) < Duration::from_secs(6 * 3600)
                && (v.login.is_some() || now.duration_since(v.last) < Duration::from_secs(1800))
        });
        if s.len() >= MAX_SESSIONS {
            return None;
        }
    }
    let t = token();
    s.insert(
        t.clone(),
        Session {
            last: now,
            login,
            device,
        },
    );
    Some(t)
}

pub(super) fn info(sh: &AskShared, req: &Request, ip: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let me = session_login(sh, req);
    let q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
    let mode = if cfg.twitch_client_id.is_some() {
        "twitch"
    } else if cfg.open_names {
        "name"
    } else {
        "none"
    };
    let can = me
        .as_ref()
        .map(|m| match q.can_submit(&m.account, ip, unix_now()) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "reason": e.message() }),
        });
    json_resp(
        200,
        json!({
            "title": cfg.title, "open": q.open, "login": mode,
            "characters": cfg.characters.iter().map(|c| c.0.clone()).collect::<Vec<_>>(),
            "max_secs": q.limits.max_secs, "waiting": q.waiting().len(),
            "me": me.map(|m| json!({ "user": m.user, "verified": m.verified })),
            "can_ask": can,
        }),
    )
}

pub(super) fn query_get<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

pub(super) fn ask(
    sh: &AskShared,
    req: &mut Request,
    query: &str,
    ip: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(me) = session_login(sh, req) else {
        return err(401, "Please log in first.");
    };
    let cfg = sh.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(ch) = query_get(query, "c")
        .and_then(|c| c.parse::<usize>().ok())
        .and_then(|i| cfg.characters.get(i).cloned())
    else {
        return err(400, "Pick a character.");
    };
    // check before reading the upload, so a closed queue or a ban costs nothing
    let (limit, pre) = {
        let q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
        let limit =
            (q.limits.max_secs.min(crate::questions::MAX_SECS_HARD) + 1.0) as usize * 48000 * 2
                + 4096;
        (limit, q.can_submit(&me.account, ip, unix_now()))
    };
    if let Err(e) = pre {
        return err(
            if e == SubmitError::Banned { 403 } else { 409 },
            &e.message(),
        );
    }
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(ip, "upload", 6, Duration::from_secs(600))
    {
        return err(429, "Too many uploads. Wait a few minutes.");
    }
    let body = match read_body(req, limit) {
        Ok(b) => b,
        Err(_) => return err(413, "That recording is too big."),
    };
    let mut q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
    match q.submit(
        &me.user,
        me.verified,
        &me.account,
        if me.verified { "" } else { ip },
        (&ch.0, &ch.1),
        &body,
        unix_now(),
    ) {
        Ok(id) => {
            let pos = q.position(id).map(|p| p.0).unwrap_or(0);
            json_resp(200, json!({ "id": id, "position": pos }))
        }
        Err(e) => err(
            if matches!(e, SubmitError::Banned) {
                403
            } else {
                400
            },
            &e.message(),
        ),
    }
}

/// The viewer's own questions and where they stand. Never anyone else's.
pub(super) fn mine(sh: &AskShared, req: &Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(me) = session_login(sh, req) else {
        return err(401, "Please log in first.");
    };
    let q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
    let now = unix_now();
    let list: Vec<Value> = q
        .items
        .iter()
        .filter(|x| x.account == me.account && now.saturating_sub(x.submitted) < 12 * 3600)
        .map(|x| {
            let pos = q.position(x.id);
            // a question waiting for review shows as "waiting": no need to say it hasn't been heard
            let state = match x.status {
                Status::New => "waiting",
                Status::Approved => "approved",
                Status::OnAir => "on_air",
                Status::Done => "done",
                Status::Skipped => "skipped",
            };
            json!({ "id": x.id, "character": x.character, "state": state, "position": pos.map(|p| p.0), "of": pos.map(|p| p.1), "votes": x.votes.len() })
        })
        .collect();
    json_resp(200, json!({ "questions": list }))
}

pub(super) fn withdraw(sh: &AskShared, req: &Request, query: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(me) = session_login(sh, req) else {
        return err(401, "Please log in first.");
    };
    let Some(id) = query_get(query, "id").and_then(|i| i.parse::<u32>().ok()) else {
        return err(400, "Which question?");
    };
    let mut q = sh.queue.lock().unwrap_or_else(|e| e.into_inner());
    match q.get(id) {
        Some(x) if x.account == me.account && x.status.waiting() => {
            q.finish(id, Status::Skipped);
            json_resp(200, json!({ "ok": true }))
        }
        _ => err(404, "That question can't be withdrawn."),
    }
}
