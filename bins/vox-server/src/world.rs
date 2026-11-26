//! The world thread's state at work: joining and leaving, messages, edits and their checks,
//! the weather, plots and portals from the directory, saving to SQLite.

#[allow(unused_imports)]
use super::*;

impl WorldState {
    pub(super) fn open(mut cfg: Config) -> Result<WorldState, String> {
        let e = |e: rusqlite::Error| e.to_string();
        let db = Connection::open(&cfg.db).map_err(e)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS chunks(x INTEGER, y INTEGER, z INTEGER, version INTEGER NOT NULL,
                 data BLOB NOT NULL, PRIMARY KEY(x, y, z));
             CREATE TABLE IF NOT EXISTS edits(seq INTEGER PRIMARY KEY, at INTEGER NOT NULL, player TEXT NOT NULL,
                 x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, old INTEGER NOT NULL, new INTEGER NOT NULL);",
        )
        .map_err(e)?;
        let meta = |k: &str| -> Option<String> {
            db.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0)).ok()
        };
        // the world keeps the palette it was made with (ids are stored in its chunks); new
        // blocks are added at the end
        let fresh = meta("palette").is_none();
        if let Some(stored) = meta("palette") {
            let mut names: Vec<String> = serde_json::from_str(&stored).map_err(|e| e.to_string())?;
            for n in &cfg.palette {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
            cfg.palette = names;
        }
        let seq: u64 = meta("seq").and_then(|s| s.parse().ok()).unwrap_or(0);
        let settings = match meta("settings") {
            Some(s) if !fresh => s,
            _ => cfg.seed.as_deref().map(vox_core::level::settings).unwrap_or_else(|| "{}".into()),
        };
        let mut world = World::new(cfg.bounds, cfg.palette.len() as u16);
        let mut dirty = HashSet::new();
        if fresh {
            if let Some(level) = &cfg.seed {
                vox_core::level::fill(&mut world, level, &cfg.palette)?;
                dirty.extend(world.keys());
            }
        } else {
            let mut q = db.prepare("SELECT x, y, z, version, data FROM chunks").map_err(e)?;
            let rows = q
                .query_map([], |r| {
                    Ok((ChunkKey([r.get(0)?, r.get(1)?, r.get(2)?]), r.get::<_, u32>(3)?, r.get::<_, Vec<u8>>(4)?))
                })
                .map_err(e)?;
            for row in rows {
                let (k, version, data) = row.map_err(e)?;
                world.put_chunk(k, Chunk::decode(&data, version)?);
            }
        }
        let spawn = match world.ground(0, 0, cfg.bounds.hi[1] - 1) {
            Some(y) => [0.5, y as f32, 0.5],
            None => [0.5, 0.0, 0.5],
        };
        let set: serde_json::Value = serde_json::from_str(&settings).unwrap_or_default();
        let ground = set.get("floor").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32 * cfg.block_size;
        let climate = match meta("climate").and_then(|c| serde_json::from_str::<Climate>(&c).ok()) {
            Some(c) if !fresh => c.sane(),
            _ => Climate::from_settings(set.get("weather"), ground),
        };
        let seed = cfg.name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
        let mut weather = Weather::new(climate, seed);
        weather.sun = match set.get("sky").and_then(|v| v.as_str()) {
            Some("earth") | None => sun_at(set.get("time").and_then(|v| v.as_f64()).unwrap_or(15.0) as f32),
            _ => 0.5,
        };
        // let the air settle before anyone arrives
        weather.step(120.0);
        let mut s = WorldState {
            cfg,
            db,
            world,
            seq,
            players: HashMap::new(),
            watchers: HashMap::new(),
            dirty,
            log: Vec::new(),
            spawn,
            settings,
            started: Instant::now(),
            last_flush: Instant::now(),
            weather,
            weather_at: Instant::now(),
            keyframe_at: Instant::now(),
            plots: Vec::new(),
            portals: Vec::new(),
            snaps: None,
            snap_prev: None,
            snap_changed: HashSet::new(),
            snap_full: true,
            last_snapshot: Instant::now(),
        };
        if fresh {
            s.flush()?;
        }
        if let Some(dir) = s.cfg.snapshots.clone() {
            let snaps = vox_format::Snapshots::new(vox_format::DirStore::new(dir));
            let latest = snaps.latest().map_err(|e| format!("snapshots: {e}"))?;
            // the last snapshot is a base only if nothing happened after it
            s.snap_full = latest.as_ref().is_none_or(|m| m.seq != s.seq) || fresh;
            s.snap_prev = latest;
            s.snaps = Some(snaps);
        }
        Ok(s)
    }

    pub(super) fn run(mut self, mut rx: mpsc::Receiver<Event>) {
        while let Some(ev) = rx.blocking_recv() {
            match ev {
                Event::Join { conn, tx, protocol, name, avatar, account } => self.join(conn, tx, protocol, name, avatar, account),
                Event::Msg { conn, msg } => self.message(conn, msg),
                Event::Watch { conn, tx, protocol } => self.watch(conn, tx, protocol),
                Event::Leave { conn } => self.leave(conn),
                Event::Tick => {
                    let behind = self.weather_at.elapsed().as_secs_f32();
                    if behind >= WEATHER_STEP {
                        self.weather.step(behind.min(5.0));
                        self.weather_at = Instant::now();
                    }
                    if self.keyframe_at.elapsed().as_secs_f32() >= KEYFRAME_EVERY {
                        self.send_weather();
                    }
                    if self.last_flush.elapsed() >= self.cfg.flush_every {
                        self.save();
                    }
                }
                Event::Status(reply) => {
                    let mut players: Vec<_> = self.players.values().map(|p| p.name.clone()).collect();
                    players.sort();
                    let _ = reply.send(Status {
                        world: self.cfg.name.clone(),
                        players,
                        blocks: self.world.count(),
                        seq: self.seq,
                    });
                }
                Event::Report(reply) => {
                    let players: Vec<serde_json::Value> = self
                        .players
                        .values_mut()
                        .filter_map(|p| {
                            let a = p.account?;
                            let placed = std::mem::take(&mut p.placed);
                            Some(serde_json::json!({ "account": a, "at": p.at, "edits": placed }))
                        })
                        .collect();
                    let _ = reply.send(serde_json::json!({
                        "world": self.cfg.name, "level": self.cfg.level, "spawn": self.spawn, "players": players,
                    }));
                }
                Event::Rules(plots, portals) => {
                    self.plots = plots;
                    self.portals = portals;
                }
                Event::Shutdown => break,
            }
        }
        for p in self.players.values() {
            let _ = p.tx.try_send(Message::Binary(wire::encode(&ToClient::Bye { why: "the world is closing".into() }).into()));
        }
        self.save();
        // closing: always a snapshot of what was built
        if let Err(e) = self.snapshot() {
            eprintln!("couldn't take a snapshot: {e}");
        }
    }

    pub(super) fn save(&mut self) {
        if let Err(e) = self.flush() {
            eprintln!("couldn't save the world: {e}");
        }
        if self.last_snapshot.elapsed() >= self.cfg.snapshot_every
            && let Err(e) = self.snapshot()
        {
            eprintln!("couldn't take a snapshot: {e}");
        }
    }

    /// Write changed chunks and the edit log in one transaction.
    pub(super) fn flush(&mut self) -> Result<(), String> {
        self.last_flush = Instant::now();
        if self.dirty.is_empty() && self.log.is_empty() && self.seq > 0 {
            return Ok(());
        }
        let e = |e: rusqlite::Error| e.to_string();
        let t = self.db.transaction().map_err(e)?;
        for &k in self.dirty.iter() {
            match self.world.chunk(k) {
                Some(c) if !c.is_empty() => {
                    t.execute(
                        "INSERT OR REPLACE INTO chunks(x, y, z, version, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![k.0[0], k.0[1], k.0[2], c.version, c.encode()],
                    )
                    .map_err(e)?;
                }
                _ => {
                    t.execute("DELETE FROM chunks WHERE x = ?1 AND y = ?2 AND z = ?3", params![k.0[0], k.0[1], k.0[2]])
                        .map_err(e)?;
                }
            }
        }
        for (seq, at, player, c, old, new) in self.log.iter() {
            let (seq, at) = (*seq, *at);
            t.execute(
                "INSERT OR REPLACE INTO edits(seq, at, player, x, y, z, old, new) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![seq as i64, at as i64, player, c[0], c[1], c[2], old, new],
            )
            .map_err(e)?;
        }
        let palette = serde_json::to_string(&self.cfg.palette).map_err(|e| e.to_string())?;
        t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('palette', ?1)", [palette]).map_err(e)?;
        t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('seq', ?1)", [self.seq.to_string()]).map_err(e)?;
        t.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('settings', ?1)", [self.settings.clone()]).map_err(e)?;
        t.commit().map_err(e)?;
        // forget what was written only now: a failed write keeps it for the next flush
        self.dirty.clear();
        self.log.clear();
        Ok(())
    }

    /// Queue a message for one player; a player who can't keep up with reliable messages
    /// is dropped. Returns false if they were.
    pub(super) fn send_to(tx: &mpsc::Sender<Message>, m: &ToClient) -> bool {
        match tx.try_send(Message::Binary(wire::encode(m).into())) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => m.droppable(),
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    pub(super) fn broadcast(&mut self, m: &ToClient, except: Option<u64>) {
        let mut gone = Vec::new();
        for (conn, p) in &self.players {
            if Some(*conn) != except && !Self::send_to(&p.tx, m) {
                gone.push(*conn);
            }
        }
        // watchers see everything players see, but not who anyone is
        let anonymous;
        let w = match m {
            ToClient::Joined { player, avatar, .. } => {
                anonymous = ToClient::Joined { player: *player, name: String::new(), avatar: avatar.clone() };
                &anonymous
            }
            _ => m,
        };
        for (conn, tx) in &self.watchers {
            if !Self::send_to(tx, w) {
                gone.push(*conn);
            }
        }
        for conn in gone {
            self.leave(conn);
        }
    }

    /// The world as a newcomer gets it: welcome, chunks, ready, weather, who's here. False if
    /// they couldn't keep up.
    pub(super) fn send_world(&self, tx: &mpsc::Sender<Message>, you: u32, name: &str, names: bool) -> bool {
        let mut ok = Self::send_to(
            tx,
            &ToClient::Welcome {
                you,
                world: self.cfg.name.clone(),
                palette: self.cfg.palette.clone(),
                block_size: self.cfg.block_size,
                bounds: self.cfg.bounds,
                spawn: self.spawn,
                settings: self.settings_now(),
                name: name.to_string(),
            },
        );
        for k in self.world.keys() {
            let c = self.world.chunk(k).expect("listed");
            ok &= Self::send_to(tx, &ToClient::Chunk { key: k, version: c.version, data: c.encode() });
        }
        ok &= Self::send_to(tx, &ToClient::Ready);
        ok &= Self::send_to(tx, &ToClient::Weather(Box::new(self.weather.keyframe())));
        for p in self.players.values() {
            let name = if names { p.name.clone() } else { String::new() };
            ok &= Self::send_to(tx, &ToClient::Joined { player: p.id, name, avatar: p.avatar.clone() });
        }
        ok
    }

    /// Someone watching without playing: the world and what happens in it, no names.
    pub(super) fn watch(&mut self, conn: u64, tx: mpsc::Sender<Message>, protocol: u32) {
        let bye = |why: &str| {
            let _ = tx.try_send(Message::Binary(wire::encode(&ToClient::Bye { why: why.into() }).into()));
        };
        if protocol != PROTOCOL {
            return bye(&format!("this world speaks version {PROTOCOL}; please update"));
        }
        if self.watchers.len() >= self.cfg.max_watchers {
            return bye("too many watching; try again soon");
        }
        // no player has this number: the watcher's own poses never come back to it
        if self.send_world(&tx, u32::MAX, "", false) {
            self.watchers.insert(conn, tx);
        }
    }

    pub(super) fn join(&mut self, conn: u64, tx: mpsc::Sender<Message>, protocol: u32, name: String, avatar: Vec<u8>, account: Option<u64>) {
        let bye = |why: &str| {
            let _ = tx.try_send(Message::Binary(wire::encode(&ToClient::Bye { why: why.into() }).into()));
        };
        if protocol != PROTOCOL {
            return bye(&format!("this world speaks version {PROTOCOL}; please update"));
        }
        if self.players.len() >= self.cfg.max_players {
            return bye("the world is full");
        }
        let id = conn as u32;
        let name = if name.is_empty() { format!("player {id}") } else { name };
        let ok = self.send_world(&tx, id, &name, true);
        if !ok {
            return;
        }
        self.broadcast(&ToClient::Joined { player: id, name: name.clone(), avatar: avatar.clone() }, None);
        eprintln!("{name} joined ({} here)", self.players.len() + 1);
        let bucket = Bucket::new(self.cfg.edit_rate, self.cfg.edit_rate);
        self.players.insert(conn, Player { id, name: name.clone(), avatar, tx, at: None, moves: Default::default(), corrected_at: -1.0, bucket, account, placed: 0 });
    }

    /// The settings a new arrival gets: the world's own, with the portals players opened.
    pub(super) fn settings_now(&self) -> String {
        if self.portals.is_empty() {
            return self.settings.clone();
        }
        let mut s: serde_json::Value = serde_json::from_str(&self.settings).unwrap_or_else(|_| serde_json::json!({}));
        let Some(obj) = s.as_object_mut() else { return self.settings.clone() };
        let list = obj.entry("portals").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = list.as_array_mut() {
            a.extend(self.portals.iter().cloned());
        }
        s.to_string()
    }

    pub(super) fn leave(&mut self, conn: u64) {
        self.watchers.remove(&conn);
        if let Some(p) = self.players.remove(&conn) {
            eprintln!("{} left ({} here)", p.name, self.players.len());
            self.broadcast(&ToClient::Left { player: p.id }, None);
        }
    }

    pub(super) fn send_weather(&mut self) {
        self.keyframe_at = Instant::now();
        let m = ToClient::Weather(Box::new(self.weather.keyframe()));
        self.broadcast(&m, None);
    }

    pub(super) fn message(&mut self, conn: u64, msg: ToServer) {
        let now = self.started.elapsed().as_secs_f64();
        let Some(p) = self.players.get_mut(&conn) else { return };
        match msg {
            ToServer::Hello { .. } | ToServer::Watch { .. } => {}
            ToServer::Climate(c) => {
                if !p.bucket.take(now) {
                    return;
                }
                let c = Climate { ground: self.weather.climate.ground, ..c.sane() };
                eprintln!("{} changed the weather", p.name);
                self.weather.climate = c;
                if let Ok(j) = serde_json::to_string(&c) {
                    let _ = self.db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('climate', ?1)", [j]);
                }
                self.send_weather();
            }
            ToServer::Pose { at, blob } => {
                // where they say they are must be somewhere they could have got to
                let b = self.cfg.bounds;
                let inside = at.iter().all(|v| v.is_finite())
                    && (at[0] as i32) > b.lo[0] - 64
                    && (at[0] as i32) < b.hi[0] + 64
                    && (at[2] as i32) > b.lo[2] - 64
                    && (at[2] as i32) < b.hi[2] + 64
                    && (at[1] as i32) > b.lo[1] - 64
                    && (at[1] as i32) < b.hi[1] + 128;
                let checked = if !inside {
                    Err(p.moves.last().unwrap_or(self.spawn))
                } else if self.cfg.trust_moves {
                    Ok(())
                } else {
                    p.moves.check(at, now, self.cfg.block_size, self.spawn)
                };
                match checked {
                    Ok(()) => {
                        p.at = Some(at);
                        let m = ToClient::Pose { player: p.id, blob };
                        self.broadcast(&m, Some(conn));
                    }
                    Err(back) => {
                        // not shown to anyone; they're told to go back
                        if now - p.corrected_at >= 0.5 {
                            p.corrected_at = now;
                            let tx = p.tx.clone();
                            let _ = Self::send_to(&tx, &ToClient::Correct { at: back });
                        }
                    }
                }
            }
            ToServer::Edit { id, cell, block } => {
                let checked = if !p.bucket.take(now) {
                    Err(Why::TooFast)
                } else {
                    // the world's own checks first (bounds, palette), then reach, then plots
                    let account = p.account;
                    let claimed = self.plots.iter().any(|pl| pl.holds(cell) && !account.is_some_and(|a| pl.builders.contains(&a)));
                    self.world.check(cell, block).and_then(|_| {
                        if !within_reach(p.at, cell) {
                            Err(Why::TooFar)
                        } else if claimed {
                            Err(Why::Claimed)
                        } else {
                            self.world.edit(cell, block)
                        }
                    })
                };
                match checked {
                    Err(why) => {
                        if !Self::send_to(&p.tx, &ToClient::Refused { id, why }) {
                            self.leave(conn);
                        }
                    }
                    Ok(old) => {
                        if block != 0 {
                            p.placed = p.placed.saturating_add(1);
                        }
                        self.seq += 1;
                        let a = Applied { seq: self.seq, player: p.id, id, cell, block };
                        self.log.push((self.seq, now_unix(), p.name.clone(), cell, old, block));
                        self.dirty.insert(ChunkKey::of(cell));
                        self.snap_changed.insert(vox_format::region_of(ChunkKey::of(cell)));
                        self.broadcast(&ToClient::Edits(vec![a]), None);
                    }
                }
            }
        }
    }
}

pub(super) fn within_reach(at: Option<[f32; 3]>, cell: [i32; 3]) -> bool {
    let Some(at) = at else { return false };
    let eye = [at[0], at[1] + EYES, at[2]];
    let d2: f32 = (0..3).map(|i| (cell[i] as f32 + 0.5 - eye[i]).powi(2)).sum();
    d2 <= REACH * REACH
}
