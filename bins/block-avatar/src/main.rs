#![recursion_limit = "256"]
// The Windows release build is a windowed app: no console window behind the avatar.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Block Avatar: drive the block-style characters from your microphone, for streaming (OBS).
//!
//! The mic moves the mouth (five lip-sync styles, see lipsync.rs), the eyes blink on their own,
//! the head nods with your voice, and the body switches between Idle and Talk as you speak.
//! Optional voice reactions (shout face, dozing off, fidgets), procedural emotes layered on top of
//! any body clip, a beat-bop mode, and every clip in the model playable or loopable. Expressions and gestures come from hotkeys or from a small control page served on
//! http://127.0.0.1:7777 that you can add to OBS as a Custom Browser Dock.
//!
//! Run:   cargo run --release
//! Flags: --model <file.glb> --mic <part of device name> --list-mics --transparent --port <n>
//!        --wav <file.wav>   (drive the mouth from a recording instead of the mic, looped)
//!        --analyze <in.wav> <out.csv> [mode]   (offline lip-sync test, no window)
//!        --webcam [--webcam-size 1280x720] [--webcam-fps 60]   (feed OBS Virtual Camera, Windows)
//!        --webcam-dump <file.nv12>   (write the webcam frames to a raw file instead, for testing)
//!        --ask-port <n>   (viewer question page, default 7778; see ask.rs)
//!        --cohost-wav <file.wav>   (drive the co-host from a recording, for testing)
//!
//! Written against Bevy 0.19.1 and cpal 0.17.

// The app's modules, grouped by what they do; each is also reachable as crate::<module>.
mod app;
mod settings;
mod cli;
mod model;
mod frame;
mod world_glue;
#[allow(unused_imports)]
pub use settings::*;
#[allow(unused_imports)]
pub use cli::*;
#[allow(unused_imports)]
pub use model::*;
#[allow(unused_imports)]
pub use frame::*;
#[allow(unused_imports)]
pub use world_glue::*;

use app::{applog, sim, startup};
mod sound;
use sound::{audio, netvoice, voicefx};
// the voice analysis is its own crate (pure Rust, quick to test)
use avatar_voice::{detect, lipsync, voice};
mod character;
use character::{face, facecap, wardrobe, jiggle, puppet, bevel};
mod scene;
use scene::{camera, lighting, stage, sets, show, particles, fx, bubbles, crowd};
mod audience;
use audience::{chat, streamchat, events, poll, ask, questions, guest, cohost};
mod controls;
use controls::{commands, control, hotkeys};
mod output;
use output::{webcam, clips, cards, snapshot};
mod network;
use network::net;
pub use vox_world::{blocks, outdoors, portals, proto};
pub use vox_world::client as world;

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use bevy::{
    app::AnimationSystems,
    core_pipeline::tonemapping::Tonemapping,
    gltf::Gltf,
    prelude::*,
    transform::TransformSystems,
    window::CompositeAlphaMode,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use audio::*;
use camera::*;
use commands::*;
use face::*;
use lipsync::{
    Analyzer, Frame, LIP_MODES, LipMode, LipSync, Mouth, NoiseFloor, PickerSettings, level01,
};
use voice::{Prosody, VowelCalibrator, VowelRefs};

const SETTINGS_FILE: &str = "avatar_settings.txt";

/// avatar_settings.txt, in your folder (applog.rs explains where that is).
fn settings_path() -> std::path::PathBuf {
    applog::user_dir().join(SETTINGS_FILE)
}
const MODELS_DIR: &str = "assets/models";
// ------------------------------------------------------------------------------------------
// App state
// ------------------------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Background {
    Green,
    Magenta,
    Studio,
    Transparent,
}

