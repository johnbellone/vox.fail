//! Microphone analysis and the lip-sync styles. Pure Rust, no Bevy, so it can be unit-tested and
//! run offline on a WAV file (`block_avatar --analyze in.wav out.csv [mode]`).
//!
//! Analysis, per 10 ms window of mono audio:
//!   level      RMS envelope, fast attack (15 ms) / slower release (120 ms), in dBFS
//!   low        share of energy below ~500 Hz   (high for "oo", "oh", "m", "n")
//!   bright     share of energy above ~900 Hz   (high for "ee", "eh")
//!   high       share of energy above ~2.5 kHz  (high for "s", "sh", "f", "ch")
//!   zcr        zero crossings per sample       (noisy consonants cross far more often)
//!   onsets     running count of syllable starts (level jumps by 6+ dB within 50 ms)
//!   pitch      fundamental frequency and how clearly voiced the sound is (see voice.rs)
//!   f1, f2     first two formants: jaw opening and lip spread vs rounding (see voice.rs)
//!
//! Five styles turn that into a mouth shape, see [`LipMode`].

/// One 10 ms analysis frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub db: f32,
    pub pitch_hz: f32,
    pub clarity: f32,
    pub f1: f32,
    pub f2: f32,
    pub bright: f32,
    pub low: f32,
    pub high: f32,
    pub zcr: f32,
    pub onsets: u32,
}

pub struct Analyzer {
    a500: f32,
    a900: f32,
    a2500: f32,
    lp500: f32,
    lp900: f32,
    lp2500: f32,
    prev_x: f32,
    hop: usize,
    n: usize,
    e_all: f32,
    e_low: f32,
    e_900: f32,
    e_high: f32,
    crossings: u32,
    env: f32,
    /// faster envelope (40 ms release) just for spotting syllable starts
    fast_env: f32,
    fast_release: f32,
    attack: f32,
    release: f32,
    smooth: f32,
    out: Frame,
    hist: [f32; 5],
    hist_i: usize,
    since_onset: u32,
    voice: VoiceTracker,
}

use crate::voice::{VoiceTracker, VowelRefs};

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let fs = sample_rate as f32;
        let hop = (fs / 100.0).round().max(1.0) as usize; // 10 ms
        let hop_s = hop as f32 / fs;
        let coef = |tau: f32| 1.0 - (-hop_s / tau).exp();
        let lp = |fc: f32| 1.0 - (-2.0 * std::f32::consts::PI * fc / fs).exp();
        Analyzer {
            a500: lp(500.0),
            a900: lp(900.0),
            a2500: lp(2500.0),
            lp500: 0.0,
            lp900: 0.0,
            lp2500: 0.0,
            prev_x: 0.0,
            hop,
            n: 0,
            e_all: 0.0,
            e_low: 0.0,
            e_900: 0.0,
            e_high: 0.0,
            crossings: 0,
            env: 0.0,
            fast_env: 0.0,
            fast_release: coef(0.040),
            // quick to open (lips shouldn't trail the voice), slower to close
            attack: coef(0.015),
            release: coef(0.120),
            smooth: coef(0.040),
            out: Frame {
                db: -100.0,
                bright: 0.3,
                low: 0.5,
                high: 0.05,
                zcr: 0.02,
                ..Default::default()
            },
            hist: [-100.0; 5],
            hist_i: 0,
            since_onset: 100,
            voice: VoiceTracker::new(sample_rate),
        }
    }

    /// Feed interleaved samples; `on_frame` is called once per completed 10 ms window.
    pub fn process(
        &mut self,
        interleaved: &[f32],
        channels: usize,
        mut on_frame: impl FnMut(Frame),
    ) {
        let ch = channels.max(1);
        for block in interleaved.chunks(ch) {
            let x = block.iter().sum::<f32>() / ch as f32;
            self.voice.push(x);
            self.lp500 += self.a500 * (x - self.lp500);
            self.lp900 += self.a900 * (x - self.lp900);
            self.lp2500 += self.a2500 * (x - self.lp2500);
            let (above900, above2500) = (x - self.lp900, x - self.lp2500);
            self.e_all += x * x;
            self.e_low += self.lp500 * self.lp500;
            self.e_900 += above900 * above900;
            self.e_high += above2500 * above2500;
            // ignore crossings in near-silence so room hiss doesn't count
            if (x >= 0.0) != (self.prev_x >= 0.0) && (x - self.prev_x).abs() > 0.002 {
                self.crossings += 1;
            }
            self.prev_x = x;
            self.n += 1;
            if self.n == self.hop {
                self.finish_window(&mut on_frame);
            }
        }
    }

    fn finish_window(&mut self, on_frame: &mut impl FnMut(Frame)) {
        let n = self.n as f32;
        let rms = (self.e_all / n).sqrt();
        let k = if rms > self.env {
            self.attack
        } else {
            self.release
        };
        self.env += k * (rms - self.env);
        let db = 20.0 * self.env.max(1e-5).log10();
        let kf = if rms > self.fast_env {
            self.attack
        } else {
            self.fast_release
        };
        self.fast_env += kf * (rms - self.fast_env);
        let fast_db = 20.0 * self.fast_env.max(1e-5).log10();
        if rms > 1e-4 && self.e_all > 1e-9 {
            let share = |e: f32| (e / self.e_all).clamp(0.0, 1.0);
            let s = self.smooth;
            self.out.bright += s * (share(self.e_900) - self.out.bright);
            self.out.low += s * (share(self.e_low) - self.out.low);
            self.out.high += s * (share(self.e_high) - self.out.high);
            self.out.zcr += s * (self.crossings as f32 / n - self.out.zcr);
        }
        // syllable onset: fast level up 6+ dB on the quietest of the last 50 ms
        let quietest = self.hist.iter().cloned().fold(f32::MAX, f32::min);
        self.since_onset += 1;
        if fast_db - quietest > 6.0 && fast_db > -60.0 && self.since_onset >= 10 {
            self.out.onsets = self.out.onsets.wrapping_add(1);
            self.since_onset = 0;
        }
        self.hist[self.hist_i] = fast_db;
        self.hist_i = (self.hist_i + 1) % self.hist.len();
        self.out.db = db;
        let v = self.voice.analyse(fast_db > -65.0);
        self.out.pitch_hz = v.pitch_hz;
        self.out.clarity = v.clarity;
        self.out.f1 = v.f1;
        self.out.f2 = v.f2;
        on_frame(self.out);
        self.n = 0;
        self.e_all = 0.0;
        self.e_low = 0.0;
        self.e_900 = 0.0;
        self.e_high = 0.0;
        self.crossings = 0;
    }
}

