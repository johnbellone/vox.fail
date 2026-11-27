//! Auto shorts and comic strips.
//!
//! A third camera renders the show in portrait (9:16, with the same bubbles, credits and retro
//! look as the window), and the last 45 seconds are kept in memory: video as JPEG frames, sound
//! as the program mix (your mic, viewers' questions, gag sounds). From that:
//!
//! - **Clip that** (E key, the control page, or a mod typing `!clip`): the last 20 s.
//! - **Auto shorts**: the moments worth posting save themselves: a splat, a laugh, a sub or raid,
//!   a gag, a poll result, the start of a song, and every viewer question with your answer.
//! - **Comic strips**: each saved moment also becomes a 4-panel strip (PNG).
//!
//! Everything lands in `clips/` next to avatar_settings.txt. Videos are put together by ffmpeg
//! (install it once: `winget install Gyan.FFmpeg`); without it the frames and sound are saved in
//! a folder with the one command that joins them.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        mpsc::{SyncSender, sync_channel},
    },
    time::Instant,
};

use bevy::{
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
};

pub const W: u32 = 540;
pub const H: u32 = 960;
const KEEP_S: f64 = 45.0;
const FPS: f64 = 30.0;
const RATE: u32 = 48000;

// ------------------------------------------------------------------------------------------
/// How long the title and end cards stay up, and the comic strip's caption bar height.
const TITLE_S: f64 = 1.4;
const END_S: f64 = 1.6;
const COMIC_BAR: usize = 72;

/// The newest frame, for the control page's preview.
static LATEST: Mutex<Option<Arc<Vec<u8>>>> = Mutex::new(None);

pub fn latest_jpeg() -> Option<Arc<Vec<u8>>> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// One clock for frames and sound
// ------------------------------------------------------------------------------------------
fn epoch() -> &'static Instant {
    static E: OnceLock<Instant> = OnceLock::new();
    E.get_or_init(Instant::now)
}

pub fn now_s() -> f64 {
    epoch().elapsed().as_secs_f64()
}

// ------------------------------------------------------------------------------------------
// Program audio: everything the stream hears, mixed, 48 kHz mono, last KEEP_S seconds
// ------------------------------------------------------------------------------------------
const BLOCK: usize = 480;

pub struct AudioRing {
    buf: Vec<f32>,
    /// absolute block number each slot block holds
    stamp: Vec<i64>,
    /// newest sample written; anything a full ring older is gone
    latest: i64,
}

impl AudioRing {
    pub fn new(secs: f64) -> Self {
        let n = ((secs * RATE as f64) as usize / BLOCK + 1) * BLOCK;
        AudioRing {
            buf: vec![0.0; n],
            stamp: vec![-1; n / BLOCK],
            latest: 0,
        }
    }
    /// Adds `s` (at `rate`) starting at time `t`.
    pub fn add(&mut self, t: f64, s: &[f32], rate: u32) {
        if s.is_empty() || t < 0.0 {
            return;
        }
        let step = rate as f64 / RATE as f64;
        let n_out = (s.len() as f64 / step) as usize;
        let start = (t * RATE as f64) as i64;
        let len = self.buf.len() as i64;
        for k in 0..n_out {
            let p = k as f64 * step;
            let i = p as usize;
            let f = (p - i as f64) as f32;
            let v = s[i] + (s[(i + 1).min(s.len() - 1)] - s[i]) * f;
            let abs = start + k as i64;
            let slot = abs.rem_euclid(len) as usize;
            let b = slot / BLOCK;
            let blk = abs.div_euclid(BLOCK as i64);
            if self.stamp[b] != blk {
                self.stamp[b] = blk;
                self.buf[b * BLOCK..(b + 1) * BLOCK].fill(0.0);
            }
            self.buf[slot] += v;
        }
        self.latest = self.latest.max(start + n_out as i64);
    }
    pub fn read(&self, t0: f64, t1: f64) -> Vec<f32> {
        let len = self.buf.len() as i64;
        let (a, b) = ((t0 * RATE as f64) as i64, (t1 * RATE as f64) as i64);
        (a..b)
            .map(|abs| {
                let slot = abs.rem_euclid(len) as usize;
                if abs >= 0
                    && abs > self.latest - len
                    && self.stamp[slot / BLOCK] == abs.div_euclid(BLOCK as i64)
                {
                    self.buf[slot]
                } else {
                    0.0
                }
            })
            .collect()
    }
}