impl Background {
    fn color(self) -> Color {
        match self {
            Background::Green => Color::srgb_u8(0, 177, 64),
            Background::Magenta => Color::srgb_u8(255, 0, 255),
            Background::Studio => Color::srgb(0.13, 0.13, 0.15),
            Background::Transparent => Color::NONE,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Background::Green => "green",
            Background::Magenta => "magenta",
            Background::Studio => "studio",
            Background::Transparent => "transparent",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        [
            Background::Green,
            Background::Magenta,
            Background::Studio,
            Background::Transparent,
        ]
        .into_iter()
        .find(|b| b.name() == s)
    }
}

#[derive(Resource)]
struct Avatar {
    models: Vec<(String, String)>, // (label, path under assets/)
    model: usize,
    gltf: Option<Handle<Gltf>>,
    root: Option<Entity>,
    player: Option<Entity>,
    clips: HashMap<String, (AnimationNodeIndex, Handle<AnimationClip>)>,
    /// clip names in file order (shared clips first), for the control page
    clip_order: Vec<String>,
    own_clips: Vec<String>,
    graph: Option<Handle<AnimationGraph>>,
    pending_scene: bool,
    // body
    body: String,
    /// what the body loops when you're not talking ("Idle" unless set with loop:<clip>)
    base_loop: Option<String>,
    gesture_active: bool,
    gesture_until: f32,
    /// fidgets give way as soon as you speak
    gesture_interruptible: bool,
    auto_talk: bool,
    speaking_since: Option<f32>,
    silent_since: f32,
    // face
    expression: usize,
    mouth: Mouth,
    level: f32,
    // reactions
    reactions: bool,
    sleep_after: f32,
    asleep: bool,
    face_override: Option<(usize, f32)>,
    loud_since: Option<f32>,
    next_fidget: f32,
    // layered motion
    emote: Option<(Emote, f32)>,
    bop: bool,
    last_beat: f32,
    beat_side: bool,
    last_onsets: u32,
    next_blink: f32,
    /// a second blink right after the first (now and then)
    blink_again: f32,
    /// the eyes flick somewhere for a moment: (eyes part, until)
    glance: Option<(&'static str, f32)>,
    next_saccade: f32,
    /// a hit squashes the head: (when, how much)
    head_squash: (f32, f32),
    /// idle life (look around, stretch ...) and when the next one may come
    idle_life: bool,
    next_idle: f32,
    /// the emote that was cut off by a new one, still fading out: (emote, start, cut at)
    emote_prev: Option<(Emote, f32, f32)>,
    /// saved scene preset names
    presets: Vec<String>,
    blink_until: f32,
    dart_side: bool,
    next_dart: f32,
    rng: u64,
    // presentation
    background: Background,
    transparent_window: bool,
    framing: usize,
    hud: bool,
    muted: bool,
    status: String,
    // body language from pitch and stress
    prosody_on: bool,
    talk_beat_t: f32,
    talk_beat_amp: f32,
    talk_beat_left: bool,
    question_until: f32,
    tilt_side: f32,
    brows_up: bool,
    liveliness: f32,
    // a viewer's question on stage (guest.rs)
    /// 1 while a guest is talking or waiting for the answer: turn to look at them
    listen_to: f32,
    listen: f32,
    /// ignore the mic while the guest's recording plays (speaker spill)
    hush: bool,
    /// widen the shot to fit the guest
    two_shot: bool,
    /// laughing until then (detect.rs)
    laugh_until: f32,
    singing: bool,
    /// a live co-host stands on your left (cohost.rs)
    cohost_on: bool,
    /// a guest from the network stands there instead (net.rs)
    remote_guest: bool,
    /// 1 while the co-host talks and you don't: glance at them
    look_left_to: f32,
    look_left: f32,
    /// the next camera move is a cut, not a glide (auto-director)
    cut: bool,
    /// when something last splatted on the face
    last_hit: f32,
    /// a different microphone was picked (setup page)
    mic_switch: Option<String>,
    /// the first-run setup steps are finished
    setup_done: bool,
}

/// Guided vowel calibration: hold "ah", then "ee", then "oo".
struct VowelCal {
    stage: usize,
    stage_start: f32,
    cal: VowelCalibrator,
    got: Vec<(f32, f32)>,
}
const VOWEL_PROMPTS: [&str; 3] = [
    "AAH (as in 'father')",
    "EEE (as in 'see')",
    "OOO (as in 'food')",
];
const VOWEL_STAGE_S: f32 = 2.5;

#[derive(Resource)]
struct Lip {
    picker: LipSync,
    floor: NoiseFloor,
    calibrate_until: f32,
    last_frames: u32,
    prosody: Prosody,
    vcal: Option<VowelCal>,
}

#[derive(Component)]
struct FacePart(&'static str);
/// Joints the layered motion touches after the animation step.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Joint {
    Root,
    Hips,
    Head,
    Leg,
    ArmL,
    ArmR,
}
#[derive(Component)]
struct HudText;

/// Commands the app sends itself (a sub plays Victory, a raid throws confetti...), handled like
/// the control page's.
#[derive(Resource, Default)]
pub struct Inject(pub Vec<String>);
#[derive(Component)]
pub(crate) struct MainCamera;

impl Avatar {
    fn rand01(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The folder holding `assets/`: the working directory if it has one (cargo run, or a shortcut
/// that starts in the app folder), otherwise next to the executable or a few folders above it
/// (an installed copy keeps assets beside the exe; a build in target/ finds the repository).
fn asset_base() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join(MODELS_DIR).is_dir() {
        return cwd;
    }
    let exe = std::env::current_exe().ok();
    let found = exe
        .iter()
        .flat_map(|p| p.ancestors().skip(1).take(8))
        .find(|d| d.join(MODELS_DIR).is_dir())
        .map(|d| d.to_path_buf());
    found.or_else(|| exe.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()))).unwrap_or(cwd)
}

fn find_models() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(asset_base().join(MODELS_DIR))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| {
                    n.ends_with(".glb") && !n.contains("stepped") && n != "character_rigged.glb"
                })
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|n| {
            let label = if n == "scene_all.glb" {
                "Lead".to_string()
            } else if n == "blue.glb" {
                "Blue".to_string()
            } else {
                let stem = n.trim_end_matches(".glb").replace('_', " ");
                let mut c = stem.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or(stem)
            };
            (label, format!("models/{n}"))
        })
        .collect();
    // Blue, the main character, first: who a first run starts as
    v.sort_by(|a, b| (a.0 != "Blue", &a.0).cmp(&(b.0 != "Blue", &b.0)));
    v
}

