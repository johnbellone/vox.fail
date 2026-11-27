//! What the server says: the welcome, chunks, edits, other players, poses, the weather.

use super::*;

/// Connect, and take in what the server sends.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut w: ResMut<WorldClient>,
    av: Res<crate::LocalAvatar>,
    blocks: Res<crate::blocks::Blocks>,
    assets: Res<AssetServer>,
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
                        for (_, e) in w.meshes.drain() {
                            commands.entity(e).try_despawn();
                        }
                        let others: Vec<_> = w.others.drain().filter_map(|(_, o)| o.entity).collect();
                        for e in others {
                            commands.entity(e).try_despawn();
                        }
                        w.welcome(you, world, palette, block_size, bounds, spawn, local);
                        w.settings = Some(serde_json::from_str(&settings).unwrap_or(serde_json::json!({})));
                        // the endless floor, fixed cameras and the camera's starting angle
                        let set = w.settings.clone().unwrap_or_default();
                        w.floor = set.get("floor").and_then(|v| v.as_i64()).map(|f| f as i32);
                        w.voice_radius = crate::voice::plugin::radius_from(set.get("voice_radius"));
                        w.shots = set
                            .get("cameras")
                            .and_then(|v| v.as_array())
                            .map(|l| l.iter().filter_map(Shot::from_json).collect())
                            .unwrap_or_default();
                        // the last world's shots are gone
                        w.shot = None;
                        w.last_shot = None;
                        w.forced_shot = None;
                        w.shot_blend = 0.0;
                        if let Some(c) = set.get("camera") {
                            if let Some(y) = c.get("yaw").and_then(|v| v.as_f64()) {
                                w.cam_yaw = (y as f32).to_radians();
                            }
                            if let Some(p) = c.get("pitch").and_then(|v| v.as_f64()).filter(|p| p.is_finite()) {
                                w.cam_pitch = (p as f32).to_radians().clamp(-1.45, 1.3);
                            }
                            if let Some(z) = c.get("zoom").and_then(|v| v.as_f64()) {
                                w.zoom = (z as f32).clamp(4.0, 45.0);
                                w.zoom_now = w.zoom;
                            }
                        }
                        w.focus = None;
                        // its portals; and if you came through one, where you come out
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
                        // a world may choose its own block bar
                        let names: Vec<String> = w
                            .settings
                            .as_ref()
                            .and_then(|s| s.get("hotbar"))
                            .and_then(|v| v.as_array())
                            .map(|l| l.iter().filter_map(|n| n.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let ids: Vec<u16> = names.iter().filter_map(|n| w.block_id(n)).take(9).collect();
                        if !ids.is_empty() {
                            w.hotbar = ids;
                            w.slot = 0;
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
                    ToClient::Travel { url, ticket, arrive } => w.travelled(&url, ticket, arrive),
                    // world entities (protocol 9): drawn from in a later step of the editor work;
                    // until then the portals still come from the settings in Welcome
                    ToClient::Entities(_) | ToClient::EntityRefused { .. } => {}
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
