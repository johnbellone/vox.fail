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

mod applog;
pub use vox_world::{blocks, outdoors, portals, proto};
pub use vox_world::client as world;
mod sets;
mod audio;
mod ask;
mod bevel;
mod bubbles;
mod camera;
mod cards;
mod chat;
mod clips;
mod cohost;
mod commands;
mod control;
mod crowd;
mod detect;
mod events;
mod face;
mod facecap;
mod fx;
mod guest;
mod hotkeys;
mod jiggle;
mod lighting;
mod lipsync;
mod particles;
mod poll;
mod puppet;
mod questions;
mod show;
mod net;
mod netvoice;
mod sim;
mod snapshot;
mod stage;
mod startup;
mod streamchat;
#[cfg(test)]
mod tests_real;
mod voice;
mod voicefx;
mod wardrobe;
mod webcam;

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
/// that starts in the app folder), otherwise next to the executable.
fn asset_base() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join(MODELS_DIR).is_dir() {
        return cwd;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or(cwd)
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
    v.sort_by(|a, b| (a.0 != "Lead", &a.0).cmp(&(b.0 != "Lead", &b.0)));
    v
}

// ------------------------------------------------------------------------------------------
// Settings file (plain key=value so it's easy to edit by hand)
// ------------------------------------------------------------------------------------------
fn load_settings() -> HashMap<String, String> {
    std::fs::read_to_string(settings_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Update some keys in the settings file, keeping the rest.
fn save_keys(pairs: &[(&str, String)]) {
    let mut m: Vec<(String, String)> = std::fs::read_to_string(settings_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    for (k, v) in pairs {
        match m.iter_mut().find(|e| e.0 == *k) {
            Some(e) => e.1 = v.clone(),
            None => m.push((k.to_string(), v.clone())),
        }
    }
    let _ = std::fs::write(
        settings_path(),
        m.iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>(),
    );
}

fn save_settings(av: &Avatar, lip: &Lip, cam: &webcam::Webcam) {
    let s = format!(
        "model={}\nexpression={}\nbackground={}\nframing={}\ngate_db={:.1}\nauto_talk={}\nhud={}\nlip_mode={}\nreactions={}\nbop={}\nsleep_after={:.0}\nwebcam={}\nwebcam_size={}x{}\nwebcam_fps={}\nbody_language={}\nformants={}\nvowels={}\n",
        av.models.get(av.model).map(|m| m.1.as_str()).unwrap_or(""),
        EXPRESSIONS[av.expression].name,
        av.background.name(),
        av.framing,
        lip.picker.settings.gate_db,
        av.auto_talk,
        av.hud,
        lip.picker.mode.name(),
        av.reactions,
        av.bop,
        av.sleep_after,
        cam.enabled,
        cam.width,
        cam.height,
        cam.fps,
        av.prosody_on,
        lip.picker.use_formants,
        lip.picker.refs.to_setting()
    );
    let pairs: Vec<(String, String)> = s
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let refs: Vec<(&str, String)> = pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    save_keys(&refs);
}

/// Deterministic pseudo-random 0..1 from an index and a time, for scattering confetti.
fn hash01(i: u32, t: f32) -> f32 {
    let mut x = i.wrapping_mul(2654435761) ^ t.to_bits().wrapping_mul(40503);
    x ^= x >> 15;
    x = x.wrapping_mul(2246822519);
    x ^= x >> 13;
    (x & 0xFFFF) as f32 / 65535.0
}

// ------------------------------------------------------------------------------------------
// main
// ------------------------------------------------------------------------------------------
struct Args {
    model: Option<String>,
    mic: Option<String>,
    wav: Option<String>,
    transparent: bool,
    port: u16,
    webcam: bool,
    webcam_size: Option<(u32, u32)>,
    webcam_fps: Option<u32>,
    webcam_dump: Option<String>,
    ask_port: Option<u16>,
    cohost_wav: Option<String>,
    no_browser: bool,
    /// --snapshot <png> [--snapshot-set <set>] [--snapshot-fx <burst>]: one picture, then quit
    /// (visual tests)
    snapshot: Option<(String, Option<String>, Option<String>)>,
    /// --watch http://relay:7779/<session> [--code ABC123]: watch someone's show
    watch: Option<(String, String)>,
    /// --join http://relay:7779/<session> --code ABC123 [--name Sam]: be a show's guest
    join: Option<(String, String, String)>,
    /// --world ws://host:7780 [--name Sam]: walk and build in a Vox world
    world: Option<String>,
    name: String,
}

fn parse_args() -> Option<Args> {
    let a: Vec<String> = std::env::args().collect();
    let get = |flag: &str| {
        a.iter()
            .position(|x| x == flag)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    if a.iter().any(|x| x == "--list-mics") {
        list_mics();
        return None;
    }
    if let Some(i) = a.iter().position(|x| x == "--analyze") {
        let (inp, out) = (
            a.get(i + 1).cloned().unwrap_or_default(),
            a.get(i + 2).cloned().unwrap_or("lipsync.csv".into()),
        );
        let mode = a
            .get(i + 3)
            .and_then(|m| LipMode::parse(m))
            .unwrap_or(LipMode::Visemes);
        let formants = !a.iter().any(|x| x == "--no-formants");
        if let Err(e) = analyze_file(&inp, &out, mode, formants) {
            eprintln!("analyze failed: {e}");
            std::process::exit(1);
        }
        return None;
    }
    Some(Args {
        model: get("--model"),
        mic: get("--mic"),
        wav: get("--wav"),
        transparent: a.iter().any(|x| x == "--transparent"),
        port: get("--port").and_then(|p| p.parse().ok()).unwrap_or(7777),
        webcam: a.iter().any(|x| x == "--webcam"),
        webcam_size: get("--webcam-size").and_then(|s| parse_size(&s)),
        webcam_fps: get("--webcam-fps").and_then(|f| f.parse().ok()),
        ask_port: get("--ask-port").and_then(|p| p.parse().ok()),
        cohost_wav: get("--cohost-wav"),
        no_browser: a.iter().any(|x| x == "--no-browser")
            || std::env::var_os("BLOCK_AVATAR_NO_BROWSER").is_some(),
        webcam_dump: get("--webcam-dump"),
        snapshot: get("--snapshot").map(|p| (p, get("--snapshot-set"), get("--snapshot-fx"))),
        watch: get("--watch").map(|u| (u, get("--code").unwrap_or_default())),
        join: get("--join").map(|u| (u, get("--code").unwrap_or_default(), get("--name").unwrap_or_default())),
        world: get("--world"),
        name: get("--name")
            .or_else(|| std::env::var("USER").ok())
            .or_else(|| std::env::var("USERNAME").ok())
            .unwrap_or_else(|| "Player".into()),
    })
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Offline mode: run the same analyzer and picker over a WAV and write a 60 fps viseme timeline.
fn analyze_file(inp: &str, out: &str, mode: LipMode, formants: bool) -> Result<(), String> {
    let (rate, ch, samples) = lipsync::read_wav(inp)?;
    let mut an = Analyzer::new(rate);
    let mut frames = Vec::new();
    an.process(&samples, ch, |f| frames.push(f));
    // calibrate the gate from the quietest stretch, as the live app does at startup
    let mut nf = NoiseFloor::new();
    for f in &frames {
        nf.push(f.db);
    }
    let floor = nf.finish().unwrap_or(-60.0);
    let mut settings = PickerSettings::default();
    settings.gate_db = (floor + 6.0).clamp(-70.0, -20.0);
    let mut picker = LipSync::new(settings, mode);
    picker.use_formants = formants;
    // use your calibrated vowels if the app has saved some
    if let Some(r) = load_settings()
        .get("vowels")
        .and_then(|v| VowelRefs::from_setting(v))
    {
        picker.refs = r;
        crate::note!("using calibrated vowels from {}", settings_path().display());
    }
    let mut csv = String::from(
        "time_s,db,bright,low,high,zcr,pitch_hz,clarity,f1,f2,mouth,level,pitch_st,event\n",
    );
    let mut prosody = Prosody::default();
    let total = frames.len() as f32 * 0.01;
    let mut t = 0.0f32;
    while t < total {
        let f = frames[((t / 0.01) as usize).min(frames.len() - 1)];
        let v = picker.update(f, 1.0 / 60.0);
        let vf = voice::VoiceFrame {
            pitch_hz: f.pitch_hz,
            clarity: f.clarity,
            f1: f.f1,
            f2: f.f2,
        };
        let ev = prosody.update(
            vf,
            f.db,
            f.onsets,
            1.0 / 60.0,
            picker.settings.gate_db,
            picker.speech_db,
        );
        let event = match (ev.question, ev.beat) {
            (true, _) => "question".to_string(),
            (_, Some(s)) => format!("beat {s:.2}"),
            _ => String::new(),
        };
        csv += &format!(
            "{t:.4},{:.1},{:.3},{:.3},{:.3},{:.3},{:.1},{:.2},{:.0},{:.0},{},{:.3},{:.1},{}\n",
            f.db,
            f.bright,
            f.low,
            f.high,
            f.zcr,
            f.pitch_hz,
            f.clarity,
            f.f1,
            f.f2,
            v.label(),
            level01(f, &picker.settings),
            prosody.rel_st,
            event
        );
        t += 1.0 / 60.0;
    }
    std::fs::write(out, csv).map_err(|e| e.to_string())?;
    println!(
        "{} mode, gate {:.1} dB (floor {:.1}); wrote {out}",
        mode.name(),
        settings.gate_db,
        floor
    );
    Ok(())
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
        let mut q = queue.lock().unwrap();
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
                        title: "Block Avatar".into(),
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

// ------------------------------------------------------------------------------------------
// Scene, model loading
// ------------------------------------------------------------------------------------------
fn setup_scene(mut commands: Commands) {
    // No tonemapping: keeps the palette's flat block colours exactly as authored.
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
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

fn load_model(mut av: ResMut<Avatar>, asset_server: Res<AssetServer>, mut commands: Commands) {
    if let Some(old) = av.root.take() {
        commands.entity(old).despawn();
    }
    begin_load(&mut av, &asset_server);
}

fn begin_load(av: &mut Avatar, asset_server: &AssetServer) {
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

fn build_graph_when_loaded(
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

fn on_model_ready(
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
fn start_whisper(
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
                *mic.0.tap.lock().unwrap() = Some(tap);
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
fn apply_hits(
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

// ------------------------------------------------------------------------------------------
// Per-frame behaviour
// ------------------------------------------------------------------------------------------
fn update_lipsync(
    mic: Res<Mic>,
    mut lip: ResMut<Lip>,
    mut av: ResMut<Avatar>,
    time: Res<Time>,
    cam: Res<webcam::Webcam>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let frames = mic.0.frames.load(Ordering::Relaxed);
    let fresh = frames != lip.last_frames;
    lip.last_frames = frames;
    let mut f = mic.frame();
    if av.muted || av.hush {
        f.db = -100.0;
    }
    if lip.calibrate_until > 0.0 {
        if fresh && !av.muted {
            lip.floor.push(f.db);
        }
        if now >= lip.calibrate_until {
            lip.calibrate_until = 0.0;
            if let Some(floor) = lip.floor.finish() {
                let gate = (floor + 6.0).clamp(-70.0, -20.0);
                lip.picker.settings.gate_db = gate;
                av.status = format!("Gate set to {gate:.0} dB (room {floor:.0} dB)");
            } else {
                av.status = "No mic signal yet, keeping the saved gate".into();
            }
        }
    }
    av.mouth = lip.picker.update(f, dt);
    av.level = level01(f, &lip.picker.settings);
    let gate = lip.picker.settings.gate_db;
    let voiced = f.db > gate;
    // speaking / silent tracking for the Idle <-> Talk body switch
    if av.mouth != Mouth::Rest {
        if av.speaking_since.is_none() {
            av.speaking_since = Some(now);
        }
        av.silent_since = now;
    } else if now - av.silent_since > 0.6 {
        av.speaking_since = None;
    }
    // beats: every syllable / drum onset while bopping
    if f.onsets != av.last_onsets {
        av.last_onsets = f.onsets;
        if av.bop && voiced && now - av.last_beat > 0.18 {
            av.last_beat = now;
            av.beat_side = !av.beat_side;
        }
    }
    // body language from pitch and stress
    let speech_db = lip.picker.speech_db;
    let v = voice::VoiceFrame {
        pitch_hz: f.pitch_hz,
        clarity: f.clarity,
        f1: f.f1,
        f2: f.f2,
    };
    let ev = lip.prosody.update(v, f.db, f.onsets, dt, gate, speech_db);
    av.brows_up = lip.prosody.brows_up;
    av.liveliness = lip.prosody.liveliness;
    if let Some(strength) = ev.beat {
        av.talk_beat_t = now;
        av.talk_beat_amp = strength;
        av.talk_beat_left = !av.talk_beat_left;
    }
    if ev.question {
        av.question_until = now + 1.3;
        av.tilt_side = if av.rand01() < 0.5 { -1.0 } else { 1.0 };
    }
    // guided vowel calibration
    if let Some(mut vc) = lip.vcal.take() {
        let el = now - vc.stage_start;
        av.status = format!(
            "Vowel calibration {}/3: hold a long {}  ({:.0})",
            vc.stage + 1,
            VOWEL_PROMPTS[vc.stage],
            (VOWEL_STAGE_S - el).max(0.0).ceil()
        );
        if el > 0.6 && fresh && !av.muted {
            vc.cal.push(v);
        }
        if el < VOWEL_STAGE_S {
            lip.vcal = Some(vc);
        } else if let Some(r) = vc.cal.finish() {
            vc.got.push(r);
            vc.stage += 1;
            vc.stage_start = now;
            if vc.stage < 3 {
                lip.vcal = Some(vc);
            } else {
                let refs = VowelRefs {
                    ah: vc.got[0],
                    ee: vc.got[1],
                    oo: vc.got[2],
                };
                // sanity: ah opens the jaw most, and ee is clearly more spread than oo
                if refs.ah.0 > refs.ee.0.max(refs.oo.0) * 1.2 && refs.ee.1 > refs.oo.1 * 1.15 {
                    lip.picker.refs = refs;
                    av.status = format!(
                        "Vowels set: ah {:.0}/{:.0}  ee {:.0}/{:.0}  oo {:.0}/{:.0} Hz",
                        refs.ah.0, refs.ah.1, refs.ee.0, refs.ee.1, refs.oo.0, refs.oo.1
                    );
                    save_settings(&av, &lip, &cam);
                } else {
                    av.status = "Those vowels came out too similar; keeping the old ones. Press P to try again, close to the mic".into();
                }
            }
        } else {
            av.status = format!(
                "Didn't hear a clear {}; press P to try again",
                VOWEL_PROMPTS[vc.stage]
            );
        }
    }
    if let Some((_, until)) = av.face_override {
        if now > until {
            av.face_override = None;
        }
    }
    if !av.reactions {
        return;
    }
    // shout: clearly louder than your own talking level for a moment -> rage face and a jolt
    if voiced && f.db > lip.picker.speech_db + 7.0 && f.db > gate + 14.0 {
        let since = *av.loud_since.get_or_insert(now);
        if now - since > 0.18 && av.face_override.is_none() {
            av.face_override = Some((RAGE, now + 1.4));
            if av.emote.is_none() {
                av.emote = Some((Emote::Jolt, now));
            }
            av.status = "Reaction: shout".into();
        }
    } else {
        av.loud_since = None;
    }
    // doze off after a long silence, wake up startled
    if !av.asleep && lip.picker.silent_for > av.sleep_after {
        av.asleep = true;
        av.status = "Reaction: dozing off".into();
    }
    if av.asleep && voiced {
        av.asleep = false;
        av.face_override = Some((SURPRISED, now + 0.9));
        if av.emote.is_none() {
            av.emote = Some((Emote::Hop, now));
        }
        av.status = "Reaction: woke up".into();
    }
    if voiced {
        av.next_fidget = av.next_fidget.max(now + 12.0);
    }
}

fn drive_body(
    mut av: ResMut<Avatar>,
    time: Res<Time>,
    clips: Res<Assets<AnimationClip>>,
    mut players: Players,
) {
    let now = time.elapsed_secs();
    let Some(pe) = av.player else { return };
    let speaking = av.speaking_since.map(|t| now - t > 0.35).unwrap_or(false);
    if av.gesture_active {
        // work out the end time once the clip is known
        if av.gesture_until == f32::MAX {
            let len = av
                .clips
                .get(&av.body)
                .and_then(|(_, h)| clips.get(h))
                .map(|c| c.duration())
                .unwrap_or(1.5);
            av.gesture_until = now + len.max(0.3);
        }
        if now < av.gesture_until && !(av.gesture_interruptible && speaking) {
            return;
        }
        av.gesture_active = false;
    }
    // idle fidget: a long idle now and then while you're quiet
    if av.reactions && !av.asleep && !speaking && now > av.next_fidget && av.base_loop.is_none() {
        let r = av.rand01();
        av.next_fidget = now + 20.0 + 25.0 * r;
        if av.clips.contains_key("Idle (long)") {
            play_gesture(&mut av, "Idle (long)", true, &mut players);
            return;
        }
    }
    let Ok((mut player, mut tr)) = players.get_mut(pe) else {
        return;
    };
    let base = av
        .base_loop
        .clone()
        .filter(|b| av.clips.contains_key(b))
        .unwrap_or_else(|| "Idle".into());
    let want =
        if av.auto_talk && speaking && av.base_loop.is_none() && av.clips.contains_key("Talk") {
            "Talk".to_string()
        } else {
            base
        };
    if av.body != want {
        if let Some((node, _)) = av.clips.get(&want).cloned() {
            tr.play(&mut player, node, Duration::from_millis(350))
                .repeat();
            av.body = want;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_hud(
    av: Res<Avatar>,
    lip: Res<Lip>,
    mic: Res<Mic>,
    cam: Res<webcam::Webcam>,
    cmds: Res<control::Commands>,
    (chat, caps, stages, guest): (
        Res<chat::Chat>,
        Res<bubbles::Captions>,
        Res<stage::Stages>,
        Res<guest::Guest>,
    ),
    wardrobe: Res<wardrobe::Wardrobe>,
    mut q: Query<(&mut Text, &mut Visibility), With<HudText>>,
) {
    let Ok((mut text, mut vis)) = q.single_mut() else {
        return;
    };
    *vis = if av.hud {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !av.hud {
        return;
    }
    let f = mic.frame();
    let bars = ((f.db + 80.0) / 80.0 * 24.0).clamp(0.0, 24.0) as usize;
    let gate = ((lip.picker.settings.gate_db + 80.0) / 80.0 * 24.0).clamp(0.0, 24.0) as usize;
    let meter: String = (0..24)
        .map(|i| {
            if i == gate {
                '|'
            } else if i < bars {
                '#'
            } else {
                '.'
            }
        })
        .collect();
    let mic_line = if let Some(e) = mic.0.error.lock().unwrap().clone() {
        format!("Mic: {e}")
    } else if !mic.0.running.load(Ordering::Relaxed) {
        "Mic: starting...".into()
    } else {
        format!(
            "Mic: {}{}",
            mic.0.device.lock().unwrap(),
            if av.muted { "  [MUTED]" } else { "" }
        )
    };
    let onoff = |b: bool| if b { "on" } else { "off" };
    text.0 = format!(
        "{} | face {} | lip {} | mouth {}\nreactions {} | bop {} | body language {} | body {}{}\n{}\n{}\n{}\n{} | retro {}\n{}\n{}\n[{}] {:.0} dB  gate {:.0} dB\n{}\n{}\n\n1-0 faces | F1-F8 gestures | F9-F12 own clips | Tab character\nL lip style | R reactions | D bop | T body language | P vowel calibration\nQ test throw | S speech bubbles | Y retro sprite mode | A next question / end it | E clip that\nZ X C J K U I O nod shake tilt hop spin bow shiver lean\nB background | V framing | W webcam | G talk gestures | M mute | N calibrate | [ ] gate | H hide\nControl page (every clip): {}",
        av.models[av.model].0,
        EXPRESSIONS[av.expression].name,
        lip.picker.mode.name(),
        av.mouth.label(),
        onoff(av.reactions),
        onoff(av.bop),
        onoff(av.prosody_on),
        av.body,
        if av.asleep { " (asleep)" } else { "" },
        cam.status_line(),
        chat.status_line(),
        caps.status_line(),
        if stages.retro {
            format!("on ({} px)", stages.scale)
        } else {
            "off".into()
        },
        {
            let q = guest.queue.lock().unwrap();
            let approved = q
                .items
                .iter()
                .filter(|x| x.status == questions::Status::Approved)
                .count();
            format!(
                "Questions: {} | {} waiting, {} approved | ask page {}{}",
                if q.open { "open" } else { "closed" },
                q.waiting().len(),
                approved,
                guest.ask_url(),
                if guest.status.is_empty() {
                    String::new()
                } else {
                    format!(" | {}", guest.status)
                }
            )
        },
        wardrobe.summary(),
        mic_line,
        meter,
        f.db,
        lip.picker.settings.gate_db,
        voice_line(&f, &lip),
        av.status,
        cmds.1
    );
}

/// "Voice: 142 Hz (+2.1 st) | F1 650 F2 1180 = ah" for the HUD
fn voice_line(f: &Frame, lip: &Lip) -> String {
    if f.pitch_hz <= 0.0 || f.clarity < 0.6 || f.db < lip.picker.settings.gate_db {
        return format!(
            "Voice: -   (vowels {})",
            if lip.picker.use_formants {
                "from formants"
            } else {
                "from tone only"
            }
        );
    }
    let mut s = format!(
        "Voice: {:.0} Hz ({:+.1} st)",
        f.pitch_hz, lip.prosody.rel_st
    );
    if f.f1 > 0.0 && f.f2 > 0.0 {
        let (v, d) = lip.picker.refs.nearest(f.f1, f.f2);
        let name = match v {
            voice::Vowel::Ah => "ah",
            voice::Vowel::Ee => "ee",
            voice::Vowel::Oo => "oo",
        };
        s += &format!(
            " | F1 {:.0} F2 {:.0} = {}{}",
            f.f1,
            f.f2,
            name,
            if d > 0.45 { "?" } else { "" }
        );
    }
    s
}

/// Snapshot for the control page's GET /state (clip list, current settings).
#[allow(clippy::too_many_arguments)]
fn publish_state(
    av: Res<Avatar>,
    lip: Res<Lip>,
    cam: Res<webcam::Webcam>,
    (chat, caps, stages, guest): (
        Res<chat::Chat>,
        Res<bubbles::Captions>,
        Res<stage::Stages>,
        Res<guest::Guest>,
    ),
    (wardrobe, moments, polls, vfx, co, sh, cl, sfx, hk, bev, lit, crowd, parts, fcap): (
        Res<wardrobe::Wardrobe>,
        Res<events::Moments>,
        Res<poll::Polls>,
        Res<voicefx::VoiceFx>,
        Res<cohost::Cohost>,
        Res<show::Show>,
        Res<clips::Clips>,
        Res<fx::Fx>,
        Res<hotkeys::Hotkeys>,
        Res<bevel::Bevel>,
        Res<lighting::Studio>,
        Res<crowd::Crowd>,
        Res<particles::Particles>,
        Res<facecap::FaceCap>,
    ),
    state: Res<control::State>,
    (netr, worldr): (Res<net::Net>, Res<world::WorldClient>),
    mut last: Local<f32>,
    mut fps: Local<f32>,
    real: Res<Time<Real>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let dt = real.delta_secs();
    if dt > 0.0 {
        *fps = if *fps == 0.0 {
            1.0 / dt
        } else {
            *fps * 0.95 + 0.05 / dt
        };
    }
    if now - *last < 0.25 {
        return;
    }
    *last = now;
    // pieces that come as JSON text
    let raw = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap_or(serde_json::Value::Null);
    let ask_login = match &guest.ask {
        Some(a) if a.shared.cfg.lock().unwrap().twitch_client_id.is_some() => "twitch",
        Some(a) if a.shared.cfg.lock().unwrap().open_names => "names",
        Some(_) => "none",
        None => "off",
    };
    let on_stage = guest.stage.as_ref().map(|s| {
        serde_json::json!({"id": s.q.id, "user": s.name, "phase": match s.phase {
            guest::Phase::Loading | guest::Phase::Entering(_) => "arriving",
            guest::Phase::Speaking => "speaking",
            guest::Phase::After(_) => "waiting for your answer",
            guest::Phase::Leaving(..) => "leaving",
        }})
    });
    let json = serde_json::json!({
        "model": av.models[av.model].0,
        "face": av.expression,
        "lip": lip.picker.mode.name(),
        "lips": LIP_MODES.iter().map(|m| m.name()).collect::<Vec<_>>(),
        "reactions": av.reactions,
        "bop": av.bop,
        "webcam": cam.enabled,
        "webcam_status": cam.status_line(),
        "body_language": av.prosody_on,
        "formants": lip.picker.use_formants,
        "bubbles": caps.enabled,
        "retro": stages.retro,
        "retro_scale": stages.scale,
        "chat": chat.enabled,
        "twitch": chat.channel.as_deref().unwrap_or(""),
        "chat_status": chat.status_line(),
        "caption_source": caps.source,
        "ask_url": guest.ask_url(),
        "ask_login": ask_login,
        "ask_names": guest.ask.as_ref().is_some_and(|a| a.shared.cfg.lock().unwrap().open_names),
        "guest": on_stage,
        "guest_status": guest.status,
        "wardrobe": raw(wardrobe.json()),
        "raid_crowd": moments.raid_crowd,
        "sub_party": moments.sub_party,
        "poll": raw(polls.json(now)),
        "voice": {"laugh": vfx.det.laugh_on, "sing": vfx.det.sing_on, "gags": vfx.gags_on, "gag_count": vfx.gags.len(), "last": vfx.last, "singing": av.singing},
        "cohost_on": co.on,
        "cohost_status": co.status,
        "net": {
            "role": match netr.role { Some(net::Role::Host) => "host", Some(net::Role::Viewer) => "viewer", Some(net::Role::Guest) => "guest", None => "" },
            "guest_on": netr.guest_on,
            "status": netr.status,
            "relay": netr.relay,
            "session": netr.session,
            "code": netr.code,
            "viewers": netr.viewers,
            "guest": netr.guest,
        },
        "world": world::json(&worldr),
        "show": {"set": sh.set, "sets": show::SETS, "director": sh.director, "press": sh.press},
        "shorts": {"on": cl.on, "auto": cl.auto, "comics": cl.comics, "cards": cl.cards, "status": *cl.status.lock().unwrap(), "saved": *cl.saved.lock().unwrap()},
        "models": av.models.iter().map(|m| m.0.clone()).collect::<Vec<_>>(),
        "mic_level": (av.level * 100.0).round() / 100.0,
        "problems": applog::problems(),
        "setup_done": av.setup_done,
        "sfx": sfx.json(),
        "hotkeys": hk.json(),
        "bevel": bev.json(),
        "light": lit.json(),
        "crowd": crowd.json(),
        "effects": parts.json(sh.live_set()),
        "facecap": fcap.json(now),
        "idle_life": av.idle_life,
        "presets": av.presets,
        // voice to lips: 10 ms analysis hop, 15 ms envelope attack, ~1.5 frames to the screen
        "lip_latency_ms": (10.0 + 15.0 + 1500.0 / fps.max(1.0)).round(),
        "fps": fps.round(),
        "rec": cl.on,
        "asleep": av.asleep,
        "body": av.body,
        "loop": av.base_loop.as_deref().unwrap_or("Idle"),
        "emotes": EMOTES.iter().map(|e| e.1).collect::<Vec<_>>(),
        "own": av.own_clips,
        "clips": av.clip_order,
    })
    .to_string();
    *state.0.lock().unwrap() = json;
}

/// Tell the world code about your character and the quality to draw at.
fn world_in(
    w: Res<world::WorldClient>,
    av: Res<Avatar>,
    studio: Res<lighting::Studio>,
    mut me: ResMut<vox_world::LocalAvatar>,
    mut render: ResMut<vox_world::WorldRender>,
) {
    if !w.active() {
        return;
    }
    render.level = studio.level;
    render.shadows = studio.shadows;
    me.model = av.models[av.model].1.clone();
    me.fallback = av.models[0].1.clone();
    me.root = av.root;
    me.gltf = av.gltf.clone();
    me.ready = av.player.is_some();
    me.can_walk = av.clips.contains_key("Walk");
}

/// What the world decided: walk or stand, and that it lights the scene.
fn world_out(
    w: Res<world::WorldClient>,
    me: Res<vox_world::LocalAvatar>,
    render: Res<vox_world::WorldRender>,
    mut av: ResMut<Avatar>,
    mut studio: ResMut<lighting::Studio>,
    mut walking: Local<bool>,
) {
    if !w.active() {
        return;
    }
    if me.walking != *walking {
        *walking = me.walking;
        av.base_loop = me.walking.then(|| "Walk".to_string());
    }
    if render.outdoors && !studio.outdoors {
        studio.outdoors = true;
    }
}