fn audio() -> &'static Mutex<AudioRing> {
    static A: OnceLock<Mutex<AudioRing>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(AudioRing::new(KEEP_S + 5.0)))
}

/// A chunk of mic input that just arrived (interleaved, `ch` channels).
pub fn program_mic(rate: u32, d: &[f32], ch: usize) {
    let ch = ch.max(1);
    let mono: Vec<f32> = d
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    let t = now_s() - mono.len() as f64 / rate as f64;
    if let Ok(mut a) = audio().try_lock() {
        a.add(t, &mono, rate);
    }
}

/// Something the app plays out loud (a viewer's question, a gag sound), starting about now.
pub fn program_play(rate: u32, s: &[f32]) {
    audio().lock().unwrap_or_else(|e| e.into_inner()).add(now_s() + 0.03, s, rate);
}

// ------------------------------------------------------------------------------------------
// Frames
// ------------------------------------------------------------------------------------------
type Frames = Arc<Mutex<VecDeque<(f64, Arc<Vec<u8>>)>>>;

#[derive(Component)]
pub struct ClipCamera;

#[derive(Clone, Debug)]
pub struct Export {
    pub kind: String,
    pub label: String,
    pub t0: f64,
    pub t1: f64,
    /// when to save (the moment plus a few seconds of what came after)
    pub at: f64,
    pub comic_at: Option<f64>,
    pub brand: crate::cards::Brand,
}

#[derive(Resource)]
pub struct Clips {
    /// keep recording the portrait view (needed for everything below)
    pub on: bool,
    pub auto: bool,
    pub comics: bool,
    /// title card, burned-in caption and end card on shorts
    pub cards: bool,
    /// whose show it is (portrait and channel for the cards), kept current by update
    pub brand: crate::cards::Brand,
    frames: Frames,
    tx: Option<SyncSender<(f64, Vec<u8>)>>,
    pub image: Option<Handle<Image>>,
    pub camera: Option<Entity>,
    next_due: f64,
    pending: Vec<Export>,
    pub status: Arc<Mutex<String>>,
    pub saved: Arc<Mutex<Vec<String>>>,
    // what's been seen, to notice new moments
    seen_hit: f32,
    seen_laugh: f32,
    seen_moment: String,
    seen_gag: String,
    seen_poll: bool,
    seen_sing: bool,
    guest_start: Option<(u32, String, f64)>,
}

impl Clips {
    pub fn new(on: bool, auto: bool, comics: bool) -> Self {
        let _ = epoch();
        Clips {
            on,
            auto,
            comics,
            cards: true,
            brand: Default::default(),
            frames: Arc::new(Mutex::new(VecDeque::new())),
            tx: None,
            image: None,
            camera: None,
            next_due: 0.0,
            pending: vec![],
            status: Arc::new(Mutex::new(String::new())),
            saved: Arc::new(Mutex::new(vec![])),
            seen_hit: -100.0,
            seen_laugh: -100.0,
            seen_moment: String::new(),
            seen_gag: String::new(),
            seen_poll: false,
            seen_sing: false,
            guest_start: None,
        }
    }

