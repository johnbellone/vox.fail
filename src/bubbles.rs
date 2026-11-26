//! Live speech bubbles: what you say appears in a pixel speech bubble next to the avatar's head,
//! typed out like a retro RPG text box, in the window and in the virtual camera.
//!
//! Where the words come from (any or all):
//! * built-in Whisper speech-to-text, when built with `--features whisper` and given a model
//!   (`whisper_model=` in avatar_settings.txt);
//! * a caption text file another program keeps updating (`caption_file=`), for example the OBS
//!   plugin LocalVocal or any captioning tool that writes its latest line to a .txt or .srt;
//! * the control page or anything that can send `GET /cmd/say:<text>`.
//!
//! Quiet speech gets the dashed whisper bubble; normal speech the solid one.

use std::{
    path::PathBuf,
    sync::{Mutex, mpsc::Receiver},
    time::SystemTime,
};

use bevy::{
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    sprite::{BorderRect, SliceScaleMode, TextureSlicer},
    window::PrimaryWindow,
};

use crate::stage::Stages;

/// Letters per second for the typewriter reveal.
const TYPE_RATE: f32 = 40.0;
/// Seconds a bubble stays up after its last update.
const HOLD_S: f32 = 3.0;
/// Longest text shown at once; older words scroll off the front.
const MAX_CHARS: usize = 84;
const PX: f32 = 3.0; // bubble art scale
const FONT_PX: f32 = 16.0;
const WRAP_PX: f32 = 22.0 * FONT_PX;

#[derive(Resource)]
pub struct Captions {
    pub enabled: bool,
    text: String,
    shown: f32,
    last_t: f32,
    /// quiet speech: use the whisper bubble
    pub quiet: bool,
    pub file: Option<PathBuf>,
    file_sig: Option<(u64, Option<SystemTime>)>,
    file_poll: f32,
    pub source: String,
    /// every caption update, for the catchphrase gags (voicefx.rs) to read
    pub heard: Vec<String>,
    whisper_rx: Option<Mutex<Receiver<String>>>,
    level_avg: f32,
}

impl Captions {
    pub fn new(enabled: bool, file: Option<PathBuf>) -> Self {
        let source = match &file {
            Some(f) => format!("caption file {}", f.display()),
            None => "control page (say:...)".into(),
        };
        Captions {
            enabled,
            text: String::new(),
            shown: 0.0,
            last_t: -100.0,
            quiet: false,
            file,
            file_sig: None,
            file_poll: 0.0,
            source,
            heard: vec![],
            whisper_rx: None,
            level_avg: 0.0,
        }
    }

    #[cfg_attr(not(feature = "whisper"), allow(dead_code))]
    pub fn attach_whisper(&mut self, rx: Receiver<String>, model: &str) {
        self.whisper_rx = Some(Mutex::new(rx));
        self.source = format!("Whisper ({model})");
    }

    /// New caption text. Growing partial results keep typing on; a new sentence starts over.
    pub fn set(&mut self, text: &str, now: f32) {
        let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            return;
        }
        let continues = !self.text.is_empty()
            && now - self.last_t < HOLD_S
            && t.starts_with(&self.text[..self.text.len().min(t.len())]);
        if !continues {
            self.shown = 0.0;
        }
        if self.heard.len() < 64 {
            self.heard.push(t.clone());
        }
        self.text = t;
        self.last_t = now;
    }

    /// The part of the text on screen right now (typewriter + scrolling off old words).
    pub fn visible(&self) -> String {
        let all: Vec<char> = self.text.chars().collect();
        let n = (self.shown as usize).min(all.len());
        let mut s: String = all[..n].iter().collect();
        if s.chars().count() > MAX_CHARS {
            let cut = s
                .char_indices()
                .rev()
                .nth(MAX_CHARS - 4)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let rest = &s[cut..];
            let rest = rest.split_once(' ').map(|(_, r)| r).unwrap_or(rest);
            s = format!("...{rest}");
        }
        s
    }

    pub fn status_line(&self) -> String {
        format!(
            "Bubbles: {} ({})",
            if self.enabled { "on" } else { "off" },
            self.source
        )
    }
}

/// Last caption line from a .txt (last non-empty line) or .srt (last text block).
pub fn last_caption(contents: &str) -> Option<String> {
    let mut lines: Vec<&str> = contents
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    // .srt: drop counters and "00:00:01,000 --> 00:00:02,000" lines
    lines.retain(|l| !l.contains("-->") && !l.chars().all(|c| c.is_ascii_digit()));
    lines.last().map(|s| s.to_string())
}

