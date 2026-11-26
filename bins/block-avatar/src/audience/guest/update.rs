//! The guest on stage, frame by frame: arriving, asking, leaving, and the face following the voice.

#[allow(unused_imports)]
use super::*;

/// Bring the next question on, run the one on stage, and feed chat votes and bans to the queue.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut g: ResMut<Guest>,
    mut av: ResMut<crate::Avatar>,
    mut chat: ResMut<crate::chat::Chat>,
    mut stages: ResMut<crate::stage::Stages>,
    lip: Res<crate::Lip>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    g.lip_mode = lip.picker.mode;
    // chat: votes and moderators' bans
    if !chat.votes.is_empty() || !chat.bans.is_empty() {
        let mut q = g.queue.lock().unwrap_or_else(|e| e.into_inner());
        for (voter, id) in std::mem::take(&mut chat.votes) {
            q.vote(id, &voter);
        }
        for (user, secs) in std::mem::take(&mut chat.bans) {
            q.chat_ban(&user, secs, crate::questions::unix_now());
        }
    }
    // next one up, once the stage is clear
    if g.stage.is_none() {
        if let Some(q) = g.pending.take() {
            let path = g.queue.lock().unwrap_or_else(|e| e.into_inner()).wav_path(q.id);
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| parse_wav_mono16(&b))
            {
                Ok((rate, samples)) => {
                    let voice = Voice::analyse(rate, samples);
                    let mut picker = LipSync::new(
                        PickerSettings {
                            gate_db: voice.gate_db,
                            ..default()
                        },
                        g.lip_mode,
                    );
                    picker.use_formants = lip.picker.use_formants;
                    let name = g.queue.lock().unwrap_or_else(|e| e.into_inner()).display_name(&q);
                    g.status = format!("On air: question {} from {}", q.id, name);
                    g.stage = Some(OnStage {
                        gltf: assets.load::<Gltf>(q.model.clone()),
                        q,
                        name,
                        graph: None,
                        idle: None,
                        talk: None,
                        root: None,
                        player: None,
                        phase: Phase::Loading,
                        voice,
                        audio: None,
                        picker,
                        prosody: Prosody::default(),
                        mouth: Mouth::Rest,
                        level: 0.0,
                        brows_up: false,
                        beat_t: -10.0,
                        beat_amp: 0.0,
                        speaking_since: None,
                        silent_since: 0.0,
                        body_talk: false,
                        next_blink: now + 1.5,
                        blink_until: 0.0,
                        reveal: 0.0,
                    });
                }
                Err(e) => {
                    g.status = format!("Question {}: can't read the recording ({e})", q.id);
                    g.queue.lock().unwrap_or_else(|e| e.into_inner()).finish(q.id, Status::Skipped);
                }
            }
        }
    }
    let output = g.output.clone();
    let Some(s) = g.stage.as_mut() else {
        av.listen_to = 0.0;
        av.hush = false;
        av.two_shot = false;
        stages.guest_model = None;
        return;
    };
    // load the character
    if s.phase == Phase::Loading && s.root.is_none() {
        if let Some(gltf) = gltfs.get(&s.gltf) {
            let mut graph = AnimationGraph::new();
            for (name, h) in gltf.named_animations.iter() {
                let node = graph.add_clip(h.clone(), 1.0, graph.root);
                match name.as_ref() {
                    "Idle" => s.idle = Some(node),
                    "Talk" => s.talk = Some(node),
                    _ => {}
                }
            }
            s.graph = Some(graphs.add(graph));
            if let Some(scene) = gltf
                .default_scene
                .clone()
                .or_else(|| gltf.scenes.first().cloned())
            {
                let root = commands
                    .spawn((
                        WorldAssetRoot(scene),
                        Transform::from_xyz(GUEST_X, 0.0, 0.0)
                            .with_rotation(Quat::from_rotation_y(GUEST_YAW))
                            .with_scale(Vec3::splat(0.001)),
                        GuestRoot,
                    ))
                    .observe(on_ready)
                    .id();
                s.root = Some(root);
            }
        }
    }
    stages.guest_model = Some(s.q.model.clone());
    av.two_shot = s.root.is_some() && !matches!(s.phase, Phase::Leaving(..));
    av.listen_to = if matches!(s.phase, Phase::Speaking | Phase::After(_)) {
        1.0
    } else {
        0.0
    };
    // play: the face follows the audio clock
    let mut frame = None;
    match s.phase {
        Phase::Entering(t0) if now - t0 > ENTER_S + LEAD_S => {
            s.audio = Some(Playback::start(
                s.voice.samples.clone(),
                s.voice.rate,
                output,
            ));
            s.phase = Phase::Speaking;
        }
        Phase::Speaking => {
            let t = s
                .audio
                .as_ref()
                .map(|a| a.played_s(s.voice.rate))
                .unwrap_or(0.0);
            frame = s.voice.frame_at(t);
            s.reveal = s.voice.spoken_at(t);
            if t >= s.voice.secs() - 0.01 {
                s.phase = Phase::After(now);
                s.reveal = 1.0;
                frame = None;
            }
        }
        Phase::Leaving(t0, status) if now - t0 > LEAVE_S => {
            if let Some(r) = s.root {
                commands.entity(r).despawn();
            }
            let id = s.q.id;
            g.stage = None;
            g.queue.lock().unwrap_or_else(|e| e.into_inner()).finish(id, status);
            g.status = format!(
                "Question {id} {}",
                if status == Status::Done {
                    "answered"
                } else {
                    "cut"
                }
            );
            return;
        }
        _ => {}
    }
    av.hush = g.mute_host && g.talking();
    if let Some(dev) = g
        .stage
        .as_ref()
        .and_then(|s| s.audio.as_ref())
        .map(|a| a.device.lock().unwrap_or_else(|e| e.into_inner()).clone())
    {
        let line = format!(
            "On air: question {} from {} | sound: {dev}",
            g.stage.as_ref().map(|s| s.q.id).unwrap_or(0),
            g.stage.as_ref().map(|s| s.name.clone()).unwrap_or_default()
        );
        if g.status != line {
            g.status = line;
        }
    }
    let Some(s) = g.stage.as_mut() else { return };
    // lip-sync and body language from the recording
    let f = frame.unwrap_or(Frame {
        db: -100.0,
        ..s.voice.frames.first().copied().unwrap_or_default()
    });
    let f = if frame.is_some() {
        f
    } else {
        Frame { db: -100.0, ..f }
    };
    s.mouth = s.picker.update(f, dt);
    s.level = level01(f, &s.picker.settings);
    let v = VoiceFrame {
        pitch_hz: f.pitch_hz,
        clarity: f.clarity,
        f1: f.f1,
        f2: f.f2,
    };
    let ev = s.prosody.update(
        v,
        f.db,
        f.onsets,
        dt,
        s.picker.settings.gate_db,
        s.picker.speech_db,
    );
    s.brows_up = s.prosody.brows_up;
    if let Some(k) = ev.beat {
        s.beat_t = now;
        s.beat_amp = k;
    }
    if s.mouth != Mouth::Rest {
        s.speaking_since.get_or_insert(now);
        s.silent_since = now;
    } else if now - s.silent_since > 0.6 {
        s.speaking_since = None;
    }
    // Idle <-> Talk
    let talk = s.speaking_since.is_some_and(|t| now - t > 0.25);
    if talk != s.body_talk {
        if let (Some(pe), Some(node)) = (s.player, if talk { s.talk.or(s.idle) } else { s.idle }) {
            if let Ok((mut p, mut tr)) = players.get_mut(pe) {
                tr.play(&mut p, node, Duration::from_millis(300)).repeat();
                s.body_talk = talk;
            }
        }
    }
    if s.mouth == Mouth::Rest && now >= s.next_blink {
        s.blink_until = now + 0.12;
        s.next_blink = now + 2.0 + crate::hash01(s.q.id, now) * 3.5;
    }
}

