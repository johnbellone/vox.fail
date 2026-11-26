//! The account pages' API: signing up, in and out, who you are, the catalogue, your character,
//! launch tokens, the worlds, the public key; and the headers every answer gets.

#[allow(unused_imports)]
use super::*;

/// Every response: no framing by other sites, no sniffing, and a tight content policy for
/// the pages (the browser client needs WebAssembly). And what Cloudflare (or a browser) may
/// keep: the game's files for five minutes (purge Cloudflare's cache after a release), the
/// API and sign-ins never, the pages only after checking back.
pub(super) async fn headers(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path().to_string();
    let mut r = next.run(req).await;
    let keep = if path.starts_with("/api/") || path.starts_with("/auth/") {
        "no-store"
    } else if ["/pkg/", "/assets/", "/data/"].iter().any(|p| path.starts_with(p)) && r.status().is_success() {
        "public, max-age=300"
    } else {
        "no-cache"
    };
    let h = r.headers_mut();
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static(keep));
    }
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; \
             img-src 'self' data: blob:; connect-src 'self' blob: data: ws: wss: http: https:; frame-ancestors 'none'",
        ),
    );
    r
}

pub(super) fn json_only(h: &HeaderMap) -> Res<()> {
    let ok = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    if ok { Ok(()) } else { Err(ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "send JSON".into())) }
}

#[derive(Deserialize)]
pub(super) struct Login {
    name: String,
    password: String,
}

pub(super) async fn signup(
    State(d): State<Directory>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(l): Json<Login>,
) -> Res<Response> {
    json_only(&h)?;
    if d.tried(&d.visitor(addr, &h)) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many tries: wait a few minutes".into()));
    }
    let name = check_name(&l.name).map_err(bad)?;
    if !(8..=128).contains(&l.password.chars().count()) {
        return Err(bad("passwords are 8 to 128 characters"));
    }
    let pass = l.password.clone();
    let hash = d
        .hash_work(move || Argon2::default().hash_password(pass.as_bytes(), &salt()).map(|h| h.to_string()))
        .await
        .ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "busy: try again in a moment".into()))?
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "try again".into()))?;
    let id = {
        let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
        match db.execute("INSERT INTO accounts (name, pass, created) VALUES (?1, ?2, ?3)", params![name, hash, now() as i64]) {
            Ok(_) => {
                let id = db.last_insert_rowid() as u64;
                d.welcome(&db, id);
                id
            }
            Err(_) => return Err(ApiError(StatusCode::CONFLICT, "that name is taken".into())),
        }
    };
    Ok(([(header::SET_COOKIE, d.session(id))], Json(json!({ "name": name }))).into_response())
}

pub(super) async fn signin(
    State(d): State<Directory>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(l): Json<Login>,
) -> Res<Response> {
    json_only(&h)?;
    let lname: String = l.name.trim().to_lowercase().chars().take(32).collect();
    if d.tried(&d.visitor(addr, &h)) || d.tried(&format!("name:{lname}")) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many tries: wait a few minutes".into()));
    }
    let row: Option<(i64, String, String)> = {
        let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT id, name, pass FROM accounts WHERE name = ?1", params![l.name.trim()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()
        .unwrap_or(None)
    };
    let pass = l.password.clone();
    // check a password even for a name that doesn't exist, so the time taken says nothing
    let stored = row.as_ref().map_or_else(|| d.0.dummy.clone(), |r| r.2.clone());
    let Some(ok) = d
        .hash_work(move || PasswordHash::new(&stored).is_ok_and(|p| Argon2::default().verify_password(pass.as_bytes(), &p).is_ok()))
        .await
    else {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "busy: try again in a moment".into()));
    };
    match row {
        Some((id, name, _)) if ok => Ok(([(header::SET_COOKIE, d.session(id as u64))], Json(json!({ "name": name }))).into_response()),
        _ => Err(ApiError(StatusCode::UNAUTHORIZED, "wrong name or password".into())),
    }
}

pub(super) async fn signout(State(d): State<Directory>, h: HeaderMap) -> Response {
    if let Some(c) = h.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in c.split(';') {
            if let Some((COOKIE, v)) = part.trim().split_once('=') {
                let _ = d.0.db.lock().unwrap_or_else(|e| e.into_inner()).execute("DELETE FROM sessions WHERE hash = ?1", params![sha(v)]);
            }
        }
    }
    let gone = HeaderValue::from_static("vox_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    ([(header::SET_COOKIE, gone)], Json(json!({}))).into_response()
}

pub(super) async fn me(State(d): State<Directory>, h: HeaderMap) -> Json<Value> {
    let offered = d.0.social.offered();
    match d.account(&h) {
        Some((id, name)) => {
            Json(json!({ "name": name, "character": d.character(id), "linked": d.linked(id), "sign_in_with": offered }))
        }
        None => Json(json!({ "name": null, "sign_in_with": offered })),
    }
}

pub(super) async fn catalogue(State(d): State<Directory>) -> Json<Value> {
    Json(json!({ "characters": d.0.catalogue.characters, "wardrobe": d.0.catalogue.wardrobe }))
}

pub(super) async fn set_character(State(d): State<Directory>, h: HeaderMap, Json(c): Json<Character>) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, _) = d.account(&h).ok_or(ApiError(StatusCode::UNAUTHORIZED, "sign in first".into()))?;
    let c = d.0.catalogue.check(&c).map_err(bad)?;
    let db = d.0.db.lock().unwrap_or_else(|e| e.into_inner());
    db.execute(
        "INSERT INTO characters (account, model, wear, updated) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(account) DO UPDATE SET model = ?2, wear = ?3, updated = ?4",
        params![id as i64, c.model, serde_json::to_string(&c.wear).unwrap_or_default(), now() as i64],
    )
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "couldn't save".into()))?;
    Ok(Json(json!({ "character": c })))
}

pub(super) async fn token(State(d): State<Directory>, h: HeaderMap) -> Res<Json<Value>> {
    json_only(&h)?;
    let (id, name) = d.account(&h).ok_or(ApiError(StatusCode::UNAUTHORIZED, "sign in first".into()))?;
    let character = d.character(id).ok_or_else(|| bad("make your character first"))?;
    let t = now();
    let claims = Claims { account: id, name: name.clone(), character: character.clone(), issued: t, expires: t + auth::LIFETIME };
    Ok(Json(json!({
        "token": auth::sign(&claims, &d.0.key),
        "expires": claims.expires,
        "name": name,
        "character": character,
        "worlds": d.0.worlds,
    })))
}

pub(super) async fn worlds(State(d): State<Directory>) -> Json<Value> {
    let downloads: Vec<Value> = d.0.downloads.iter().map(|(label, url)| json!({ "label": label, "url": url })).collect();
    Json(json!({ "worlds": d.0.worlds, "downloads": downloads }))
}

pub(super) async fn key(State(d): State<Directory>) -> String {
    d.public_key()
}