/// Mouth shapes. Each expression says which face part draws each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mouth {
    /// silent: the expression's resting mouth
    Rest,
    /// quiet speech, half-open
    Mid,
    /// loud open vowel ("ah")
    Open,
    /// rounded vowel ("oo", "oh", "w")
    Round,
    /// spread vowel ("ee", "eh")
    Bright,
    /// teeth together for hiss ("s", "sh", "f", "ch")
    Teeth,
    /// lips pressed ("m", "b", "p")
    Closed,
}

impl Mouth {
    pub fn label(self) -> &'static str {
        match self {
            Mouth::Rest => "rest",
            Mouth::Mid => "mid",
            Mouth::Open => "open",
            Mouth::Round => "round",
            Mouth::Bright => "bright",
            Mouth::Teeth => "teeth",
            Mouth::Closed => "closed",
        }
    }
    pub fn slot(self) -> usize {
        self as usize
    }
}

/// Lip-sync styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LipMode {
    /// Five shapes from loudness and tone. The default: readable and steady.
    Visemes,
    /// Seven shapes: adds teeth for "s/sh/f" and pressed lips for "m/b/p". Busiest, most "real".
    Detailed,
    /// Two shapes, open while you're loud enough: classic PNGTuber.
    Flap,
    /// Three steps by loudness only (closed / half / open). Most forgiving in noisy rooms.
    Volume,
    /// Cartoon gibberish: a new random shape every syllable, like retro RPG text boxes.
    Chatter,
}

pub const LIP_MODES: [LipMode; 5] = [
    LipMode::Visemes,
    LipMode::Detailed,
    LipMode::Flap,
    LipMode::Volume,
    LipMode::Chatter,
];

impl LipMode {
    pub fn name(self) -> &'static str {
        match self {
            LipMode::Visemes => "Visemes",
            LipMode::Detailed => "Detailed",
            LipMode::Flap => "Flap",
            LipMode::Volume => "Volume",
            LipMode::Chatter => "Chatter",
        }
    }
    pub fn parse(s: &str) -> Option<LipMode> {
        LIP_MODES
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(s))
    }
}

/// Tunables. Levels in dBFS.
#[derive(Clone, Copy, Debug)]
pub struct PickerSettings {
    pub gate_db: f32,
    pub open_above_gate: f32,
    pub round_below: f32,
    pub bright_above: f32,
    pub min_hold_s: f32,
}

