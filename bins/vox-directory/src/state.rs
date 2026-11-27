//! The directory itself: opening the database (the schema), the router, sessions, sign-in limits.

#[allow(unused_imports)]
use super::*;

impl Directory {
    pub fn open(cfg: Config) -> Result<Directory, String> {
        let db = Connection::open(&cfg.db).map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS accounts (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 pass TEXT NOT NULL, created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (
                 hash TEXT PRIMARY KEY, account INTEGER NOT NULL, expires INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS sessions_expires ON sessions (expires);
             CREATE TABLE IF NOT EXISTS characters (
                 account INTEGER PRIMARY KEY, model TEXT NOT NULL, wear TEXT NOT NULL,
                 updated INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS identities (
                 site TEXT NOT NULL, subject TEXT NOT NULL, account INTEGER NOT NULL,
                 created INTEGER NOT NULL, PRIMARY KEY (site, subject));
             CREATE INDEX IF NOT EXISTS identities_account ON identities (account);
             CREATE TABLE IF NOT EXISTS currencies (
                 code TEXT PRIMARY KEY, name TEXT NOT NULL, world TEXT UNIQUE, issuer INTEGER,
                 supply INTEGER NOT NULL, created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS balances (
                 account INTEGER NOT NULL, currency TEXT NOT NULL, amount INTEGER NOT NULL,
                 PRIMARY KEY (account, currency));
             CREATE TABLE IF NOT EXISTS ledger (
                 id INTEGER PRIMARY KEY, at INTEGER NOT NULL, currency TEXT NOT NULL, src INTEGER,
                 dst INTEGER, amount INTEGER NOT NULL, memo TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS ledger_src ON ledger (src);
             CREATE INDEX IF NOT EXISTS ledger_dst ON ledger (dst);
             CREATE TABLE IF NOT EXISTS items (
                 id INTEGER PRIMARY KEY, account INTEGER NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL,
                 world TEXT, ref INTEGER, detail TEXT NOT NULL, acquired INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS items_account ON items (account);
             CREATE TABLE IF NOT EXISTS world_owners (world TEXT PRIMARY KEY, account INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS world_state (
                 world TEXT PRIMARY KEY, spawn TEXT NOT NULL, level TEXT NOT NULL, reported INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS whereabouts (
                 account INTEGER NOT NULL, world TEXT NOT NULL, x REAL NOT NULL, y REAL NOT NULL, z REAL NOT NULL,
                 at INTEGER NOT NULL, PRIMARY KEY (account, world));
             CREATE TABLE IF NOT EXISTS activity (
                 account INTEGER PRIMARY KEY, minutes INTEGER NOT NULL, edits INTEGER NOT NULL,
                 day INTEGER NOT NULL, earned INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS plots (
                 id INTEGER PRIMARY KEY, world TEXT NOT NULL, owner INTEGER NOT NULL, x0 INTEGER NOT NULL,
                 z0 INTEGER NOT NULL, x1 INTEGER NOT NULL, z1 INTEGER NOT NULL, grp INTEGER, created INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS plots_world ON plots (world);
             CREATE TABLE IF NOT EXISTS portals (
                 id INTEGER PRIMARY KEY, world TEXT NOT NULL, owner INTEGER NOT NULL, plot INTEGER NOT NULL,
                 x REAL NOT NULL, y REAL NOT NULL, z REAL NOT NULL, axis TEXT NOT NULL, dest TEXT NOT NULL,
                 created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS structures (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL, owner INTEGER NOT NULL, public TEXT,
                 version TEXT NOT NULL, derived_from INTEGER, created INTEGER NOT NULL, updated INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS structures_owner ON structures (owner);
             CREATE TABLE IF NOT EXISTS structure_versions (
                 structure INTEGER NOT NULL, version TEXT NOT NULL, author INTEGER NOT NULL, at INTEGER NOT NULL,
                 PRIMARY KEY (structure, version));
             CREATE TABLE IF NOT EXISTS structure_grants (
                 structure INTEGER NOT NULL, account INTEGER NOT NULL, right TEXT NOT NULL,
                 PRIMARY KEY (structure, account));
             CREATE INDEX IF NOT EXISTS structure_grants_account ON structure_grants (account);
             CREATE TABLE IF NOT EXISTS friends (
                 a INTEGER NOT NULL, b INTEGER NOT NULL, since INTEGER NOT NULL, PRIMARY KEY (a, b));
             CREATE TABLE IF NOT EXISTS friend_requests (
                 src INTEGER NOT NULL, dst INTEGER NOT NULL, at INTEGER NOT NULL, PRIMARY KEY (src, dst));
             CREATE TABLE IF NOT EXISTS crews (
                 id INTEGER PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, leader INTEGER NOT NULL,
                 target TEXT, started INTEGER, ends INTEGER, together INTEGER NOT NULL DEFAULT 0,
                 done INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS crew_members (
                 crew INTEGER NOT NULL, account INTEGER NOT NULL, joined INTEGER NOT NULL, PRIMARY KEY (crew, account));
             CREATE TABLE IF NOT EXISTS instances (
                 id TEXT PRIMARY KEY, template TEXT NOT NULL, party INTEGER NOT NULL, created INTEGER NOT NULL,
                 ends INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS instances_party ON instances (party, template);
             CREATE TABLE IF NOT EXISTS instance_opens (party INTEGER NOT NULL, at INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS instance_opens_party ON instance_opens (party, at);
             CREATE TABLE IF NOT EXISTS crew_invites (
                 crew INTEGER NOT NULL, account INTEGER NOT NULL, by INTEGER NOT NULL, at INTEGER NOT NULL,
                 PRIMARY KEY (crew, account));",
        )
        .map_err(|e| e.to_string())?;
        let catalogue = Catalogue::load(&cfg.data);
        if catalogue.characters.is_empty() {
            return Err(format!("no characters in {}/characters.json (tools/assets/build_catalogue.py makes it)", cfg.data.display()));
        }
        Ok(Directory(
            Arc::new(Inner {
                db: Mutex::new(db),
                key: cfg.key,
                catalogue,
                worlds: cfg.worlds,
                secure: cfg.secure_cookies,
                downloads: cfg.downloads,
                behind_proxy: cfg.behind_proxy,
                social: cfg.social,
                http: social::http(),
                world_secret: cfg.world_secret.filter(|s| !s.trim().is_empty()),
                report_every: cfg.report_every,
                instance_host: cfg.instance_host.map(|h| h.trim_end_matches('/').to_string()),
                atlases: cfg.atlases,
                tries: Mutex::new(HashMap::new()),
                hashing: tokio::sync::Semaphore::new(HASHING_AT_ONCE),
                dummy: Argon2::default()
                    .hash_password(random_hex(16).as_bytes(), &salt())
                    .map(|h| h.to_string())
                    .map_err(|e| e.to_string())?,
            }),
            Arc::new((cfg.web, cfg.assets, cfg.data)),
        ))
    }

    /// Make `name` the owner of world `world` (they can then issue its currency).
    pub fn own_world(&self, world: &str, name: &str) -> Result<String, String> {
        let w = self.0.worlds.iter().find(|w| w.id == world).ok_or_else(|| format!("there's no world {world} (worlds: {})", self.world_ids()))?;
        let db = self.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let (id, name) = people::named(&db, name).map_err(|e| e.1)?;
        db.execute(
            "INSERT INTO world_owners (world, account) VALUES (?1, ?2) ON CONFLICT(world) DO UPDATE SET account = ?2",
            params![world, id as i64],
        )
        .map_err(|e| e.to_string())?;
        Ok(format!("{name} owns {}", w.name))
    }

    /// Give `name` some authority (from the operators).
    pub fn grant(&self, name: &str, amount: i64, memo: &str) -> Result<String, String> {
        if amount <= 0 {
            return Err("grant at least 1".into());
        }
        let db = self.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let (id, name) = people::named(&db, name).map_err(|e| e.1)?;
        game::mint(&db, id, game::AUTH, amount, memo).map_err(|e| e.to_string())?;
        Ok(format!("{name} has {} authority", game::balance(&db, id, game::AUTH)))
    }

    pub(super) fn world_ids(&self) -> String {
        self.0.worlds.iter().map(|w| w.id.as_str()).collect::<Vec<_>>().join(", ")
    }

    /// The public half of the signing key, as hex: give it to world servers.
    pub fn public_key(&self) -> String {
        auth::public_hex(&self.0.key.verifying_key())
    }

    pub fn router(&self) -> Router {
        let (web, assets, data) = &*self.1;
        Router::new()
            .route("/api/me", get(me))
            .route("/api/signup", post(signup))
            .route("/api/signin", post(signin))
            .route("/api/signout", post(signout))
            .route("/api/catalogue", get(catalogue))
            .route("/api/character", put(set_character))
            .route("/api/token", post(token))
            .route("/api/worlds", get(worlds))
            .route("/api/key", get(key))
            .route("/api/wallet", get(game::wallet))
            .route("/api/send", post(game::send))
            .route("/api/inventory", get(game::inventory))
            .route("/api/feats", get(game::feats))
            .route("/api/feats/plot", post(game::claim_plot))
            .route("/api/feats/portal", post(game::open_portal))
            .route("/api/feats/currency", post(game::issue_currency))
            .route("/api/feats/raid", post(game::start_raid))
            .route("/api/plots/share", post(game::share_plot))
            .route("/api/plots/release", post(game::release_plot))
            .route("/api/structures", post(structures::make).get(structures::mine))
            .route("/api/structures/{id}", get(structures::show))
            .route("/api/structures/version", post(structures::version))
            .route("/api/structures/grant", post(structures::grant))
            .route("/api/structures/public", post(structures::public))
            .route("/api/structures/copy", post(structures::copy))
            .route("/api/structures/delete", post(structures::delete))
            .route("/api/world/structures/{id}", get(structures::check))
            .route("/api/people", get(people::people))
            .route("/api/friends", post(people::befriend))
            .route("/api/friends/answer", post(people::answer_friend))
            .route("/api/friends/remove", post(people::unfriend))
            .route("/api/crews", post(people::new_crew))
            .route("/api/crews/invite", post(people::invite))
            .route("/api/crews/answer", post(people::answer_invite))
            .route("/api/crews/leave", post(people::leave))
            .route("/api/world/report", post(game::report))
            .route("/api/world/travel", post(game::travel))
            .route("/api/world/objective", post(game::objective))
            .route("/api/world/{id}/rules", get(game::rules))
            .route("/auth/steam", get(social::steam_start))
            .route("/auth/steam/return", get(social::steam_return))
            .route("/auth/discord", get(social::discord_start))
            .route("/auth/discord/return", get(social::discord_return))
            .route_service("/", ServeFile::new(web.join("index.html")))
            .route_service("/create", ServeFile::new(web.join("create.html")))
            .route_service("/play", ServeFile::new(web.join("play.html")))
            .route_service("/inventory", ServeFile::new(web.join("inventory.html")))
            .route_service("/friends", ServeFile::new(web.join("friends.html")))
            .route_service("/help", ServeFile::new(web.join("help.html")))
            .nest_service("/assets", ServeDir::new(assets))
            .nest_service("/data", ServeDir::new(data))
            // the browser client is big: served pre-compressed when tools/dev/build_web.sh made the
            // .br / .gz files
            .fallback_service(ServeDir::new(web).precompressed_br().precompressed_gzip())
            .layer(axum::middleware::from_fn(headers))
            .with_state(self.clone())
    }

    /// Where a request came from (for counting sign-in tries).
    pub(super) fn visitor(&self, addr: SocketAddr, h: &HeaderMap) -> String {
        if self.0.behind_proxy {
            if let Some(ip) = h.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
                return ip.trim().to_string();
            }
            if let Some(ip) = h.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit(',').next()) {
                return ip.trim().to_string();
            }
        }
        addr.ip().to_string()
    }

    /// Work out a password hash (or check one) with at most a few at a time; `None` when it's
    /// been too busy for a few seconds.
    pub(super) async fn hash_work<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let permit = tokio::time::timeout(Duration::from_secs(5), self.0.hashing.acquire()).await.ok()?.ok()?;
        let out = tokio::task::spawn_blocking(f).await.ok();
        drop(permit);
        out
    }

    /// Too many sign-in tries from `who` lately?
    pub(super) fn tried(&self, who: &str) -> bool {
        self.tried_up_to(who, ATTEMPTS)
    }

    pub(super) fn tried_up_to(&self, who: &str, most: u32) -> bool {
        let mut t = self.0.tries.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        t.retain(|_, (_, at)| now.duration_since(*at) < WINDOW);
        // keys are bounded (names are cut short), and so is the table: when it's full, anyone
        // not already in it waits (under attack, refusing beats running out of memory)
        let who: String = who.chars().take(80).collect();
        if t.len() >= MAX_TRACKED && !t.contains_key(&who) {
            return true;
        }
        let e = t.entry(who).or_insert((0, now));
        e.0 += 1;
        e.0 > most
    }

    /// The account a request's session cookie belongs to.
    pub(super) fn account(&self, h: &HeaderMap) -> Option<(u64, String)> {
        let cookie = cookie(h, COOKIE)?;
        let db = self.0.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row(
            "SELECT a.id, a.name FROM sessions s JOIN accounts a ON a.id = s.account WHERE s.hash = ?1 AND s.expires > ?2",
            params![sha(&cookie), now() as i64],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub(super) fn character(&self, account: u64) -> Option<Character> {
        let db = self.0.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT model, wear FROM characters WHERE account = ?1", params![account as i64], |r| {
            Ok(Character {
                model: r.get(0)?,
                wear: serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default(),
            })
        })
        .optional()
        .ok()
        .flatten()
    }

    /// A new session for `account`: its cookie.
    pub(super) fn session(&self, account: u64) -> HeaderValue {
        let token = random_hex(32);
        let db = self.0.db.lock().unwrap_or_else(|e| e.into_inner());
        let _ = db.execute("DELETE FROM sessions WHERE expires < ?1", params![now() as i64]);
        let _ = db.execute(
            "INSERT INTO sessions (hash, account, expires) VALUES (?1, ?2, ?3)",
            params![sha(&token), account as i64, (now() + SESSION) as i64],
        );
        let secure = if self.0.secure { "; Secure" } else { "" };
        HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION}{secure}")).expect("cookie")
    }
}
