//! Building the app's resources from the command line and the settings file: the avatar,
//! lip sync, chat and its feeds, viewer questions and the ask page, the webcam, the show, the
//! studio, and the rest. `main` calls [`configure`] before adding the plugins.

#[allow(unused_imports)]
use crate::*;

/// Read the settings, start what runs on its own threads (the mic, the control page, the ask
/// page, chat feeds) and insert every resource the plugins' systems use.
pub(crate) fn configure(app: &mut App, args: &Args) {
    // no settings yet: this is the first run, so the control page opens on the setup steps
    let first_run = !settings_path().exists();
    let saved = load_settings();
    let models = find_models();
    if models.is_empty() {
        crate::problem!(
            "No models found in {}. Copy the .glb files from block-cast.zip there.",
            asset_base().join(MODELS_DIR).display()
        );
        std::process::exit(1);
    }
    let want = args
        .model
        .clone()
        .or_else(|| saved.get("model").cloned())
        .unwrap_or_default();
    let model = models
        .iter()
        .position(|m| !want.is_empty() && (m.1.ends_with(want.trim_start_matches("models/"))))
        .unwrap_or(0);
    let mut background = saved
        .get("background")
        .and_then(|b| Background::parse(b))
        .unwrap_or(Background::Green);
    if background == Background::Transparent && !args.transparent {
        background = Background::Green;
    }
    if args.transparent {
        background = Background::Transparent;
    }
    let mut settings = PickerSettings::default();
    if let Some(g) = saved.get("gate_db").and_then(|g| g.parse().ok()) {
        settings.gate_db = g;
    }
    let lip_mode = saved
        .get("lip_mode")
        .and_then(|m| LipMode::parse(m))
        .unwrap_or(LipMode::Visemes);
    let mut picker = LipSync::new(settings, lip_mode);
    picker.use_formants = saved.get("formants").map(|v| v != "false").unwrap_or(true);
    if let Some(r) = saved.get("vowels").and_then(|v| VowelRefs::from_setting(v)) {
        picker.refs = r;
    }
    let avatar = Avatar {
        models: models.clone(),
        model,
        gltf: None,
        root: None,
        player: None,
        clips: HashMap::new(),
        clip_order: vec![],
        own_clips: vec![],
        graph: None,
        pending_scene: false,
        body: "Idle".into(),
        base_loop: None,
        gesture_active: false,
        gesture_until: 0.0,
        gesture_interruptible: false,
        auto_talk: saved.get("auto_talk").map(|v| v != "false").unwrap_or(true),
        speaking_since: None,
        silent_since: 0.0,
        expression: saved
            .get("expression")
            .and_then(|n| EXPRESSIONS.iter().position(|e| e.name == n))
            .unwrap_or(0),
        mouth: Mouth::Rest,
        level: 0.0,
        reactions: saved.get("reactions").map(|v| v != "false").unwrap_or(true),
        prosody_on: saved
            .get("body_language")
            .map(|v| v != "false")
            .unwrap_or(true),
        talk_beat_t: -10.0,
        talk_beat_amp: 0.0,
        talk_beat_left: false,
        question_until: 0.0,
        tilt_side: 1.0,
        brows_up: false,
        liveliness: 1.0,
        listen_to: 0.0,
        listen: 0.0,
        hush: false,
        two_shot: false,
        laugh_until: 0.0,
        singing: false,
        cohost_on: false,
        remote_guest: false,
        look_left_to: 0.0,
        look_left: 0.0,
        cut: false,
        last_hit: -100.0,
        mic_switch: None,
        setup_done: !first_run || saved.get("setup_done").is_some_and(|v| v == "true"),
        sleep_after: saved
            .get("sleep_after")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30.0),
        asleep: false,
        face_override: None,
        loud_since: None,
        next_fidget: 20.0,
        emote: None,
        bop: saved.get("bop").map(|v| v == "true").unwrap_or(false),
        last_beat: -10.0,
        beat_side: false,
        last_onsets: 0,
        next_blink: 2.0,
        blink_until: 0.0,
        dart_side: false,
        blink_again: -1.0,
        glance: None,
        next_saccade: 3.0,
        head_squash: (-100.0, 0.0),
        idle_life: saved.get("idle_life").map(|v| v != "false").unwrap_or(true),
        next_idle: 10.0,
        emote_prev: None,
        presets: std::fs::read_to_string(applog::user_dir().join("presets.json"))
            .ok()
            .and_then(|t| {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok()
            })
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default(),
        next_dart: 1.0,
        rng: 0x9E3779B97F4A7C15,
        background,
        transparent_window: args.transparent,
        framing: saved
            .get("framing")
            .and_then(|f| f.parse().ok())
            .unwrap_or(0),
        hud: saved.get("hud").map(|v| v != "false").unwrap_or(true),
        muted: false,
        status: String::new(),
    };
    let (ww, wh) = args
        .webcam_size
        .or_else(|| saved.get("webcam_size").and_then(|s| parse_size(s)))
        .unwrap_or((1280, 720));
    let wfps = args
        .webcam_fps
        .or_else(|| saved.get("webcam_fps").and_then(|f| f.parse().ok()))
        .unwrap_or(60);
    let won = args.webcam
        || args.webcam_dump.is_some()
        || saved.get("webcam").is_some_and(|v| v == "true");
    let sink = match args.webcam_dump.clone() {
        Some(p) => webcam::Sink::Dump(p),
        None => webcam::Sink::ObsVirtualCamera,
    };
    let cam = webcam::Webcam::new(won, ww, wh, wfps, sink);
    let state = control::State::default();
    // viewer questions: the queue, the public ask page, and the guest on stage
    let queue = Arc::new(Mutex::new(questions::Queue::load(
        applog::user_dir().join("questions"),
    )));
    {
        let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
        q.open = saved.get("ask_open").is_some_and(|v| v == "true");
        if let Some(m) = saved
            .get("ask_max_secs")
            .and_then(|v| v.parse::<f32>().ok())
        {
            q.limits.max_secs = m.clamp(5.0, questions::MAX_SECS_HARD);
        }
        if let Some(n) = saved.get("ask_per_viewer").and_then(|v| v.parse().ok()) {
            q.limits.per_user_total = n;
        }
    }
    let ask_chars: Vec<(String, String)> =
        match saved.get("ask_characters").filter(|v| !v.trim().is_empty()) {
            Some(list) => {
                let want: Vec<String> = list.split(',').map(|x| x.trim().to_lowercase()).collect();
                models
                    .iter()
                    .filter(|m| want.contains(&m.0.to_lowercase()))
                    .cloned()
                    .collect()
            }
            None => models.clone(),
        };
    let ask = ask::start(
        ask::AskConfig {
            port: {
                let p = args
                    .ask_port
                    .or_else(|| saved.get("ask_port").and_then(|p| p.parse().ok()))
                    .unwrap_or(7778);
                control::ASK_PORT.store(p, std::sync::atomic::Ordering::Relaxed);
                p
            },
            twitch_client_id: saved
                .get("twitch_client_id")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            auth_base: saved
                .get("twitch_auth_url")
                .cloned()
                .unwrap_or_else(|| ask::TWITCH_AUTH.into()),
            scopes: saved.get("twitch_scopes").cloned().unwrap_or_default(),
            open_names: saved.get("ask_names").is_some_and(|v| v == "true"),
            characters: ask_chars,
            title: saved
                .get("ask_title")
                .cloned()
                .unwrap_or_else(|| "Ask a question".into()),
            font: std::fs::read(asset_base().join("assets/fonts/PressStart2P-Regular.ttf"))
                .ok()
                .map(Arc::new),
        },
        queue.clone(),
    );
    if let Err(e) = &ask {
        crate::problem!("[ask] {e}");
    }
    let guest = guest::Guest::new(
        queue.clone(),
        ask,
        saved
            .get("guest_mutes_host")
            .map(|v| v != "false")
            .unwrap_or(true),
        saved.get("guest_output").cloned().filter(|s| !s.is_empty()),
    );
    let (cmd_rx, url) = control::start(args.port, state.clone(), queue.clone());
    if first_run && !args.no_browser {
        applog::open_url(&format!("{url}?setup"));
    }
    let stages = stage::Stages::new(
        saved.get("retro").is_some_and(|v| v == "true"),
        saved
            .get("retro_scale")
            .and_then(|v| v.parse().ok())
            .unwrap_or(4),
        args.transparent,
    );
    // twitch_irc=host:port points chat at a stand-in server (for testing without Twitch)
    let irc = saved
        .get("twitch_irc")
        .and_then(|v| v.rsplit_once(':'))
        .and_then(|(h, p)| Some((h.to_string(), p.parse::<u16>().ok()?)));
    let mut chat = chat::Chat::with_host(
        saved
            .get("chat_throws")
            .map(|v| v != "false")
            .unwrap_or(true),
        saved.get("twitch_channel").cloned(),
        irc,
    );
    // YouTube and Kick chat (src/streamchat.rs); youtube_api / kick_pusher point them at stand-ins
    if let (Some(key), Some(video)) = (
        saved.get("youtube_key").filter(|k| !k.is_empty()),
        saved
            .get("youtube_video")
            .and_then(|v| streamchat::youtube_id(v)),
    ) {
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
                key.clone(),
                video,
                poll,
            )),
        );
    }
    if let Some(k) = saved.get("kick_channel").filter(|k| !k.trim().is_empty()) {
        chat.set_feed(
            "kick",
            Some(streamchat::start_kick(
                saved
                    .get("kick_pusher")
                    .cloned()
                    .unwrap_or_else(|| streamchat::KICK_WS.into()),
                k.trim().to_ascii_lowercase(),
            )),
        );
    }
    let mut captions = bubbles::Captions::new(
        saved.get("bubbles").map(|v| v != "false").unwrap_or(true),
        saved
            .get("caption_file")
            .filter(|f| !f.is_empty())
            .map(std::path::PathBuf::from),
    );
    let mic = match args.wav.clone() {
        Some(w) => start_wav(w),
        None => start_mic(
            args.mic
                .clone()
                .or_else(|| saved.get("mic").cloned().filter(|m| !m.is_empty())),
        ),
    };
    start_whisper(&saved, &mut captions, &mic, &queue);
    app
        .insert_resource(ClearColor(background.color()))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 600.0,
            ..default()
        })
        .insert_resource(mic)
        .insert_resource(stages)
        .insert_resource(chat)
        .insert_resource(captions)
        .insert_resource(guest)
        .insert_resource(wardrobe::Wardrobe::new(
            saved
                .get("dressup_chat")
                .map(|v| v != "false")
                .unwrap_or(true),
            saved
                .get("dressup_who")
                .and_then(|v| chat::Who::parse(v))
                .unwrap_or(chat::Who::Everyone),
            saved
                .get("dressup_minutes")
                .and_then(|v| v.parse().ok())
                .unwrap_or(10.0),
        ))
        .insert_resource(events::Moments::new(
            saved
                .get("raid_crowd")
                .map(|v| v != "false")
                .unwrap_or(true),
            saved.get("sub_party").map(|v| v != "false").unwrap_or(true),
        ))
        .insert_resource(poll::Polls::default())
        .insert_resource({
            let mut c = clips::Clips::new(
                saved.get("clips").map(|v| v != "false").unwrap_or(true),
                saved
                    .get("auto_shorts")
                    .map(|v| v != "false")
                    .unwrap_or(true),
                saved.get("comics").map(|v| v != "false").unwrap_or(true),
            );
            c.cards = saved
                .get("short_cards")
                .map(|v| v != "false")
                .unwrap_or(true);
            c
        })
        .insert_resource(show::Show::new(
            saved.get("set").cloned().unwrap_or_else(|| "none".into()),
            saved.get("director").is_some_and(|v| v == "true"),
            saved.get("press").is_some_and(|v| v == "true"),
        ))
        .insert_resource(hotkeys::Hotkeys::start())
        .insert_resource({
            let mut st = lighting::Studio::new(
                saved
                    .get("studio_light")
                    .map(|v| v != "false")
                    .unwrap_or(true),
                saved
                    .get("studio_shadows")
                    .map(|v| v != "false")
                    .unwrap_or(true),
                saved
                    .get("studio_glow")
                    .map(|v| v != "false")
                    .unwrap_or(true),
                saved.get("quality").map(|s| s.as_str()).unwrap_or("auto"),
            );
            st.grade = saved
                .get("studio_grade")
                .map(|v| v != "false")
                .unwrap_or(true);
            if let Some(b) = saved.get("studio_blur").and_then(|v| v.parse::<f32>().ok()) {
                st.blur = (b / 100.0).clamp(0.0, 1.0);
            }
            st
        })
        .init_resource::<lighting::Cues>()
        .insert_resource({
            let mut fc = facecap::FaceCap::new(
                saved.get("face_capture").map(|v| v != "false").unwrap_or(true),
                saved.get("facecap_vmc_port").and_then(|p| p.parse().ok()).unwrap_or(39539),
                saved.get("facecap_ifm_port").and_then(|p| p.parse().ok()).unwrap_or(49983),
            );
            fc.mirror = saved.get("facecap_mirror").is_some_and(|v| v == "true");
            fc
        })
        .insert_resource(particles::Particles::new(
            saved.get("effects").map(|v| v != "false").unwrap_or(true),
            saved.get("weather").map(|s| s.as_str()),
        ))
        .insert_resource(crowd::Crowd::new(
            saved
                .get("chat_crowd")
                .map(|v| v != "false")
                .unwrap_or(true),
        ))
        .insert_resource(bevel::Bevel::new(
            saved.get("bevel").map(|v| v != "false").unwrap_or(true),
            bevel::Look {
                bevel: saved
                    .get("bevel_strength")
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 100.0)
                    .unwrap_or(0.6),
                grid: saved
                    .get("bevel_grid")
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 100.0)
                    .unwrap_or(0.0),
                wobble: saved
                    .get("bevel_wobble")
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 100.0)
                    .unwrap_or(0.1),
                detail: saved
                    .get("bevel_detail")
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 100.0)
                    .unwrap_or(0.7),
                ao: 0.0,
                blend: saved
                    .get("bevel_blend")
                    .map(|v| v != "false")
                    .unwrap_or(true),
            },
        ))
        .insert_resource(fx::Fx::new(
            saved.get("sfx").map(|v| v != "false").unwrap_or(true),
            saved
                .get("sfx_volume")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(70.0)
                / 100.0,
        ))
        .insert_resource(voicefx::VoiceFx::new(
            saved.get("laugh").map(|v| v != "false").unwrap_or(true),
            saved.get("sing").map(|v| v != "false").unwrap_or(true),
            saved.get("gags").map(|v| v != "false").unwrap_or(true),
        ))
        .insert_resource(cohost::Cohost::new(
            saved.get("cohost_on").is_some_and(|v| v == "true"),
            saved
                .get("cohost_model")
                .cloned()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "models/dog.glb".into()),
            saved.get("cohost_mic").cloned().filter(|m| !m.is_empty()),
            args.cohost_wav.clone(),
        ))
        .insert_resource({
            let mut n = net::Net::new(
                saved
                    .get("relay_url")
                    .cloned()
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "http://127.0.0.1:7779".into()),
            );
            if let Some((url, code)) = &args.watch {
                if let Err(e) = n.watch(url, code) {
                    eprintln!("--watch: {e}");
                }
            }
            n.voice_output = saved.get("net_output").cloned().filter(|v| !v.is_empty());
            if let Some((url, code, name)) = &args.join {
                if let Err(e) = n.join(url, code, name) {
                    eprintln!("--join: {e}");
                }
            }
            n
        })
        .insert_resource(world::WorldClient::new(args.world.clone(), args.name.clone()))
        .init_resource::<outdoors::Outdoors>()
        .init_resource::<Inject>()
        .init_resource::<startup::Preload>()
        .insert_resource(Lip {
            picker: picker,
            floor: NoiseFloor::new(),
            calibrate_until: 1.5,
            last_frames: 0,
            prosody: Prosody::default(),
            vcal: None,
        })
        .insert_resource(state)
        .insert_resource(cam)
        .insert_resource(avatar)
        .insert_resource(control::Commands(Mutex::new(cmd_rx), url));
}
