//! The guest on stage: an approved viewer question, asked by the viewer's chosen character next
//! to yours, in the viewer's own recorded voice.
//!
//! When a question airs, the character pops in beside you, the camera widens to a two-shot, the
//! recording plays (through the default sound output, or `guest_output=` in the settings, e.g. a
//! virtual cable for OBS) and drives the guest's mouth, brows and head with the same lip-sync and
//! body-language analysis as yours. Your character turns to listen and, unless you turn it off
//! (`guest_mutes_host=false`), ignores your mic while the guest talks so speaker spill doesn't
//! move your mouth. After the question the guest waits with a quizzical tilt until you dismiss it
//! (A key, or Done on the control page). Cut stops everything at once.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use bevy::{
    gltf::Gltf,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::lipsync::{Analyzer, Frame, LipMode, LipSync, Mouth, PickerSettings, level01};
use crate::questions::{Question, Queue, Status, parse_wav_mono16};
use crate::voice::{Prosody, VoiceFrame};

/// Where the guest stands, beside the host.
pub const GUEST_X: f32 = 1.05;
const GUEST_YAW: f32 = -0.38;
const ENTER_S: f32 = 0.45;
const LEAVE_S: f32 = 0.3;
/// gap between landing and talking
const LEAD_S: f32 = 0.25;

#[derive(Component)]
pub struct GuestRoot;
#[derive(Component)]
pub struct GuestPart(&'static str);
#[derive(Component)]
pub struct GuestHead;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Loading,
    /// popping in since t
    Entering(f32),
    Speaking,
    /// question finished at t; waiting for the answer
    After(f32),
    /// popping out since t, then the question is marked with this status
    Leaving(f32, Status),
}

// ------------------------------------------------------------------------------------------
// Playback: one thread owns the output stream and reports how far it has played
// ------------------------------------------------------------------------------------------
pub struct Playback {
    pos: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    pub device: Arc<Mutex<String>>,
}