    fn start_encoder(&mut self) {
        let (tx, rx) = sync_channel::<(f64, Vec<u8>)>(3);
        let frames = self.frames.clone();
        std::thread::Builder::new()
            .name("clip-jpeg".into())
            .spawn(move || {
                for (t, rgba) in rx {
                    let mut out = Vec::with_capacity(80_000);
                    let enc = jpeg_encoder::Encoder::new(&mut out, 82);
                    if enc
                        .encode(&rgba, W as u16, H as u16, jpeg_encoder::ColorType::Rgba)
                        .is_err()
                    {
                        continue;
                    }
                    let jpg = Arc::new(out);
                    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(jpg.clone());
                    let mut f = frames.lock().unwrap_or_else(|e| e.into_inner());
                    f.push_back((t, jpg));
                    while f.front().is_some_and(|x| t - x.0 > KEEP_S) {
                        f.pop_front();
                    }
                }
            })
            .expect("spawn clip encoder");
        self.tx = Some(tx);
    }

    pub fn queue(&mut self, kind: &str, label: &str, t0: f64, t1: f64, comic_at: Option<f64>) {
        let now = now_s();
        let t0 = t0.max(now - KEEP_S + 1.0);
        // a burst of the same thing (a cheer's volley of tomatoes) makes one short, not one per hit
        if kind != "clip" {
            if let Some(e) = self
                .pending
                .iter_mut()
                .find(|e| e.kind == kind && t0 <= e.t1)
            {
                e.t1 = e.t1.max(t1).min(e.t0 + 30.0);
                e.at = e.t1;
                return;
            }
        }
        self.pending.push(Export {
            kind: kind.into(),
            label: label.into(),
            t0,
            t1,
            at: t1,
            comic_at,
            brand: self.brand.clone(),
        });
    }

    /// clip (last 20 s), clip:auto, clip:comics, clip:rec (recording on/off)
    pub fn command(&mut self, arg: &str) -> Option<String> {
        Some(match arg {
            "" | "now" => {
                if !self.on {
                    return Some("Turn clip recording on first".into());
                }
                let n = now_s();
                self.queue("clip", "Clip", n - 20.0, n, Some(n - 3.0));
                "Saving the last 20 seconds".into()
            }
            "cards" => {
                self.cards = !self.cards;
                format!(
                    "Title cards and captions {}",
                    if self.cards { "on" } else { "off" }
                )
            }
            "auto" => {
                self.auto = !self.auto;
                format!("Auto shorts {}", if self.auto { "on" } else { "off" })
            }
            "comics" => {
                self.comics = !self.comics;
                format!("Comic strips {}", if self.comics { "on" } else { "off" })
            }
            "reel" => {
                let shorts: Vec<String> = self
                    .saved
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|n| n.ends_with(".mp4") && !n.starts_with("reel-"))
                    .cloned()
                    .collect();
                // the newest REEL_MAX, oldest first: a reel people will watch to the end
                let shorts = shorts[shorts.len().saturating_sub(REEL_MAX)..].to_vec();
                if shorts.len() < 2 {
                    return Some(
                        "The highlight reel needs two or more shorts from this stream".into(),
                    );
                }
                let (status, saved, n) = (self.status.clone(), self.saved.clone(), shorts.len());
                std::thread::spawn(move || {
                    let msg = match reel(&clips_dir(), &shorts) {
                        Ok(name) => {
                            saved.lock().unwrap_or_else(|e| e.into_inner()).push(name.clone());
                            format!("Highlight reel saved: {name}")
                        }
                        Err(e) => format!("Highlight reel failed: {e}"),
                    };
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = msg;
                });
                format!("Making a highlight reel of {n} shorts")
            }
            "rec" => {
                self.on = !self.on;
                format!("Clip recording {}", if self.on { "on" } else { "off" })
            }
            _ => return None,
        })
    }
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut clips: ResMut<Clips>) {
    let mut image = Image::new_target_texture(W, H, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    let e = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -5,
                is_active: clips.on,
                clear_color: ClearColorConfig::Custom(Color::srgb_u8(0, 177, 64)),
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Tonemapping::Linear,
            Transform::default(),
            ClipCamera,
        ))
        .observe(on_frame)
        .id();
    clips.image = Some(handle);
    clips.camera = Some(e);
    check_ffmpeg(clips.status.clone());
}

