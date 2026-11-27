//! The scene and the character: lights and camera, loading a model and its animation graph,
//! Whisper, and hits landing.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
// Scene, model loading
// ------------------------------------------------------------------------------------------
pub(super) fn setup_scene(mut commands: Commands) {
    // No tonemapping: keeps the palette's flat block colours exactly as authored.
    commands.spawn((
        Camera3d::default(),
        Tonemapping::Linear,
        MainCamera,
        vox_world::WorldCamera,
        Transform::from_xyz(0.45, 1.6, 2.8).looking_at(Vec3::new(0.0, 1.42, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 3500.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-2.0, 4.0, 4.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        lighting::KeyLight,
        lighting::key_cascades(),
    ));
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        TextShadow::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));
}

pub(super) fn load_model(mut av: ResMut<Avatar>, asset_server: Res<AssetServer>, mut commands: Commands) {
    if let Some(old) = av.root.take() {
        commands.entity(old).despawn();
    }
    begin_load(&mut av, &asset_server);
}

pub(super) fn begin_load(av: &mut Avatar, asset_server: &AssetServer) {
    av.player = None;
    av.clips.clear();
    av.clip_order.clear();
    av.own_clips.clear();
    av.graph = None;
    av.gesture_active = false;
    av.emote = None;
    let path = av.models[av.model].1.clone();
    av.gltf = Some(asset_server.load::<Gltf>(path));
    av.pending_scene = true;
    av.status = format!("Loading {}...", av.models[av.model].0);
}

pub(super) fn build_graph_when_loaded(
    mut av: ResMut<Avatar>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut commands: Commands,
) {
    if !av.pending_scene {
        return;
    }
    let Some(gltf) = av.gltf.as_ref().and_then(|h| gltfs.get(h)) else {
        return;
    };
    // Names by handle, then walk the clips in file order so we know which are the character's own.
    let by_handle: HashMap<AssetId<AnimationClip>, String> = gltf
        .named_animations
        .iter()
        .map(|(n, h)| (h.id(), n.to_string()))
        .collect();
    let mut graph = AnimationGraph::new();
    let mut clips = HashMap::new();
    let mut own = vec![];
    let mut order = vec![];
    for (i, h) in gltf.animations.iter().enumerate() {
        let Some(name) = by_handle.get(&h.id()) else {
            continue;
        };
        let node = graph.add_clip(h.clone(), 1.0, graph.root);
        clips.insert(name.clone(), (node, h.clone()));
        if !name.starts_with("Face: ") {
            order.push(name.clone());
        }
        if i >= 100 && !name.starts_with("Face: ") {
            own.push(name.clone());
        }
    }
    let scene = gltf
        .default_scene
        .clone()
        .or_else(|| gltf.scenes.first().cloned());
    let Some(scene) = scene else {
        av.status = "That model has no scene".into();
        av.pending_scene = false;
        return;
    };
    av.graph = Some(graphs.add(graph));
    av.clips = clips;
    av.clip_order = order;
    av.own_clips = own;
    av.pending_scene = false;
    av.body = String::new();
    let root = commands
        .spawn((WorldAssetRoot(scene), Transform::default(), Joint::Root))
        .observe(on_model_ready)
        .id();
    av.root = Some(root);
    av.status = format!("{} ready", av.models[av.model].0);
}

pub(super) fn on_model_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    mut players: Query<&mut AnimationPlayer>,
    mut av: ResMut<Avatar>,
    mic: Res<Mic>,
) {
    mic.0.go.store(true, Ordering::Relaxed);
    let Some(graph) = av.graph.clone() else {
        return;
    };
    for e in children.iter_descendants(ready.entity) {
        if let Ok(name) = names.get(e) {
            if let Some(part) = FACE_PARTS.iter().find(|p| **p == name.as_str()) {
                commands.entity(e).insert(FacePart(part));
            } else {
                let joint = match name.as_str() {
                    "Head" => Some(Joint::Head),
                    "Hips" => Some(Joint::Hips),
                    "UpperLeg_L" | "UpperLeg_R" => Some(Joint::Leg),
                    "UpperArm_L" => Some(Joint::ArmL),
                    "UpperArm_R" => Some(Joint::ArmR),
                    _ => None,
                };
                if name.as_str() == "LowerArm_R" {
                    commands.entity(e).insert(wardrobe::HostHand);
                }
                if let Some(j) = joint {
                    commands.entity(e).insert(j);
                }
            }
        }
        if let Ok(mut player) = players.get_mut(e) {
            let mut transitions = AnimationTransitions::new();
            let base = av
                .base_loop
                .clone()
                .filter(|b| av.clips.contains_key(b))
                .unwrap_or("Idle".into());
            if let Some((idle, _)) = av.clips.get(&base) {
                transitions
                    .play(&mut player, *idle, Duration::ZERO)
                    .repeat();
            }
            commands
                .entity(e)
                .insert((AnimationGraphHandle(graph.clone()), transitions));
            av.player = Some(e);
            av.body = base;
        }
    }
}

