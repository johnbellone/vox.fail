//! The world thread's state at work: joining and leaving, messages, edits and their checks,
//! the weather, plots and portals from the directory, saving to SQLite.

#[allow(unused_imports)]
use super::*;
use crate::entities::{self, Entities};
use vox_core::edit::{self, Ctx, EYES, PlotArea, REACH, Role};
use vox_core::entity::{Axis, Component, Dest, EntityWhy, Portal, Settings, Spawn};
use vox_core::portal::Door;
use vox_core::wire::EntityChange;

/// How far from a doorway's plane (blocks) a player may be and still go through it: their last
/// checked pose can be a step or two behind the client's.
const THROUGH_SLACK: f32 = 3.0;

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
        crate::entities::create_tables(&db)?;
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
            _ => match &cfg.tile {
                Some(t) => t.settings().to_string(),
                None => cfg.seed.as_deref().map(vox_core::level::settings).unwrap_or_else(|| "{}".into()),
            },
        };
        let mut world = World::new(cfg.bounds, cfg.palette.len() as u16);
        let mut dirty = HashSet::new();
        if fresh && let Some(t) = &cfg.tile {
            vox_core::terrain::fill(&mut world, t, &cfg.palette);
            dirty.extend(world.keys());
        } else if fresh {
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
        let ground = ground_of(&set, cfg.block_size);
        // entities: a new world takes its seed level (or an atlas tile's settings); a world
        // from before entities existed takes its stored settings once; then the spawn. The
        // mark, not the map, says whether a world was seeded: an emptied world stays empty.
        let mut ents = Entities::load(&db)?;
        let mut reseeded = false;
        if meta("entities_seeded").is_none() {
            reseeded = true;
            let mut seed = if !fresh {
                seed_from_settings(&settings)
            } else if let Some(t) = &cfg.tile {
                vec![Component::Settings(Settings { json: t.settings().to_string() })]
            } else if let Some(level) = &cfg.seed {
                vox_core::level::entities(level)?
            } else {
                Vec::new()
            };
            seed.push(Component::Spawn(Spawn { at: spawn, yaw: 0.0 }));
            ents.seed(seed);
        }
        let climate = match meta("climate").and_then(|c| serde_json::from_str::<Climate>(&c).ok()) {
            Some(c) if !fresh => c.sane(),
            _ => Climate::from_settings(set.get("weather"), ground),
        };
        let weather = weather_from(climate, &set, name_seed(&cfg.name));
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
            party_of: HashMap::new(),
            snaps: None,
            snap_prev: None,
            snap_changed: HashSet::new(),
            snap_full: true,
            last_snapshot: Instant::now(),
            trips: None,
            objective: None,
            reached: false,
            goals: None,
            entities: ents,
            editors: None,
            editor_cache: HashMap::new(),
            portals: None,
            portal_routes: HashMap::new(),
            refunds_due: Vec::new(),
            applied: set,
            settings_unsaved: false,
        };
        // the entities are the source of truth: where the stored settings differ from them, the
        // world takes the entities' (a change made while it was closed)
        s.refresh_from_entities();
        if fresh || s.settings_unsaved {
            s.flush()?;
        }
        if let Some(dir) = s.cfg.snapshots.clone() {
            let snaps = vox_format::Snapshots::new(SnapStore::open(&dir, s.cfg.snapshot_oxen.as_deref())?);
            let latest = snaps.latest().map_err(|e| format!("snapshots: {e}"))?;
            // the last snapshot is a base only if nothing happened after it, and only if it holds
            // entities (one from before them doesn't, so the next snapshot is whole); entities
            // seeded now are not in any snapshot yet
            let predates = latest.as_ref().is_some_and(|m| !m.entities);
            s.snap_full = latest.as_ref().is_none_or(|m| m.seq != s.seq) || fresh || predates || reseeded;
            s.snap_prev = latest;
            s.snaps = Some(snaps);
        }
        Ok(s)
    }

    pub(super) fn run(mut self, mut rx: mpsc::Receiver<Event>) {
        while let Some(ev) = rx.blocking_recv() {
            match ev {
                Event::Join { conn, tx, protocol, name, avatar, account, arrive } => self.join(conn, tx, protocol, name, avatar, account, arrive),
                Event::Msg { conn, msg } => self.message(conn, msg),
                Event::Watch { conn, tx, protocol } => self.watch(conn, tx, protocol),
                Event::Leave { conn } => self.leave(conn),
                Event::Tick => {
                    let behind = self.weather_at.elapsed().as_secs_f32();
                    if behind >= WEATHER_STEP {
                        if let Some(w) = &mut self.weather {
                            w.step(behind.min(5.0));
                        }
                        self.weather_at = Instant::now();
                    }
                    if self.weather.is_some() && self.keyframe_at.elapsed().as_secs_f32() >= KEYFRAME_EVERY {
                        self.send_weather();
                    }
                    if self.last_flush.elapsed() >= self.cfg.flush_every {
                        self.save();
                    }
                    // a refund that could not be queued is tried once more, here
                    for (account, key) in std::mem::take(&mut self.refunds_due) {
                        if !self.ask_refund(account, key) {
                            eprintln!("couldn't refund portal {key} to account {account}: the directory's queue is full twice");
                        }
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
                        watchers: self.watchers.len(),
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
                Event::Rules(plots, portals, parties) => {
                    self.plots = plots;
                    let changes = self.reconcile_portals(&portals);
                    self.broadcast_entities(changes);
                    // rebuilt whole, so a party the directory dropped stops applying. Parties come
                    // from the directory's rules, fetched every `rules_every` (default 30 s), so a
                    // new party can take up to that long to be able to talk.
                    self.party_of.clear();
                    for (i, party) in parties.iter().enumerate() {
                        for &account in party {
                            self.party_of.entry(account).or_insert(i as u32);
                        }
                    }
                }
                Event::Ticket { conn, url, ticket, arrive } => {
                    if let Some(p) = self.players.get(&conn) {
                        let _ = Self::send_to(&p.tx, &ToClient::Travel { url, ticket, arrive });
                    }
                }
                Event::Editor { conn, account, editor } => self.editor_answer(conn, account, editor),
                Event::Portal { conn, account, call, status, body } => self.portal_answer(conn, account, call, status, body),
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
        if self.dirty.is_empty() && self.log.is_empty() && !self.entities.changed() && !self.settings_unsaved && self.seq > 0 {
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
        self.entities.write(&t)?;
        t.commit().map_err(e)?;
        // forget what was written only now: a failed write keeps it for the next flush
        self.dirty.clear();
        self.log.clear();
        self.entities.written();
        self.settings_unsaved = false;
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

    /// Makes the directory's portals the keyed portal entities: a changed one is replaced, a new
    /// one added, one the directory dropped removed. Each change takes a new edit number and is
    /// logged like an edit; the changes come back for everyone to be told.
    pub(super) fn reconcile_portals(&mut self, portals: &[serde_json::Value]) -> Vec<EntityChange> {
        let mut wanted: std::collections::BTreeMap<i64, Component> = std::collections::BTreeMap::new();
        let mut routes = HashMap::new();
        for v in portals {
            // a portal without a valid key (or doorway) is left out
            if let Some((key, c, route)) = entities::from_directory(v)
                && !wanted.contains_key(&key)
            {
                wanted.insert(key, c);
                routes.insert(key, route);
            }
        }
        // where each keyed portal leads (for travel and for a destination change), as the list says
        self.portal_routes = routes;
        let (have, stale) = self.entities.keyed_portals(&wanted);
        let at = now_unix();
        let mut changes = Vec::new();
        for id in stale {
            // the edit number is taken only once the change is made, so none is skipped
            let seq = self.seq + 1;
            let left = self.entities.map.get(&id).map(|(_, old)| entities::zone_key(old));
            if self.entities.remove(seq, at, "directory".into(), id).is_some() {
                self.seq = seq;
                // the zone it leaves is changed for the next snapshot
                self.snap_changed.extend(left);
                changes.push(EntityChange { seq, entity: id, component: None });
            }
        }
        for (key, c) in wanted {
            let held = have.get(&key).copied();
            if held.and_then(|id| self.entities.map.get(&id)).is_some_and(|(_, old)| *old == c) {
                continue;
            }
            let seq = self.seq + 1;
            // the zones it leaves and enters are changed for the next snapshot
            let left = held.and_then(|id| self.entities.map.get(&id)).map(|(_, old)| entities::zone_key(old));
            // None only when the id counter is spent: this portal is then not placed
            if let Some(id) = self.entities.put(seq, at, "directory".into(), held, c.clone()) {
                self.seq = seq;
                self.snap_changed.extend(left);
                self.snap_changed.insert(entities::zone_key(&c));
                changes.push(EntityChange { seq, entity: id, component: Some(c) });
            }
        }
        // the entities changed: an Objective or Settings is taken up here too (the editor's Put
        // and Remove, when they land, call this the same way)
        self.refresh_from_entities();
        changes
    }

    /// Takes the entities' word for the objective and for what the Settings entity sets (the
    /// ground, the climate, the sun), so that a change made while the world was closed, or by a
    /// restore, is what the world runs. Only a setting that changed is rebuilt: a weather change
    /// restarts the weather, a floor change moves the ground, and nothing else touches the
    /// running simulation.
    pub(super) fn refresh_from_entities(&mut self) {
        let objective = objective_of(&self.entities);
        if objective != self.objective {
            // a new place to reach is reached afresh
            self.reached = false;
            self.objective = objective;
        }
        let set = settings_of(&self.entities);
        let weather_changed = set.get("weather") != self.applied.get("weather");
        let floor_changed = set.get("floor") != self.applied.get("floor");
        let sun_changed =
            set.get("sky") != self.applied.get("sky") || set.get("time") != self.applied.get("time");
        let ground = ground_of(&set, self.cfg.block_size);
        if weather_changed {
            let climate = Climate::from_settings(set.get("weather"), ground);
            self.weather = weather_from(climate, &set, name_seed(&self.cfg.name));
        } else if floor_changed && let Some(w) = &mut self.weather {
            w.climate.ground = ground;
        }
        if sun_changed && let Some(w) = &mut self.weather {
            w.sun = sun_for(&set);
        }
        // a world with no air (weather "none") has no climate: a floor change touches nothing
        if weather_changed || (floor_changed && self.weather.is_some()) {
            // open reads the stored climate back, so it must follow the settings too (players'
            // changes to it are stored the same way, see `ToServer::Climate`). Going to no air
            // leaves no climate to keep, so the stored one goes.
            match self.weather.as_ref().and_then(|w| serde_json::to_string(&w.climate).ok()) {
                Some(j) => {
                    let _ = self.db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('climate', ?1)", [j]);
                }
                None => {
                    let _ = self.db.execute("DELETE FROM meta WHERE key = 'climate'", []);
                }
            }
            self.send_weather();
        }
        // the stored settings now say what was applied, so the next opening doesn't take a
        // change that is already in the entities as news (and overwrite a player's climate)
        let settings = set.to_string();
        if settings != self.settings {
            self.settings = settings;
            self.settings_unsaved = true;
        }
        self.applied = set;
    }

    /// Sends entity changes to everyone here (players and watchers), in `Entities` messages.
    pub(super) fn broadcast_entities(&mut self, changes: Vec<EntityChange>) {
        for batch in entities::batches(changes) {
            self.broadcast(&ToClient::Entities(batch), None);
        }
    }

    /// The world as a newcomer gets it: welcome, chunks, ready, weather, who's here, then every
    /// entity, nearest `from` first. False if they couldn't keep up.
    pub(super) fn send_world(&self, tx: &mpsc::Sender<Message>, you: u32, name: &str, names: bool, from: [f32; 3]) -> bool {
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
        if let Some(w) = &self.weather {
            ok &= Self::send_to(tx, &ToClient::Weather(Box::new(w.keyframe())));
        }
        for p in self.players.values() {
            let name = if names { p.name.clone() } else { String::new() };
            ok &= Self::send_to(tx, &ToClient::Joined { player: p.id, name, avatar: p.avatar.clone() });
        }
        for batch in entities::batches(self.entities.all_from(from)) {
            ok &= Self::send_to(tx, &ToClient::Entities(batch));
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
        let from = self.spawn;
        if self.send_world(&tx, u32::MAX, "", false, from) {
            self.watchers.insert(conn, tx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn join(&mut self, conn: u64, tx: mpsc::Sender<Message>, protocol: u32, name: String, avatar: Vec<u8>, account: Option<u64>, arrive: Option<[f32; 3]>) {
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
        let ok = self.send_world(&tx, id, &name, true, arrive.unwrap_or(self.spawn));
        if !ok {
            return;
        }
        self.broadcast(&ToClient::Joined { player: id, name: name.clone(), avatar: avatar.clone() }, None);
        eprintln!("{name} joined ({} here)", self.players.len() + 1);
        let bucket = Bucket::new(self.cfg.edit_rate, self.cfg.edit_rate);
        // Where they start, as far as move checks go: a ticket's arrival point, or for someone
        // signed in without one the spawn (a signed-in player coming through a portal brings a
        // ticket). Guests, who can't get tickets, and worlds without sign-in take the first
        // position as it is.
        let mut moves = vox_core::moves::Moves::default();
        let now = self.started.elapsed().as_secs_f64();
        let start = arrive.or_else(|| (account.is_some() && !self.cfg.trust_moves).then_some(self.spawn));
        if let Some(at) = start {
            moves.start(at, now);
        }
        let voice = Bucket::new(MAX_VOICE_PER_SECOND, VOICE_BURST);
        let edits = Bucket::new(EDIT_RATE, EDIT_RATE);
        self.players.insert(conn, Player { id, name: name.clone(), avatar, tx, at: start, moves, corrected_at: -1.0, bucket, voice, account, placed: 0, edits, pending: None });
    }

    /// On to `url`, arriving at `arrive`: with a ticket from the directory when there is one to
    /// be had (a signed-in player, a directory), else straight away without.
    fn send_on(&self, conn: u64, account: Option<u64>, tx: &mpsc::Sender<Message>, url: String, arrive: [f32; 3]) {
        match (&self.trips, account) {
            (Some(trips), Some(account)) => {
                let trip = Trip { conn, account, to: url.clone(), arrive };
                if trips.try_send(trip).is_err() {
                    // the directory is far behind: go on without a ticket
                    let _ = Self::send_to(tx, &ToClient::Travel { url, ticket: None, arrive });
                }
            }
            // no directory here, or a guest: no ticket to be had
            _ => {
                let _ = Self::send_to(tx, &ToClient::Travel { url, ticket: None, arrive });
            }
        }
    }

    /// Off this tile's `edge` (`east`, `west`, `south`, `north`) onto the next tile of the
    /// atlas, if there is one that way and they're really at that edge: they come out at the
    /// same place on the map, just inside the next tile, standing on its ground.
    fn over_edge(&mut self, conn: u64, edge: &str) {
        let Some(tile) = &self.cfg.tile else { return };
        let Some((_, d)) = vox_core::terrain::EDGES.iter().find(|(n, _)| *n == edge) else { return };
        let Some(next) = tile.next(*d) else { return };
        let Some(p) = self.players.get(&conn) else { return };
        let Some(at) = p.at else { return };
        let b = self.cfg.bounds;
        let (lo, hi) = ([b.lo[0] as f32, b.lo[2] as f32], [b.hi[0] as f32, b.hi[2] as f32]);
        let near = match d {
            [1, 0] => at[0] >= hi[0] - THROUGH_SLACK,
            [-1, 0] => at[0] <= lo[0] + THROUGH_SLACK,
            [0, 1] => at[2] >= hi[1] - THROUGH_SLACK,
            _ => at[2] <= lo[1] + THROUGH_SLACK,
        };
        if !self.cfg.trust_moves && !near {
            return;
        }
        // the same place on the map, a step inside the next tile
        let t = vox_core::terrain::TILE as f32;
        let mut x = (at[0] - d[0] as f32 * t).clamp(lo[0] + 2.0, hi[0] - 2.0);
        let mut z = (at[2] - d[1] as f32 * t).clamp(lo[1] + 2.0, hi[1] - 2.0);
        if !x.is_finite() || !z.is_finite() {
            (x, z) = (0.5, 0.5);
        }
        let ground = next.height(x.floor() as i32, z.floor() as i32).max(vox_core::terrain::SEA) + 1;
        let arrive = [x, ground as f32, z];
        let url = format!("/w/{}/ws", next.id());
        let (account, tx) = (p.account, p.tx.clone());
        self.send_on(conn, account, &tx, url, arrive);
    }

    /// Someone stands at the objective: it's reached (once), everyone here is told, and the
    /// directory hears who was here.
    pub(super) fn reach(&mut self) {
        self.reached = true;
        let name = self.objective.as_ref().map_or_else(|| "the objective".into(), |o| o.name.clone());
        let mut c = name.chars();
        let name = c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect());
        eprintln!("{name} reached");
        self.broadcast(&ToClient::Notice { text: format!("{name}: reached") }, None);
        let here: Vec<u64> = self.players.values().filter_map(|p| p.account).collect();
        if let Some(goals) = &self.goals
            && !here.is_empty()
        {
            let _ = goals.try_send(here);
        }
    }

    /// The settings a new arrival gets: the world's Settings entity (its portals and objective
    /// are entities of their own, sent with the rest).
    pub(super) fn settings_now(&self) -> String {
        let json = self
            .entities
            .map
            .values()
            .find_map(|(_, c)| match c {
                Component::Settings(s) => Some(s.json.as_str()),
                _ => None,
            })
            .unwrap_or("{}");
        let mut s: serde_json::Value = serde_json::from_str(json).unwrap_or_else(|_| serde_json::json!({}));
        if let Some(obj) = s.as_object_mut() {
            // how far nearby voice carries here, so clients fade voices over the same distance
            obj.insert("voice_radius".into(), serde_json::json!(self.cfg.voice_radius));
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
        let Some(w) = &self.weather else { return };
        let m = ToClient::Weather(Box::new(w.keyframe()));
        self.broadcast(&m, None);
    }

    /// A player's `Put` or `Remove`: refused at once when they're over their edit rate or
    /// already waiting on the directory, otherwise judged now, or, for advanced rights not yet
    /// known, held while the directory answers (`editor_answer`).
    fn entity_edit(&mut self, conn: u64, id: u32, edit: Edit) {
        let now = self.started.elapsed().as_secs_f64();
        let Some(p) = self.players.get_mut(&conn) else { return };
        let (account, tx) = (p.account, p.tx.clone());
        if !p.edits.take(now) || p.pending.is_some() {
            return self.refuse_entity(conn, &tx, id, EntityWhy::TooFast);
        }
        // a guest is judged as a builder, which the rules refuse
        let Some(account) = account else {
            return self.apply_entity_edit(conn, id, edit, Role::Builder);
        };
        if let Some(role) = self.role_of(account) {
            return self.apply_entity_edit(conn, id, edit, role);
        }
        // advanced rights are asked of the directory; the edit waits for the answer
        let asked = self.editors.as_ref().is_some_and(|asks| asks.try_send(Asking { conn, account }).is_ok());
        if !asked {
            return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
        }
        if let Some(p) = self.players.get_mut(&conn) {
            // the role is set when the answer comes (see `Pending`)
            p.pending = Some(Pending { id, edit, role: Role::Builder });
        }
    }

    /// What a signed-in account may do without asking: everyone (`--editors-all`), builders
    /// (no directory to ask), or the answer kept for `EDITOR_CACHE`. None: ask the directory.
    fn role_of(&self, account: u64) -> Option<Role> {
        if self.cfg.editors_all {
            return Some(Role::Editor);
        }
        if self.editors.is_none() {
            return Some(Role::Builder);
        }
        self.editor_cache
            .get(&account)
            .filter(|(at, _)| at.elapsed() < EDITOR_CACHE)
            .map(|(_, editor)| if *editor { Role::Editor } else { Role::Builder })
    }

    /// The directory answered whether `account` may make advanced edits (`None`: it did not
    /// answer, which refuses the held edit). A held edit is then judged, as it would have been.
    fn editor_answer(&mut self, conn: u64, account: u64, editor: Option<bool>) {
        if let Some(e) = editor {
            self.editor_cache.retain(|_, (at, _)| at.elapsed() < EDITOR_CACHE);
            self.editor_cache.insert(account, (Instant::now(), e));
        }
        let Some(p) = self.players.get_mut(&conn) else { return };
        let Some(pending) = p.pending.take() else { return };
        let tx = p.tx.clone();
        match editor {
            None => self.refuse_entity(conn, &tx, pending.id, EntityWhy::Unavailable),
            Some(true) => self.apply_entity_edit(conn, pending.id, pending.edit, Role::Editor),
            Some(false) => self.apply_entity_edit(conn, pending.id, pending.edit, Role::Builder),
        }
    }

    /// Tells the sender their edit was refused, and no one else.
    fn refuse_entity(&mut self, conn: u64, tx: &mpsc::Sender<Message>, id: u32, why: EntityWhy) {
        if !Self::send_to(tx, &ToClient::EntityRefused { id, why }) {
            self.leave(conn);
        }
    }

    /// Checks an edit against the rules, then makes it. A player portal the directory keeps
    /// (a new one, which costs Authority, or a keyed one, moved, re-linked or removed) is held
    /// while the directory answers (`hold_for_directory`, `portal_answer`); anything else is
    /// made at once (`commit_edit`).
    fn apply_entity_edit(&mut self, conn: u64, id: u32, edit: Edit, role: Role) {
        let Some(p) = self.players.get(&conn) else { return };
        let (account, at, tx) = (p.account, p.at, p.tx.clone());
        if let Err(why) = self.judge(&edit, role, account, at) {
            return self.refuse_entity(conn, &tx, id, why);
        }
        if let Some(account) = account {
            match self.directory_first(&edit, role, false) {
                Ok(None) => {}
                Ok(Some((call, body))) => return self.hold_for_directory(conn, Pending { id, edit, role }, account, call, body),
                Err(why) => return self.refuse_entity(conn, &tx, id, why),
            }
        }
        self.commit_edit(conn, id, edit);
    }

    /// The directory call a judged edit needs before the world may make it, if any: a new
    /// portal by a builder is charged (`Make`); a keyed portal is moved (`Update`, unless
    /// `moved` is already done), given another destination (`Link`), or removed (`Remove`).
    /// None: the world makes the edit alone. Errors refuse the edit: a destination the
    /// directory cannot hold for a keyed portal (see `keyed_call`).
    fn directory_first(&self, edit: &Edit, role: Role, moved: bool) -> Result<Option<(PortalCall, serde_json::Value)>, EntityWhy> {
        match edit {
            Edit::Put { entity: None, component: Component::Portal(p), .. } if role == Role::Builder => {
                // the directory charges a builder's portal and keeps its key, so it is asked
                // before it is placed; an editor's is free, and unkeyed
                Ok(Some((PortalCall::Make, make_body(p))))
            }
            Edit::Put { entity: Some(id), component: Component::Portal(new), .. } => match self.entities.map.get(id) {
                Some((_, Component::Portal(old))) if old.key.is_some() => {
                    // a player's portal belongs to its owner: an editor's change is refused here,
                    // not asked of the directory, which would refuse it for the owner check
                    if role == Role::Editor {
                        return Err(EntityWhy::NotYours);
                    }
                    let current = old.key.and_then(|k| self.portal_routes.get(&k)).and_then(|r| r.to.as_deref());
                    keyed_call(old, new, moved, current)
                }
                _ => Ok(None),
            },
            Edit::Remove { entity, .. } => match self.entities.map.get(entity) {
                Some((_, Component::Portal(p))) => match p.key {
                    // as above: an editor may not remove a player's portal
                    Some(_) if role == Role::Editor => Err(EntityWhy::NotYours),
                    Some(key) => Ok(Some((PortalCall::Remove, serde_json::json!({ "key": key })))),
                    None => Ok(None),
                },
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }

    /// Holds a judged edit while the directory answers a portal call (one held edit per player),
    /// and asks it. Without a directory, or when the question can't be queued, the edit is
    /// refused as unavailable.
    ///
    /// A `Make` carries an idempotency key built from the world, the account, the connection
    /// and the edit's id, so a retry of the same edit on the same connection is answered by the
    /// directory without a second charge. A client that retries after `Unavailable` with a new
    /// edit id (or after reconnecting) makes a new request, which the directory may charge
    /// again. The editor reuses the id for a retry; any other client must do the same.
    fn hold_for_directory(&mut self, conn: u64, pending: Pending, account: u64, call: PortalCall, mut body: serde_json::Value) {
        let Some(tx) = self.players.get(&conn).map(|p| p.tx.clone()) else { return };
        let id = pending.id;
        if self.portals.is_none() {
            return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
        }
        if call == PortalCall::Make {
            // one request id per edit: a repeat of the same request is answered, not charged again
            body["idem"] = serde_json::json!(idem_key(&self.cfg.name, account, conn, id));
        }
        let asked = self.portals.as_ref().is_some_and(|asks| {
            asks.try_send(PortalAsk { conn: Some(conn), account, call, body }).is_ok()
        });
        if !asked {
            return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
        }
        if let Some(p) = self.players.get_mut(&conn) {
            p.pending = Some(pending);
        }
    }

    /// Asks the directory to refund a portal's charge, for a portal the world did not place.
    /// Nobody waits for the answer; a failed refund is logged by the office.
    fn refund_portal(&mut self, account: u64, key: i64) {
        if !self.ask_refund(account, key) {
            // the queue is full: the tick tries once more (see `run`)
            self.refunds_due.push((account, key));
        }
    }

    fn ask_refund(&self, account: u64, key: i64) -> bool {
        self.portals.as_ref().is_some_and(|asks| {
            asks.try_send(PortalAsk { conn: None, account, call: PortalCall::Refund, body: serde_json::json!({ "key": key }) }).is_ok()
        })
    }

    /// Where walking through a doorway leads: its own url and arrival, or for a keyed portal to
    /// a world or a pair, the url and arrival its rules row gives (None: no travel).
    fn travel_of(&self, p: &Portal) -> Option<(Option<String>, [f32; 3])> {
        match &p.to {
            Dest::World { .. } | Dest::Pair { .. } => {
                let route = self.portal_routes.get(&p.key?)?;
                Some((Some(route.url.clone()?), route.arrive))
            }
            _ => p.travel(),
        }
    }

    /// The directory answered a held portal call for `conn`'s edit. Refusals (no answer, a
    /// refused call) refuse the edit. A new portal the directory made is judged again as it
    /// stands now, and placed with its key (or refunded, when the rules no longer allow it). A
    /// moved keyed portal is then given its destination, if that changed, or is made here.
    fn portal_answer(&mut self, conn: u64, account: u64, call: PortalCall, status: Option<u16>, body: serde_json::Value) {
        let Some(p) = self.players.get_mut(&conn) else {
            // they left while the directory answered: a portal it made is given back, not placed
            if let (PortalCall::Make, Some(200), Some(key)) = (call, status, body["key"].as_i64()) {
                self.refund_portal(account, key);
            }
            return;
        };
        let (tx, at) = (p.tx.clone(), p.at);
        let Some(Pending { id, edit, role }) = p.pending.take() else { return };
        let Some(status) = status else {
            return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
        };
        if status != 200 {
            return self.refuse_entity(conn, &tx, id, portal_refusal(call, status));
        }
        match call {
            PortalCall::Make => {
                let Edit::Put { component: Component::Portal(mut made), .. } = edit else {
                    return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
                };
                // a reply with no key cannot be placed or refunded by key: the player is refused
                let Some(key) = body["key"].as_i64() else {
                    return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
                };
                // a rules fetch that came first has already placed this portal (its key is in
                // the list the directory keeps): it is placed, so nothing more is answered
                if self.entities.map.values().any(|(_, c)| matches!(c, Component::Portal(p) if p.key == Some(key))) {
                    return;
                }
                // judged as the builder proposed it (with no key, as a new portal must be), now
                let proposed = Edit::Put { entity: None, expect: None, component: Component::Portal(made.clone()) };
                if let Err(why) = self.judge(&proposed, role, Some(account), at) {
                    self.refund_portal(account, key);
                    return self.refuse_entity(conn, &tx, id, why);
                }
                made.key = Some(key);
                if let Some(name) = body["name"].as_str() {
                    made.name = name.to_string();
                }
                if let Err(why) = Component::Portal(made.clone()).validate() {
                    self.refund_portal(account, key);
                    return self.refuse_entity(conn, &tx, id, why);
                }
                let placed = self.commit_edit(conn, id, Edit::Put { entity: None, expect: None, component: Component::Portal(made) });
                if !placed {
                    // no id was left for it: the charge is given back (the player was refused)
                    self.refund_portal(account, key);
                }
            }
            PortalCall::Update => match self.directory_first(&edit, role, true) {
                Ok(Some((next, next_body))) => self.hold_for_directory(conn, Pending { id, edit, role }, account, next, next_body),
                Ok(None) => self.commit_after_directory(conn, id, edit, role),
                Err(why) => self.refuse_entity(conn, &tx, id, why),
            },
            PortalCall::Link => self.commit_after_directory(conn, id, edit, role),
            PortalCall::Remove => {
                if body["removed"].as_bool() == Some(true) {
                    self.commit_after_directory(conn, id, edit, role);
                } else {
                    // the directory had no such portal: the list it keeps has already moved on
                    self.refuse_entity(conn, &tx, id, EntityWhy::Changed);
                }
            }
            PortalCall::Refund => {}
        }
    }

    /// Judges an edit again, now the directory has answered (the place, the rights and the
    /// caps may all have changed while it was held), then makes it.
    fn commit_after_directory(&mut self, conn: u64, id: u32, edit: Edit, role: Role) {
        let Some(p) = self.players.get(&conn) else { return };
        let (account, at, tx) = (p.account, p.at, p.tx.clone());
        if let Err(why) = self.judge(&edit, role, account, at) {
            return self.refuse_entity(conn, &tx, id, why);
        }
        self.commit_edit(conn, id, edit);
    }

    /// Makes an edit the rules have passed: a new edit number (taken only once the change is
    /// made), the entity log, and the change to everyone. An Objective or Settings change is
    /// taken up by the running world. Returns whether it was made; when it was not (no id left
    /// for a new entity), the player has been refused.
    fn commit_edit(&mut self, conn: u64, id: u32, edit: Edit) -> bool {
        let Some(p) = self.players.get(&conn) else { return false };
        let (name, tx) = (p.name.clone(), p.tx.clone());
        let seq = self.seq + 1;
        let when = now_unix();
        let (change, zones, touched) = match edit {
            Edit::Put { entity, component, .. } => {
                let old = entity.and_then(|e| self.entities.map.get(&e)).map(|(_, c)| c.clone());
                let Some(eid) = self.entities.put(seq, when, name, entity, component.clone()) else {
                    // no id left to give a new entity
                    self.refuse_entity(conn, &tx, id, EntityWhy::TooMany);
                    return false;
                };
                let mut zones = vec![entities::zone_key(&component)];
                zones.extend(old.as_ref().map(entities::zone_key));
                let touched = is_objective_or_settings(&component) || old.as_ref().is_some_and(is_objective_or_settings);
                (EntityChange { seq, entity: eid, component: Some(component) }, zones, touched)
            }
            Edit::Remove { entity, .. } => {
                let Some(old) = self.entities.remove(seq, when, name, entity) else {
                    self.refuse_entity(conn, &tx, id, EntityWhy::Changed);
                    return false;
                };
                (EntityChange { seq, entity, component: None }, vec![entities::zone_key(&old)], is_objective_or_settings(&old))
            }
        };
        self.seq = seq;
        // the zones it was in and is in are changed for the next snapshot
        self.snap_changed.extend(zones);
        self.broadcast_entities(vec![change]);
        if touched {
            self.refresh_from_entities();
        }
        true
    }

    /// The entity the edit is about, as the sender last saw it: it must exist, and `expect` must
    /// be the seq of its last change (`Changed` otherwise).
    fn held(&self, id: EntityId, expect: Option<u64>) -> Result<&Component, EntityWhy> {
        let seq = self.entities.seq_of(id).ok_or(EntityWhy::Changed)?;
        if expect != Some(seq) {
            return Err(EntityWhy::Changed);
        }
        self.entities.map.get(&id).map(|(_, c)| c).ok_or(EntityWhy::Changed)
    }

    /// The rules for an edit, in order: staleness, then `vox_core::edit`'s checks (rights,
    /// destinations, place, caps), then the world's own: the old place is in reach too (so a
    /// far entity cannot be dragged), and at most one Objective and one Settings.
    fn judge(&self, edit: &Edit, role: Role, account: Option<u64>, at: Option<[f32; 3]>) -> Result<(), EntityWhy> {
        let plots: Vec<PlotArea> = self.plots.iter().map(Plot::area).collect();
        let ground = |c: [i32; 3]| self.world.get(c) != 0;
        let spawns = self.entities.map.values().filter(|(_, c)| matches!(c, Component::Spawn(_))).count();
        match edit {
            Edit::Put { entity, expect, component } => {
                // the values first: the zone and plot helpers below read the component's position
                component.validate()?;
                let old = match entity {
                    None => None,
                    Some(id) => Some(self.held(*id, *expect)?),
                };
                let zone = entities::zone_key(component);
                let ctx = Ctx {
                    role,
                    account,
                    at,
                    bounds: self.cfg.bounds,
                    plots: &plots,
                    zone_entities: self.entities.zone_count(zone),
                    moves_zone: old.is_some_and(|o| entities::zone_key(o) != zone),
                    plot_portals: plot_portals_at(&plots, component, &self.entities),
                    spawns,
                    ground: &ground,
                };
                edit::check_put(&ctx, old, component)?;
                if let Some(from) = old.and_then(entities::position)
                    && !within_reach(at, cell_of(from))
                {
                    return Err(EntityWhy::TooFar);
                }
                if is_objective_or_settings(component) {
                    let kind = std::mem::discriminant(component);
                    if self.entities.map.iter().any(|(&other, (_, c))| Some(other) != *entity && std::mem::discriminant(c) == kind) {
                        return Err(EntityWhy::TooMany);
                    }
                }
                Ok(())
            }
            Edit::Remove { entity, expect } => {
                let old = self.held(*entity, *expect)?;
                let ctx = Ctx {
                    role,
                    account,
                    at,
                    bounds: self.cfg.bounds,
                    plots: &plots,
                    zone_entities: 0,
                    moves_zone: false,
                    plot_portals: 0,
                    spawns,
                    ground: &ground,
                };
                edit::check_remove(&ctx, old)
            }
        }
    }

    pub(super) fn message(&mut self, conn: u64, msg: ToServer) {
        let now = self.started.elapsed().as_secs_f64();
        let Some(p) = self.players.get_mut(&conn) else { return };
        match msg {
            ToServer::Hello { .. } | ToServer::Watch { .. } => {}
            ToServer::Put { id, entity, expect, component } => {
                self.entity_edit(conn, id, Edit::Put { entity, expect, component });
            }
            ToServer::Remove { id, entity, expect } => {
                self.entity_edit(conn, id, Edit::Remove { entity, expect });
            }
            // building nodes (phase 5) is not yet handled: refused, so the sender isn't left waiting
            ToServer::Build { id, .. } => {
                Self::send_to(&p.tx, &ToClient::EntityRefused { id, why: EntityWhy::Unavailable });
            }
            ToServer::Through { portal, side } => {
                if !p.bucket.take(now) {
                    return;
                }
                if let Some(edge) = portal.strip_prefix("edge:") {
                    return self.over_edge(conn, edge);
                }
                let (at, account, tx) = (p.at, p.account, p.tx.clone());
                // the doorway is a portal entity by name. A doorway within this world (no url)
                // is not a trip; a world or pair doorway is resolved by the directory (phase 2),
                // so it is not a trip here either. `travel` says which is which.
                let door = self.entities.map.values().find_map(|(_, c)| match c {
                    Component::Portal(p) if p.name == portal => self
                        .travel_of(p)
                        .filter(|(url, _)| url.is_some())
                        .map(|(url, arrive)| Door::from_portal(p, url, arrive)),
                    _ => None,
                });
                let Some((door, at)) = door.zip(at) else { return };
                // they must be in the doorway (or as good as: a pose or two behind), unless this
                // world takes positions on trust
                let side = if side < 0 { -1.0 } else { 1.0 };
                if !self.cfg.trust_moves && !(door.within(at) && door.side_of(at).abs() <= THROUGH_SLACK) {
                    return;
                }
                let arrive = door.carry(at, side);
                let url = door.url.clone().unwrap_or_default();
                self.send_on(conn, account, &tx, url, arrive);
            }
            ToServer::Climate(c) => {
                // no air here to change
                let Some(w) = &mut self.weather else { return };
                if !p.bucket.take(now) {
                    return;
                }
                let c = Climate { ground: w.climate.ground, ..c.sane() };
                eprintln!("{} changed the weather", p.name);
                w.climate = c;
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
                        if !self.reached && self.objective.as_ref().is_some_and(|o| o.holds(at)) {
                            self.reach();
                        }
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
            ToServer::Voice(packet) => {
                // only joined players get here (watchers are not in `players`); the shape and
                // the rate are checked before anything else is looked at
                if !p.voice.take(now) || !packet.valid() {
                    return;
                }
                let Some(at) = p.at else { return };
                let sender = VoiceWho { id: p.id, at, account: p.account };
                let others: Vec<VoiceWho> = self
                    .players
                    .iter()
                    .filter(|(c, _)| **c != conn)
                    .map(|(_, o)| VoiceWho { id: o.id, at: o.at.unwrap_or([f32::NAN; 3]), account: o.account })
                    .collect();
                let audience: HashSet<u32> = voice_audience(&sender, &others, self.cfg.voice_radius, &self.party_of, packet.channel).into_iter().collect();
                if audience.is_empty() {
                    return;
                }
                // relayed as they are: never kept, never logged. Encoded once; each recipient
                // gets a cheap clone of the same frame.
                let frame = Message::Binary(wire::encode(&ToClient::Voice { player: sender.id, packet }).into());
                for (c, o) in &self.players {
                    // a full queue drops the packet; speech is never worth waiting for
                    if *c != conn && audience.contains(&o.id) {
                        let _ = o.tx.try_send(frame.clone());
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
                        self.snap_changed.insert(vox_format::zone_of(ChunkKey::of(cell)));
                        self.broadcast(&ToClient::Edits(vec![a]), None);
                    }
                }
            }
        }
    }
}

/// The body of a directory `Make` call for a new portal: where, which way, how big, and where it
/// leads (a world by id, or a pair's key). Other destinations never reach here: the rules refuse
/// them for a builder first.
fn make_body(p: &Portal) -> serde_json::Value {
    let mut body = serde_json::json!({ "at": p.at, "axis": axis_name(p.axis), "size": p.size, "yaw": p.yaw });
    match &p.to {
        Dest::World { world } => body["to"] = serde_json::json!(world),
        Dest::Pair { key } => body["pair"] = serde_json::json!(key),
        _ => {}
    }
    body
}

/// The directory call a keyed portal's edit needs, after the rules have passed it: `Update`
/// when its place, axis, size or yaw changed (and `moved` is not yet done), else `Link` when its
/// destination changed, else None. `current` is the world the portal leads to now (its rules
/// row's `to`). A pair is `Link` to its other end. A world is the portal's own world: from a
/// pair it is `Link` with no pair, and otherwise no call. Any other world is `Unavailable` (make
/// a new portal for it).
fn keyed_call(old: &Portal, new: &Portal, moved: bool, current: Option<&str>) -> Result<Option<(PortalCall, serde_json::Value)>, EntityWhy> {
    let key = old.key.unwrap_or_default();
    let changed = old.at != new.at || old.axis != new.axis || old.size != new.size || old.yaw != new.yaw;
    if changed && !moved {
        let body = serde_json::json!({ "key": key, "at": new.at, "axis": axis_name(new.axis), "size": new.size, "yaw": new.yaw });
        return Ok(Some((PortalCall::Update, body)));
    }
    if old.to == new.to {
        return Ok(None);
    }
    let was_paired = matches!(old.to, Dest::Pair { .. });
    match &new.to {
        Dest::Pair { key: other } => Ok(Some((PortalCall::Link, serde_json::json!({ "key": key, "pair": other })))),
        Dest::World { world } if current == Some(world.as_str()) => {
            if was_paired { Ok(Some((PortalCall::Link, serde_json::json!({ "key": key, "pair": null })))) } else { Ok(None) }
        }
        _ => Err(EntityWhy::Unavailable),
    }
}

/// The name the directory's wire form gives an axis.
fn axis_name(a: Axis) -> &'static str {
    match a {
        Axis::X => "x",
        Axis::Z => "z",
    }
}

/// What a refused directory call says to the player: not enough Authority (the directory's 402)
/// is `CantAfford`, a portal not theirs or gone (403, 404) is `NotYours`, anything else
/// `Unavailable`.
fn portal_refusal(call: PortalCall, status: u16) -> EntityWhy {
    match status {
        402 if call == PortalCall::Make => EntityWhy::CantAfford,
        403 | 404 => EntityWhy::NotYours,
        _ => EntityWhy::Unavailable,
    }
}

/// The directory's request id for a portal this world asks it to make: 32 hex characters
/// from the world's name, the account, the connection and the edit id. Hashed because the
/// directory accepts at most 64 characters of `[A-Za-z0-9._:-]` and a world's name can be long
/// or hold other characters; two 64-bit FNV-1a hashes with different offsets make a collision
/// between two requests practically impossible (and the directory also checks the world and
/// account of a repeat).
fn idem_key(world: &str, account: u64, conn: u64, id: u32) -> String {
    let text = format!("{world}\u{0}{account}\u{0}{conn}\u{0}{id}");
    let fnv = |offset: u64| text.bytes().fold(offset, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{:016x}{:016x}", fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142))
}

pub(super) fn within_reach(at: Option<[f32; 3]>, cell: [i32; 3]) -> bool {
    let Some(at) = at else { return false };
    let eye = [at[0], at[1] + EYES, at[2]];
    let d2: f32 = (0..3).map(|i| (cell[i] as f32 + 0.5 - eye[i]).powi(2)).sum();
    d2 <= REACH * REACH
}

/// The cell a position (feet, in blocks) stands in.
fn cell_of(at: [f32; 3]) -> [i32; 3] {
    [at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32]
}

/// Objective and Settings are the world's own: at most one of each, and a change to one is taken
/// up by the running world (`refresh_from_entities`).
fn is_objective_or_settings(c: &Component) -> bool {
    matches!(c, Component::Objective(_) | Component::Settings(_))
}

/// The keyed portals (the directory's player portals) already in the plot a new component
/// stands in: the cap a builder's new portal is checked against.
fn plot_portals_at(plots: &[PlotArea], c: &Component, ents: &entities::Entities) -> usize {
    let Some(at) = entities::position(c) else { return 0 };
    let cell = cell_of(at);
    let Some(plot) = plots.iter().find(|p| p.holds(cell[0], cell[2])) else { return 0 };
    ents.map
        .values()
        .filter(|(_, c)| matches!(c, Component::Portal(Portal { key: Some(_), .. })))
        .filter(|(_, c)| entities::position(c).is_some_and(|p| plot.holds(cell_of(p)[0], cell_of(p)[2])))
        .count()
}

/// A joined player as voice routing sees them. `at` is NaN for someone who has not reported
/// a position yet: they can still hear a party, but not what is nearby.
struct VoiceWho {
    id: u32,
    at: [f32; 3],
    account: Option<u64>,
}

/// The ground a settings' `floor` gives, scaled by the block size (as it always was).
fn ground_of(set: &serde_json::Value, block_size: f32) -> f32 {
    set.get("floor").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32 * block_size
}

/// The sun a settings' sky and time give: the earth's by the hour, a steady half for any other sky.
fn sun_for(set: &serde_json::Value) -> f32 {
    match set.get("sky").and_then(|v| v.as_str()) {
        Some("earth") | None => sun_at(set.get("time").and_then(|v| v.as_f64()).unwrap_or(15.0) as f32),
        _ => 0.5,
    }
}

/// The weather a world runs with `climate`: none when its settings say "none" (no air to
/// simulate or send), else the weather with the sun the settings give, the air settled before
/// anyone arrives.
fn weather_from(climate: Climate, set: &serde_json::Value, seed: u32) -> Option<Weather> {
    let airless = set.get("weather").and_then(|v| v.as_str()) == Some("none");
    (!airless).then(|| {
        let mut weather = Weather::new(climate, seed);
        weather.sun = sun_for(set);
        weather.step(120.0);
        weather
    })
}

/// The seed of a world's weather: a hash of its name, so the same world always starts alike.
fn name_seed(name: &str) -> u32 {
    name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

/// The Settings entity's JSON (`{}` when there is none), read as `settings_now` reads it.
fn settings_of(ents: &Entities) -> serde_json::Value {
    let json = ents
        .map
        .values()
        .find_map(|(_, c)| match c {
            Component::Settings(s) => Some(s.json.as_str()),
            _ => None,
        })
        .unwrap_or("{}");
    serde_json::from_str(json).unwrap_or_else(|_| serde_json::json!({}))
}

/// The world's objective, from its Objective entity (the first, if there are two).
fn objective_of(ents: &Entities) -> Option<Objective> {
    ents.map.values().find_map(|(_, c)| match c {
        Component::Objective(o) => Some(Objective { name: o.name.clone(), at: o.at, radius: o.radius.min(64.0) }),
        _ => None,
    })
}

/// The entities a world from before entities takes from its stored settings. A stored value
/// that doesn't seed (a malformed objective, say) is logged and left out, and the rest is
/// seeded, so the world still opens: first all of it, then without the objective, then without
/// the portals too, and at last with no settings entity (the world then reads its defaults).
fn seed_from_settings(settings: &str) -> Vec<Component> {
    let mut world: serde_json::Map<String, serde_json::Value> = serde_json::from_str(settings).unwrap_or_default();
    for drop in [None, Some("objective"), Some("portals")] {
        if let Some(key) = drop {
            world.remove(key);
        }
        match vox_core::level::entities(&serde_json::json!({ "world": world }).to_string()) {
            Ok(list) => return list,
            Err(why) => eprintln!("stored settings don't seed entities (without {drop:?}): {why}"),
        }
    }
    Vec::new()
}

fn finite(p: [f32; 3]) -> bool {
    p.iter().all(|v| v.is_finite())
}

/// Who hears a packet from `sender` on `channel`, as player ids. Nearby: everyone within
/// `radius` blocks (3D, inclusive). Party: everyone whose account is in the sender's raid
/// party, at any distance; a guest has no party, so a guest's party packet goes nowhere.
/// The sender never hears their own packet, and an unknown channel reaches nobody.
fn voice_audience(sender: &VoiceWho, others: &[VoiceWho], radius: f32, party_of: &HashMap<u64, u32>, channel: u8) -> Vec<u32> {
    match channel {
        wire::VOICE_NEARBY => {
            if !finite(sender.at) {
                return Vec::new();
            }
            let r2 = radius * radius;
            others
                .iter()
                .filter(|o| o.id != sender.id && finite(o.at))
                .filter(|o| {
                    let d = [o.at[0] - sender.at[0], o.at[1] - sender.at[1], o.at[2] - sender.at[2]];
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2
                })
                .map(|o| o.id)
                .collect()
        }
        wire::VOICE_PARTY => {
            let Some(party) = sender.account.and_then(|a| party_of.get(&a)) else { return Vec::new() };
            others
                .iter()
                .filter(|o| o.id != sender.id && o.account.and_then(|a| party_of.get(&a)) == Some(party))
                .map(|o| o.id)
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;

    #[test]
    fn a_stored_objective_that_does_not_seed_leaves_the_rest() {
        // no radius: the objective can't be read, so it is left out and the settings are kept
        let list = seed_from_settings(r#"{"sky":"earth","objective":{"name":"altar","at":[0,0,0]}}"#);
        assert!(list.iter().any(|c| matches!(c, Component::Settings(s) if s.json.contains("earth"))));
        assert!(!list.iter().any(|c| matches!(c, Component::Objective(_))));
    }

    #[test]
    fn a_stored_portal_that_does_not_seed_leaves_the_settings() {
        // a name past MAX_NAME: the portals are left out, the settings are not
        let long = "p".repeat(vox_core::entity::MAX_NAME + 1);
        let s = format!(r#"{{"sky":"earth","portals":[{{"name":"{long}","at":[0,0,5],"level":"x","url":":7781","arrive":[0,0,0]}}]}}"#);
        let list = seed_from_settings(&s);
        assert!(list.iter().any(|c| matches!(c, Component::Settings(_))));
        assert!(!list.iter().any(|c| matches!(c, Component::Portal(_))));
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    fn who(id: u32, at: [f32; 3], account: Option<u64>) -> VoiceWho {
        VoiceWho { id, at, account }
    }

    fn parties(list: &[(u64, u32)]) -> HashMap<u64, u32> {
        list.iter().copied().collect()
    }

    #[test]
    fn nearby_within_radius_only() {
        let s = who(1, [0.0, 0.0, 0.0], None);
        let others = [who(2, [10.0, 0.0, 0.0], None), who(3, [0.0, 0.0, 40.0], None)];
        let none = HashMap::new();
        assert_eq!(voice_audience(&s, &others, 32.0, &none, wire::VOICE_NEARBY), vec![2]);
    }

    #[test]
    fn boundary_is_inclusive_in_3d() {
        let s = who(1, [0.0, 0.0, 0.0], None);
        let none = HashMap::new();
        // exactly on the radius counts
        let on = [who(2, [3.0, 0.0, 4.0], None)];
        assert_eq!(voice_audience(&s, &on, 5.0, &none, wire::VOICE_NEARBY), vec![2]);
        // a hair past it does not
        let past = [who(2, [3.0, 0.0, 4.001], None)];
        assert!(voice_audience(&s, &past, 5.0, &none, wire::VOICE_NEARBY).is_empty());
        // height counts too
        let up = [who(2, [0.0, 6.0, 0.0], None)];
        assert!(voice_audience(&s, &up, 5.0, &none, wire::VOICE_NEARBY).is_empty());
    }

    #[test]
    fn party_hears_at_any_distance() {
        let p = parties(&[(100, 0), (200, 0), (300, 1)]);
        let s = who(1, [0.0, 0.0, 0.0], Some(100));
        let others = [
            who(2, [5000.0, 0.0, 0.0], Some(200)),
            who(3, [0.0, 0.0, 0.0], Some(300)),
            who(4, [1.0, 0.0, 0.0], None),
        ];
        assert_eq!(voice_audience(&s, &others, 32.0, &p, wire::VOICE_PARTY), vec![2]);
    }

    #[test]
    fn party_member_without_position_still_hears_party() {
        let p = parties(&[(100, 0), (200, 0)]);
        let s = who(1, [0.0, 0.0, 0.0], Some(100));
        let others = [who(2, [f32::NAN; 3], Some(200))];
        assert_eq!(voice_audience(&s, &others, 32.0, &p, wire::VOICE_PARTY), vec![2]);
    }

    #[test]
    fn guest_has_no_party() {
        let p = parties(&[(100, 0), (200, 0)]);
        // a guest sends to party: nobody, even a party member nearby
        let guest = who(1, [0.0, 0.0, 0.0], None);
        let others = [who(2, [0.0, 0.0, 0.0], Some(100)), who(3, [0.0, 0.0, 0.0], Some(200))];
        assert!(voice_audience(&guest, &others, 32.0, &p, wire::VOICE_PARTY).is_empty());
        // a signed-in player with no party: nobody either
        let loner = who(1, [0.0, 0.0, 0.0], Some(999));
        assert!(voice_audience(&loner, &others, 32.0, &p, wire::VOICE_PARTY).is_empty());
        // and a guest's nearby packet still reaches those nearby
        assert_eq!(voice_audience(&guest, &others, 32.0, &p, wire::VOICE_NEARBY), vec![2, 3]);
    }

    #[test]
    fn sender_never_hears_self() {
        let p = parties(&[(100, 0)]);
        let s = who(1, [0.0, 0.0, 0.0], Some(100));
        let same = [who(1, [0.0, 0.0, 0.0], Some(100))];
        assert!(voice_audience(&s, &same, 32.0, &p, wire::VOICE_NEARBY).is_empty());
        assert!(voice_audience(&s, &same, 32.0, &p, wire::VOICE_PARTY).is_empty());
    }

    #[test]
    fn non_finite_positions_hear_nothing_nearby() {
        let none = HashMap::new();
        let others = [who(2, [f32::NAN, 0.0, 0.0], None), who(3, [0.0, f32::INFINITY, 0.0], None)];
        let s = who(1, [0.0, 0.0, 0.0], None);
        assert!(voice_audience(&s, &others, 32.0, &none, wire::VOICE_NEARBY).is_empty());
        // a sender with a non-finite position reaches nobody nearby
        let bad = who(1, [f32::NAN, 0.0, 0.0], None);
        let near = [who(2, [0.0, 0.0, 0.0], None)];
        assert!(voice_audience(&bad, &near, 32.0, &none, wire::VOICE_NEARBY).is_empty());
    }

    #[test]
    fn unknown_channel_reaches_nobody() {
        let p = parties(&[(100, 0)]);
        let s = who(1, [0.0, 0.0, 0.0], Some(100));
        let others = [who(2, [0.0, 0.0, 0.0], Some(100))];
        assert!(voice_audience(&s, &others, 32.0, &p, 7).is_empty());
    }
}
