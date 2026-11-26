//! Throws on stage: launching tomatoes, eggs and confetti, their flight, splats, cheers for bits, fading out.

#[allow(unused_imports)]
use super::*;

pub fn setup(
    mut chat: ResMut<Chat>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    chat.cube = Some(meshes.add(Cuboid::new(0.035, 0.035, 0.006)));
    for c in [
        (230, 57, 70),
        (255, 190, 11),
        (58, 134, 255),
        (131, 56, 236),
        (6, 214, 160),
        (255, 255, 255),
    ] {
        chat.colors.push(mats.add(StandardMaterial {
            base_color: Color::srgb_u8(c.0, c.1, c.2),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        }));
    }
}

/// Chat messages -> queued throws -> things in the air.
pub fn launch(
    mut chat: ResMut<Chat>,
    mut av: ResMut<crate::Avatar>,
    time: Res<Time>,
    flying: Query<(), With<Flying>>,
    head: Query<(&crate::Joint, &GlobalTransform)>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let mut events: Vec<ChatEvent> = chat
        .rx
        .as_ref()
        .map(|rx| rx.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect())
        .unwrap_or_default();
    for f in &chat.feeds {
        // (an empty ban is a feed's "are you still there" ping)
        events.extend(
            f.1.rx
                .lock()
                .unwrap()
                .try_iter()
                .filter(|e| !matches!(e, ChatEvent::Ban(u, _) if u.is_empty())),
        );
    }
    chat.inbox.clear();
    chat.events.clear();
    chat.seen.clear();
    chat.said.clear();
    chat.banned_now.clear();
    chat.muted.retain(|_, until| *until > now);
    for ev in events {
        match ev {
            ChatEvent::Msg(m) if chat.muted.contains_key(&viewer_key(m.platform, &m.user)) => {}
            ChatEvent::Msg(m) => {
                chat.seen.push(m.clone());
                chat.said.push(m.text.clone());
                if m.bits > 0 && chat.enabled {
                    cheer(&mut chat, &m.user, m.bits, now);
                }
                if let Some(id) = vote_of(&m.text) {
                    chat.votes.push((m.user, id));
                } else if let Some(t) = command_of(&m.text).filter(|_| chat.enabled) {
                    chat.queue.push((t, m.user));
                } else if m.text.starts_with('!') {
                    chat.inbox.push(m);
                }
            }
            ChatEvent::Ban(u, secs) => {
                let key = ban_key(&u);
                chat.muted.insert(
                    key.clone(),
                    secs.map(|s| now + s as f32).unwrap_or(f32::INFINITY),
                );
                chat.banned_now.push(key.clone());
                // viewer questions are signed in with Twitch: only Twitch bans reach them
                if key.starts_with("tw:") {
                    chat.bans.push((u, secs));
                }
            }
            other => chat.events.push(other),
        }
    }
    // cheers go out on their own schedule
    let due: Vec<(f32, Throw, String, f32)> = chat
        .barrage
        .iter()
        .filter(|b| b.0 <= now)
        .cloned()
        .collect();
    chat.barrage.retain(|b| b.0 > now);
    let in_air = flying.iter().count();
    let queue = std::mem::take(&mut chat.queue);
    let mut launched = 0;
    let items: Vec<(Throw, String, f32, bool)> = queue
        .into_iter()
        .map(|(k, u)| (k, u, 1.0, false))
        .chain(due.into_iter().map(|(_, k, u, s)| (k, u, s, true)))
        .collect();
    for (kind, user, size, cheered) in items {
        // the streamer's own buttons and cheers skip the per-viewer cooldown
        let quiet = user == QUIET;
        let mine = user.is_empty() || quiet;
        if !mine && !cheered && !chat.gate.allow(&user, now, in_air + launched) {
            continue;
        }
        launched += 1;
        let who = if mine { String::new() } else { user };
        if !cheered && !quiet {
            chat.credit = Some((
                if who.is_empty() {
                    format!("{}!", kind.name())
                } else {
                    format!(
                        "{who} threw {}",
                        if kind == Throw::Confetti {
                            "confetti".into()
                        } else {
                            format!("a {}", kind.name())
                        }
                    )
                },
                now,
            ));
        }
        let head_pos = head
            .iter()
            .find(|(j, _)| **j == crate::Joint::Head)
            .map(|(_, t)| t.translation())
            .unwrap_or(Vec3::new(0.0, 1.1, 0.0));
        match kind {
            Throw::Confetti => {
                let (Some(cube), false) = (chat.cube.clone(), chat.colors.is_empty()) else {
                    continue;
                };
                for i in 0..70 {
                    let r = |k: u32| crate::hash01(i * 7 + k, now);
                    let pos = head_pos
                        + Vec3::new((r(1) - 0.5) * 1.6, 0.7 + r(2) * 0.6, (r(3) - 0.2) * 0.6);
                    let t = Transform::from_translation(pos).with_rotation(Quat::from_euler(
                        EulerRot::XYZ,
                        r(4) * 6.0,
                        r(5) * 6.0,
                        r(6) * 6.0,
                    ));
                    commands.spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(chat.colors[(i as usize) % chat.colors.len()].clone()),
                        t,
                        crate::sim::Interp::new(t),
                        Confetto {
                            vel: Vec3::new(
                                (r(7) - 0.5) * 0.6,
                                -0.2 - r(8) * 0.5,
                                (r(9) - 0.5) * 0.3,
                            ),
                            spin: Vec3::new(r(10) * 9.0, r(11) * 9.0, r(12) * 4.0),
                            t0: now,
                        },
                    ));
                }
                chat.hits.push((kind, size));
            }
            _ => {
                let side = if av.rand01() < 0.5 { -1.0 } else { 1.0 };
                let from = head_pos
                    + Vec3::new(
                        side * (1.3 + av.rand01() * 0.4),
                        -0.7 - av.rand01() * 0.3,
                        1.4,
                    );
                let path = if kind == Throw::Tomato {
                    "props/lead/tomato.glb"
                } else {
                    "props/lead/egg.glb"
                };
                let aim = Vec2::new((av.rand01() - 0.5) * 0.18, (av.rand01() - 0.5) * 0.14);
                // the eyes catch it coming (eyes_left looks toward the left of the picture)
                if now - av.last_hit > 0.3 {
                    av.glance = Some((
                        if side < 0.0 {
                            "eyes_left"
                        } else {
                            "eyes_right"
                        },
                        now + 0.5,
                    ));
                }
                commands.spawn((
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Transform::from_translation(from).with_scale(Vec3::splat(1.7 * size)),
                    Flying {
                        kind,
                        size,
                        from,
                        t0: now,
                        dur: 0.55 + av.rand01() * 0.15,
                        spin: Vec3::new(av.rand01() * 14.0, av.rand01() * 10.0, 4.0),
                        aim,
                    },
                ));
            }
        }
    }
}

