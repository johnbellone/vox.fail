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
    let mut app = App::new();
    app::configure::configure(&mut app, &args);
    app
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
        .add_plugins(app::plugins::AvatarPlugins);

    if let Some((out, set, fx)) = args.snapshot.clone() {
        // every frame exactly one simulation tick, however fast the test machine renders
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / sim::HZ),
        ));
        let mut snap = snapshot::Snapshot::new(out.into(), set, fx);
        snap.act = args.snapshot_do.clone();
        snap.at = args.snapshot_at.unwrap_or(0.5);
        app.insert_resource(snap);
    }
    app.run();
}