impl Default for PickerSettings {
    fn default() -> Self {
        PickerSettings {
            gate_db: -45.0,
            open_above_gate: 14.0,
            round_below: 0.17,
            bright_above: 0.48,
            min_hold_s: 0.07,
        }
    }
}

pub struct LipSync {
    pub settings: PickerSettings,
    pub mode: LipMode,
    current: Mouth,
    held: f32,
    /// Running average of your speaking level, so "open" means louder than *you* usually are.
    pub speech_db: f32,
    /// seconds since the voice was last above the gate
    pub silent_for: f32,
    /// recent loudness peak, falling 10 dB/s: dips below it mark gaps between syllables
    peak_db: f32,
    last_onsets: u32,
    chatter_t: f32,
    rng: u32,
    /// your corner vowels; calibrated with the vowel calibration
    pub refs: VowelRefs,
    /// read vowels from the formants (Visemes and Detailed styles)
    pub use_formants: bool,
}

impl LipSync {
    pub fn new(settings: PickerSettings, mode: LipMode) -> Self {
        LipSync {
            speech_db: settings.gate_db + settings.open_above_gate,
            settings,
            mode,
            current: Mouth::Rest,
            held: 1.0,
            silent_for: 999.0,
            peak_db: -100.0,
            last_onsets: 0,
            chatter_t: 0.0,
            rng: 0x1234_5678,
            refs: VowelRefs::default(),
            use_formants: true,
        }
    }