pub(super) fn on_ready(
    ready: On<WorldInstanceReady>,
    mut g: ResMut<Guest>,
    time: Res<Time>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut players: Query<&mut AnimationPlayer>,
    mut commands: Commands,
) {
    let Some(s) = g.stage.as_mut().filter(|s| s.root == Some(ready.entity)) else {
        return;
    };
    let Some(graph) = s.graph.clone() else { return };
    for e in children.iter_descendants(ready.entity) {
        if let Ok(name) = names.get(e) {
            if let Some(part) = crate::FACE_PARTS.iter().find(|p| **p == name.as_str()) {
                commands.entity(e).insert(GuestPart(part));
            } else if name.as_str() == "Head" {
                commands.entity(e).insert(GuestHead);
            }
        }
        if let Ok(mut player) = players.get_mut(e) {
            let mut tr = AnimationTransitions::new();
            if let Some(idle) = s.idle {
                tr.play(&mut player, idle, Duration::ZERO).repeat();
            }
            commands
                .entity(e)
                .insert((AnimationGraphHandle(graph.clone()), tr));
            s.player = Some(e);
        }
    }
    s.phase = Phase::Entering(time.elapsed_secs());
}

/// After the animation step: the guest's face, head and pop in/out.
pub fn drive_face(
    g: Res<Guest>,
    time: Res<Time>,
    mut parts: Query<(&GuestPart, &mut Transform), (Without<GuestHead>, Without<GuestRoot>)>,
    mut heads: Query<&mut Transform, (With<GuestHead>, Without<GuestRoot>)>,
    mut roots: Query<&mut Transform, With<GuestRoot>>,
) {
    let Some(s) = g.stage.as_ref() else { return };
    let now = time.elapsed_secs();
    let ex = &crate::EXPRESSIONS[0];
    let after = matches!(s.phase, Phase::After(_));
    let mut eyes = if now < s.blink_until {
        "eyes_shut"
    } else {
        ex.eyes
    };
    let brows = if s.brows_up || after {
        "brows_raised"
    } else {
        ex.brows
    };
    let mouth = if after {
        "mouth_smirk"
    } else {
        ex.mouths[s.mouth.slot()]
    };
    if matches!(s.phase, Phase::Entering(t0) if now - t0 < 0.2) {
        eyes = "eyes_wide";
    }
    for (p, mut t) in &mut parts {
        t.scale = if p.0 == eyes || p.0 == brows || p.0 == mouth {
            Vec3::ONE
        } else {
            Vec3::ZERO
        };
    }
    // head: nod with the voice, a quick dip on stressed syllables, a curious tilt while waiting
    let mut x = s.level * 0.10
        + if s.mouth != Mouth::Rest {
            (now * 7.0).sin() * 0.025
        } else {
            0.0
        };
    let b = (now - s.beat_t) / 0.3;
    if b < 1.0 {
        x += 0.09 * (std::f32::consts::PI * b).sin() * s.beat_amp;
    }
    let mut z = (now * 1.2 + 1.0).sin() * 0.02;
    let mut y = 0.0;
    if let Phase::After(t0) = s.phase {
        let e = ((now - t0) / 0.35).clamp(0.0, 1.0);
        let e = e * e * (3.0 - 2.0 * e);
        z += 0.2 * e;
        y += 0.1 * e;
        x -= 0.05 * e;
    }
    for mut t in &mut heads {
        t.rotation *= Quat::from_euler(EulerRot::YXZ, y, x, z);
    }
    // pop in with a little overshoot, pop out
    let scale = match s.phase {
        Phase::Loading => 0.001,
        Phase::Entering(t0) => {
            let u = ((now - t0) / ENTER_S).clamp(0.0, 1.0);
            let c = 1.70158;
            (1.0 + (c + 1.0) * (u - 1.0).powi(3) + c * (u - 1.0).powi(2)).max(0.001)
        }
        Phase::Leaving(t0, _) => (1.0 - ((now - t0) / LEAVE_S).clamp(0.0, 1.0))
            .powi(2)
            .max(0.001),
        _ => 1.0,
    };
    let hop = match s.phase {
        Phase::Entering(t0) => {
            0.25 * (std::f32::consts::PI * ((now - t0) / ENTER_S).clamp(0.0, 1.0)).sin()
        }
        _ => 0.0,
    };
    for mut t in &mut roots {
        t.scale = Vec3::splat(scale);
        t.translation = Vec3::new(GUEST_X, hop, 0.0);
    }
}
