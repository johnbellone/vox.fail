//! What the server says: the welcome, chunks, edits, other players, poses, the weather.

use super::*;

impl WorldClient {
    /// Sends an editor's entity message (a Put, a Remove) through the same link as the rest.
    pub(crate) fn send_entity(&self, m: &ToServer) {
        self.send(m);
    }

    /// The link is up and the world has finished sending itself: an edit sent now can arrive.
    pub(crate) fn linked(&self) -> bool {
        self.ready && self.link.is_some()
    }
}

/// Connect, and take in what the server sends.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut w: ResMut<WorldClient>,
    av: Res<crate::LocalAvatar>,
    blocks: Res<crate::blocks::Blocks>,
    assets: Res<AssetServer>,
    mut entities: ResMut<crate::entities::WorldEntities>,
    mut editor: Option<ResMut<crate::editor::EditorOutbox>>,
    mut editing: Option<ResMut<crate::editor::EditorState>>,
    mut commands: Commands,
) {
    if !w.active() {
        return;
    }
    let Some(local) = blocks.palette.as_ref() else { return };
    if w.link.is_none() && w.status.is_empty() {
        w.connect();
    }
    #[cfg(target_arch = "wasm32")]
    web_link::pump();
    let msgs: Vec<Incoming> = match &w.link {
        Some(l) => l.drain(),
        None => return,
    };
    let now = w.now();
    for m in msgs {
        match m {
            Incoming::Status(s) => {
                if s == "Connected" && w.watch {
                    w.send(&ToServer::Watch { protocol: PROTOCOL });
                } else if s == "Connected" {
                    // say hello with what you look like
                    let hello = proto::Hello {
                        version: proto::VERSION,
                        name: w.name.clone(),
                        model: av.model.clone(),
                        nodes: w.rig.as_ref().map_or(0, |r| r.1.hash),
                    };
                    let avatar = postcard::to_stdvec(&hello).unwrap_or_default();
                    let name = w.name.clone();
                    // a portal can lead to any host: the sign-in token only goes where you signed
                    // in; a ticket goes to the world it's for (it's good nowhere else), once
                    let here = w.url.as_deref().map(url_host);
                    let token = match w.ticket.take() {
                        Some(t) => Some(t),
                        None if here == w.token_host => w.token.clone(),
                        None => None,
                    };
                    w.send(&ToServer::Hello { protocol: PROTOCOL, name, avatar, token });
                } else {
                    w.ready = false;
                }
                w.status = s;
            }
            Incoming::Data(b) => {
                let Ok(msg) = wire::decode::<ToClient>(&b) else { continue };
                match msg {
                    ToClient::Welcome { you, world, palette, block_size, bounds, spawn, settings, name } => {
                        w.name = name;
                        // the editor (edit mode only) hears the world it is in, and its palette
                        // and what it told the page of the last world no longer holds
                        if let Some(st) = editing.as_mut() {
                            st.reset();
                        }
                        if let Some(out) = editor.as_mut() {
                            out.0.push(crate::editor::bridge::Event::Ready { world: world.clone(), palette: palette.clone() });
                        }
                        for (_, e) in w.meshes.drain() {
                            commands.entity(e).try_despawn();
                        }
                        let others: Vec<_> = w.others.drain().filter_map(|(_, o)| o.entity).collect();
                        for e in others {
                            commands.entity(e).try_despawn();
                        }
                        w.welcome(you, world, palette, block_size, bounds, spawn, local);
                        w.settings = Some(serde_json::from_str(&settings).unwrap_or(serde_json::json!({})));
                        let set = w.settings.clone().unwrap_or_default();
                        // the endless floor, fixed cameras, the camera's starting angle and the block bar
                        w.apply_settings(&set, true);
                        w.settings_seen = Some(settings_key(&set));
                        w.focus = None;
                        // the last world's entities are gone; this world's come in Entities
                        entities.clear();
                        // Welcome carries the settings itself, so the flag clear() just set
                        // does not need a second apply
                        entities.settings_changed = false;
                        // its portals (from the settings, until its entities say otherwise);
                        // and if you came through one, where you come out
                        w.portals = crate::portals::parse(&set);
                        w.edges = set["atlas"]["edges"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|e| e.as_str()).take(4).map(String::from).collect())
                            .unwrap_or_default();
                        w.portals_gen = w.portals_gen.wrapping_add(1);
                        w.travel = None;
                        if let Some(a) = w.arrival.take() {
                            w.arrive(a);
                        }
                        w.status = format!("Loading {}", w.world);
                    }
                    ToClient::Chunk { key, version, data } => match Chunk::decode(&data, version) {
                        Ok(c) => {
                            // note its lamps and fires
                            let o = key.origin();
                            w.lights.retain(|l, _| ChunkKey::of(*l) != key);
                            if !c.is_empty() {
                                let n = vox_core::CHUNK;
                                for y in 0..n {
                                    for z in 0..n {
                                        for x in 0..n {
                                            let cell = [o[0] + x, o[1] + y, o[2] + z];
                                            if let Some(k) = w.light_kind(c.get(cell)) {
                                                w.lights.insert(cell, k);
                                            }
                                        }
                                    }
                                }
                            }
                            w.view.chunk(key, c);
                            w.dirty.insert(key);
                            for d in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
                                w.dirty.insert(ChunkKey([key.0[0] + d[0], key.0[1] + d[1], key.0[2] + d[2]]));
                            }
                        }
                        Err(e) => crate::note!("[world] bad chunk {key:?}: {e}"),
                    },
                    ToClient::Ready => {
                        // every entity has arrived (they come before Ready): the editor's page gets them all as
                        // its first Entities event, and hears of nothing before it
                        if let Some(st) = editing.as_mut() {
                            st.ready();
                        }
                        // a new world has nothing selected yet: tell the page so its selection is cleared
                        if let Some(out) = editor.as_mut() {
                            out.0.push(crate::editor::bridge::Event::Selected { entity: None });
                        }
                        w.ready = true;
                        w.status = format!("In {}", w.world);
                        crate::note!("[world] in {}: {} blocks", w.world, w.view.confirmed.count());
                    }
                    ToClient::Edits(list) => {
                        let me = w.you.unwrap_or(u32::MAX);
                        for a in list {
                            match w.light_kind(a.block) {
                                Some(k) => {
                                    w.lights.insert(a.cell, k);
                                }
                                None => {
                                    w.lights.remove(&a.cell);
                                }
                            }
                            if let Some(c) = w.view.applied(&a, me) {
                                w.mark(c);
                            }
                        }
                    }
                    ToClient::Refused { id, why } => {
                        if let Some(c) = w.view.refused(id) {
                            w.mark(c);
                        }
                        w.refused += 1;
                        w.notice(why.to_string());
                    }
                    ToClient::Joined { player, name, avatar } => {
                        let model = postcard::from_bytes::<proto::Hello>(&avatar)
                            .ok()
                            .map(|h| h.model)
                            .filter(|m| m.ends_with(".glb") && !m.contains("..") && !m.starts_with('/'))
                            .unwrap_or_else(|| av.fallback.clone());
                        let e = commands
                            .spawn((
                                Remote::world_player(model.clone(), assets.load::<Gltf>(model)),
                                Transform::from_translation(Vec3::new(0.0, -1000.0, 0.0)),
                                Visibility::default(),
                                Name::new(format!("player {name}")),
                            ))
                            .id();
                        w.notice(format!("{name} is here"));
                        let old = w.others.insert(
                            player,
                            Other { name, entity: Some(e), stream: Stream::default(), places: VecDeque::new() },
                        );
                        if let Some(e) = old.and_then(|o| o.entity) {
                            commands.entity(e).try_despawn();
                        }
                    }
                    ToClient::Left { player } => {
                        if let Some(o) = w.others.remove(&player) {
                            if let Some(e) = o.entity {
                                commands.entity(e).try_despawn();
                            }
                            w.notice(format!("{} left", o.name));
                        }
                    }
                    ToClient::Pose { player, blob } => {
                        let Ok(p) = postcard::from_bytes::<Placed>(&blob) else { continue };
                        if !p.at.iter().all(|v| v.is_finite()) || !p.yaw.is_finite() {
                            continue;
                        }
                        if let Some(o) = w.others.get_mut(&player) {
                            let tick = p.pose.tick;
                            o.stream.push(p.pose, now);
                            let i = o.places.partition_point(|q| q.0 < tick);
                            o.places.insert(i, (tick, Vec3::from(p.at), p.yaw));
                            while o.places.len() > 120 {
                                o.places.pop_front();
                            }
                        }
                    }
                    ToClient::Weather(k) => match Weather::from_keyframe(&k) {
                        Ok(wx) => {
                            w.set_weather(Some(wx));
                        }
                        Err(e) => crate::note!("[world] bad weather: {e}"),
                    },
                    ToClient::Bye { why } => {
                        // refused on the far side of a portal: go back at once, not after a wait
                        w.status = if w.travel.is_some() { format!("Can't reach the world: {why}") } else { why };
                        w.ready = false;
                    }
                    ToClient::Travel { url, ticket, arrive, motion, frame } => w.travelled(&url, ticket, arrive, motion, frame),
                    // the far side drawn ahead of time and doorway looks come with vox-81l.3 and
                    // vox-81l.8; until then they are heard and left
                    ToClient::Prepare { .. } | ToClient::DoorState { .. } => {}
                    // world entities (protocol 9); the portals are drawn from them in portals.rs
                    ToClient::Entities(changes) => {
                        entities.apply(&changes);
                        // a Settings entity that changed while you are here applies live
                        if entities.settings_changed {
                            entities.settings_changed = false;
                            if let Some(s) = entities.settings_json() {
                                w.apply_live_settings(s);
                            }
                        }
                    }
                    // a refused Put, Remove or Build: a prop from your hand says why; otherwise
                    // the editor (edit mode only) hears it
                    ToClient::EntityRefused { id, why } => {
                        if !w.hand_refused(id, &why)
                            && let (Some(out), Some(st)) = (editor.as_mut(), editing.as_mut())
                        {
                            out.0.push(st.refused(id, why));
                        }
                    }
                    // an applied Put or Remove: the editor (edit mode only) tells the page
                    ToClient::EntityDone { id, entity, seq } => {
                        if let (Some(out), Some(st)) = (editor.as_mut(), editing.as_mut())
                            && let Some(ev) = st.done(id, entity, seq)
                        {
                            out.0.push(ev);
                        }
                    }
                    ToClient::Notice { text } => {
                        // from the server, so plain text only, and short
                        let text: String = text.chars().filter(|c| !c.is_control()).take(120).collect();
                        if !text.trim().is_empty() {
                            w.notice(text);
                        }
                    }
                    ToClient::Correct { at } => {
                        // the server says we can't have got here: back to where it last saw us
                        if at.iter().all(|v| v.is_finite()) {
                            w.body.pos = at;
                            w.vy = 0.0;
                            w.goal = None;
                        }
                    }
                    ToClient::Voice { player, packet } => w.push_voice(player, packet),
                }
            }
        }
    }
}