    fn rand(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn open_at(&self) -> f32 {
        let s = &self.settings;
        (self.speech_db + 1.5)
            .max(s.gate_db + 6.0)
            .min(s.gate_db + s.open_above_gate + 6.0)
    }

    /// Vowel from the formants, when the frame is clearly voiced and close to one of your corner
    /// vowels. `None` hands the decision back to the loudness/brightness rules.
    pub fn vowel_shape(&self, f: Frame) -> Option<Mouth> {
        if !self.use_formants || f.f1 <= 0.0 || f.f2 <= 0.0 || f.clarity < 0.6 {
            return None;
        }
        let rel = f.db - self.settings.gate_db;
        let (open, front) = self.refs.coords(f.f1, f.f2);
        // thresholds tuned on real recordings (6 speakers, tools/test/eval_voice.py)
        Some(if open > 0.55 {
            // jaw-open vowel (ah, aw, a, uh): wide unless it's barely above the gate
            if rel > 3.0 { Mouth::Open } else { Mouth::Mid }
        } else if front > 0.68 {
            Mouth::Bright // ee, i, eh: lips spread
        } else if open > 0.35 && front > 0.4 {
            Mouth::Mid // neutral, schwa-like
        } else {
            Mouth::Round // oo, oh, foot; modern English often fronts "oo", so anything not clearly spread counts
        })
    }

    /// Raw choice for one frame in the shape-based styles, before the minimum hold.
    pub fn classify(&self, f: Frame) -> Mouth {
        let s = &self.settings;
        let rel = f.db - s.gate_db;
        if rel < 0.0 {
            return Mouth::Rest;
        }
        match self.mode {
            LipMode::Detailed => {
                if f.high > 0.30 && f.zcr > 0.16 {
                    return Mouth::Teeth;
                }
                // a quick dip between syllables on a low, buzzy sound: lips pressed for m / b / p
                if f.db < self.peak_db - 5.0 && f.low > 0.6 && f.bright < 0.15 && f.zcr < 0.16 {
                    return Mouth::Closed;
                }
                if let Some(m) = self.vowel_shape(f) {
                    return m;
                }
                if f.bright > s.bright_above {
                    return Mouth::Bright;
                }
                if rel > 4.0 && f.bright < s.round_below {
                    return Mouth::Round;
                }
                if f.db > self.open_at() {
                    Mouth::Open
                } else {
                    Mouth::Mid
                }
            }
            LipMode::Flap => {
                // open on each syllable's peak, shut in the dips between them (with hysteresis)
                let below = if self.current == Mouth::Open {
                    5.0
                } else {
                    3.0
                };
                if rel > 2.0 && f.db > self.peak_db - below {
                    Mouth::Open
                } else {
                    Mouth::Rest
                }
            }
            LipMode::Volume => {
                if f.db > self.open_at() {
                    Mouth::Open
                } else if rel > 2.0 {
                    Mouth::Mid
                } else {
                    Mouth::Rest
                }
            }
            LipMode::Visemes | LipMode::Chatter => {
                if let Some(m) = self
                    .vowel_shape(f)
                    .filter(|_| self.mode == LipMode::Visemes)
                {
                    return m;
                }
                if f.bright > s.bright_above {
                    return Mouth::Bright;
                }
                if rel > 4.0 && f.bright < s.round_below {
                    return Mouth::Round;
                }
                if f.db > self.open_at() {
                    Mouth::Open
                } else {
                    Mouth::Mid
                }
            }
        }
    }

    /// Advance by `dt` seconds with the latest frame; returns the mouth to show.
    pub fn update(&mut self, f: Frame, dt: f32) -> Mouth {
        self.held += dt;
        let voiced = f.db > self.settings.gate_db;
        if voiced {
            let k = 1.0 - (-dt / 1.5).exp();
            self.speech_db += k * (f.db - self.speech_db);
            self.silent_for = 0.0;
        } else {
            self.silent_for += dt;
        }
        self.peak_db = (self.peak_db - 10.0 * dt).max(f.db);
        let onsets = f.onsets.wrapping_sub(self.last_onsets);
        self.last_onsets = f.onsets;

        if self.mode == LipMode::Chatter {
            if !voiced {
                if self.held >= 0.06 {
                    self.current = Mouth::Rest;
                }
                return self.current;
            }
            // new random shape on every syllable, and at least every 120 ms while talking
            self.chatter_t += dt;
            if (onsets > 0 || self.chatter_t > 0.12) && self.held >= 0.06 {
                const POOL: [Mouth; 6] = [
                    Mouth::Open,
                    Mouth::Mid,
                    Mouth::Round,
                    Mouth::Bright,
                    Mouth::Open,
                    Mouth::Teeth,
                ];
                let mut pick = POOL[(self.rand() % POOL.len() as u32) as usize];
                if pick == self.current {
                    pick = if pick == Mouth::Open {
                        Mouth::Mid
                    } else {
                        Mouth::Open
                    };
                }
                self.current = pick;
                self.held = 0.0;
                self.chatter_t = 0.0;
            }
            return self.current;
        }

        // Flap: a new syllable while the mouth is already open snaps it shut for a beat, so
        // run-on speech still flaps instead of hanging open
        if self.mode == LipMode::Flap
            && onsets > 0
            && self.current == Mouth::Open
            && self.held >= 0.12
        {
            self.current = Mouth::Rest;
            self.held = 0.0;
            return self.current;
        }
        let hold = match self.mode {
            LipMode::Flap => 0.09,
            LipMode::Detailed => 0.05,
            _ => self.settings.min_hold_s,
        };
        let want = self.classify(f);
        if want != self.current && self.held >= hold {
            self.current = want;
            self.held = 0.0;
        }
        self.current
    }
}

/// Speaking level 0..1 for meters and head bob.
pub fn level01(f: Frame, s: &PickerSettings) -> f32 {
    ((f.db - s.gate_db) / (s.open_above_gate + 10.0)).clamp(0.0, 1.0)
}

/// Tracks the quietest recent level so the gate can be set automatically.
pub struct NoiseFloor {
    pub floor_db: f32,
    samples: Vec<f32>,
}

impl NoiseFloor {
    pub fn new() -> Self {
        NoiseFloor {
            floor_db: -60.0,
            samples: Vec::new(),
        }
    }
    pub fn reset(&mut self) {
        self.samples.clear();
    }
    pub fn push(&mut self, db: f32) {
        self.samples.push(db);
    }
    /// 10th percentile of what was collected, i.e. room tone with the odd breath ignored.
    pub fn finish(&mut self) -> Option<f32> {
        if self.samples.len() < 20 {
            return None;
        }
        let mut v = self.samples.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.floor_db = v[v.len() / 10];
        self.samples.clear();
        Some(self.floor_db)
    }
}

/// Minimal 16-bit / float PCM WAV reader for the offline `--analyze` and `--wav` modes.
pub fn read_wav(path: &str) -> Result<(u32, usize, Vec<f32>), String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    parse_wav(&b)
}

/// Any 16-bit or float WAV, mixed down to mono.
pub fn decode_wav_mono(b: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let (rate, ch, s) = parse_wav(b)?;
    let ch = ch.max(1);
    Ok((
        rate,
        s.chunks(ch)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect(),
    ))
}