fn main() {
    applog::attach_console_for_cli();
    // the world code reads the game's files from here, and logs through ours
    vox_world::files::set_base(asset_base());
    vox_world::files::set_user_dir(applog::user_dir());
    vox_world::files::set_logger(|s| applog::write(false, s));
    let Some(args) = parse_args() else { return };
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

    let mut app = App::new();
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
        .insert_resource(control::Commands(Mutex::new(cmd_rx), url))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_base().join("assets").to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Vox".into(),
                        resolution: (900, 900).into(),
                        transparent: args.transparent,
                        composite_alpha_mode: if args.transparent {
                            CompositeAlphaMode::PostMultiplied
                        } else {
                            CompositeAlphaMode::Auto
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(sim::SimPlugin)
        .add_plugins((
            MaterialPlugin::<blocks::BlockMaterial>::default(),
            MaterialPlugin::<blocks::WaterMaterial>::default(),
            MaterialPlugin::<outdoors::SkyMaterial>::default(),
            MaterialPlugin::<blocks::FlameMaterial>::default(),
            MaterialPlugin::<outdoors::FloorMaterial>::default(),
            MaterialPlugin::<outdoors::RainMaterial>::default(),
            MaterialPlugin::<portals::PortalMaterial>::default(),
        ))
        .add_plugins(bevy::ui_render::prelude::UiMaterialPlugin::<
            stage::RetroMaterial,
        >::default())
        .add_systems(
            Startup,
            (
                setup_scene,
                load_model,
                webcam::setup,
                clips::setup,
                stage::setup,
                bubbles::spawn,
                poll::spawn_labels,
                startup::spawn_card,
                chat::setup,
                fx::setup,
                lighting::setup,
                particles::setup,
                blocks::setup,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                handle_input,
                hotkeys::pump,
                handle_remote,
                startup::switch_mic,
                guest::update,
                update_lipsync,
                build_graph_when_loaded,
                drive_body,
                update_camera,
                webcam::follow,
                clips::update,
                stage::update,
                update_hud,
                publish_state,
                startup::update_card,
                startup::preload,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                chat::launch,
                wardrobe::tag_heads,
                wardrobe::update,
                wardrobe::swap_heads,
                events::update,
                poll::update,
                voicefx::update,
                cohost::update,
                show::update,
                puppet::load,
                puppet::animate,
                chat::fly,
                apply_hits,
                fx::update,
                bubbles::poll,
                bubbles::layout,
            )
                .chain()
                .after(update_camera),
        )
        .add_systems(
            PostUpdate,
            (drive_face, guest::drive_face, puppet::faces)
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .add_systems(PostUpdate, bevel::apply)
        .add_systems(
            Update,
            (net::receive, particles::take_cues, net::take_cues, lighting::update).chain(),
        )
        .add_systems(
            Update,
            (
                net::find_body,
                net::hold_still,
                net::load_remotes,
                net::voice,
                net::stage.after(cohost::update),
            ),
        )
        .add_systems(
            PostUpdate,
            (net::send, net::apply, net::place_guest)
                .chain()
                .after(drive_face)
                .after(AnimationSystems)
                .before(jiggle::update)
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            FixedUpdate,
            (particles::step, crowd::step, fx::step, chat::fade),
        )
        .add_systems(Update, facecap::update)
        .add_systems(Update, sets::build)
        .init_resource::<vox_world::WorldRender>()
        .init_resource::<vox_world::LocalAvatar>()
        // walking and building in a world read the same actions as the player client
        .add_plugins(vox_world::input::InputPlugin)
        .add_systems(
            Update,
            (world_in, world::receive, world::attach, world::controls, world_out, world::remesh, world::hud)
                .chain()
                .before(update_camera),
        )
        .add_systems(Update, world::camera.after(update_camera).after(net::receive))
        .add_systems(Update, world::weather.after(world::receive))
        .add_systems(Update, (outdoors::update, outdoors::weather_look, world_out).chain().after(world::camera).after(lighting::update))
        .add_systems(Update, portals::update.after(world::camera).after(world::receive))
        .init_resource::<portals::Portals>()
        .add_systems(
            PostUpdate,
            (world::send_pose, world::apply_others)
                .chain()
                .after(net::place_guest)
                .before(jiggle::update)
                .before(TransformSystems::Propagate),
        )
        .add_systems(PostUpdate, lighting::blur.after(TransformSystems::Propagate))
        .add_systems(
            Update,
            (
                idle_life,
                jiggle::attach,
                crowd::update,
                crowd::signs.after(crowd::update),
            ),
        )
        .add_systems(
            PostUpdate,
            jiggle::update
                .after(drive_face)
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .add_systems(Update, snapshot::run.after(startup::update_card));
    if let Some((out, set, fx)) = args.snapshot.clone() {
        // every frame exactly one simulation tick, however fast the test machine renders
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / sim::HZ),
        ));
        app.insert_resource(snapshot::Snapshot::new(out.into(), set, fx));
    }
    app.run();
}