#[derive(Component)]
pub struct BubbleUi {
    view: usize,
    /// whose bubble: the streamer's character, or a viewer's question on stage (guest.rs)
    guest: bool,
    tail: Entity,
    text: Entity,
    bg: Entity,
}

/// The nine-slice picture behind the text. It's a separate absolutely-placed child: a node that
/// shows an image is measured by that image, so it couldn't also grow to fit the text.
#[derive(Component)]
pub struct BubbleBg;

#[derive(Component)]
pub struct CreditUi {
    view: usize,
}

/// Startup, after the stages: one bubble (and one chat-credit line) per output.
pub fn spawn(mut commands: Commands, stages: Res<Stages>, assets: Res<AssetServer>) {
    let nearest = |s: &mut ImageLoaderSettings| s.sampler = ImageSampler::nearest();
    let body = assets
        .load_builder()
        .with_settings(nearest)
        .load::<Image>("ui/speech-9slice-14px-border5.png");
    let tail = assets
        .load_builder()
        .with_settings(nearest)
        .load::<Image>("ui/speech-tail.png");
    let font: Handle<Font> = assets.load("fonts/PressStart2P-Regular.ttf");
    let slicer = TextureSlicer {
        border: BorderRect::all(5.0),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: PX,
    };
    for (i, v) in stages.views.iter().enumerate() {
        for guest in [false, true] {
            let text = commands
                .spawn((
                    Text::new(""),
                    TextFont {
                        font: font.clone().into(),
                        font_size: FontSize::Px(FONT_PX),
                        ..default()
                    },
                    TextColor(Color::srgb_u8(31, 29, 36)),
                    Node {
                        max_width: Val::Px(WRAP_PX),
                        ..default()
                    },
                ))
                .id();
            let tail_e = commands
                .spawn((
                    ImageNode::new(tail.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(14.0 * PX),
                        height: Val::Px(12.0 * PX),
                        ..default()
                    },
                    Visibility::Hidden,
                    ChildOf(v.root),
                ))
                .id();
            let bg = commands
                .spawn((
                    ImageNode::new(body.clone()).with_mode(NodeImageMode::Sliced(slicer.clone())),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BubbleBg,
                ))
                .id();
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        padding: UiRect::axes(Val::Px(6.0 * PX), Val::Px(5.0 * PX)),
                        ..default()
                    },
                    Visibility::Hidden,
                    BubbleUi {
                        view: i,
                        guest,
                        tail: tail_e,
                        text,
                        bg,
                    },
                    ChildOf(v.root),
                ))
                .add_children(&[bg, text]);
        }
        commands.spawn((
            Text::new(""),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextShadow {
                offset: Vec2::splat(2.0),
                color: Color::srgb_u8(20, 18, 24),
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                bottom: Val::Px(14.0),
                ..default()
            },
            CreditUi { view: i },
            ChildOf(v.root),
        ));
    }
}

/// Pull in new caption text from Whisper and the caption file.
pub fn poll(mut caps: ResMut<Captions>, time: Res<Time>, av: Res<crate::Avatar>) {
    let now = time.elapsed_secs();
    // quiet speech -> whisper bubble (smoothed speaking level)
    let k = 1.0 - (-time.delta_secs() / 1.2).exp();
    if av.level > 0.02 {
        caps.level_avg += k * (av.level - caps.level_avg);
    }
    caps.quiet = caps.level_avg < 0.22;
    let got: Vec<String> = caps
        .whisper_rx
        .as_ref()
        .map(|rx| rx.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect())
        .unwrap_or_default();
    for t in got {
        caps.set(&t, now);
    }
    if let Some(path) = caps.file.clone() {
        caps.file_poll -= time.delta_secs();
        if caps.file_poll <= 0.0 {
            caps.file_poll = 0.2;
            if let Ok(meta) = std::fs::metadata(&path) {
                let sig = (meta.len(), meta.modified().ok());
                if caps.file_sig != Some(sig) {
                    caps.file_sig = Some(sig);
                    if let Some(line) = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|c| last_caption(&c))
                    {
                        caps.set(&line, now);
                    }
                }
            }
        }
    }
    let n = caps.text.chars().count() as f32;
    caps.shown = (caps.shown + TYPE_RATE * time.delta_secs()).min(n);
}