fn check_ffmpeg(status: Arc<Mutex<String>>) {
    std::thread::spawn(move || {
        let ok = std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        *status.lock().unwrap_or_else(|e| e.into_inner()) = if ok {
            "ffmpeg found: clips are saved as .mp4".into()
        } else {
            "ffmpeg not found: clips are saved as frames + sound (winget install Gyan.FFmpeg)"
                .into()
        };
    });
}

fn on_frame(ev: On<ReadbackComplete>, mut clips: ResMut<Clips>) {
    let Some(tx) = clips.tx.clone() else { return };
    let t = now_s();
    if t + 0.002 < clips.next_due {
        return;
    }
    clips.next_due = if clips.next_due + 2.0 / FPS < t {
        t + 1.0 / FPS
    } else {
        clips.next_due + 1.0 / FPS
    };
    let expected = (W * H * 4) as usize;
    let data = if ev.data.len() == expected {
        ev.data.clone()
    } else {
        crate::webcam::strip_padding(&ev.data, W as usize, H as usize)
    };
    let _ = tx.try_send((t, data));
}

/// Follow the window camera; switch recording on and off; notice moments; save what's due.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut clips: ResMut<Clips>,
    av: Res<crate::Avatar>,
    moments: Res<crate::events::Moments>,
    vfx: Res<crate::voicefx::VoiceFx>,
    polls: Res<crate::poll::Polls>,
    guest: Res<crate::guest::Guest>,
    chat: Res<crate::chat::Chat>,
    clear: Res<ClearColor>,
    stages: Res<crate::stage::Stages>,
    mut cams: Query<(&mut Transform, &mut Camera), With<ClipCamera>>,
    src: Query<&Transform, (With<crate::MainCamera>, Without<ClipCamera>)>,
    mut commands: Commands,
) {
    // camera
    if let (Ok(s), Ok((mut t, mut c))) = (src.single(), cams.single_mut()) {
        *t = *s;
        // portrait is narrow: step back when there's more than one character to fit
        if av.two_shot || av.cohost_on || av.remote_guest {
            let back = t.back();
            t.translation += back * 1.4;
        }
        c.is_active = clips.on;
        if !stages.retro {
            let bg = if clear.0.alpha() < 1.0 {
                Color::srgb_u8(0, 177, 64)
            } else {
                clear.0
            };
            c.clear_color = ClearColorConfig::Custom(bg);
        }
    }
    match (
        clips.on,
        clips.tx.is_some(),
        clips.camera,
        clips.image.clone(),
    ) {
        (true, false, Some(e), Some(img)) => {
            commands.entity(e).insert(Readback::texture(img));
            clips.start_encoder();
        }
        (false, true, Some(e), _) => {
            commands.entity(e).remove::<Readback>();
            clips.tx = None;
        }
        _ => {}
    }
    let n = now_s();
    let (model, character) = (av.models[av.model].1.clone(), av.models[av.model].0.clone());
    if clips.brand.model != model || clips.brand.channel != chat.channel {
        clips.brand = crate::cards::Brand {
            model,
            character,
            channel: chat.channel.clone(),
        };
    }
    // a mod's !clip
    for m in &chat.inbox {
        if m.text
            .split_whitespace()
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case("!clip"))
            && (m.moderator || m.broadcaster)
            && clips.on
        {
            clips.queue(
                "clip",
                &format!("Clipped by {}", m.user),
                n - 20.0,
                n,
                Some(n - 3.0),
            );
        }
    }
    // moments (all clocks here are the app clock; the "seen" values are its own times)
    if clips.on && clips.auto {
        let mut new: Vec<(&str, String)> = vec![];
        if av.last_hit > clips.seen_hit {
            clips.seen_hit = av.last_hit;
            new.push((
                "splat",
                chat.credit
                    .as_ref()
                    .map(|c| c.0.clone())
                    .unwrap_or_else(|| "Splat".into()),
            ));
        }
        if av.laugh_until > 0.0 && av.laugh_until > clips.seen_laugh + 2.5 {
            clips.seen_laugh = av.laugh_until;
            new.push(("laugh", "Cracked up".into()));
        }
        if moments.last != clips.seen_moment {
            clips.seen_moment = moments.last.clone();
            if !moments.last.is_empty() {
                new.push(("moment", moments.last.clone()));
            }
        }
        if vfx.last.starts_with("Gag") && vfx.last != clips.seen_gag {
            clips.seen_gag = vfx.last.clone();
            new.push(("gag", vfx.last.clone()));
        }
        let poll_done = polls
            .poll
            .as_ref()
            .is_some_and(|p| matches!(p.phase, crate::poll::Phase::Result(_)));
        if poll_done && !clips.seen_poll {
            new.push((
                "poll",
                polls
                    .poll
                    .as_ref()
                    .map(|p| p.line(0.0).replace('\n', " - "))
                    .unwrap_or_default(),
            ));
        }
        clips.seen_poll = poll_done;
        if av.singing && !clips.seen_sing {
            new.push(("song", "Singing".into()));
        }
        clips.seen_sing = av.singing;
        for (kind, label) in new {
            clips.queue(kind, &label, n - 9.0, n + 5.0, Some(n));
        }
        // a viewer question: from when they arrive until they leave (your answer included)
        let on_stage = guest.stage.as_ref().map(|s| (s.q.id, s.name.clone()));
        match (&clips.guest_start, on_stage) {
            (None, Some((id, name))) => clips.guest_start = Some((id, name, n)),
            (Some((id, name, t0)), now_on) if now_on.as_ref().map(|x| x.0) != Some(*id) => {
                let (id, name, t0) = (*id, name.clone(), *t0);
                // only questions that were answered, never one you cut
                let answered = guest
                    .queue
                    .lock()
                    .unwrap()
                    .get(id)
                    .is_some_and(|q| q.status == crate::questions::Status::Done);
                if answered {
                    let t0 = t0.max(n - 44.0) - 0.5;
                    clips.queue(
                        "question",
                        &format!("Question {id} from {name}"),
                        t0,
                        n + 0.5,
                        Some(t0 + 2.0),
                    );
                }
                clips.guest_start = None;
            }
            _ => {}
        }
    }
    // save what's due
    let due: Vec<Export> = clips
        .pending
        .iter()
        .filter(|e| e.at <= n)
        .cloned()
        .collect();
    clips.pending.retain(|e| e.at > n);
    for e in due {
        let frames: Vec<(f64, Arc<Vec<u8>>)> = clips
            .frames
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.0 >= e.t0 && f.0 <= e.t1)
            .cloned()
            .collect();
        if frames.len() < 10 {
            *clips.status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Not enough recorded for \"{}\" yet", e.label);
            continue;
        }
        crate::note!(
            "[clips] {} \"{}\": {} frames {:.1}-{:.1}s (asked {:.1}-{:.1}s, now {:.1}s)",
            e.kind,
            e.label,
            frames.len(),
            frames[0].0,
            frames[frames.len() - 1].0,
            e.t0,
            e.t1,
            n
        );
        let sound = audio()
            .lock()
            .unwrap()
            .read(frames[0].0, frames[frames.len() - 1].0 + 1.0 / FPS);
        let (status, saved, comics, cards) = (
            clips.status.clone(),
            clips.saved.clone(),
            clips.comics,
            clips.cards,
        );
        std::thread::spawn(move || {
            let r = save(&e, &frames, &sound, comics, cards);
            match r {
                Ok(names) => {
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Saved: {}", names.join(", "));
                    let mut s = saved.lock().unwrap_or_else(|e| e.into_inner());
                    s.extend(names);
                    let len = s.len();
                    if len > 20 {
                        s.drain(..len - 20);
                    }
                }
                Err(err) => {
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Couldn't save \"{}\": {err}", e.label)
                }
            }
        });
    }
}

