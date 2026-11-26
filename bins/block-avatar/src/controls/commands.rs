//! One command set for the keyboard, the control page, hotkeys and scene presets: `parse_cmd`
//! turns text into a `Cmd`, `apply_cmd` runs it, `apply_extra` handles everything else.

use crate::*;

// ------------------------------------------------------------------------------------------
// Input: keyboard and the control page share one command set
// ------------------------------------------------------------------------------------------
pub(crate) enum Cmd {
    /// switch to a character by its label or model path
    ModelTo(String),
    Expression(usize),
    Play(String),
    Own(usize),
    Loop(Option<String>),
    Emote(Emote),
    Lip(Option<LipMode>),
    Reactions,
    Bop,
    Model(i32),
    Background,
    BackgroundTo(Background),
    Framing,
    FramingTo(usize),
    Hud,
    Mute,
    Calibrate,
    Gate(f32),
    AutoTalk,
    Webcam,
    BodyLanguage,
    Vowels,
    Formants,
}

pub(crate) fn parse_cmd(s: &str) -> Option<Cmd> {
    let (k, v) = s.split_once(':').unwrap_or((s, ""));
    Some(match k {
        "face" => Cmd::Expression(v.parse::<usize>().ok().filter(|i| *i < EXPRESSIONS.len())?),
        // "gesture" kept from the first version; both play any clip once
        "gesture" | "play" => Cmd::Play(v.to_string()),
        "own" => Cmd::Own(v.parse().ok()?),
        "loop" => Cmd::Loop(Some(v.to_string()).filter(|v| !v.is_empty())),
        "emote" => Cmd::Emote(EMOTES.iter().find(|e| e.1 == v)?.0),
        "lip" => Cmd::Lip(if v.is_empty() {
            None
        } else {
            Some(LipMode::parse(v)?)
        }),
        "react" => Cmd::Reactions,
        "frame" if !v.is_empty() => Cmd::FramingTo(v.parse::<usize>().ok().filter(|f| *f < 3)?),
        "bg" if !v.is_empty() => Cmd::BackgroundTo(Background::parse(v)?),
        "bop" => Cmd::Bop,
        "model" => match v {
            "prev" => Cmd::Model(-1),
            "" | "next" => Cmd::Model(1),
            name => Cmd::ModelTo(name.to_string()),
        },
        "bg" => Cmd::Background,
        "frame" => Cmd::Framing,
        "hud" => Cmd::Hud,
        "mute" => Cmd::Mute,
        "calibrate" => Cmd::Calibrate,
        "gate" => Cmd::Gate(v.parse().ok()?),
        "autotalk" => Cmd::AutoTalk,
        "webcam" => Cmd::Webcam,
        "body" => Cmd::BodyLanguage,
        "vowels" => Cmd::Vowels,
        "formants" => Cmd::Formants,
        _ => return None,
    })
}

pub(crate) type Players<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut AnimationPlayer,
        &'static mut AnimationTransitions,
    ),
>;