/// Bits: one tomato per 100 bits (up to 8) in a quick volley; 500+ adds a giant one at the end.
/// Under 100 bits: a single egg.
pub fn cheer(chat: &mut Chat, user: &str, bits: u32, now: f32) {
    let n = (bits / 100).clamp(1, 8);
    let kind = if bits < 100 {
        Throw::Egg
    } else {
        Throw::Tomato
    };
    for i in 0..n {
        chat.barrage
            .push((now + i as f32 * 0.22, kind, user.to_string(), 1.0));
    }
    if bits >= 500 {
        chat.barrage.push((
            now + n as f32 * 0.22 + 0.5,
            Throw::Tomato,
            user.to_string(),
            2.6,
        ));
    }
    chat.credit = Some((format!("{user} cheered {bits} bits!"), now));
}

/// Arc toward the face; on arrival, a splat sticks to the head and the avatar flinches.
pub fn fly(
    mut chat: ResMut<Chat>,
    time: Res<Time>,
    mut q: Query<(Entity, &Flying, &mut Transform)>,
    head: Query<(Entity, &crate::Joint, &GlobalTransform)>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let Some((head_e, _, head_tf)) = head.iter().find(|(_, j, _)| **j == crate::Joint::Head) else {
        return;
    };
    for (e, f, mut t) in &mut q {
        let face_local = Vec3::new(f.aim.x, 0.27 + f.aim.y, 0.34);
        let target = head_tf.transform_point(face_local);
        let u = ((now - f.t0) / f.dur).clamp(0.0, 1.0);
        let pos = f.from.lerp(target, u) + Vec3::Y * (4.0 * u * (1.0 - u) * 0.45);
        t.translation = pos;
        t.rotation = Quat::from_euler(EulerRot::XYZ, f.spin.x * u, f.spin.y * u, f.spin.z * u);
        if u >= 1.0 {
            commands.entity(e).despawn();
            let (path, rot, scale) = match f.kind {
                Throw::Tomato => (
                    "props/lead/fx_splat.glb",
                    Quat::from_rotation_z((f.aim.x * 3.0).clamp(-0.4, 0.4)),
                    1.0,
                ),
                // the egg splat is modelled lying flat; stand it up against the face
                _ => (
                    "props/lead/fx_eggsplat.glb",
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    0.8,
                ),
            };
            let t = Transform::from_translation(Vec3::new(f.aim.x, 0.30 + f.aim.y, 0.31))
                .with_rotation(rot)
                .with_scale(Vec3::splat(scale * f.size.sqrt()));
            commands.spawn((
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                t,
                crate::sim::Interp::new(t),
                Splat { t0: now, life: 3.5 },
                ChildOf(head_e),
            ));
            chat.hits.push((f.kind, f.size));
        }
    }
}

/// One fixed tick (`FixedUpdate`): splats slide down and shrink at the end; confetti bits fall
/// and spin.
pub fn fade(
    time: Res<Time>,
    mut splats: Query<(Entity, &Splat, &mut crate::sim::Interp), Without<Confetto>>,
    mut bits: Query<(Entity, &Confetto, &mut crate::sim::Interp), Without<Splat>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (e, s, mut ip) in &mut splats {
        let t = &mut ip.curr;
        let age = now - s.t0;
        if age > s.life {
            commands.entity(e).despawn();
        } else if age > s.life - 0.5 {
            // slide down and shrink away
            let k = (s.life - age) / 0.5;
            t.scale = Vec3::splat(t.scale.x.max(0.01) * (0.9 + 0.1 * k));
            t.translation.y -= 0.12 * dt;
        }
    }
    for (e, c, mut ip) in &mut bits {
        let t = &mut ip.curr;
        let age = now - c.t0;
        if age > 3.0 {
            commands.entity(e).despawn();
            continue;
        }
        let v = c.vel + Vec3::new((age * 3.0 + c.spin.x).sin() * 0.25, 0.0, 0.0);
        t.translation += v * dt;
        t.rotate_local(Quat::from_euler(
            EulerRot::XYZ,
            c.spin.x * dt,
            c.spin.y * dt,
            c.spin.z * dt,
        ));
    }
}