/// How many shorts a highlight reel takes at most (the newest ones).
pub const REEL_MAX: usize = 10;

/// The ffmpeg concat list for a reel: one `file '<name>'` line per short, quotes escaped.
pub fn concat_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("file '{}'\n", n.replace('\'', "'\\''")))
        .collect()
}

/// Joins this stream's shorts, oldest first, into reel-<time>.mp4 without re-encoding (they all
/// come out of the same encoder settings). Returns the file name.
pub fn reel(dir: &Path, shorts: &[String]) -> Result<String, String> {
    let list = dir.join("reel-list.txt");
    std::fs::write(&list, concat_list(shorts)).map_err(|e| e.to_string())?;
    let name = format!("reel-{}.mp4", stamp());
    let out = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            "reel-list.txt",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            &name,
        ])
        .current_dir(dir)
        .output();
    let _ = std::fs::remove_file(&list);
    match out {
        Ok(o) if o.status.success() => Ok(name),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr)
            .lines()
            .last()
            .unwrap_or("ffmpeg error")
            .to_string()),
        Err(_) => Err("ffmpeg not found (winget install Gyan.FFmpeg)".into()),
    }
}

fn clips_dir() -> PathBuf {
    crate::applog::user_dir().join("clips")
}

fn stamp() -> String {
    // local-ish wall clock for file names: YYYYMMDD-HHMMSS (UTC)
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (s / 86400, s % 86400);
    let (y, m, d) = civil(days as i64);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// days since 1970-01-01 -> (year, month, day)
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Writes the short (mp4 via ffmpeg, or frames + sound) and the comic strip. Returns file names.
pub fn save(
    e: &Export,
    frames: &[(f64, Arc<Vec<u8>>)],
    sound: &[f32],
    comics: bool,
    cards: bool,
) -> Result<Vec<String>, String> {
    let dir = clips_dir();
    std::fs::create_dir_all(&dir).map_err(|x| x.to_string())?;
    let base = format!("{}-{}", stamp(), e.kind);
    let mut names = vec![];
    if comics {
        if let Some(at) = e.comic_at {
            let p = dir.join(format!("{base}-comic.png"));
            comic(frames, at, &p, &e.label, &e.brand)?;
            names.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    let work = dir.join(&base);
    std::fs::create_dir_all(&work).map_err(|x| x.to_string())?;
    // title card, caption on every frame, end card (when on and the font is there); all at the
    // same JPEG quality, as the encoder switches to 4:4:4 colour at 90+ and ffmpeg would drop
    // the title card when the format changes
    let painter = if cards {
        crate::cards::Painter::load()
    } else {
        None
    };
    let (title_s, end_s) = if painter.is_some() {
        (TITLE_S, END_S)
    } else {
        (0.0, 0.0)
    };
    let mut list = String::new();
    if let Some(p) = &painter {
        let t = crate::cards::title_card(p, W as usize, H as usize, &e.label, &e.brand);
        std::fs::write(work.join("title.jpg"), t.jpeg(88)?).map_err(|x| x.to_string())?;
        list += &format!("file 'title.jpg'\nduration {title_s:.4}\n");
    }
    for (i, (_, jpg)) in frames.iter().enumerate() {
        let bytes = match &painter {
            Some(p) => {
                let mut c = crate::cards::Canvas::from_jpeg(jpg)?;
                crate::cards::caption(p, &mut c, &e.label);
                c.jpeg(88)?
            }
            None => jpg.to_vec(),
        };
        std::fs::write(work.join(format!("frame_{i:05}.jpg")), bytes).map_err(|x| x.to_string())?;
    }
    let mut padded = vec![0.0f32; (title_s * RATE as f64) as usize];
    padded.extend_from_slice(sound);
    padded.extend(std::iter::repeat_n(0.0, (end_s * RATE as f64) as usize));
    std::fs::write(
        work.join("sound.wav"),
        crate::questions::wav16_bytes(RATE, &padded),
    )
    .map_err(|x| x.to_string())?;
    // each frame shown for as long as it really was, so a dropped or slow frame keeps the sound in sync
    for (i, f) in frames.iter().enumerate() {
        let d = frames
            .get(i + 1)
            .map(|n| n.0 - f.0)
            .unwrap_or(1.0 / FPS)
            .max(0.001);
        list += &format!("file 'frame_{i:05}.jpg'\nduration {d:.4}\n");
    }
    if let Some(p) = &painter {
        let c = crate::cards::end_card(p, W as usize, H as usize, &e.brand);
        std::fs::write(work.join("end.jpg"), c.jpeg(88)?).map_err(|x| x.to_string())?;
        list += &format!("file 'end.jpg'\nduration {end_s:.4}\nfile 'end.jpg'\n");
    } else {
        list += &format!("file 'frame_{:05}.jpg'\n", frames.len() - 1);
    }
    std::fs::write(work.join("frames.txt"), list).map_err(|x| x.to_string())?;
    let out = dir.join(format!("{base}.mp4"));
    let args = |out: &Path| -> Vec<String> {
        vec![
            "-y".into(),
            "-loglevel".into(),
            "error".into(),
            "-f".into(),
            "concat".into(),
            "-safe".into(),
            "0".into(),
            "-i".into(),
            "frames.txt".into(),
            "-i".into(),
            "sound.wav".into(),
            "-vf".into(),
            "scale=1080:1920:flags=lanczos,fps=30".into(),
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            "20".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "160k".into(),
            "-shortest".into(),
            "-movflags".into(),
            "+faststart".into(),
            out.to_string_lossy().into_owned(),
        ]
    };
    let ran = std::process::Command::new("ffmpeg")
        .args(args(&out))
        .current_dir(&work)
        .output();
    match ran {
        Ok(o) if o.status.success() && out.exists() => {
            let _ = std::fs::remove_dir_all(&work);
            names.push(out.file_name().unwrap().to_string_lossy().into_owned());
        }
        _ => {
            let cmd = format!(
                "ffmpeg {}",
                args(Path::new(&format!("../{base}.mp4")))
                    .iter()
                    .map(|a| if a.contains(' ') || a.contains(':') {
                        format!("\"{a}\"")
                    } else {
                        a.clone()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let _ = std::fs::write(
                work.join("MAKE_VIDEO.txt"),
                format!(
                    "{}\n\nInstall ffmpeg (winget install Gyan.FFmpeg), then run this in this folder:\n\n{cmd}\n",
                    e.label
                ),
            );
            names.push(format!("{base}/ (frames + sound)"));
        }
    }
    Ok(names)
}

/// Four panels around the moment, side by side, with comic borders. Shows whatever was on
/// screen, speech bubbles included.
pub fn comic(
    frames: &[(f64, Arc<Vec<u8>>)],
    at: f64,
    path: &Path,
    label: &str,
    brand: &crate::cards::Brand,
) -> Result<(), String> {
    let picks: Vec<&Arc<Vec<u8>>> = [-4.5, -2.5, -0.6, 1.5]
        .iter()
        .map(|dt| {
            let t = at + dt;
            &frames
                .iter()
                .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
                .unwrap()
                .1
        })
        .collect();
    let (pw, ph) = (W as usize, H as usize);
    let (border, gutter, margin) = (6usize, 18usize, 24usize);
    let width = margin * 2 + 4 * (pw + 2 * border) + 3 * gutter;
    let painter = crate::cards::Painter::load();
    let bar = if painter.is_some() { COMIC_BAR } else { 0 };
    let height = margin * 2 + ph + 2 * border + bar;
    let mut img = vec![255u8; width * height * 3];
    for (k, jpg) in picks.iter().enumerate() {
        let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpg.as_slice()));
        let px = d.decode().map_err(|e| e.to_string())?;
        let info = d.info().ok_or("no jpeg info")?;
        let (w, h) = (info.width as usize, info.height as usize);
        let x0 = margin + k * (pw + 2 * border + gutter);
        // black frame
        for y in margin..margin + ph + 2 * border {
            for x in x0..x0 + pw + 2 * border {
                let i = (y * width + x) * 3;
                img[i..i + 3].copy_from_slice(&[20, 20, 24]);
            }
        }
        for y in 0..ph.min(h) {
            for x in 0..pw.min(w) {
                let s = (y * w + x) * 3;
                let i = ((margin + border + y) * width + x0 + border + x) * 3;
                img[i..i + 3].copy_from_slice(&px[s..s + 3]);
            }
        }
    }
    if let Some(p) = &painter {
        let mut c = crate::cards::Canvas {
            w: width,
            h: height,
            px: img,
        };
        crate::cards::comic_bar(p, &mut c, height - bar, bar, margin, label, brand);
        img = c.px;
    }
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), width as u32, height as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&img).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_audio_mixes_by_time_and_forgets_old_sound() {
        let mut a = AudioRing::new(2.0);
        a.add(0.5, &vec![0.25; 16000], 16000); // 1 s at 16 kHz from 0.5 s
        a.add(1.0, &vec![0.5; 4800], 48000); // 0.1 s at 48 kHz from 1.0 s, mixed on top
        let r = a.read(0.4, 1.6);
        let at = |t: f64| r[((t - 0.4) * 48000.0) as usize];
        assert_eq!(at(0.45), 0.0);
        assert!((at(0.7) - 0.25).abs() < 1e-3);
        assert!((at(1.05) - 0.75).abs() < 1e-3);
        assert_eq!(at(1.55), 0.0);
        // much later, the ring has moved on: old sound isn't read back as new
        a.add(10.0, &[0.1; 480], 48000);
        assert!(a.read(0.5, 1.5).iter().all(|v| *v == 0.0));
        assert!(a.read(12.0, 12.5).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn reel_list_quotes_names() {
        assert_eq!(
            concat_list(&["a.mp4".into(), "it's.mp4".into()]),
            "file 'a.mp4'\nfile 'it'\\''s.mp4'\n"
        );
    }

    #[test]
    fn dates_for_file_names() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_778), (2026, 11, 21));
    }

    #[test]
    fn saves_a_comic_and_a_short_from_frames() {
        // four flat-colour frames, as the encoder would make them
        let mut frames = vec![];
        for i in 0..40 {
            let rgba: Vec<u8> = (0..(W * H))
                .flat_map(|_| [(i * 6) as u8, 100, 200, 255])
                .collect();
            let mut out = vec![];
            jpeg_encoder::Encoder::new(&mut out, 80)
                .encode(&rgba, W as u16, H as u16, jpeg_encoder::ColorType::Rgba)
                .unwrap();
            frames.push((i as f64 / 10.0, Arc::new(out)));
        }
        let dir = std::env::temp_dir().join(format!("avatar-comic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.png");
        comic(
            &frames,
            2.0,
            &p,
            "Raid from SomeStreamer (60)",
            &crate::cards::Brand {
                model: "models/dog.glb".into(),
                character: "Dog".into(),
                channel: Some("chan".into()),
            },
        )
        .unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&p).unwrap()));
        let r = dec.read_info().unwrap();
        assert_eq!(
            (r.info().width, r.info().height),
            (
                24 * 2 + 4 * (W + 12) + 3 * 18,
                24 * 2 + H + 12 + COMIC_BAR as u32
            )
        );
    }
}