/// Place each output's bubble by the head, flip it if it would run off the right edge.
#[allow(clippy::too_many_arguments)]
pub fn layout(
    caps: Res<Captions>,
    stages: Res<Stages>,
    chat: Res<crate::chat::Chat>,
    time: Res<Time>,
    webcam: Res<crate::webcam::Webcam>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform)>,
    head: Query<(&crate::Joint, &GlobalTransform)>,
    (guest, guest_head, clips): (
        Res<crate::guest::Guest>,
        Query<&GlobalTransform, With<crate::guest::GuestHead>>,
        Res<crate::clips::Clips>,
    ),
    mut bubbles: Query<(&BubbleUi, &mut Node, &mut Visibility, &ComputedNode), Without<CreditUi>>,
    mut tails: Query<
        (&mut Node, &mut Visibility, &mut ImageNode),
        (Without<BubbleUi>, Without<CreditUi>, Without<BubbleBg>),
    >,
    mut bgs: Query<&mut ImageNode, With<BubbleBg>>,
    mut texts: Query<&mut Text, Without<CreditUi>>,
    mut credits: Query<
        (&CreditUi, &mut Text, &mut Visibility),
        (Without<BubbleUi>, Without<ImageNode>),
    >,
    assets: Res<AssetServer>,
    mut style: Local<Option<(Handle<Image>, Handle<Image>, Handle<Image>, Handle<Image>)>>,
) {
    let now = time.elapsed_secs();
    let nearest = |s: &mut ImageLoaderSettings| s.sampler = ImageSampler::nearest();
    let (speech, speech_tail, quiet_bg, quiet_tail) = style
        .get_or_insert_with(|| {
            (
                assets
                    .load_builder()
                    .with_settings(nearest)
                    .load("ui/speech-9slice-14px-border5.png"),
                assets
                    .load_builder()
                    .with_settings(nearest)
                    .load("ui/speech-tail.png"),
                assets
                    .load_builder()
                    .with_settings(nearest)
                    .load("ui/whisper-9slice-14px-border5.png"),
                assets
                    .load_builder()
                    .with_settings(nearest)
                    .load("ui/whisper-tail.png"),
            )
        })
        .clone();
    let host_head = head
        .iter()
        .find(|(j, _)| **j == crate::Joint::Head)
        .map(|(_, t)| *t);
    let host_show =
        caps.enabled && !caps.text.is_empty() && now - caps.last_t < HOLD_S && host_head.is_some();
    let host_words = caps.visible();
    let guest_words = guest.bubble_text();
    let guest_on = guest.stage.is_some();
    for (b, mut node, mut vis, computed) in &mut bubbles {
        // the guest's bubble shows their question; with a guest on stage (to the right), yours
        // goes on the left of your head so the two don't collide
        let (head_pos, show, words, quiet, prefer_left) = if b.guest {
            let h = guest_head.iter().next().copied();
            (
                h,
                guest_words.is_some() && h.is_some(),
                guest_words.clone().unwrap_or_default(),
                false,
                false,
            )
        } else {
            (
                host_head,
                host_show,
                host_words.clone(),
                caps.quiet,
                guest_on,
            )
        };
        let v = &stages.views[b.view];
        let final_size = if v.window {
            windows
                .single()
                .map(|w| Vec2::new(w.width(), w.height()))
                .unwrap_or(Vec2::splat(900.0))
        } else {
            v.image_size
                .map(|s| s.as_vec2())
                .unwrap_or(Vec2::new(webcam.width as f32, webcam.height as f32))
        };
        let active = v.window || if v.clip { clips.on } else { webcam.enabled };
        // just above the cap, to the right of centre (or the left, when the bubble has to flip)
        let at = |x: f32| {
            head_pos.and_then(|h| {
                let (cam, tf) = cams.get(v.cam3d).ok()?;
                crate::stage::to_final(
                    v,
                    cam,
                    tf,
                    h.transform_point(Vec3::new(x, 0.80, 0.0)),
                    final_size,
                )
            })
        };
        let anchor = at(if prefer_left { -0.18 } else { 0.18 });
        let (Some(p), true) = (anchor, show && active) else {
            *vis = Visibility::Hidden;
            if let Ok((_, mut tv, _)) = tails.get_mut(b.tail) {
                *tv = Visibility::Hidden;
            }
            continue;
        };
        if let Ok(mut t) = texts.get_mut(b.text) {
            if t.0 != words {
                t.0 = words.clone();
            }
        }
        if let Ok(mut img) = bgs.get_mut(b.bg) {
            img.image = if quiet {
                quiet_bg.clone()
            } else {
                speech.clone()
            };
        }
        let size = computed.size() * computed.inverse_scale_factor();
        let tail_w = 14.0 * PX;
        let tail_h = 12.0 * PX;
        let flip = prefer_left || p.x + tail_w * 0.4 + size.x > final_size.x - 8.0;
        let p = if flip && !prefer_left {
            at(-0.18).unwrap_or(p)
        } else {
            p
        };
        // bubble sits above the anchor, the tail's tip on the anchor
        let bottom = (final_size.y - p.y + tail_h - 2.0 * PX)
            .clamp(4.0, (final_size.y - size.y - 4.0).max(4.0));
        // keep the whole bubble on screen; the tail still points at the head
        let want_left = if flip {
            p.x + tail_w * 0.6 - size.x
        } else {
            p.x - tail_w * 0.4
        };
        node.right = Val::Auto;
        node.left = Val::Px(want_left.clamp(4.0, (final_size.x - size.x - 4.0).max(4.0)));
        node.bottom = Val::Px(bottom);
        node.top = Val::Auto;
        *vis = Visibility::Visible;
        if let Ok((mut tn, mut tv, mut ti)) = tails.get_mut(b.tail) {
            ti.image = if quiet {
                quiet_tail.clone()
            } else {
                speech_tail.clone()
            };
            ti.flip_x = flip;
            tn.bottom = Val::Px(final_size.y - p.y);
            if flip {
                tn.left = Val::Px(p.x - tail_w);
            } else {
                tn.left = Val::Px(p.x);
            }
            *tv = Visibility::Visible;
        }
    }
    // "name threw a tomato" under the avatar
    for (c, mut t, mut vis) in &mut credits {
        let v = &stages.views[c.view];
        match &chat.credit {
            Some((line, t0))
                if now - t0 < 2.5
                    && (v.window || if v.clip { clips.on } else { webcam.enabled }) =>
            {
                if t.0 != *line {
                    t.0 = line.clone();
                }
                *vis = Visibility::Visible;
            }
            _ => *vis = Visibility::Hidden,
        }
    }
}

