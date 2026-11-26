//! The public "ask" page: viewers log in with Twitch, pick a character, record a question (up to
//! 30 s by default), listen back and send it to the queue (questions.rs).
//!
//! It runs on its own port (7778 by default), separate from the control page, and serves only
//! the page and its small API: nothing on it can drive the avatar, approve or air anything.
//! It listens on localhost; to let viewers in, put a tunnel in front of it, for example
//! `cloudflared tunnel --url http://localhost:7778`, and share the https address it prints.
//!
//! Login uses Twitch's device code flow, so it works behind any tunnel address with no
//! redirect URL to register: the page shows a code and a link to twitch.tv/activate, the viewer
//! approves, and the app reads their Twitch username once and throws the token away. Without a
//! Twitch client ID the page can instead take a typed name (marked "unverified"), if allowed.

mod http;
mod routes;
mod login;
mod page;
#[allow(unused_imports)]
pub use http::*;
#[allow(unused_imports)]
pub use routes::*;
#[allow(unused_imports)]
pub use login::*;
#[allow(unused_imports)]
pub use page::*;

use std::{
    collections::HashMap,
    io::Read,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::questions::{Queue, Status, SubmitError, unix_now};

pub const TWITCH_AUTH: &str = "https://id.twitch.tv";
const SESSION_HEADER: &str = "x-ask-session";
const MAX_SESSIONS: usize = 5000;
const WORKERS: usize = 8;

#[derive(Clone)]
pub struct AskConfig {
    pub port: u16,
    pub twitch_client_id: Option<String>,
    pub auth_base: String,
    pub scopes: String,
    /// without a Twitch client ID: let viewers type a name instead
    pub open_names: bool,
    /// characters viewers may pick: (label, model path under assets/)
    pub characters: Vec<(String, String)>,
    pub title: String,
    pub font: Option<Arc<Vec<u8>>>,
}

#[derive(Clone)]
struct Login {
    user: String,
    verified: bool,
    account: String,
}

struct Device {
    code: String,
    interval: Duration,
    next_poll: Instant,
    expires: Instant,
}

struct Session {
    last: Instant,
    login: Option<Login>,
    device: Option<Device>,
}

/// Simple per-address counters: at most `max` events per `window`.
#[derive(Default)]
struct Limiter(HashMap<(String, &'static str), (Instant, u32)>);

impl Limiter {
    fn allow(&mut self, ip: &str, kind: &'static str, max: u32, window: Duration) -> bool {
        let now = Instant::now();
        if self.0.len() > 20000 {
            self.0
                .retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(600));
        }
        let e = self.0.entry((ip.to_string(), kind)).or_insert((now, 0));
        if now.duration_since(e.0) > window {
            *e = (now, 0);
        }
        e.1 += 1;
        e.1 <= max
    }
}

pub struct AskShared {
    pub cfg: Mutex<AskConfig>,
    pub queue: Arc<Mutex<Queue>>,
    sessions: Mutex<HashMap<String, Session>>,
    limiter: Mutex<Limiter>,
    pub status: Mutex<String>,
}

pub struct AskServer {
    pub shared: Arc<AskShared>,
    pub url: String,
}

fn token() -> String {
    let mut b = [0u8; 18];
    getrandom::fill(&mut b).expect("system random numbers");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Starts the ask server (port 0 picks a free one; handy for tests).
pub fn start(cfg: AskConfig, queue: Arc<Mutex<Queue>>) -> Result<AskServer, String> {
    let server = Server::http(("127.0.0.1", cfg.port))
        .map_err(|e| format!("can't listen on port {}: {e}", cfg.port))?;
    let addr = server.server_addr().to_ip().ok_or("no address")?;
    let url = format!("http://127.0.0.1:{}/", addr.port());
    let shared = Arc::new(AskShared {
        cfg: Mutex::new(cfg),
        queue,
        sessions: Mutex::new(HashMap::new()),
        limiter: Mutex::new(Limiter::default()),
        status: Mutex::new(String::new()),
    });
    let server = Arc::new(server);
    for i in 0..WORKERS {
        let (server, shared) = (server.clone(), shared.clone());
        std::thread::Builder::new()
            .name(format!("ask-{i}"))
            .spawn(move || {
                while let Ok(req) = server.recv() {
                    handle(&shared, req);
                }
            })
            .map_err(|e| e.to_string())?;
    }
    crate::note!("[ask] viewer page on {url}");
    Ok(AskServer { shared, url })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A stand-in for id.twitch.tv: device code, one "pending" answer, then a token for `viewer1`.
    fn fake_twitch(client_id: &'static str, login: &'static str) -> (String, Arc<AtomicU32>) {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let polls = Arc::new(AtomicU32::new(0));
        let p = polls.clone();
        std::thread::spawn(move || {
            for mut r in server.incoming_requests() {
                let mut body = String::new();
                let _ = r.as_reader().read_to_string(&mut body);
                let url = r.url().to_string();
                let (code, v) = match url.as_str() {
                    "/oauth2/device" => {
                        assert!(body.contains(&format!("client_id={client_id}")), "{body}");
                        (
                            200,
                            json!({"device_code":"DEV123","user_code":"ABCDEFGH","verification_uri":format!("http://127.0.0.1:{port}/activate?device-code=ABCDEFGH"),"interval":1,"expires_in":1800}),
                        )
                    }
                    "/oauth2/token" => {
                        assert!(body.contains("device_code=DEV123") && body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"), "{body}");
                        if p.fetch_add(1, Ordering::SeqCst) == 0 {
                            (400, json!({"status":400,"message":"authorization_pending"}))
                        } else {
                            (
                                200,
                                json!({"access_token":"TOKEN1","refresh_token":"R","expires_in":14000,"scope":[],"token_type":"bearer"}),
                            )
                        }
                    }
                    "/oauth2/validate" => {
                        let auth = r
                            .headers()
                            .iter()
                            .find(|h| h.field.equiv("Authorization"))
                            .map(|h| h.value.as_str().to_string());
                        assert_eq!(auth.as_deref(), Some("OAuth TOKEN1"));
                        (
                            200,
                            json!({"client_id":client_id,"login":login,"user_id":"42","scopes":[],"expires_in":14000}),
                        )
                    }
                    "/oauth2/revoke" => (200, json!({})),
                    _ => (404, json!({})),
                };
                let _ = r.respond(Response::from_data(v.to_string()).with_status_code(code));
            }
        });
        (format!("http://127.0.0.1:{port}"), polls)
    }

    fn real_question() -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                .join("demo/questions/bdl-a0042.wav"),
        )
        .unwrap()
    }

    fn server(
        name: &str,
        client_id: Option<&str>,
        auth_base: &str,
        open_names: bool,
    ) -> (AskServer, Arc<Mutex<Queue>>) {
        let dir = std::env::temp_dir().join(format!("avatar-ask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut q = Queue::load(dir);
        q.open = true;
        let q = Arc::new(Mutex::new(q));
        let cfg = AskConfig {
            port: 0,
            twitch_client_id: client_id.map(|s| s.to_string()),
            auth_base: auth_base.to_string(),
            scopes: String::new(),
            open_names,
            characters: vec![
                ("Dog".into(), "models/dog.glb".into()),
                ("Cat".into(), "models/cat.glb".into()),
            ],
            title: "Ask the dog".into(),
            font: None,
        };
        (start(cfg, q.clone()).unwrap(), q)
    }

    fn call(method: &str, url: &str, sess: Option<&str>, body: Option<Vec<u8>>) -> (u16, Value) {
        let a: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut r = match (method, body) {
            ("GET", _) => {
                let mut rq = a.get(url);
                if let Some(s) = sess {
                    rq = rq.header("X-Ask-Session", s);
                }
                rq.call().unwrap()
            }
            (_, b) => {
                let mut rq = a.post(url);
                if let Some(s) = sess {
                    rq = rq.header("X-Ask-Session", s);
                }
                rq.send(b.unwrap_or_default()).unwrap()
            }
        };
        let code = r.status().as_u16();
        let s = r.body_mut().read_to_string().unwrap_or_default();
        (code, serde_json::from_str(&s).unwrap_or(Value::Null))
    }

    #[test]
    fn twitch_login_then_ask_with_a_real_recording() {
        let (base, polls) = fake_twitch("client-abc", "viewer1");
        let (srv, q) = server("twitch", Some("client-abc"), &base, false);
        let u = |p: &str| format!("{}{}", srv.url.trim_end_matches('/'), p);
        let (_, info) = call("GET", &u("/api/info"), None, None);
        assert_eq!(info["login"], "twitch");
        assert_eq!(info["characters"][1], "Cat");
        // portraits for the picker (from assets/portraits)
        let (code, png) = call("GET", &u("/portrait/1.png"), None, None);
        assert_eq!(code, 200, "cat portrait");
        let _ = png;
        assert_eq!(call("GET", &u("/portrait/9.png"), None, None).0, 404);
        assert_eq!(call("GET", &u("/portrait/../x.png"), None, None).0, 404);
        assert!(info["me"].is_null());
        // can't ask before logging in
        assert_eq!(
            call("POST", &u("/api/ask?c=0"), None, Some(real_question())).0,
            401
        );
        let (code, start) = call("POST", &u("/api/login/twitch"), None, None);
        assert_eq!(code, 200, "{start}");
        assert_eq!(start["user_code"], "ABCDEFGH");
        let s = start["session"].as_str().unwrap().to_string();
        // the page polls; we only ask Twitch once per interval
        let mut state = String::new();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(150));
            let (_, p) = call("POST", &u("/api/login/poll"), Some(&s), None);
            state = p["state"].as_str().unwrap_or("").to_string();
            if state == "done" {
                assert_eq!(p["user"], "viewer1");
                break;
            }
        }
        assert_eq!(state, "done");
        assert_eq!(
            polls.load(Ordering::SeqCst),
            2,
            "pending once, then the token"
        );
        let (_, info) = call("GET", &u("/api/info"), Some(&s), None);
        assert_eq!(info["me"]["user"], "viewer1");
        assert_eq!(info["can_ask"]["ok"], true);
        // ask
        let (code, a) = call("POST", &u("/api/ask?c=1"), Some(&s), Some(real_question()));
        assert_eq!(code, 200, "{a}");
        let id = a["id"].as_u64().unwrap() as u32;
        {
            let q = q.lock().unwrap_or_else(|e| e.into_inner());
            let x = q.get(id).unwrap();
            assert_eq!(
                (
                    x.user.as_str(),
                    x.verified,
                    x.account.as_str(),
                    x.character.as_str(),
                    x.model.as_str()
                ),
                ("viewer1", true, "tw:viewer1", "Cat", "models/cat.glb")
            );
            assert!((x.secs - 2.58).abs() < 0.05, "{}", x.secs);
        }
        // a second one is refused while the first waits
        let (code, e) = call("POST", &u("/api/ask?c=1"), Some(&s), Some(real_question()));
        assert_eq!(code, 409, "{e}");
        let (_, m) = call("GET", &u("/api/mine"), Some(&s), None);
        assert_eq!(m["questions"][0]["state"], "waiting");
        assert_eq!(m["questions"][0]["position"], 1);
        // somebody else can't see or withdraw it
        let (code, other) = call("POST", &u("/api/login/twitch"), None, None);
        assert_eq!(code, 200);
        let other = other["session"].as_str().unwrap();
        assert_eq!(
            call("GET", &u("/api/mine"), Some(other), None).0,
            401,
            "not logged in yet"
        );
        assert_eq!(
            call(
                "POST",
                &u(&format!("/api/withdraw?id={id}")),
                Some("forged"),
                None
            )
            .0,
            401
        );
        // withdraw
        assert_eq!(
            call(
                "POST",
                &u(&format!("/api/withdraw?id={id}")),
                Some(&s),
                None
            )
            .0,
            200
        );
        assert_eq!(q.lock().unwrap_or_else(|e| e.into_inner()).get(id).unwrap().status, Status::Skipped);
    }

    #[test]
    fn ask_page_has_no_control_commands_and_checks_uploads() {
        let (srv, q) = server("guard", None, TWITCH_AUTH, true);
        let u = |p: &str| format!("{}{}", srv.url.trim_end_matches('/'), p);
        // nothing from the control page exists here
        for p in ["/cmd/face:1", "/state", "/questions", "/q/1.wav"] {
            assert_eq!(call("GET", &u(p), None, None).0, 404, "{p}");
        }
        // the page itself, with a strict content policy
        let a: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let r = a.get(&u("/")).call().unwrap();
        let csp = r
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            csp.contains("default-src 'self'") && !csp.contains("unsafe"),
            "{csp}"
        );
        // open mode: typed names, checked
        assert_eq!(
            call(
                "POST",
                &u("/api/login/name"),
                None,
                Some(br#"{"name":"<script>"}"#.to_vec())
            )
            .0,
            400
        );
        let (code, l) = call(
            "POST",
            &u("/api/login/name"),
            None,
            Some(br#"{"name":"Guest_7"}"#.to_vec()),
        );
        assert_eq!(code, 200);
        let s = l["session"].as_str().unwrap().to_string();
        // Twitch login isn't offered without a client ID
        assert_eq!(call("POST", &u("/api/login/twitch"), None, None).0, 403);
        // garbage, an unknown character, a huge upload
        assert_eq!(
            call(
                "POST",
                &u("/api/ask?c=0"),
                Some(&s),
                Some(b"hello".to_vec())
            )
            .0,
            400
        );
        assert_eq!(
            call("POST", &u("/api/ask?c=9"), Some(&s), Some(real_question())).0,
            400
        );
        assert_eq!(
            call(
                "POST",
                &u("/api/ask?c=0"),
                Some(&s),
                Some(vec![0u8; 4_000_000])
            )
            .0,
            413
        );
        let (code, a) = call("POST", &u("/api/ask?c=0"), Some(&s), Some(real_question()));
        assert_eq!(code, 200, "{a}");
        let id = a["id"].as_u64().unwrap() as u32;
        assert!(!q.lock().unwrap_or_else(|e| e.into_inner()).get(id).unwrap().verified);
        // closing the queue stops new questions
        q.lock().unwrap_or_else(|e| e.into_inner()).open = false;
        let (_, info) = call("GET", &u("/api/info"), Some(&s), None);
        assert_eq!(info["open"], false);
        // with names turned off, nobody can log in
        srv.shared.cfg.lock().unwrap_or_else(|e| e.into_inner()).open_names = false;
        assert_eq!(
            call(
                "POST",
                &u("/api/login/name"),
                None,
                Some(br#"{"name":"Other"}"#.to_vec())
            )
            .0,
            403
        );
    }
}