pub fn parse_wav(b: &[u8]) -> Result<(u32, usize, Vec<f32>), String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let (mut rate, mut ch, mut bits, mut fmt) = (0u32, 0usize, 0u16, 0u16);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        if id == b"fmt " {
            if body.len() < 16 {
                return Err("short format chunk".into());
            }
            fmt = u16::from_le_bytes([body[0], body[1]]);
            ch = u16::from_le_bytes([body[2], body[3]]) as usize;
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            let samples: Vec<f32> = match (fmt, bits) {
                (1, 16) => body
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                    .collect(),
                (3, 32) => body
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
                _ => return Err(format!("unsupported WAV format {fmt}/{bits}-bit")),
            };
            return Ok((rate, ch, samples));
        }
        i += 8 + len + (len & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const FS: u32 = 48000;
    fn tone(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(secs * FS as f32) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / FS as f32).sin())
            .collect()
    }
    fn noise(amp: f32, secs: f32) -> Vec<f32> {
        let mut s = 22222u32;
        (0..(secs * FS as f32) as usize)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                amp * ((s as f32 / u32::MAX as f32) * 2.0 - 1.0)
            })
            .collect()
    }
    fn run(sig: &[f32], mode: LipMode) -> Vec<Mouth> {
        run_with(sig, mode, true)
    }
    /// pure test tones aren't speech, so the tone-rule tests switch the formant reading off
    fn run_tones(sig: &[f32], mode: LipMode) -> Vec<Mouth> {
        run_with(sig, mode, false)
    }
    fn run_with(sig: &[f32], mode: LipMode, formants: bool) -> Vec<Mouth> {
        let mut a = Analyzer::new(FS);
        let mut p = LipSync::new(PickerSettings::default(), mode);
        p.use_formants = formants;
        let mut out = vec![];
        a.process(sig, 1, |f| out.push(p.update(f, 0.01)));
        out
    }
    #[test]
    fn silence_is_rest_in_every_mode() {
        for m in LIP_MODES {
            assert!(
                run(&vec![0.0; 48000], m).iter().all(|v| *v == Mouth::Rest),
                "{m:?}"
            );
        }
    }
    #[test]
    fn loud_low_tone_is_round_or_open() {
        let last = *run_tones(&tone(200.0, 0.5, 1.0), LipMode::Visemes)
            .last()
            .unwrap();
        assert!(last == Mouth::Round || last == Mouth::Open, "{last:?}");
    }
    #[test]
    fn whistle_is_bright() {
        assert_eq!(
            *run_tones(&tone(3000.0, 0.2, 1.0), LipMode::Visemes)
                .last()
                .unwrap(),
            Mouth::Bright
        );
    }
    #[test]
    fn hiss_is_teeth_in_detailed() {
        assert_eq!(
            *run(&noise(0.2, 1.0), LipMode::Detailed).last().unwrap(),
            Mouth::Teeth
        );
    }
    #[test]
    fn quiet_tone_is_mid() {
        assert_eq!(
            *run_tones(&tone(700.0, 0.02, 1.0), LipMode::Visemes)
                .last()
                .unwrap(),
            Mouth::Mid
        );
    }
    #[test]
    fn flap_is_binary() {
        let v = run(&tone(300.0, 0.3, 1.0), LipMode::Flap);
        assert!(v.iter().all(|m| *m == Mouth::Open || *m == Mouth::Rest));
        assert_eq!(*v.last().unwrap(), Mouth::Open);
    }
    #[test]
    fn chatter_keeps_changing_while_voiced() {
        let v = run(&tone(300.0, 0.3, 1.0), LipMode::Chatter);
        let changes = v.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(changes >= 5, "{changes}");
    }
    #[test]
    fn formants_pick_the_vowel_shape() {
        use crate::voice::tests::synth_vowel;
        for (f1, f2, want) in [
            (740.0, 1220.0, Mouth::Open),
            (300.0, 2250.0, Mouth::Bright),
            (330.0, 880.0, Mouth::Round),
        ] {
            let sig: Vec<f32> = synth_vowel(FS as f32, 120.0, f1, f2, 0.6);
            assert_eq!(
                *run(&sig, LipMode::Visemes).last().unwrap(),
                want,
                "F1 {f1} F2 {f2}"
            );
        }
    }

    #[test]
    fn syllables_count_as_onsets() {
        let mut sig = vec![];
        for _ in 0..4 {
            sig.extend(tone(250.0, 0.3, 0.15));
            sig.extend(vec![0.0; (0.15 * FS as f32) as usize]);
        }
        let mut a = Analyzer::new(FS);
        let mut last = 0;
        a.process(&sig, 1, |f| last = f.onsets);
        assert_eq!(last, 4);
    }
}
