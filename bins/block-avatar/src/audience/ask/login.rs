//! Signing in: by name, or with Twitch (the device-code flow, polled by the page).

#[allow(unused_imports)]
use super::*;

pub(super) fn login_name(sh: &AskShared, body: &[u8], ip: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if cfg.twitch_client_id.is_some() || !cfg.open_names {
        return err(403, "Log in with Twitch instead.");
    }
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(ip, "name", 5, Duration::from_secs(600))
    {
        return err(429, "Too many tries. Wait a few minutes.");
    }
    let name = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();
    if !(2..=25).contains(&name.chars().count())
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return err(400, "Use 2 to 25 letters, numbers or underscores.");
    }
    let login = Login {
        user: name.clone(),
        verified: false,
        account: format!("name:{}", name.to_ascii_lowercase()),
    };
    match new_session(sh, Some(login), None) {
        Some(t) => json_resp(
            200,
            json!({ "session": t, "user": name, "verified": false }),
        ),
        None => err(503, "Too busy right now. Try again soon."),
    }
}

pub(super) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into()
}

pub(super) fn post_form(url: &str, form: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let mut r = agent()
        .post(url)
        .send_form(form.iter().copied())
        .map_err(|e| format!("can't reach Twitch: {e}"))?;
    let code = r.status().as_u16();
    let body = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok((code, serde_json::from_str(&body).unwrap_or(Value::Null)))
}

/// Step 1 of the device code flow: ask Twitch for a code the viewer approves on twitch.tv.
pub(super) fn login_twitch(sh: &AskShared, ip: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(client_id) = cfg.twitch_client_id.clone() else {
        return err(403, "Twitch login isn't set up for this stream.");
    };
    if !sh
        .limiter
        .lock()
        .unwrap()
        .allow(ip, "device", 6, Duration::from_secs(600))
    {
        return err(429, "Too many login attempts. Wait a few minutes.");
    }
    let (code, v) = match post_form(
        &format!("{}/oauth2/device", cfg.auth_base),
        &[("client_id", &client_id), ("scopes", &cfg.scopes)],
    ) {
        Ok(x) => x,
        Err(e) => {
            *sh.status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch login: {e}");
            return err(502, "Couldn't reach Twitch. Try again.");
        }
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    let (Some(device_code), Some(user_code), Some(uri)) =
        (s("device_code"), s("user_code"), s("verification_uri"))
    else {
        *sh.status.lock().unwrap_or_else(|e| e.into_inner()) = format!(
            "Twitch login: device request failed ({code}: {})",
            v.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("no details")
        );
        return err(502, "Twitch didn't accept the login request.");
    };
    if !(uri.starts_with("https://")
        || (cfg.auth_base != TWITCH_AUTH && uri.starts_with("http://127.0.0.1")))
    {
        return err(502, "Unexpected answer from Twitch.");
    }
    let interval = Duration::from_secs(
        v.get("interval")
            .and_then(|x| x.as_u64())
            .unwrap_or(5)
            .clamp(1, 30),
    );
    let expires = Duration::from_secs(
        v.get("expires_in")
            .and_then(|x| x.as_u64())
            .unwrap_or(1800)
            .min(1800),
    );
    let now = Instant::now();
    let dev = Device {
        code: device_code,
        interval,
        next_poll: now + interval,
        expires: now + expires,
    };
    match new_session(sh, None, Some(dev)) {
        Some(t) => json_resp(
            200,
            json!({ "session": t, "user_code": user_code, "verification_uri": uri, "interval": interval.as_secs() }),
        ),
        None => err(503, "Too busy right now. Try again soon."),
    }
}

/// Step 2: the page polls; we ask Twitch (no more often than it allows) whether it's approved.
pub(super) fn login_poll(sh: &AskShared, req: &Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let cfg = sh.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(client_id) = cfg.twitch_client_id.clone() else {
        return err(403, "Twitch login isn't set up.");
    };
    let Some(t) = req_header(req, SESSION_HEADER) else {
        return err(401, "No login in progress.");
    };
    let device_code = {
        let mut s = sh.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sess) = s.get_mut(&t) else {
            return err(401, "No login in progress.");
        };
        sess.last = Instant::now();
        if let Some(l) = &sess.login {
            return json_resp(
                200,
                json!({ "state": "done", "user": l.user, "verified": l.verified }),
            );
        }
        let Some(d) = sess.device.as_mut() else {
            return err(401, "No login in progress.");
        };
        let now = Instant::now();
        if now > d.expires {
            s.remove(&t);
            return json_resp(200, json!({ "state": "expired" }));
        }
        if now < d.next_poll {
            return json_resp(200, json!({ "state": "pending" }));
        }
        d.next_poll = now + d.interval;
        d.code.clone()
    };
    let grant = "urn:ietf:params:oauth:grant-type:device_code";
    let (code, v) = match post_form(
        &format!("{}/oauth2/token", cfg.auth_base),
        &[
            ("client_id", &client_id),
            ("scopes", &cfg.scopes),
            ("device_code", &device_code),
            ("grant_type", grant),
        ],
    ) {
        Ok(x) => x,
        Err(_) => return json_resp(200, json!({ "state": "pending" })),
    };
    let Some(access) = v.get("access_token").and_then(|x| x.as_str()) else {
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
        if code == 400 && (msg.contains("pending") || msg.contains("slow_down")) {
            if msg.contains("slow_down") {
                if let Some(d) = sh
                    .sessions
                    .lock()
                    .unwrap()
                    .get_mut(&t)
                    .and_then(|s| s.device.as_mut())
                {
                    d.interval += Duration::from_secs(5);
                }
            }
            return json_resp(200, json!({ "state": "pending" }));
        }
        sh.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&t);
        return json_resp(
            200,
            json!({ "state": if msg.contains("denied") { "denied" } else { "expired" } }),
        );
    };
    // who is it? then throw the token away
    let who = agent()
        .get(&format!("{}/oauth2/validate", cfg.auth_base))
        .header("Authorization", &format!("OAuth {access}"))
        .call();
    let _ = post_form(
        &format!("{}/oauth2/revoke", cfg.auth_base),
        &[("client_id", &client_id), ("token", access)],
    );
    let v: Value = match who {
        Ok(mut r) if r.status().as_u16() == 200 => r
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|b| serde_json::from_str(&b).ok())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let login = v
        .get("login")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let same_app = v.get("client_id").and_then(|x| x.as_str()) == Some(client_id.as_str());
    if login.is_empty()
        || !same_app
        || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        sh.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&t);
        return json_resp(200, json!({ "state": "expired" }));
    }
    let l = Login {
        user: login.clone(),
        verified: true,
        account: format!("tw:{login}"),
    };
    if let Some(sess) = sh.sessions.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&t) {
        sess.login = Some(l);
        sess.device = None;
    }
    json_resp(
        200,
        json!({ "state": "done", "user": login, "verified": true }),
    )
}