/// Where the Whisper model is: the saved setting, else `whisper/ggml-base.en.bin` in your files
/// folder, else `assets/models/whisper/` next to the app (the first of these that exists).
pub fn whisper_model(saved: Option<&str>) -> std::path::PathBuf {
    if let Some(m) = saved {
        let p = std::path::Path::new(m);
        return if p.is_absolute() {
            p.to_path_buf()
        } else {
            asset_base().join(p)
        };
    }
    let mine = applog::user_dir().join("whisper").join("ggml-base.en.bin");
    let bundled = asset_base().join("assets/models/whisper/ggml-base.en.bin");
    if !mine.exists() && bundled.exists() {
        bundled
    } else {
        mine
    }
}

/// Starts built-in speech-to-text for the bubbles, if this build has it and a model is there.
#[allow(unused_variables)]
pub(super) fn start_whisper(
    saved: &HashMap<String, String>,
    caps: &mut bubbles::Captions,
    mic: &Mic,
    queue: &Arc<Mutex<questions::Queue>>,
) {
    #[cfg(feature = "whisper")]
    {
        let path = whisper_model(saved.get("whisper_model").map(|s| s.as_str()));
        if !path.exists() {
            caps.source = format!("Whisper: no model at {} (see README)", path.display());
            return;
        }
        // viewer questions get transcribed for their speech bubble
        if let Err(e) =
            questions::start_transcriber(path.to_string_lossy().into_owned(), queue.clone())
        {
            crate::problem!("[ask] {e}");
        }
        match bubbles::stt::start(path.to_string_lossy().into_owned()) {
            Ok((tap, rx)) => {
                *mic.0.tap.lock().unwrap_or_else(|e| e.into_inner()) = Some(tap);
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                caps.attach_whisper(rx, &name);
            }
            Err(e) => caps.source = e,
        }
    }
}

/// A tomato or egg landed: flinch. Confetti: celebrate.
pub(super) fn apply_hits(
    mut chat: ResMut<chat::Chat>,
    mut av: ResMut<Avatar>,
    mut fx: ResMut<fx::Fx>,
    mut cues: ResMut<lighting::Cues>,
    time: Res<Time>,
    mut players: Players,
) {
    let now = time.elapsed_secs();
    for (h, size) in std::mem::take(&mut chat.hits) {
        match h {
            chat::Throw::Confetti => {
                fx.play("pop", 0.8);
                av.face_override = Some((1, now + 1.8));
                av.emote = Some((Emote::Hop, now));
            }
            _ => {
                av.last_hit = now;
                av.glance = None;
                fx.play("splat", if h == chat::Throw::Egg { 0.6 } else { 0.9 });
                // squash the head; a giant one shakes the whole picture
                av.head_squash = (now, (0.10 * size.sqrt()).min(0.2));
                if size > 1.5 {
                    cues.0.push(lighting::Cue::BigHit);
                    fx.play("thud", 1.0);
                    fx.shake(now, 0.045 * size.min(3.0));
                    fx.dust.push((Vec3::new(0.0, 0.0, 0.1), 1.3));
                }
                let face = if av.rand01() < 0.5 { SURPRISED } else { 2 };
                av.face_override = Some((face, now + 1.4));
                if av.emote.is_none() {
                    av.emote = Some((Emote::Jolt, now));
                }
                if !av.gesture_active && av.clips.contains_key("Hit reaction (light)") {
                    play_gesture(&mut av, "Hit reaction (light)", true, &mut players);
                }
            }
        }
    }
}