impl Playback {
    /// Plays mono samples on the default output (or the first device whose name contains
    /// `want`). With no usable output it still runs the clock, so the guest still talks.
    pub fn start(samples: Arc<Vec<f32>>, rate: u32, want: Option<String>) -> Playback {
        Self::start_with(samples, rate, want, true)
    }
    /// The same, without a line in the log (sound effects).
    pub fn quiet(samples: Arc<Vec<f32>>, rate: u32) -> Playback {
        Self::start_with(samples, rate, None, false)
    }
    fn start_with(samples: Arc<Vec<f32>>, rate: u32, want: Option<String>, log: bool) -> Playback {
        let pos = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let device = Arc::new(Mutex::new(String::from("starting")));
        let (p, st, dev) = (pos.clone(), stop.clone(), device.clone());
        std::thread::Builder::new()
            .name("guest-audio".into())
            .spawn(move || {
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                if log {
                    crate::note!("[guest] audio started at {epoch:.3}");
                }
                // what has actually played goes into the program mix for clips, so a cut stays cut
                let mut sent = 0usize;
                let mut feed = |upto: usize| {
                    let upto = upto.min(samples.len());
                    if upto > sent {
                        crate::clips::program_play(rate, &samples[sent..upto]);
                        sent = upto;
                    }
                };
                match open_output(samples.clone(), rate, want, p.clone(), st.clone()) {
                    Ok((stream, name)) => {
                        *dev.lock().unwrap_or_else(|e| e.into_inner()) = name;
                        while !st.load(Ordering::Relaxed)
                            && (p.load(Ordering::Relaxed) as usize) < samples.len()
                        {
                            std::thread::sleep(Duration::from_millis(10));
                            feed(p.load(Ordering::Relaxed) as usize);
                        }
                        // let the device play out its buffer
                        std::thread::sleep(Duration::from_millis(150));
                        drop(stream);
                    }
                    Err(e) => {
                        *dev.lock().unwrap_or_else(|e| e.into_inner()) = format!("no sound output ({e}); timing only");
                        let t0 = std::time::Instant::now();
                        while !st.load(Ordering::Relaxed) {
                            let n = (t0.elapsed().as_secs_f64() * rate as f64) as u64;
                            p.store(n.min(samples.len() as u64), Ordering::Relaxed);
                            feed(n as usize);
                            if n as usize >= samples.len() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
            })
            .expect("spawn guest audio thread");
        Playback { pos, stop, device }
    }
    /// Seconds of the recording played so far.
    pub fn played_s(&self, rate: u32) -> f32 {
        self.pos.load(Ordering::Relaxed) as f32 / rate as f32
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn finished(&self, rate: u32) -> bool {
        self.stop.load(Ordering::Relaxed)
            || self.played_s(rate) > 3600.0
            || Arc::strong_count(&self.pos) == 1
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(deprecated)]
fn open_output(
    samples: Arc<Vec<f32>>,
    rate: u32,
    want: Option<String>,
    pos: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let device = match &want {
        Some(w) => host
            .output_devices()
            .ok()
            .and_then(|mut it| {
                it.find(|d| {
                    d.name()
                        .map(|n| n.to_lowercase().contains(&w.to_lowercase()))
                        .unwrap_or(false)
                })
            })
            .or_else(|| host.default_output_device()),
        None => host.default_output_device(),
    }
    .ok_or("no output device")?;
    let name = device.name().unwrap_or_else(|_| "output".into());
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let (dev_rate, ch) = (config.sample_rate() as f64, config.channels() as usize);
    let step = rate as f64 / dev_rate;
    // fractional read position in the recording, advanced per output frame
    let mut at = 0.0f64;
    let mut next = move || -> f32 {
        if stop.load(Ordering::Relaxed) {
            return 0.0;
        }
        let i = at as usize;
        if i + 1 >= samples.len() {
            pos.store(samples.len() as u64, Ordering::Relaxed);
            return 0.0;
        }
        let f = (at - i as f64) as f32;
        let v = samples[i] + (samples[i + 1] - samples[i]) * f;
        at += step;
        pos.store(at as u64, Ordering::Relaxed);
        v
    };
    let err = |e: cpal::StreamError| crate::problem!("[guest] output error: {e}");
    let cfg: cpal::StreamConfig = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &cfg,
            move |d: &mut [f32], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = next();
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &cfg,
            move |d: &mut [i16], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = (next() * 32767.0) as i16;
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_output_stream(
            &cfg,
            move |d: &mut [i32], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = (next() as f64 * 2147483647.0) as i32;
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &cfg,
            move |d: &mut [u16], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = (next() * 32767.0 + 32768.0) as u16;
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        other => return Err(format!("unsupported output format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

// ------------------------------------------------------------------------------------------
// The recording, analysed up front so the face can follow the playback clock exactly
// ------------------------------------------------------------------------------------------
pub struct Voice {
    pub rate: u32,
    pub samples: Arc<Vec<f32>>,
    pub frames: Vec<Frame>,
    pub hop_s: f32,
    /// share of the speech heard by each frame (0..1), for revealing the words as they're said
    pub spoken: Vec<f32>,
    pub gate_db: f32,
}

impl Voice {
    pub fn analyse(rate: u32, samples: Vec<f32>) -> Voice {
        let mut an = Analyzer::new(rate);
        let mut frames = vec![];
        an.process(&samples, 1, |f| frames.push(f));
        let hop = (rate as f32 / 100.0).round().max(1.0);
        // gate from the recording itself: a little above its quietest stretch
        let mut dbs: Vec<f32> = frames.iter().map(|f| f.db).collect();
        dbs.sort_by(|a, b| a.total_cmp(b));
        let floor = dbs.get(dbs.len() / 10).copied().unwrap_or(-70.0);
        let loud = dbs.get(dbs.len() * 9 / 10).copied().unwrap_or(-20.0);
        let gate_db = (floor + 6.0).max(loud - 30.0).clamp(-70.0, -25.0);
        let voiced: Vec<bool> = frames.iter().map(|f| f.db > gate_db).collect();
        let total = voiced.iter().filter(|v| **v).count().max(1) as f32;
        let mut acc = 0.0;
        let spoken = voiced
            .iter()
            .map(|v| {
                if *v {
                    acc += 1.0;
                }
                acc / total
            })
            .collect();
        Voice {
            rate,
            samples: Arc::new(samples),
            frames,
            hop_s: hop / rate as f32,
            spoken,
            gate_db,
        }
    }
    pub fn secs(&self) -> f32 {
        self.samples.len() as f32 / self.rate as f32
    }
    pub fn frame_at(&self, t: f32) -> Option<Frame> {
        let i = (t / self.hop_s) as usize;
        self.frames
            .get(i.min(self.frames.len().saturating_sub(1)))
            .copied()
    }
    pub fn spoken_at(&self, t: f32) -> f32 {
        if t >= self.secs() {
            return 1.0;
        }
        let i = (t / self.hop_s) as usize;
        self.spoken.get(i).copied().unwrap_or(1.0)
    }
}

pub struct OnStage {
    pub q: Question,
    /// name as shown on stream (hidden if it matches the blocked words)
    pub name: String,
    gltf: Handle<Gltf>,
    graph: Option<Handle<AnimationGraph>>,
    idle: Option<AnimationNodeIndex>,
    talk: Option<AnimationNodeIndex>,
    pub root: Option<Entity>,
    player: Option<Entity>,
    pub phase: Phase,
    voice: Voice,
    audio: Option<Playback>,
    picker: LipSync,
    prosody: Prosody,
    pub mouth: Mouth,
    pub level: f32,
    brows_up: bool,
    beat_t: f32,
    beat_amp: f32,
    speaking_since: Option<f32>,
    silent_since: f32,
    body_talk: bool,
    next_blink: f32,
    blink_until: f32,
    /// 0..1 of the words revealed in the bubble
    pub reveal: f32,
}

#[derive(Resource)]
pub struct Guest {
    pub queue: Arc<Mutex<Queue>>,
    pub ask: Option<crate::ask::AskServer>,
    pub ask_error: Option<String>,
    pub mute_host: bool,
    pub output: Option<String>,
    pub stage: Option<OnStage>,
    /// a question waiting for the stage to clear
    pending: Option<Question>,
    pub status: String,
    pub lip_mode: LipMode,
}

impl Guest {
    pub fn new(
        queue: Arc<Mutex<Queue>>,
        ask: Result<crate::ask::AskServer, String>,
        mute_host: bool,
        output: Option<String>,
    ) -> Self {
        let (ask, ask_error) = match ask {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(e)),
        };
        Guest {
            queue,
            ask,
            ask_error,
            mute_host,
            output,
            stage: None,
            pending: None,
            status: String::new(),
            lip_mode: LipMode::Visemes,
        }
    }

    pub fn ask_url(&self) -> String {
        match (&self.ask, &self.ask_error) {
            (Some(a), _) => a.url.clone(),
            (None, Some(e)) => format!("off ({e})"),
            _ => "off".into(),
        }
    }

    /// Is the guest's recording playing right now?
    pub fn talking(&self) -> bool {
        self.stage
            .as_ref()
            .is_some_and(|s| s.phase == Phase::Speaking)
    }

    /// The bubble text: name on the first line, then the question, revealed as it's spoken.
    pub fn bubble_text(&self) -> Option<String> {
        let s = self.stage.as_ref()?;
        if matches!(
            s.phase,
            Phase::Loading | Phase::Entering(_) | Phase::Leaving(..)
        ) {
            return None;
        }
        let text = s.q.text.trim();
        if text.is_empty() {
            return Some(format!("{} asks:", s.name));
        }
        let all: Vec<char> = text.chars().collect();
        let n = ((all.len() as f32) * s.reveal).ceil() as usize;
        let shown: String = all[..n.min(all.len())].iter().collect();
        Some(format!("{}:\n{}", s.name, shown))
    }

    /// Control page / hotkey commands starting with "ask:". Returns a status line.
    pub fn command(&mut self, m: &str, now: f32) -> Option<String> {
        let rest = m.strip_prefix("ask:")?;
        let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
        let id = arg.split(':').next().and_then(|x| x.parse::<u32>().ok());
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let msg = match verb {
            "open" | "close" | "toggle" => {
                q.open = match verb {
                    "open" => true,
                    "close" => false,
                    _ => !q.open,
                };
                format!("Questions {}", if q.open { "open" } else { "closed" })
            }
            "names" => {
                let Some(a) = &self.ask else {
                    return Some("The ask page isn't running".into());
                };
                let mut c = a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner());
                c.open_names = !c.open_names;
                format!(
                    "Typed names (no Twitch login) {}",
                    if c.open_names { "allowed" } else { "off" }
                )
            }
            "approve" => match id.map(|i| q.approve(i)) {
                Some(Ok(())) => format!("Question {} approved", id.unwrap_or(0)),
                Some(Err(e)) => e,
                None => "approve: which question?".into(),
            },
            "skip" => {
                let Some(i) = id else {
                    return Some("skip: which question?".into());
                };
                if self.stage.as_ref().is_some_and(|s| s.q.id == i) {
                    drop(q);
                    self.leave(now, Status::Skipped, true);
                    return Some(format!("Question {i} cut"));
                }
                q.finish(i, Status::Skipped);
                format!("Question {i} skipped")
            }
            "ban" => {
                let Some(i) = id else {
                    return Some("ban: which question?".into());
                };
                let acct = q.ban_question(i);
                drop(q);
                if self.stage.as_ref().is_some_and(|s| s.q.id == i) {
                    self.leave(now, Status::Skipped, true);
                }
                return Some(match acct {
                    Some(a) => format!("Banned {a}"),
                    None => format!("No question {i}"),
                });
            }
            "unban" => {
                let acct = crate::control::decode(arg);
                if q.unban(&acct) {
                    format!("Unbanned {acct}")
                } else {
                    format!("{acct} wasn't banned")
                }
            }
            "text" => {
                let Some(i) = id else {
                    return Some("text: which question?".into());
                };
                let text = arg.split_once(':').map(|x| x.1).unwrap_or("");
                q.set_text(i, &crate::control::decode(text));
                let t = q.get(i).map(|x| x.text.clone()).unwrap_or_default();
                if let Some(s) = self.stage.as_mut().filter(|s| s.q.id == i) {
                    s.q.text = t;
                }
                format!("Bubble text for question {i} saved")
            }
            "air" | "next" => {
                let Some(i) = (if verb == "next" {
                    q.next_approved()
                } else {
                    id
                }) else {
                    return Some("Nothing approved to air".into());
                };
                match q.take_for_air(i) {
                    Ok(x) => {
                        drop(q);
                        if self.stage.is_some() {
                            self.leave(now, Status::Done, false);
                        }
                        if let Some(old) = self.pending.replace(x) {
                            // a question that was waiting for the stage goes back in line
                            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                            q.finish_back(old.id);
                        }
                        return Some(format!("Question {i} going on air"));
                    }
                    Err(e) => e,
                }
            }
            "done" | "dismiss" => {
                drop(q);
                return Some(if self.leave(now, Status::Done, false) {
                    "Thanks for the question!".into()
                } else {
                    "Nobody on stage".into()
                });
            }
            "cut" => {
                drop(q);
                if let Some(p) = self.pending.take() {
                    self.queue.lock().unwrap_or_else(|e| e.into_inner()).finish(p.id, Status::Skipped);
                }
                return Some(if self.leave(now, Status::Skipped, true) {
                    "Cut".into()
                } else {
                    "Nobody on stage".into()
                });
            }
            "key" => {
                // the A key: air the next one, or end the current one (cut while it's still talking)
                drop(q);
                return match self.stage.as_ref().map(|s| s.phase) {
                    None => self.command("ask:next", now),
                    Some(Phase::Speaking) | Some(Phase::Loading) | Some(Phase::Entering(_)) => {
                        self.command("ask:cut", now)
                    }
                    Some(_) => self.command("ask:done", now),
                };
            }
            "clear" => {
                q.clear_finished();
                "Finished questions cleared".into()
            }
            "words" => {
                q.reload_words();
                format!("Blocked words reloaded ({})", q.blocked_words.len())
            }
            _ => return None,
        };
        Some(msg)
    }

    /// Starts the guest's exit. `now_cut` stops the audio at once.
    fn leave(&mut self, now: f32, status: Status, now_cut: bool) -> bool {
        let Some(s) = self.stage.as_mut() else {
            return false;
        };
        if now_cut || s.phase == Phase::Loading {
            if let Some(a) = s.audio.take() {
                a.stop();
            }
        }
        if !matches!(s.phase, Phase::Leaving(..)) {
            s.phase = Phase::Leaving(now, status);
        }
        true
    }
}

impl Queue {
    /// A question taken for air that never made it on stage goes back to approved.
    pub fn finish_back(&mut self, id: u32) {
        if let Some(q) = self
            .items
            .iter_mut()
            .find(|q| q.id == id && q.status == Status::OnAir)
        {
            q.status = Status::Approved;
        }
        self.save();
    }
}

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

fn on_ready(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_of_a_real_question_tracks_the_speech() {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("demo/questions/clb-b0352.wav");
        let (rate, s) = parse_wav_mono16(&std::fs::read(path).unwrap()).unwrap();
        let s = crate::questions::level_voice(&s).unwrap();
        let v = Voice::analyse(rate, s);
        assert!((v.hop_s - 0.01).abs() < 1e-4);
        assert!((v.secs() - 2.27).abs() < 0.02);
        // the words come out over the clip, not all at once
        assert!(v.spoken_at(0.05) < 0.1, "{}", v.spoken_at(0.05));
        assert!(
            (0.25..0.75).contains(&v.spoken_at(v.secs() / 2.0)),
            "{}",
            v.spoken_at(v.secs() / 2.0)
        );
        assert_eq!(v.spoken_at(v.secs()), 1.0);
        // and the mouth moves while it plays: several shape changes, not stuck open or shut
        let mut picker = LipSync::new(
            PickerSettings {
                gate_db: v.gate_db,
                ..Default::default()
            },
            LipMode::Visemes,
        );
        let mut shapes = std::collections::HashSet::new();
        let mut changes = 0;
        let mut last = Mouth::Rest;
        let mut t = 0.0;
        while t < v.secs() {
            let m = picker.update(v.frame_at(t).unwrap(), 1.0 / 60.0);
            if m != last {
                changes += 1;
                last = m;
            }
            shapes.insert(m.slot());
            t += 1.0 / 60.0;
        }
        assert!(
            changes >= 8 && shapes.len() >= 3,
            "changes {changes}, shapes {shapes:?}"
        );
    }

    #[test]
    fn playback_clock_runs_without_a_sound_card() {
        // in CI there's no output device: the clock must still advance at real speed
        let p = Playback::start(
            Arc::new(vec![0.0; 16000]),
            16000,
            Some("no-such-device-xyz".into()),
        );
        std::thread::sleep(Duration::from_millis(300));
        let t = p.played_s(16000);
        assert!(t > 0.15 && t <= 1.0, "{t}");
        p.stop();
    }
}