/// Built-in speech-to-text (`--features whisper`). A worker thread gets the mic samples, finds
/// stretches of speech, and re-transcribes the current one about once a second so the bubble
/// fills in while you talk; the final pass runs when you pause.
#[cfg(feature = "whisper")]
pub mod stt {
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    pub fn start(model: String) -> Result<(SyncSender<(u32, Vec<f32>)>, Receiver<String>), String> {
        let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
            .map_err(|e| format!("can't load Whisper model {model}: {e}"))?;
        let (tap_tx, tap_rx) = mpsc::sync_channel::<(u32, Vec<f32>)>(256);
        let (txt_tx, txt_rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("whisper".into())
            .spawn(move || {
                let Ok(mut state) = ctx.create_state() else {
                    return;
                };
                let mut seg = super::Segmenter::new();
                for (rate, mono) in tap_rx {
                    for job in seg.push(rate, &mono) {
                        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                        p.set_language(Some("en"));
                        p.set_n_threads(
                            std::thread::available_parallelism()
                                .map(|n| n.get().min(8) as i32)
                                .unwrap_or(4),
                        );
                        p.set_no_context(true);
                        p.set_single_segment(true);
                        p.set_suppress_blank(true);
                        p.set_print_progress(false);
                        p.set_print_realtime(false);
                        p.set_print_special(false);
                        p.set_print_timestamps(false);
                        if state.full(p, &job).is_err() {
                            continue;
                        }
                        let mut text = String::new();
                        for s in state.as_iter() {
                            if let Ok(t) = s.to_str_lossy() {
                                text.push_str(&t);
                            }
                        }
                        let text = super::clean_transcript(&text);
                        if !text.is_empty() && txt_tx.send(text).is_err() {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok((tap_tx, txt_rx))
    }
}

#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
/// Drop what Whisper says on silence or noise: "[BLANK_AUDIO]", "(music)", "*coughs*".
pub fn clean_transcript(t: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '[' | '(' | '*' if depth == 0 => depth = 1,
            ']' | ')' | '*' if depth == 1 => depth = 0,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
/// Cuts the mic into stretches of speech at 16 kHz and decides when to (re)transcribe.
pub struct Segmenter {
    buf: Vec<f32>,
    frac: f64,
    speaking: bool,
    silent_s: f32,
    since_job_s: f32,
    floor_db: f32,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
impl Segmenter {
    pub fn new() -> Self {
        Segmenter {
            buf: vec![],
            frac: 0.0,
            speaking: false,
            silent_s: 0.0,
            since_job_s: 0.0,
            floor_db: -60.0,
        }
    }

    /// Feed mono samples at `rate`; returns audio clips (16 kHz) to transcribe now.
    pub fn push(&mut self, rate: u32, mono: &[f32]) -> Vec<Vec<f32>> {
        let mut jobs = vec![];
        // resample to 16 kHz (linear) in 20 ms blocks so the voice detector has steady steps
        let step = rate as f64 / 16000.0;
        let mut out16 = vec![];
        while self.frac < mono.len() as f64 - 1.0 {
            let i = self.frac as usize;
            let f = (self.frac - i as f64) as f32;
            out16.push(mono[i] * (1.0 - f) + mono[i + 1] * f);
            self.frac += step;
        }
        self.frac -= mono.len() as f64;
        if self.frac < 0.0 {
            self.frac = 0.0;
        }
        for block in out16.chunks(320) {
            let rms = (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt();
            let db = 20.0 * rms.max(1e-6).log10();
            // noise floor: falls fast, rises slowly
            self.floor_db = if db < self.floor_db {
                db
            } else {
                self.floor_db + 0.002 * (db - self.floor_db)
            };
            let dt = block.len() as f32 / 16000.0;
            let voiced = db > self.floor_db + 12.0 && db > -55.0;
            if voiced {
                self.speaking = true;
                self.silent_s = 0.0;
            } else {
                self.silent_s += dt;
            }
            if self.speaking {
                self.buf.extend_from_slice(block);
                self.since_job_s += dt;
                let len_s = self.buf.len() as f32 / 16000.0;
                if self.silent_s > 0.6 {
                    // pause: final pass on the whole stretch
                    if len_s > 0.9 {
                        jobs.push(std::mem::take(&mut self.buf));
                    }
                    self.buf.clear();
                    self.speaking = false;
                    self.since_job_s = 0.0;
                } else if self.since_job_s >= 1.0 && len_s >= 1.0 {
                    // partial result while still talking
                    jobs.push(self.buf.clone());
                    self.since_job_s = 0.0;
                    if len_s > 12.0 {
                        // very long stretch: start a new bubble, keep the last second for context
                        let keep = self.buf.split_off(self.buf.len() - 16000);
                        self.buf = keep;
                    }
                }
            }
        }
        jobs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_txt_and_srt_captions() {
        assert_eq!(
            last_caption("hello there\nsecond line\n\n").as_deref(),
            Some("second line")
        );
        let srt = "1\n00:00:01,000 --> 00:00:02,500\nFirst bit\n\n2\n00:00:03,000 --> 00:00:04,000\nThe latest words\n";
        assert_eq!(last_caption(srt).as_deref(), Some("The latest words"));
        assert_eq!(last_caption("\n \n"), None);
    }

    #[test]
    fn partial_results_keep_typing_new_sentences_restart() {
        let mut c = Captions::new(true, None);
        c.set("we are going", 0.0);
        c.shown = 12.0;
        c.set("we are going to the rally", 0.5);
        assert_eq!(c.shown, 12.0, "a growing partial keeps the typed part");
        c.set("Different sentence", 1.0);
        assert_eq!(c.shown, 0.0);
        c.shown = 1000.0;
        let long = "word ".repeat(40);
        c.set(&long, 1.1);
        c.shown = 1000.0;
        let v = c.visible();
        assert!(
            v.starts_with("...") && v.chars().count() <= MAX_CHARS,
            "{v}"
        );
    }

    #[test]
    fn whisper_noise_tags_are_dropped() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO]"), "");
        assert_eq!(
            clean_transcript(" (music) Hello  *coughs* world"),
            "Hello world"
        );
    }

    #[test]
    fn segmenter_cuts_real_speech_into_jobs() {
        // a real recording: LibriSpeech reader, 16 kHz
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("demo/libri-male-5703-47212-0000.wav");
        let (rate, ch, s) = crate::lipsync::read_wav(path.to_str().unwrap()).unwrap();
        assert_eq!(ch, 1);
        let mut seg = Segmenter::new();
        let mut jobs = vec![];
        for block in s.chunks(rate as usize / 50) {
            jobs.extend(seg.push(rate, block));
        }
        // then a second of silence to close the last stretch
        for _ in 0..50 {
            jobs.extend(seg.push(rate, &vec![0.0; rate as usize / 50]));
        }
        let secs: Vec<f32> = jobs.iter().map(|j| j.len() as f32 / 16000.0).collect();
        // 13.9 s of reading: partial passes about every second, each at most 12 s long
        assert!(jobs.len() >= 8, "{secs:?}");
        assert!(secs.iter().all(|s| *s <= 13.0 && *s >= 0.9), "{secs:?}");
    }
}