#[allow(clippy::too_many_arguments)]
pub(crate) type Extras<'w> = (
    ResMut<'w, chat::Chat>,
    ResMut<'w, bubbles::Captions>,
    ResMut<'w, stage::Stages>,
    ResMut<'w, guest::Guest>,
    Show<'w>,
);
pub(crate) type Show<'w> = (
    ResMut<'w, wardrobe::Wardrobe>,
    ResMut<'w, events::Moments>,
    ResMut<'w, poll::Polls>,
    ResMut<'w, Inject>,
    ResMut<'w, voicefx::VoiceFx>,
    ResMut<'w, cohost::Cohost>,
    ResMut<'w, show::Show>,
    ResMut<'w, clips::Clips>,
    ResMut<'w, fx::Fx>,
    ResMut<'w, hotkeys::Hotkeys>,
    ResMut<'w, bevel::Bevel>,
    ResMut<'w, lighting::Studio>,
    ResMut<'w, crowd::Crowd>,
    ResMut<'w, particles::Particles>,
    ResMut<'w, facecap::FaceCap>,
    (ResMut<'w, net::Net>, ResMut<'w, crate::world::WorldClient>),
);

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut queue: Local<Vec<Cmd>>,
    mut av: ResMut<Avatar>,
    mut lip: ResMut<Lip>,
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut clear: ResMut<ClearColor>,
    mut cam: ResMut<webcam::Webcam>,
    mut players: Players,
    (mut chat, mut caps, mut stages, mut guest, mut show): Extras,
) {
    // in a world the keys walk, fly and build instead
    if show.15.1.active() {
        return;
    }
    // Q test throw (tomato, egg, confetti at random), S speech bubbles, Y retro sprite mode,
    // A viewer question: air the next approved one / end the one on stage (cut while it talks)
    for (k, c) in [
        (KeyCode::KeyQ, "throw:cycle"),
        (KeyCode::KeyS, "bubbles"),
        (KeyCode::KeyY, "retro"),
        (KeyCode::KeyA, "ask:key"),
        (KeyCode::KeyE, "clip"),
    ] {
        if keys.just_pressed(k) {
            apply_extra(
                c,
                &mut av,
                &mut chat,
                &mut caps,
                &mut stages,
                &mut guest,
                &mut show,
                time.elapsed_secs(),
            );
        }
    }
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    const FKEYS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    // Z X C J K U I O: nod, shake, tilt, hop, spin, bow, shiver, lean in
    const EMOTE_KEYS: [KeyCode; 8] = [
        KeyCode::KeyZ,
        KeyCode::KeyX,
        KeyCode::KeyC,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyU,
        KeyCode::KeyI,
        KeyCode::KeyO,
    ];
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) {
            queue.push(Cmd::Expression(i));
        }
    }
    for (i, k) in FKEYS.iter().enumerate() {
        if keys.just_pressed(*k) {
            queue.push(if i < 8 {
                Cmd::Play(GESTURES[i].into())
            } else {
                Cmd::Own(i - 8)
            });
        }
    }
    for (i, k) in EMOTE_KEYS.iter().enumerate() {
        if keys.just_pressed(*k) {
            queue.push(Cmd::Emote(EMOTES[i].0));
        }
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let map = [
        (
            KeyCode::Tab,
            if shift { Cmd::Model(-1) } else { Cmd::Model(1) },
        ),
        (KeyCode::KeyL, Cmd::Lip(None)),
        (KeyCode::KeyR, Cmd::Reactions),
        (KeyCode::KeyD, Cmd::Bop),
        (KeyCode::KeyB, Cmd::Background),
        (KeyCode::KeyV, Cmd::Framing),
        (KeyCode::KeyH, Cmd::Hud),
        (KeyCode::KeyM, Cmd::Mute),
        (KeyCode::KeyN, Cmd::Calibrate),
        (KeyCode::BracketLeft, Cmd::Gate(-2.0)),
        (KeyCode::BracketRight, Cmd::Gate(2.0)),
        (KeyCode::KeyG, Cmd::AutoTalk),
        (KeyCode::KeyW, Cmd::Webcam),
        (KeyCode::KeyT, Cmd::BodyLanguage),
        (KeyCode::KeyP, Cmd::Vowels),
    ];
    for (k, c) in map {
        if keys.just_pressed(k) {
            queue.push(c);
        }
    }
    for cmd in queue.drain(..) {
        apply_cmd(
            cmd,
            &mut av,
            &mut lip,
            &time,
            &mut commands,
            &asset_server,
            &mut clear,
            &mut cam,
            &mut players,
        );
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_remote(
    rx: Res<control::Commands>,
    mut av: ResMut<Avatar>,
    mut lip: ResMut<Lip>,
    time: Res<Time>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut clear: ResMut<ClearColor>,
    mut cam: ResMut<webcam::Webcam>,
    mut players: Players,
    (mut chat, mut caps, mut stages, mut guest, mut show): Extras,
) {
    let mut msgs: Vec<String> = std::mem::take(&mut show.3.0);
    msgs.extend(rx.0.lock().unwrap_or_else(|e| e.into_inner()).try_iter());
    for m in msgs {
        match parse_cmd(&m) {
            Some(cmd) => apply_cmd(
                cmd,
                &mut av,
                &mut lip,
                &time,
                &mut commands,
                &asset_server,
                &mut clear,
                &mut cam,
                &mut players,
            ),
            None => {
                if !apply_extra(
                    &m,
                    &mut av,
                    &mut chat,
                    &mut caps,
                    &mut stages,
                    &mut guest,
                    &mut show,
                    time.elapsed_secs(),
                ) {
                    av.status = format!("Unknown command '{m}'");
                }
            }
        }
    }
}

/// Commands for chat throws, speech bubbles and retro mode. Returns false if `m` isn't one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_extra(
    m: &str,
    av: &mut Avatar,
    chat: &mut chat::Chat,
    caps: &mut bubbles::Captions,
    stages: &mut stage::Stages,
    guest: &mut guest::Guest,
    show: &mut Show,
    now: f32,
) -> bool {
    let (k, v) = m.split_once(':').unwrap_or((m, ""));
    match k {
        "mic" => {
            av.mic_switch = Some(v.to_string());
            save_keys(&[("mic", v.to_string())]);
            av.status = format!(
                "Switching to mic: {}",
                if v.is_empty() { "default" } else { v }
            );
        }
        "askcfg" => {
            let (what, val) = v.split_once(':').unwrap_or((v, ""));
            match (what, &guest.ask) {
                ("client", Some(a)) => {
                    let id = val.trim().to_string();
                    a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner()).twitch_client_id =
                        Some(id.clone()).filter(|s| !s.is_empty());
                    save_keys(&[("twitch_client_id", id.clone())]);
                    av.status = if id.is_empty() {
                        "Twitch sign-in for questions off".into()
                    } else {
                        "Twitch sign-in for questions set up".into()
                    };
                }
                _ => return false,
            }
        }
        "setup" => {
            av.setup_done = true;
            save_keys(&[("setup_done", "true".to_string())]);
            av.status = "Setup finished".into();
        }
        "problems" => {
            applog::clear_problems();
        }
        "folder" => {
            let dir = applog::user_dir();
            applog::open_folder(&match v {
                "clips" => dir.join("clips"),
                "sounds" => dir.join("sounds"),
                _ => dir,
            });
        }
        "wear" => {
            let Some(msg) = show.0.command(v, now) else {
                return false;
            };
            if v == "chat" || v.starts_with("who:") {
                save_keys(&[
                    ("dressup_chat", show.0.chat.to_string()),
                    ("dressup_who", show.0.who.name().to_string()),
                ]);
            }
            av.status = msg;
        }
        "event" if v == "cheer" => {
            chat::cheer(chat, "TestCheerer", 600, now);
            av.status = "Test cheer: 600 bits".into();
        }
        "event" => {
            let Some(msg) = show.1.command(v) else {
                return false;
            };
            save_keys(&[
                ("raid_crowd", show.1.raid_crowd.to_string()),
                ("sub_party", show.1.sub_party.to_string()),
            ]);
            av.status = msg;
        }
        "poll" => {
            let Some(msg) = show.2.command(v) else {
                return false;
            };
            av.status = msg;
        }
        "clip" => {
            let Some(msg) = show.7.command(v) else {
                return false;
            };
            let c = &show.7;
            save_keys(&[
                ("clips", c.on.to_string()),
                ("auto_shorts", c.auto.to_string()),
                ("comics", c.comics.to_string()),
                ("short_cards", c.cards.to_string()),
            ]);
            av.status = msg;
        }
        "set" | "director" | "press" => {
            let Some(msg) = show.6.command(k, v) else {
                return false;
            };
            let s = &show.6;
            save_keys(&[
                ("set", s.set.clone()),
                ("director", s.director.to_string()),
                ("press", s.press.to_string()),
            ]);
            av.status = msg;
        }
        "preset" => {
            // scene presets: the character, set, framing, background, lighting and outfit
            let (verb, name) = v.split_once(':').unwrap_or((v, ""));
            let name: String = name
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(40)
                .collect();
            let path = applog::user_dir().join("presets.json");
            let mut all: serde_json::Map<String, serde_json::Value> =
                std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default();
            match verb {
                "save" if !name.is_empty() => {
                    let mut cmds = vec![
                        format!("model:{}", av.models[av.model].0),
                        format!("frame:{}", av.framing),
                        format!("bg:{}", av.background.name()),
                        format!("set:{}", show.6.set),
                        format!("light:{}", if show.11.on { "studio" } else { "flat" }),
                        "wear:clear".into(),
                    ];
                    cmds.extend(
                        show.0
                            .worn
                            .iter()
                            .flatten()
                            .map(|o| format!("wear:{}", wardrobe::ITEMS[o.item].name)),
                    );
                    all.insert(name.clone(), serde_json::json!(cmds));
                    av.status = format!("Saved scene \"{name}\"");
                }
                "load" => {
                    let Some(cmds) = all.get(&name).and_then(|c| c.as_array()).cloned() else {
                        return false;
                    };
                    show.3
                        .0
                        .extend(cmds.iter().filter_map(|c| c.as_str().map(String::from)));
                    av.status = format!("Scene \"{name}\"");
                }
                "del" => {
                    if all.remove(&name).is_none() {
                        return false;
                    }
                    av.status = format!("Deleted scene \"{name}\"");
                }
                _ => return false,
            }
            if verb != "load" {
                let _ = std::fs::write(
                    &path,
                    serde_json::to_string_pretty(&all).unwrap_or_default(),
                );
            }
            av.presets = all.keys().cloned().collect();
        }
        "crowd" => {
            let Some(msg) = show.12.command(v) else {
                return false;
            };
            save_keys(&[("chat_crowd", show.12.on.to_string())]);
            av.status = msg;
        }
        "idle" => {
            av.idle_life = !av.idle_life;
            save_keys(&[("idle_life", av.idle_life.to_string())]);
            av.status = format!("Idle life {}", if av.idle_life { "on" } else { "off" });
        }
        "light" | "quality" => {
            let Some(msg) = show.11.command(k, v) else {
                return false;
            };
            let l = &show.11;
            save_keys(&[
                ("studio_light", l.on.to_string()),
                ("studio_shadows", l.shadows.to_string()),
                ("studio_glow", l.glow.to_string()),
                ("studio_grade", l.grade.to_string()),
                ("studio_blur", format!("{:.0}", l.blur * 100.0)),
                ("quality", l.quality_setting()),
            ]);
            av.status = msg;
        }
        "facecap" => {
            let Some(msg) = show.14.command(v) else { return false };
            save_keys(&[("face_capture", show.14.on.to_string()), ("facecap_mirror", show.14.mirror.to_string())]);
            av.status = msg;
        }
        "world" => {
            if !show.15.1.active() {
                av.status = "Not in a world (start with --world ws://host:7780)".into();
                return false;
            }
            show.15.1.cmds.push(v.to_string());
        }
        "net" => {
            let Some(msg) = show.15.0.command(v) else {
                return false;
            };
            av.status = msg;
        }
        "fx" => {
            let Some(msg) = show.13.command(v) else {
                return false;
            };
            save_keys(&[
                ("effects", show.13.on.to_string()),
                ("weather", show.13.weather_setting()),
            ]);
            av.status = msg;
        }
        "bevel" => {
            let Some(msg) = show.10.command(v) else {
                return false;
            };
            let b = &show.10;
            save_keys(&[
                ("bevel", b.on.to_string()),
                ("bevel_strength", format!("{:.0}", b.look.bevel * 100.0)),
                ("bevel_grid", format!("{:.0}", b.look.grid * 100.0)),
                ("bevel_wobble", format!("{:.0}", b.look.wobble * 100.0)),
                ("bevel_detail", format!("{:.0}", b.look.detail * 100.0)),
                ("bevel_blend", b.look.blend.to_string()),
            ]);
            av.status = msg;
        }
        "hotkey" => {
            let Some(msg) = show.9.command(v) else {
                return false;
            };
            av.status = msg;
        }
        "sfx" => {
            let Some(msg) = show.8.command(v) else {
                return false;
            };
            save_keys(&[
                ("sfx", show.8.sounds_on.to_string()),
                ("sfx_volume", format!("{:.0}", show.8.volume * 100.0)),
            ]);
            av.status = msg;
        }
        "voice" => {
            let Some(msg) = show.4.command(v) else {
                return false;
            };
            save_keys(&[
                ("laugh", show.4.det.laugh_on.to_string()),
                ("sing", show.4.det.sing_on.to_string()),
                ("gags", show.4.gags_on.to_string()),
            ]);
            av.status = msg;
        }
        "cohost" => {
            let models = av.models.clone();
            let Some(msg) = show.5.command(v, &models) else {
                return false;
            };
            let c = &show.5;
            save_keys(&[
                ("cohost_on", c.on.to_string()),
                ("cohost_model", c.model.clone()),
                ("cohost_mic", c.mic_name.clone().unwrap_or_default()),
            ]);
            av.status = msg;
        }
        "ask" => {
            let Some(msg) = guest.command(m, now) else {
                return false;
            };
            if m == "ask:open" || m == "ask:close" || m == "ask:toggle" {
                save_keys(&[("ask_open", guest.queue.lock().unwrap_or_else(|e| e.into_inner()).open.to_string())]);
            }
            if m == "ask:names" {
                if let Some(a) = &guest.ask {
                    save_keys(&[(
                        "ask_names",
                        a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner()).open_names.to_string(),
                    )]);
                }
            }
            av.status = msg;
        }
        "throw" => {
            let (kind, user) = v.split_once(':').unwrap_or((v, ""));
            let t = if kind == "cycle" {
                chat::THROWS[(av.rand01() * 3.0) as usize % 3]
            } else {
                match chat::Throw::parse(kind) {
                    Some(t) => t,
                    None => return false,
                }
            };
            chat.queue.push((t, user.trim().to_string()));
        }
        "chat" => {
            chat.enabled = !chat.enabled;
            av.status = format!("Chat throws: {}", if chat.enabled { "on" } else { "off" });
            save_keys(&[("chat_throws", chat.enabled.to_string())]);
        }
        "youtube" => {
            // youtube:key:<api key>, youtube:<video id or link> (empty: off)
            let saved = load_settings();
            if let Some(key) = v.strip_prefix("key:") {
                save_keys(&[("youtube_key", key.trim().to_string())]);
                av.status = "YouTube API key saved".into();
                return true;
            }
            let video = streamchat::youtube_id(v);
            let key = saved.get("youtube_key").cloned().unwrap_or_default();
            save_keys(&[("youtube_video", video.clone().unwrap_or_default())]);
            match video {
                Some(vid) if !key.is_empty() => {
                    let poll = saved
                        .get("youtube_poll_s")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(8);
                    chat.set_feed(
                        "youtube",
                        Some(streamchat::start_youtube(
                            saved
                                .get("youtube_api")
                                .cloned()
                                .unwrap_or_else(|| streamchat::YT_API.into()),
                            key,
                            vid.clone(),
                            poll,
                        )),
                    );
                    av.status = format!("Reading YouTube chat: {vid}");
                }
                Some(_) => av.status = "Add a YouTube API key first".into(),
                None => {
                    chat.set_feed("youtube", None);
                    av.status = if v.trim().is_empty() {
                        "YouTube chat off".into()
                    } else {
                        "That isn't a YouTube video link or id".into()
                    };
                }
            }
        }
        "kick" => {
            let ch = v.trim().trim_start_matches('@').to_ascii_lowercase();
            save_keys(&[("kick_channel", ch.clone())]);
            if ch.is_empty() {
                chat.set_feed("kick", None);
                av.status = "Kick chat off".into();
            } else {
                chat.set_feed(
                    "kick",
                    Some(streamchat::start_kick(
                        load_settings()
                            .get("kick_pusher")
                            .cloned()
                            .unwrap_or_else(|| streamchat::KICK_WS.into()),
                        ch.clone(),
                    )),
                );
                av.status = format!("Reading Kick chat: {ch}");
            }
        }
        "twitch" => {
            chat.connect(v.to_string());
            av.status = match &chat.channel {
                Some(c) => format!("Reading Twitch chat: #{c}"),
                None => "Twitch chat off".into(),
            };
            save_keys(&[("twitch_channel", chat.channel.clone().unwrap_or_default())]);
        }
        "say" => caps.set(v, now),
        "bubbles" => {
            caps.enabled = !caps.enabled;
            av.status = format!(
                "Speech bubbles: {}",
                if caps.enabled { "on" } else { "off" }
            );
            save_keys(&[("bubbles", caps.enabled.to_string())]);
        }
        "caption_file" => {
            caps.file = (!v.trim().is_empty()).then(|| std::path::PathBuf::from(v.trim()));
            caps.source = match &caps.file {
                Some(f) => format!("caption file {}", f.display()),
                None => "control page (say:...)".into(),
            };
            save_keys(&[("caption_file", v.trim().to_string())]);
        }
        "retro" => {
            match v.parse::<u32>() {
                Ok(n) => {
                    stages.scale = n.clamp(2, 12);
                    stages.retro = true;
                }
                Err(_) => stages.retro = !stages.retro,
            }
            av.status = format!(
                "Retro sprite mode: {}",
                if stages.retro {
                    format!("on, {} px pixels", stages.scale)
                } else {
                    "off".into()
                }
            );
            save_keys(&[
                ("retro", stages.retro.to_string()),
                ("retro_scale", stages.scale.to_string()),
            ]);
        }
        _ => return false,
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_cmd(
    cmd: Cmd,
    av: &mut Avatar,
    lip: &mut Lip,
    time: &Time,
    commands: &mut Commands,
    asset_server: &AssetServer,
    clear: &mut ClearColor,
    cam: &mut webcam::Webcam,
    players: &mut Players,
) {
    let now = time.elapsed_secs();
    match cmd {
        Cmd::Expression(i) => {
            av.expression = i;
            av.face_override = None;
            av.status = format!("Face: {}", EXPRESSIONS[i].name);
        }
        Cmd::Play(name) => play_gesture(av, &name, false, players),
        Cmd::Own(i) => {
            if let Some(name) = av.own_clips.get(i).cloned() {
                play_gesture(av, &name, false, players);
            } else {
                av.status = "This character has no clip on that key".into();
            }
        }
        Cmd::Loop(name) => match name {
            Some(n) if av.clips.contains_key(&n) => {
                av.status = format!("Body loop: {n}");
                av.base_loop = Some(n);
            }
            Some(n) => av.status = format!("No '{n}' clip on this character"),
            None => {
                av.base_loop = None;
                av.status = "Body loop: Idle".into();
            }
        },
        Cmd::Emote(e) => {
            av.emote = Some((e, now));
            av.status = format!("Emote: {}", e.name());
        }
        Cmd::Lip(m) => {
            let next = m.unwrap_or_else(|| {
                let i = LIP_MODES
                    .iter()
                    .position(|x| *x == lip.picker.mode)
                    .unwrap_or(0);
                LIP_MODES[(i + 1) % LIP_MODES.len()]
            });
            lip.picker.mode = next;
            av.status = format!("Lip sync: {}", next.name());
        }
        Cmd::Reactions => {
            av.reactions = !av.reactions;
            av.asleep = false;
            av.status = format!(
                "Voice reactions: {}",
                if av.reactions { "on" } else { "off" }
            );
        }
        Cmd::Bop => {
            av.bop = !av.bop;
            av.status = format!("Bop to the beat: {}", if av.bop { "on" } else { "off" });
        }
        Cmd::ModelTo(name) => {
            let Some(i) = av.models.iter().position(|m| {
                m.1 == name || m.0.eq_ignore_ascii_case(&name) || m.1.ends_with(&format!("/{name}"))
            }) else {
                av.status = format!("No character called {name}");
                return;
            };
            if i != av.model {
                av.model = i;
                if let Some(old) = av.root.take() {
                    commands.entity(old).despawn();
                }
                begin_load(av, asset_server);
            }
        }
        Cmd::Model(step) => {
            let n = av.models.len() as i32;
            av.model = ((av.model as i32 + step).rem_euclid(n)) as usize;
            if let Some(old) = av.root.take() {
                commands.entity(old).despawn();
            }
            begin_load(av, asset_server);
        }
        Cmd::Background => {
            let order = if av.transparent_window {
                vec![
                    Background::Transparent,
                    Background::Green,
                    Background::Magenta,
                    Background::Studio,
                ]
            } else {
                vec![Background::Green, Background::Magenta, Background::Studio]
            };
            let i = order.iter().position(|b| *b == av.background).unwrap_or(0);
            av.background = order[(i + 1) % order.len()];
            clear.0 = av.background.color();
            av.status = format!("Background: {}", av.background.name());
        }
        Cmd::BackgroundTo(b) => {
            if b != Background::Transparent || av.transparent_window {
                av.background = b;
                clear.0 = av.background.color();
            }
        }
        Cmd::FramingTo(f) => {
            av.framing = f;
            av.status = [
                "Framing: head and shoulders",
                "Framing: waist up",
                "Framing: full body",
            ][f]
                .into();
        }
        Cmd::Framing => {
            av.framing = (av.framing + 1) % 3;
            av.status = [
                "Framing: head and shoulders",
                "Framing: waist up",
                "Framing: full body",
            ][av.framing]
                .into();
        }
        Cmd::Hud => av.hud = !av.hud,
        Cmd::Mute => {
            av.muted = !av.muted;
            av.status = if av.muted {
                "Mic muted".into()
            } else {
                "Mic live".into()
            };
        }
        Cmd::Calibrate => {
            lip.floor.reset();
            lip.calibrate_until = now + 1.5;
            av.status = "Calibrating: stay quiet for a moment".into();
        }
        Cmd::Gate(d) => {
            let g = (lip.picker.settings.gate_db + d).clamp(-80.0, -10.0);
            lip.picker.settings.gate_db = g;
            av.status = format!("Gate {g:.0} dB");
        }
        Cmd::Webcam => {
            let on = !cam.enabled;
            webcam::set_enabled(cam, on, commands);
            av.status = format!(
                "Webcam output: {}",
                if on { cam.sink_name() } else { "off".into() }
            );
        }
        Cmd::BodyLanguage => {
            av.prosody_on = !av.prosody_on;
            av.status = format!(
                "Body language from your voice: {}",
                if av.prosody_on { "on" } else { "off" }
            );
        }
        Cmd::Vowels => {
            lip.vcal = Some(VowelCal {
                stage: 0,
                stage_start: now,
                cal: VowelCalibrator::default(),
                got: vec![],
            });
        }
        Cmd::Formants => {
            lip.picker.use_formants = !lip.picker.use_formants;
            av.status = format!(
                "Vowel reading from formants: {}",
                if lip.picker.use_formants { "on" } else { "off" }
            );
        }
        Cmd::AutoTalk => {
            av.auto_talk = !av.auto_talk;
            av.status = format!(
                "Talk gestures while speaking: {}",
                if av.auto_talk { "on" } else { "off" }
            );
        }
    }
    save_settings(av, lip, cam);
}

/// Play any clip once, then fall back to the base loop. `interruptible` clips (fidgets) stop as
/// soon as you start talking.
pub(crate) fn play_gesture(
    av: &mut Avatar,
    name: &str,
    interruptible: bool,
    players: &mut Players,
) {
    let Some((node, _)) = av.clips.get(name).cloned() else {
        av.status = format!("No '{name}' clip on this character");
        return;
    };
    let Some(pe) = av.player else { return };
    if let Ok((mut player, mut tr)) = players.get_mut(pe) {
        tr.play(&mut player, node, Duration::from_millis(250));
        av.body = name.to_string();
        av.gesture_active = true;
        av.gesture_interruptible = interruptible;
        av.gesture_until = f32::MAX; // set from the clip length in drive_body
        if !interruptible {
            av.status = format!("Play: {name}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert!(matches!(parse_cmd("face:0"), Some(Cmd::Expression(0))));
        assert!(parse_cmd(&format!("face:{}", EXPRESSIONS.len())).is_none());
        assert!(matches!(
            parse_cmd("emote:nod"),
            Some(Cmd::Emote(Emote::Nod))
        ));
        assert!(parse_cmd("emote:moonwalk").is_none());
        assert!(matches!(parse_cmd("model"), Some(Cmd::Model(1))));
        assert!(matches!(parse_cmd("model:prev"), Some(Cmd::Model(-1))));
        assert!(matches!(parse_cmd("model:Medic"), Some(Cmd::ModelTo(n)) if n == "Medic"));
        // a bare bg / frame cycles, with a value it sets
        assert!(matches!(parse_cmd("bg"), Some(Cmd::Background)));
        assert!(matches!(
            parse_cmd("bg:green"),
            Some(Cmd::BackgroundTo(Background::Green))
        ));
        assert!(parse_cmd("bg:plaid").is_none());
        assert!(matches!(parse_cmd("frame"), Some(Cmd::Framing)));
        assert!(matches!(parse_cmd("frame:2"), Some(Cmd::FramingTo(2))));
        assert!(parse_cmd("frame:3").is_none());
        assert!(matches!(parse_cmd("gate:-2"), Some(Cmd::Gate(g)) if g == -2.0));
        assert!(parse_cmd("gate:loud").is_none());
        assert!(matches!(parse_cmd("play:Victory"), Some(Cmd::Play(n)) if n == "Victory"));
        assert!(parse_cmd("nonsense").is_none());
    }
}
