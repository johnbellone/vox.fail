//! The world thread's state at work: joining and leaving, messages, edits and their checks,
//! the weather, plots and portals from the directory, saving to SQLite.

#[allow(unused_imports)]
use super::*;
use crate::entities::{self, Entities};
use crate::props;
use vox_core::edit::{self, Ctx, EYES, PlotArea, REACH, Role};
use vox_core::entity::{Axis, Component, Dest, EntityWhy, Portal, Prop, Settings, Spawn, prop_cell};
use vox_core::portal::Door;
use vox_core::travel::{DoorState, Frame, Motion};

/// How often doorways are checked for the players near them.
const DOOR_CHECK: Duration = Duration::from_secs(1);
/// How near a doorway (blocks) a player is told what it shows: 24 m.
const DOOR_NEAR: f32 = vox_core::in_blocks(24.0);
/// The longest a player is held at a waking doorway before the way counts as closed (the host's
/// slowest start, a big world loading, with room).
const HOLD_MOST: Duration = Duration::from_secs(90);
/// Farther than this (blocks) from the sill, a held player has walked away: 3 m.
const HOLD_NEAR: f32 = vox_core::in_blocks(3.0);
use vox_core::wire::EntityChange;

/// How far from a doorway's plane (blocks) a player may be and still go through it: their last
/// checked pose can be a step or two behind the client's.
const THROUGH_SLACK: f32 = vox_core::in_blocks(1.5);
/// How often a world starts a build (`ToServer::Build`): one per this long.
const BUILD_EVERY: Duration = Duration::from_secs(10);
/// Blocks a build applies each tick (ticks come every 100 ms), so other players' edits
/// interleave with a big build.
const BUILD_PER_TICK: usize = 4_096;
/// A chunk that one tick's batch changes by more than this many blocks is sent whole (`Chunk`)
/// rather than as a run of `Applied` entries.
const BULK_CHUNK: usize = 256;

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
                 account INTEGER, x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, old INTEGER NOT NULL, new INTEGER NOT NULL);",
        )
        .map_err(e)?;
        // a level may give its world's size (the Void is far bigger than the rest); read from the
        // level at every start, so a change to the level file reaches a world already made
        if cfg.tile.is_none()
            && let Some(b) = cfg.seed.as_deref().and_then(vox_core::level::bounds)
        {
            cfg.bounds = b;
        }
        // a log from before accounts were recorded gets the column (the table is kept as it is)
        crate::entities::ensure_account_column(&db, "edits")?;
        crate::entities::create_tables(&db)?;
        // a world saved at half-metre blocks is converted once, before anything reads it
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let converted = crate::rescale::migrate(&db, now)?;
        if let Some(what) = &converted {
            eprintln!("{}: {what}", cfg.name);
        }
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
        let mut props = props::PropLibrary::load(&cfg.props);
        props.index_all(ents.map.iter().map(|(id, (_, c))| (*id, c)));
        let climate = match meta("climate").and_then(|c| serde_json::from_str::<Climate>(&c).ok()) {
            Some(c) if !fresh => c.sane(),
            _ => Climate::from_settings(set.get("weather"), ground),
        };
        let weather = weather_from(climate, &set, name_seed(&cfg.name));
        let build_palette = vox_graph::entity::build_palette(&cfg.palette);
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
            props,
            editors: None,
            editor_cache: HashMap::new(),
            portals: None,
            portal_routes: HashMap::new(),
            held: Vec::new(),
            doors_at: Instant::now(),
            refunds_due: Vec::new(),
            last_keyed_commit: None,
            rules_now: None,
            applied: set,
            settings_unsaved: false,
            build_palette,
            builds: VecDeque::new(),
            last_build: None,
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
            // seeded now are not in any snapshot yet. A world just converted to quarter-metre
            // blocks has the same edit number as its last snapshot but none of its zones: the
            // next snapshot is whole, or untouched zones would keep half-metre blocks in it
            let predates = latest.as_ref().is_some_and(|m| !m.entities);
            s.snap_full = latest.as_ref().is_none_or(|m| m.seq != s.seq) || fresh || predates || reseeded || converted.is_some();
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
                    self.build_step();
                    if self.doors_at.elapsed() >= DOOR_CHECK {
                        self.doors_at = Instant::now();
                        self.check_doors();
                        self.send_held();
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
                Event::Rules(plots, portals, parties, started) => {
                    self.plots = plots;
                    // a list fetched before this world last changed a keyed portal predates that
                    // change, and would undo it: it is not applied, and the fetch the change woke
                    // brings the portals up to date
                    if !self.last_keyed_commit.is_some_and(|t| started < t) {
                        let changes = self.reconcile_portals(&portals);
                        self.broadcast_entities(changes);
                    }
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
                Event::Ticket { conn, url, ticket, arrive, motion, frame } => {
                    if let Some(p) = self.players.get(&conn) {
                        let _ = Self::send_to(&p.tx, &ToClient::Travel { url, ticket, arrive, motion, frame });
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
        for (seq, at, player, account, c, old, new) in self.log.iter() {
            let (seq, at) = (*seq, *at);
            t.execute(
                "INSERT OR REPLACE INTO edits(seq, at, player, account, x, y, z, old, new) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![seq as i64, at as i64, player, account.map(|a| a as i64), c[0], c[1], c[2], old, new],
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
            if let Some(old) = self.entities.remove(seq, at, "directory".into(), None, id) {
                self.props.index_change(id, Some(&old), None);
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
            let was = held.and_then(|id| self.entities.map.get(&id)).map(|(_, old)| old.clone());
            // None only when the id counter is spent: this portal is then not placed
            if let Some(id) = self.entities.put(seq, at, "directory".into(), None, held, c.clone()) {
                self.props.index_change(id, was.as_ref(), Some(&c));
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

    /// The world as a newcomer gets it: welcome, chunks, every entity (nearest `from` first),
    /// ready, weather, who's here. The entities come before `Ready` so a client has them all
    /// when it is told the world is complete. False if they couldn't keep up.
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
        for batch in entities::batches(self.entities.all_from(from)) {
            ok &= Self::send_to(tx, &ToClient::Entities(batch));
        }
        ok &= Self::send_to(tx, &ToClient::Ready);
        if let Some(w) = &self.weather {
            ok &= Self::send_to(tx, &ToClient::Weather(Box::new(w.keyframe())));
        }
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
        self.players.insert(conn, Player { id, name: name.clone(), avatar, tx, at: start, moves, corrected_at: -1.0, bucket, voice, account, placed: 0, edits, pending: None, kept: VecDeque::new(), doors: HashMap::new() });
    }

    /// On to `url`, arriving at `arrive`: with a ticket from the directory when there is one to
    /// be had (a signed-in player, a directory), else straight away without.
    /// Send `conn` on to `url`, arriving at `arrive` moving as `motion` (both in the arriving
    /// world's frame) through `frame`: with a ticket from the directory when there is one.
    #[allow(clippy::too_many_arguments)]
    fn send_on(&self, conn: u64, account: Option<u64>, tx: &mpsc::Sender<Message>, url: String, arrive: [f32; 3], motion: Motion, frame: Frame) {
        // (turned into the far frame it may have grown past a limit by rounding: held again)
        let motion = motion.sane(self.cfg.block_size);
        match (&self.trips, account) {
            (Some(trips), Some(account)) => {
                let trip = Trip { conn, account, to: url.clone(), arrive, motion, frame };
                if trips.try_send(trip).is_err() {
                    // the directory is far behind: go on without a ticket
                    let _ = Self::send_to(tx, &ToClient::Travel { url, ticket: None, arrive, motion, frame });
                }
            }
            // no directory here, or a guest: no ticket to be had
            _ => {
                let _ = Self::send_to(tx, &ToClient::Travel { url, ticket: None, arrive, motion, frame });
            }
        }
    }

    /// Where a doorway leads, as far as this world knows ([`doors::Dest`]). Only addresses on
    /// this host (`":7781"`, `"/w/<id>/ws"`) are asked of the host; one elsewhere stays unchecked.
    fn door_dest(&self, p: &Portal) -> doors::Dest {
        let url = match &p.to {
            Dest::Here { .. } => return doors::Dest::Here,
            Dest::Dungeon { .. } => return doors::Dest::Dungeon,
            Dest::Url { url, .. } => url.clone(),
            Dest::World { .. } | Dest::Pair { .. } => match p.key.and_then(|k| self.portal_routes.get(&k)) {
                None => return doors::Dest::Unrouted,
                Some(r) => match &r.url {
                    None => return doors::Dest::Nowhere,
                    Some(u) => u.clone(),
                },
            },
        };
        let here = url.starts_with(':') || url.starts_with('/');
        match doors::name_of(&url).filter(|_| here) {
            Some(name) => doors::Dest::World(name),
            None => doors::Dest::Unrouted,
        }
    }

    /// Who a player is, for what doorways show them.
    fn who(&self, account: Option<u64>) -> doors::Who {
        doors::Who { signed_in: account.is_some(), in_party: account.is_some_and(|a| self.party_of.contains_key(&a)) }
    }

    /// Every player near a doorway is told what it shows, when that changes (ADR 0004 decision
    /// 12). Run every [`DOOR_CHECK`].
    fn check_doors(&mut self) {
        let near: Vec<(String, [f32; 3], doors::Dest)> = self
            .entities
            .map
            .values()
            .filter_map(|(_, c)| match c {
                Component::Portal(p) => Some((p.name.clone(), p.at, self.door_dest(p))),
                _ => None,
            })
            .collect();
        if near.is_empty() {
            return;
        }
        let sign_in = self.cfg.directory.is_some();
        let ways = self.cfg.ways.clone();
        let whos: HashMap<u64, doors::Who> = self.players.iter().map(|(c, p)| (*c, self.who(p.account))).collect();
        for (conn, p) in self.players.iter_mut() {
            let Some(at) = p.at else { continue };
            let who = whos[conn];
            for (name, door_at, dest) in &near {
                let d2: f32 = (0..3).map(|i| (at[i] - door_at[i]).powi(2)).sum();
                if d2 > DOOR_NEAR * DOOR_NEAR {
                    continue;
                }
                let state = doors::state(dest, who, sign_in, ways.as_deref());
                if p.doors.get(name) != Some(&state) {
                    p.doors.insert(name.clone(), state);
                    let _ = Self::send_to(&p.tx, &ToClient::DoorState { portal: name.clone(), state });
                }
            }
        }
    }

    /// Players held at a waking doorway: sent on once the world there is up, or told the way
    /// closed if it went away or took too long.
    fn send_held(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let ways = self.cfg.ways.clone();
        for h in std::mem::take(&mut self.held) {
            let Some(p) = self.players.get(&h.conn) else { continue };
            // walked away from the doorway: no longer waiting
            let away = p.at.is_none_or(|at| (0..3).map(|i| (at[i] - h.sill[i]).powi(2)).sum::<f32>() > HOLD_NEAR * HOLD_NEAR);
            if away {
                continue;
            }
            let (account, tx) = (p.account, p.tx.clone());
            let way = ways.as_deref().map_or(doors::Way::Running, |w| w.way(&h.name));
            match way {
                doors::Way::Running => self.send_on(h.conn, account, &tx, h.url, h.arrive, h.motion, h.frame),
                doors::Way::Absent => {
                    let text = DoorState::Unlinked.line(&h.door).unwrap_or_default();
                    let _ = Self::send_to(&tx, &ToClient::Notice { text });
                }
                _ if h.since.elapsed() >= HOLD_MOST => {
                    let text = DoorState::Unlinked.line(&h.door).unwrap_or_default();
                    let _ = Self::send_to(&tx, &ToClient::Notice { text });
                }
                _ => self.held.push(h),
            }
        }
    }

    /// Off this tile's `edge` (`east`, `west`, `south`, `north`) onto the next tile of the
    /// atlas, if there is one that way and they're really at that edge: they come out at the
    /// same place on the map, just inside the next tile, standing on its ground.
    fn over_edge(&mut self, conn: u64, edge: &str, motion: Motion) {
        let Some(tile) = &self.cfg.tile else { return };
        let Some((_, d)) = vox_core::terrain::EDGES.iter().find(|(n, _)| *n == edge) else { return };
        let Some(next) = tile.next(*d) else { return };
        // an edge the Settings entity no longer lists is closed; a world whose settings say
        // nothing about edges stays open, as every world was before this rule
        let closed = serde_json::from_str::<serde_json::Value>(&self.settings_now())
            .ok()
            .and_then(|s| s["atlas"]["edges"].as_array().map(|e| !e.iter().any(|x| x.as_str() == Some(edge))))
            .unwrap_or(false);
        if closed {
            return;
        }
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
        // the same place on the map, a step (a metre) inside the next tile
        let t = vox_core::terrain::TILE as f32;
        let step = vox_core::in_blocks(1.0);
        let mut x = (at[0] - d[0] as f32 * t).clamp(lo[0] + step, hi[0] - step);
        let mut z = (at[2] - d[1] as f32 * t).clamp(lo[1] + step, hi[1] - step);
        if !x.is_finite() || !z.is_finite() {
            (x, z) = (0.5, 0.5);
        }
        let ground = next.height(x.floor() as i32, z.floor() as i32).max(vox_core::terrain::SEA) + 1;
        let arrive = [x, ground as f32, z];
        let url = format!("/w/{}/ws", next.id());
        // the tiles share the map's heights and lie a tile apart: moved, not turned
        let frame = Frame::moved([-(d[0] as f32) * t, 0.0, -(d[1] as f32) * t]);
        let (account, tx) = (p.account, p.tx.clone());
        self.send_on(conn, account, &tx, url, arrive, motion, frame);
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
        // a prop's owner comes from the server, like a portal's key: a new prop is stamped with the
        // account judging it, an existing one keeps the owner it has, so whatever owner the client
        // sent is overwritten (a page holds ids as JavaScript numbers, which lose a large id)
        let kept = match &edit {
            Edit::Put { entity: Some(id), .. } => match self.entities.map.get(id) {
                Some((_, Component::Prop(o))) => Some(o.owner),
                _ => None,
            },
            _ => None,
        };
        let edit = stamp_prop_owner(edit, account.or_else(|| self.cfg.editors_all.then(|| guest_account(conn))), kept);
        // a guest is judged as a builder, which the rules refuse; on a local `--editors-all`
        // world (no directory) a guest is an editor, judged as its local account (`guest_account`)
        let Some(account) = account else {
            let role = if self.cfg.editors_all { Role::Editor } else { Role::Builder };
            return self.apply_entity_edit(conn, id, edit, role);
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
        let (signed_in, at, tx) = (p.account, p.at, p.tx.clone());
        let judged = signed_in.or_else(|| self.cfg.editors_all.then(|| guest_account(conn)));
        if let Err(why) = self.judge(&edit, role, judged, at) {
            return self.refuse_entity(conn, &tx, id, why);
        }
        // only a signed-in player has a directory account to ask about portals
        if let Some(account) = signed_in {
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
        let (name, tx, account) = (p.name.clone(), p.tx.clone(), p.account);
        let by = name.clone();
        let seq = self.seq + 1;
        let when = now_unix();
        let keyed: bool;
        // the cells a prop stood in before this edit took it away or moved it: what stood on it
        // (on a table) may have lost its support
        let mut vacated: Vec<[i32; 3]> = Vec::new();
        let (change, zones, touched) = match edit {
            Edit::Put { entity, component, .. } => {
                let old = entity.and_then(|e| self.entities.map.get(&e)).map(|(_, c)| c.clone());
                keyed = matches!(component, Component::Portal(ref p) if p.key.is_some())
                    || matches!(old, Some(Component::Portal(ref p)) if p.key.is_some());
                let Some(eid) = self.entities.put(seq, when, name, account, entity, component.clone()) else {
                    // no id left to give a new entity
                    self.refuse_entity(conn, &tx, id, EntityWhy::TooMany);
                    return false;
                };
                self.props.index_change(eid, old.as_ref(), Some(&component));
                if let Some(Component::Prop(q)) = &old {
                    vacated = self.props.cells(q);
                }
                let mut zones = vec![entities::zone_key(&component)];
                zones.extend(old.as_ref().map(entities::zone_key));
                let touched = is_objective_or_settings(&component) || old.as_ref().is_some_and(is_objective_or_settings);
                (EntityChange { seq, entity: eid, component: Some(component) }, zones, touched)
            }
            Edit::Build { node } => return self.start_build(conn, id, node),
            Edit::Remove { entity, .. } => {
                let Some(old) = self.entities.remove(seq, when, name, account, entity) else {
                    self.refuse_entity(conn, &tx, id, EntityWhy::Changed);
                    return false;
                };
                self.props.index_change(entity, Some(&old), None);
                if let Component::Prop(q) = &old {
                    vacated = self.props.cells(q);
                }
                keyed = matches!(old, Component::Portal(ref p) if p.key.is_some());
                (EntityChange { seq, entity, component: None }, vec![entities::zone_key(&old)], is_objective_or_settings(&old))
            }
        };
        self.seq = seq;
        if keyed {
            // the directory's list is about to change: its fetch is woken, and any list fetched
            // from now on (that began after this) is applied. The time is taken first, so a
            // fetch that begins after the wake is never older than it.
            self.last_keyed_commit = Some(Instant::now());
            if let Some(now) = &self.rules_now {
                now.notify_one();
            }
        }
        // the zones it was in and is in are changed for the next snapshot
        self.snap_changed.extend(zones);
        let done = ToClient::EntityDone { id, entity: change.entity, seq };
        self.broadcast_entities(vec![change]);
        // to the sender alone, after the change itself, so the page's `done` follows the
        // `entities` event that carries it
        if !Self::send_to(&tx, &done) {
            self.leave(conn);
        }
        if touched {
            self.refresh_from_entities();
        }
        // the same rule as for a broken block: what was held up by the prop that went (or moved
        // away) goes with it, as the player who did it
        if !vacated.is_empty() {
            self.drop_unsupported(&by, account, vacated);
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

    /// The prop rules for an edit of `editing` (None for a new entity): whether the world accepts
    /// a prop's hash, whether its footprint is free, the cells it covers and whether it has a
    /// surface to stand on. The footprint check
    /// leaves out the entity being changed, so a prop may be turned or moved onto its own old cells.
    #[allow(clippy::type_complexity)]
    fn prop_rules(
        &self,
        editing: Option<EntityId>,
    ) -> (
        impl Fn(&[u8; 16]) -> bool + '_,
        impl Fn(&Prop) -> bool + '_,
        impl Fn(&Prop) -> Vec<[i32; 3]> + '_,
        impl Fn(&Prop) -> bool + '_,
    ) {
        let lib = &self.props;
        let known = move |hash: &[u8; 16]| lib.known(hash);
        let cells = move |p: &Prop| lib.cells(p);
        let free = move |p: &Prop| {
            // only the props near `p` (the index), not every prop in the world
            let others = lib
                .near(p)
                .into_iter()
                .filter(move |id| Some(*id) != editing)
                .filter_map(|id| match self.entities.map.get(&id) {
                    Some((_, Component::Prop(q))) => Some(q),
                    _ => None,
                });
            props::free(lib, p, others, |c| self.world.get(c) != 0)
        };
        let supported = move |p: &Prop| self.stands(p);
        (known, free, cells, supported)
    }

    /// Is this block water? The palette names it, as the terrain does (`vox_core::terrain`).
    fn is_water(&self, block: u16) -> bool {
        block != 0 && self.cfg.palette.get(usize::from(block) - 1).is_some_and(|n| n == "water")
    }

    /// The height below which the world is solid with no blocks (the Void's endless floor): the
    /// live Settings' `floor`, read as the client reads it, so both agree on what is ground.
    fn endless_floor(&self) -> Option<i32> {
        self.applied.get("floor").and_then(|v| v.as_i64()).map(|f| f as i32)
    }

    /// Does the prop stand on a surface it accepts, in the world as it is now? Blocks that hold a
    /// prop up are those that are neither air nor water, and the endless floor; a table is
    /// another prop.
    fn stands(&self, p: &Prop) -> bool {
        let floor = self.endless_floor();
        let solid = |c: [i32; 3]| {
            if floor.is_some_and(|f| c[1] < f) {
                return true;
            }
            let b = self.world.get(c);
            b != 0 && !self.is_water(b)
        };
        let water = |c: [i32; 3]| self.is_water(self.world.get(c));
        self.props.stands(p, &solid, &water, |id| match self.entities.map.get(&id) {
            Some((_, Component::Prop(q))) => Some(q),
            _ => None,
        })
    }

    /// Removes the props that have lost what they stood on, after blocks changed at `broken` (a
    /// cell that held a block: broken, replaced, or turned to water). A prop that no longer has any
    /// of its surfaces and leaned on one of those cells is removed as the player `by` made it
    /// go: the same entity log, edit number and broadcast as an editor's removal. The removed
    /// prop's own cells then count as broken, so what stood on it (on a table) goes too. Only
    /// props in the chunk columns around each cell are looked at. A prop already floating before
    /// (placed before this rule) is left alone, and so is an orphan (its hash is not in the
    /// catalogue, so it has no surfaces to lose). Nothing goes back to an inventory yet: there
    /// is none.
    pub(super) fn drop_unsupported(&mut self, by: &str, account: Option<u64>, broken: Vec<[i32; 3]>) {
        let mut changes = Vec::new();
        let mut broken = broken;
        while !broken.is_empty() {
            let cells: HashSet<[i32; 3]> = broken.drain(..).collect();
            // the props around any of the cells, once each
            let mut candidates: Vec<EntityId> = cells.iter().flat_map(|&c| self.props.around(c)).collect();
            candidates.sort_unstable();
            candidates.dedup();
            for id in candidates {
                let Some((_, Component::Prop(p))) = self.entities.map.get(&id) else { continue };
                if !self.props.leans_on(p, &cells) || self.stands(p) {
                    continue;
                }
                let p = p.clone();
                let seq = self.seq + 1;
                let Some(old) = self.entities.remove(seq, now_unix(), by.to_string(), account, id) else { continue };
                self.props.index_change(id, Some(&old), None);
                self.seq = seq;
                self.snap_changed.insert(entities::zone_key(&old));
                changes.push(EntityChange { seq, entity: id, component: None });
                broken.extend(self.props.cells(&p));
            }
        }
        if !changes.is_empty() {
            self.broadcast_entities(changes);
        }
    }

    /// Starts a build the rules have passed (`judge`): the node's recipe is expanded and checked
    /// (`expand_node`), and the build is queued for `build_step`. A refusal is answered to the
    /// builder alone. The build is made by the builder as ordinary edits are, so it is logged
    /// the same way. Returns whether it was started.
    fn start_build(&mut self, conn: u64, id: u32, node: EntityId) -> bool {
        let Some(p) = self.players.get(&conn) else { return false };
        let (player, name, account, tx) = (p.id, p.name.clone(), p.account, p.tx.clone());
        // the slot is taken before the recipe is expanded, so a refused build costs it too
        self.last_build = Some(Instant::now());
        let blocks = match self.expand_node(node, account) {
            Ok(blocks) => blocks,
            Err(why) => {
                self.refuse_entity(conn, &tx, id, why);
                return false;
            }
        };
        self.builds.push_back(BuildJob {
            conn,
            id,
            node,
            player,
            name,
            account,
            blocks: VecDeque::from(blocks),
            last_seq: self.seq,
            undo: None,
            changed: Vec::new(),
        });
        true
    }

    /// A player's `Unbuild`: queues the undo of one of their kept builds (`KeptBuild`), applied
    /// by `build_step` like a build. Refused as `TooFast` over their edit rate, as `Unavailable`
    /// while one of their builds or undos is still running, and as `Changed` when no such build
    /// is kept for them. The kept build is used up by its undo. A cell now in a plot the player
    /// has no part in is left as it is, as a build there would have been refused.
    fn start_unbuild(&mut self, conn: u64, id: u32, build: u32) {
        let now = self.started.elapsed().as_secs_f64();
        // an editor the owner has since removed (as far as the cache already knows; the directory
        // is not asked, so a plain revert never waits on it) may not undo their build either
        let revoked = self.players.get(&conn).and_then(|p| p.account).and_then(|a| self.role_of(a)) == Some(Role::Builder);
        let Some(p) = self.players.get_mut(&conn) else { return };
        let (account, tx) = (p.account, p.tx.clone());
        if !p.edits.take(now) {
            return self.refuse_entity(conn, &tx, id, EntityWhy::TooFast);
        }
        if revoked {
            return self.refuse_entity(conn, &tx, id, EntityWhy::NotYours);
        }
        if self.builds.iter().any(|j| j.conn == conn) {
            return self.refuse_entity(conn, &tx, id, EntityWhy::Unavailable);
        }
        let Some(i) = p.kept.iter().position(|k| k.build == build) else {
            return self.refuse_entity(conn, &tx, id, EntityWhy::Changed);
        };
        let Some(kept) = p.kept.remove(i) else { return };
        let (player, name) = (p.id, p.name.clone());
        let mut blocks = VecDeque::with_capacity(kept.changed.len());
        let mut expect = VecDeque::with_capacity(kept.changed.len());
        let mut left = 0;
        for (cell, old, new) in kept.changed {
            if self.plots.iter().any(|pl| pl.holds(cell) && !account.is_some_and(|a| pl.builders.contains(&a))) {
                left += 1;
                continue;
            }
            blocks.push_back((cell, old));
            expect.push_back(new);
        }
        self.builds.push_back(BuildJob {
            conn,
            id,
            node: EntityId(0),
            player,
            name,
            account,
            blocks,
            last_seq: self.seq,
            undo: Some(Undo { expect, restored: 0, left }),
            changed: Vec::new(),
        });
    }

    /// The blocks a node's recipe builds, as (cell, block id), or why they can't be built: the
    /// node must be there, the world must have a palette vox-art takes, the recipe must expand
    /// (`Invalid`, or `TooBig` over the cap), and every cell must be inside the world and outside
    /// other people's plots.
    fn expand_node(&self, node: EntityId, account: Option<u64>) -> Result<Vec<([i32; 3], u16)>, EntityWhy> {
        let Some((_, Component::Node(n))) = self.entities.map.get(&node) else { return Err(EntityWhy::Changed) };
        let Some((palette, ids)) = &self.build_palette else { return Err(EntityWhy::Unavailable) };
        let graph = vox_graph::entity::from_entity(node.0, n).map_err(|_| EntityWhy::Invalid)?;
        let cells = vox_graph::expand(&graph, palette).map_err(|error| match error {
            vox_graph::ExpandError::TooBig { .. } => EntityWhy::TooBig,
            vox_graph::ExpandError::Invalid(_) => EntityWhy::Invalid,
        })?;
        let mut blocks = Vec::with_capacity(cells.len());
        for (at, name) in cells {
            let block = *ids.get(&name).ok_or(EntityWhy::Invalid)?;
            if !self.cfg.bounds.contains(at) {
                return Err(EntityWhy::Outside);
            }
            if self.plots.iter().any(|pl| pl.holds(at) && !account.is_some_and(|a| pl.builders.contains(&a))) {
                return Err(EntityWhy::NotYours);
            }
            blocks.push((at, block));
        }
        Ok(blocks)
    }

    /// Applies the next batch of the first queued build, once a tick: up to `BUILD_PER_TICK`
    /// blocks, each an ordinary edit made by the builder (a new edit number, a log row, a change
    /// to the chunk for the next save). Other players' edits made meanwhile are in the same order.
    /// A chunk this batch changes by more than `BULK_CHUNK` blocks is sent whole; the rest go out
    /// as `Applied`. When the last block is in, the builder is answered with `EntityDone`, if
    /// they are still here. A build keeps going when its builder leaves; when the world closes
    /// mid-build, the blocks already applied are saved like any edit and the rest are lost, so
    /// the build is left part done.
    pub(super) fn build_step(&mut self) {
        let Some(mut job) = self.builds.pop_front() else { return };
        let mut made: Vec<(u64, [i32; 3], u16)> = Vec::new();
        let mut per_chunk: HashMap<ChunkKey, usize> = HashMap::new();
        // cells that held a block and now hold another: props may have lost what they stood on
        let mut changed_cells: Vec<[i32; 3]> = Vec::new();
        while made.len() < BUILD_PER_TICK {
            let Some((cell, block)) = job.blocks.pop_front() else { break };
            // an undo puts the old block back only where the cell still holds what the build put
            if let Some(u) = job.undo.as_mut()
                && u.expect.pop_front() != Some(self.world.get(cell))
            {
                u.left += 1;
                continue;
            }
            // nor does a build or its undo put a solid block where a prop has come to stand, as an
            // ordinary edit may not (Why::Blocked)
            if block != 0
                && self.props.covers_cell(cell, |id| match self.entities.map.get(&id) {
                    Some((_, Component::Prop(q))) => Some(q),
                    _ => None,
                })
            {
                if let Some(u) = job.undo.as_mut() {
                    u.left += 1;
                }
                continue;
            }
            // a block already there is no change, as an ordinary edit refuses it
            let Ok(old) = self.world.edit(cell, block) else { continue };
            match job.undo.as_mut() {
                Some(u) => u.restored += 1,
                None => job.changed.push((cell, old, block)),
            }
            if old != 0 {
                changed_cells.push(cell);
            }
            self.seq += 1;
            self.log.push((self.seq, now_unix(), job.name.clone(), job.account, cell, old, block));
            let key = ChunkKey::of(cell);
            self.dirty.insert(key);
            self.snap_changed.insert(vox_format::zone_of(key));
            *per_chunk.entry(key).or_default() += 1;
            made.push((self.seq, cell, block));
        }
        if let Some(&(seq, _, _)) = made.last() {
            job.last_seq = seq;
        }
        let bulk: HashSet<ChunkKey> = per_chunk.iter().filter(|(_, n)| **n > BULK_CHUNK).map(|(k, _)| *k).collect();
        let applied: Vec<Applied> = made
            .iter()
            .filter(|(_, cell, _)| !bulk.contains(&ChunkKey::of(*cell)))
            // Id 0 is never a client's edit number (the client counts from 1), so a builder's
            // predicted edits, which the client drops by matching id, are not dropped by a build.
            .map(|&(seq, cell, block)| Applied { seq, player: job.player, id: 0, cell, block })
            .collect();
        if !applied.is_empty() {
            self.broadcast(&ToClient::Edits(applied), None);
        }
        let mut whole: Vec<ChunkKey> = bulk.into_iter().collect();
        whole.sort();
        for key in whole {
            let m = match self.world.chunk(key) {
                Some(c) => ToClient::Chunk { key, version: c.version, data: c.encode() },
                None => continue,
            };
            self.broadcast(&m, None);
        }
        // after the blocks, so everyone sees the support go and then the prop
        if !changed_cells.is_empty() {
            self.drop_unsupported(&job.name, job.account, changed_cells);
        }
        if job.blocks.is_empty() {
            // a build that changed nothing has nothing to undo, and would take a kept slot
            if job.undo.is_none()
                && !job.changed.is_empty()
                && let Some(p) = self.players.get_mut(&job.conn)
            {
                // kept for the builder's undo, as long as they are here
                p.kept.retain(|k| k.build != job.id);
                p.kept.push_back(KeptBuild { build: job.id, changed: std::mem::take(&mut job.changed) });
                while p.kept.len() > KEPT_BUILDS {
                    p.kept.pop_front();
                }
            }
            if let Some(tx) = self.players.get(&job.conn).map(|p| p.tx.clone()) {
                let done = ToClient::EntityDone { id: job.id, entity: job.node, seq: job.last_seq };
                let mut sent = Self::send_to(&tx, &done);
                if let Some(u) = &job.undo {
                    let text = format!("Undid the build: {} blocks back; {} changed since were left", u.restored, u.left);
                    sent = sent && Self::send_to(&tx, &ToClient::Notice { text });
                }
                if !sent {
                    self.leave(job.conn);
                }
            }
        } else {
            self.builds.push_front(job);
        }
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
                let (known_prop, footprint_free, prop_cells, prop_supported) = self.prop_rules(*entity);
                let props_in_plot = match component {
                    Component::Prop(p) => props_in_plot(&plots, &self.props, &self.entities, prop_cell(p.at)),
                    _ => 0,
                };
                let ctx = Ctx {
                    role,
                    account,
                    at,
                    bounds: self.cfg.bounds,
                    plots: &plots,
                    props_in_plot,
                    prop_footprint_free: &footprint_free,
                    prop_cells: &prop_cells,
                    known_prop: &known_prop,
                    prop_supported: &prop_supported,
                    zone_entities: self.entities.zone_count(zone),
                    moves_zone: old.is_some_and(|o| entities::zone_key(o) != zone),
                    plot_portals: plot_portals_at(&plots, component, &self.entities),
                    spawns,
                    ground: &ground,
                };
                edit::check_put(&ctx, old, component)?;
                // a tile's Settings may close its edges (see over_edge), but may name only the
                // ways this tile has; bad JSON is already refused by `validate` above
                if let Component::Settings(s) = component
                    && let Some(tile) = &self.cfg.tile
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s.json)
                    && let Some(field) = v.get("atlas").and_then(|a| a.get("edges"))
                {
                    // anything but a list would read as "says nothing" in over_edge, so every
                    // edge would quietly open: refuse it instead
                    let Some(edges) = field.as_array() else { return Err(EntityWhy::Invalid) };
                    let allowed: Vec<&str> = tile.edges();
                    let mut seen: Vec<&str> = Vec::new();
                    for e in edges {
                        match e.as_str() {
                            Some(n) if allowed.contains(&n) && !seen.contains(&n) => seen.push(n),
                            _ => return Err(EntityWhy::Invalid),
                        }
                    }
                }
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
            Edit::Build { node } => {
                // advanced rights only, as every advanced edit; the node must be in reach (as a
                // Put of it would be), and a world starts one build per BUILD_EVERY
                if role != Role::Editor {
                    return Err(EntityWhy::NotYours);
                }
                match self.entities.map.get(node) {
                    None => return Err(EntityWhy::Changed),
                    Some((_, Component::Node(n))) if within_reach(at, n.at) => {}
                    Some((_, Component::Node(_))) => return Err(EntityWhy::TooFar),
                    Some(_) => return Err(EntityWhy::Invalid),
                }
                if self.last_build.is_some_and(|t| t.elapsed() < BUILD_EVERY) {
                    return Err(EntityWhy::TooFast);
                }
                Ok(())
            }
            Edit::Remove { entity, expect } => {
                let old = self.held(*entity, *expect)?;
                let (known_prop, footprint_free, prop_cells, prop_supported) = self.prop_rules(Some(*entity));
                let ctx = Ctx {
                    role,
                    account,
                    at,
                    bounds: self.cfg.bounds,
                    plots: &plots,
                    props_in_plot: 0,
                    prop_footprint_free: &footprint_free,
                    prop_cells: &prop_cells,
                    known_prop: &known_prop,
                    prop_supported: &prop_supported,
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
        // a block that held something and was changed: who changed it, and where (props that
        // stood on it are dealt with once the player is no longer borrowed)
        let mut broke: Option<(String, Option<u64>, [i32; 3])> = None;
        match msg {
            ToServer::Hello { .. } | ToServer::Watch { .. } => {}
            ToServer::Put { id, entity, expect, component } => {
                self.entity_edit(conn, id, Edit::Put { entity, expect, component });
            }
            ToServer::Remove { id, entity, expect } => {
                self.entity_edit(conn, id, Edit::Remove { entity, expect });
            }
            ToServer::Build { id, node } => self.entity_edit(conn, id, Edit::Build { node }),
            ToServer::Unbuild { id, build } => self.start_unbuild(conn, id, build),
            ToServer::Through { portal, side, motion } => {
                if !p.bucket.take(now) {
                    return;
                }
                // how they say they move, held to what a player can do here
                let motion = motion.sane(self.cfg.block_size);
                if let Some(edge) = portal.strip_prefix("edge:") {
                    return self.over_edge(conn, edge, motion);
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
                let frame = door.frame(side);
                let url = door.url.clone().unwrap_or_default();
                // what the doorway shows decides whether it lets them through now (decision 4)
                let dest = self.entities.map.values().find_map(|(_, c)| match c {
                    Component::Portal(p) if p.name == portal => Some(self.door_dest(p)),
                    _ => None,
                });
                let who = self.who(account);
                let state = dest.as_ref().map_or(DoorState::Unknown, |d| doors::state(d, who, self.cfg.directory.is_some(), self.cfg.ways.as_deref()));
                match (state, dest) {
                    (DoorState::Waking, Some(doors::Dest::World(name))) => {
                        let text = state.line(&door.name).unwrap_or_default();
                        let _ = Self::send_to(&tx, &ToClient::Notice { text });
                        self.held.retain(|h| h.conn != conn);
                        self.held.push(Held { conn, name, door: door.name.clone(), sill: door.at, url, arrive, motion: motion.through(&frame), frame, since: Instant::now() });
                    }
                    (s, _) if !s.lets_through() => {
                        let text = s.line(&door.name).unwrap_or_default();
                        let _ = Self::send_to(&tx, &ToClient::Notice { text });
                    }
                    _ => self.send_on(conn, account, &tx, url, arrive, motion.through(&frame), frame),
                }
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
                        // a solid block may not go where a colliding prop stands (breaking one is
                        // not refused here: a prop that loses its support is removed after it,
                        // see `drop_unsupported`)
                        let in_prop = block != 0
                            && self.props.covers_cell(cell, |id| match self.entities.map.get(&id) {
                                Some((_, Component::Prop(q))) => Some(q),
                                _ => None,
                            });
                        if !within_reach(p.at, cell) {
                            Err(Why::TooFar)
                        } else if claimed {
                            Err(Why::Claimed)
                        } else if in_prop {
                            Err(Why::Blocked)
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
                        self.log.push((self.seq, now_unix(), p.name.clone(), p.account, cell, old, block));
                        self.dirty.insert(ChunkKey::of(cell));
                        self.snap_changed.insert(vox_format::zone_of(ChunkKey::of(cell)));
                        if old != 0 {
                            broke = Some((p.name.clone(), p.account, cell));
                        }
                        self.broadcast(&ToClient::Edits(vec![a]), None);
                    }
                }
            }
        }
        if let Some((by, account, cell)) = broke {
            self.drop_unsupported(&by, account, vec![cell]);
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

/// The account a guest's edits are judged as on a local `--editors-all` world (no directory).
/// A directory's accounts are small numbers counted up from one, so `u64::MAX - conn` never
/// matches a real one; the world only uses it to judge the edit and never sends it anywhere.
fn guest_account(conn: u64) -> u64 {
    u64::MAX - conn
}

/// A new prop is owned by the account placing it (`judged`), whatever owner the client named.
/// An edit of an existing prop keeps the owner that prop has (`kept`), so nobody hands a prop
/// over or takes one; `vox_core::edit` still checks the owner is unchanged.
fn stamp_prop_owner(mut edit: Edit, judged: Option<u64>, kept: Option<Option<u64>>) -> Edit {
    match (&mut edit, judged, kept) {
        (Edit::Put { entity: None, component: Component::Prop(p), .. }, Some(account), _) => p.owner = Some(account),
        (Edit::Put { entity: Some(_), component: Component::Prop(p), .. }, _, Some(owner)) => p.owner = owner,
        _ => {}
    }
    edit
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

/// The props standing in the plot that `cell` is in (0 when `cell` is in no plot). Counted from
/// the props' columns (`PropLibrary::count_in`), so a Put costs its plot, not every entity.
fn props_in_plot(plots: &[PlotArea], props: &props::PropLibrary, ents: &Entities, cell: [i32; 3]) -> usize {
    let Some(plot) = plots.iter().find(|p| p.holds(cell[0], cell[2])) else { return 0 };
    props.count_in(plot.x0..plot.x1, plot.z0..plot.z1, |id| match ents.map.get(&id) {
        Some((_, Component::Prop(q))) => Some(q),
        _ => None,
    })
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
        Component::Objective(o) => Some(Objective { name: o.name.clone(), at: o.at, radius: o.radius.min(vox_core::in_blocks(32.0)) }),
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
