//! Voice analysis beyond loudness: pitch (for body language) and the first two formants (for
//! vowels). Pure Rust, runs on the audio thread in well under a millisecond per 10 ms frame.
//!
//! * The signal is low-passed and decimated to about 11-16 kHz, which is all speech needs.
//! * Pitch: YIN (de Cheveigne & Kawahara 2002) on a 40 ms window, 70-400 Hz, with a clarity score
//!   (1 = clean periodic voice, 0 = noise) that doubles as the voicing decision.
//! * Formants: Burg LPC (order fs/1000 + 2) on a 25 ms pre-emphasised window; F1 and F2 come from
//!   the roots of the predictor, as in Praat. F1 tracks how open the jaw is (ee/oo ~300 Hz, ah ~750 Hz); F2 tracks
//!   lip spread versus rounding (oo ~900 Hz, ee ~2300 Hz). A median of the last 5 voiced frames
//!   removes one-frame glitches.
//! * [`Prosody`] turns pitch and loudness into body-language events: stressed beats, questions
//!   (pitch rising at the end of a phrase, then a pause) and raised brows on high pitch.
//! * [`VowelRefs`] holds where *your* "ah", "ee" and "oo" sit, and [`VowelCalibrator`] measures them.

use std::collections::VecDeque;
use std::f32::consts::PI;

/// Second-order low-pass (RBJ cookbook), used twice in a row before decimating.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn lowpass(fs: f32, fc: f32, q: f32) -> Self {
        let w = 2.0 * PI * fc / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        Biquad {
            b0: (1.0 - c) / 2.0 / a0,
            b1: (1.0 - c) / a0,
            b2: (1.0 - c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// What the voice tracker reports each 10 ms.
#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceFrame {
    /// fundamental frequency in Hz, 0 when not voiced
    pub pitch_hz: f32,
    /// 0..1, how periodic the signal is (voiced speech is usually above 0.7)
    pub clarity: f32,
    /// first and second formant in Hz, 0 when unknown
    pub f1: f32,
    pub f2: f32,
}

pub struct VoiceTracker {
    q: usize,
    phase: usize,
    pub fs: f32,
    lp: [Biquad; 2],
    ring: VecDeque<f32>,
    cap: usize,
    f1_hist: VecDeque<f32>,
    f2_hist: VecDeque<f32>,
    scratch: Vec<f32>,
    pub last: VoiceFrame,
}

impl VoiceTracker {
    pub fn new(sample_rate: u32) -> Self {
        let fs_in = sample_rate as f32;
        let q = ((fs_in / 11025.0).floor() as usize).max(1);
        let fs = fs_in / q as f32;
        let fc = (0.42 * fs).min(fs_in * 0.45);
        let cap = (0.040 * fs).round() as usize;
        VoiceTracker {
            q,
            phase: 0,
            fs,
            lp: [
                Biquad::lowpass(fs_in, fc, 0.54),
                Biquad::lowpass(fs_in, fc, 1.31),
            ],
            ring: VecDeque::with_capacity(cap + 1),
            cap,
            f1_hist: VecDeque::new(),
            f2_hist: VecDeque::new(),
            scratch: Vec::new(),
            last: VoiceFrame::default(),
        }
    }

    /// One mono input sample.
    #[inline]
    pub fn push(&mut self, x: f32) {
        let y = if self.q > 1 {
            let a = self.lp[0].run(x);
            self.lp[1].run(a)
        } else {
            x
        };
        self.phase += 1;
        if self.phase >= self.q {
            self.phase = 0;
            if self.ring.len() == self.cap {
                self.ring.pop_front();
            }
            self.ring.push_back(y);
        }
    }

    /// Analyse the most recent window. `active` is false in silence, which skips the work.
    pub fn analyse(&mut self, active: bool) -> VoiceFrame {
        if !active || self.ring.len() < self.cap {
            self.last = VoiceFrame::default();
            return self.last;
        }
        self.scratch.clear();
        self.scratch.extend(self.ring.iter());
        let (pitch, clarity) = yin(&self.scratch, self.fs);
        let mut out = VoiceFrame {
            pitch_hz: pitch,
            clarity,
            f1: 0.0,
            f2: 0.0,
        };
        if clarity > 0.5 {
            let n = (0.025 * self.fs).round() as usize;
            let win = &self.scratch[self.scratch.len() - n..];
            if let Some((f1, f2)) = formants(win, self.fs) {
                push_hist(&mut self.f1_hist, f1);
                push_hist(&mut self.f2_hist, f2);
                out.f1 = median(&self.f1_hist);
                out.f2 = median(&self.f2_hist);
            }
        } else {
            self.f1_hist.clear();
            self.f2_hist.clear();
        }
        self.last = out;
        out
    }
}

fn push_hist(h: &mut VecDeque<f32>, v: f32) {
    if h.len() == 5 {
        h.pop_front();
    }
    h.push_back(v);
}

fn median(h: &VecDeque<f32>) -> f32 {
    let mut v: Vec<f32> = h.iter().copied().collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// YIN pitch: returns (Hz, clarity); Hz is 0 when nothing periodic was found.
pub fn yin(x: &[f32], fs: f32) -> (f32, f32) {
    let min_lag = (fs / 400.0).floor() as usize;
    let max_lag = (fs / 70.0).ceil() as usize;
    if x.len() < max_lag * 2 {
        return (0.0, 0.0);
    }
    let w = x.len() - max_lag;
    let mut d = vec![0.0f32; max_lag + 1];
    for tau in 1..=max_lag {
        let mut s = 0.0;
        for j in 0..w {
            let e = x[j] - x[j + tau];
            s += e * e;
        }
        d[tau] = s;
    }
    // cumulative mean normalised difference
    let mut cmnd = vec![1.0f32; max_lag + 1];
    let mut run = 0.0;
    for tau in 1..=max_lag {
        run += d[tau];
        cmnd[tau] = if run > 0.0 {
            d[tau] * tau as f32 / run
        } else {
            1.0
        };
    }
    let mut best = None;
    let mut tau = min_lag.max(2);
    while tau < max_lag {
        if cmnd[tau] < 0.15 {
            while tau + 1 < max_lag && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            best = Some(tau);
            break;
        }
        tau += 1;
    }
    let tau = match best {
        Some(t) => t,
        None => {
            // no dip under the threshold: take the deepest one, and let clarity say how good it is
            let t = (min_lag.max(2)..max_lag)
                .min_by(|a, b| cmnd[*a].partial_cmp(&cmnd[*b]).unwrap())
                .unwrap();
            if cmnd[t] > 0.45 {
                return (0.0, (1.0 - cmnd[t]).max(0.0));
            }
            t
        }
    };
    // parabolic interpolation for sub-sample accuracy
    let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
    let den = a - 2.0 * b + c;
    let shift = if den.abs() > 1e-9 {
        0.5 * (a - c) / den
    } else {
        0.0
    };
    let period = tau as f32 + shift.clamp(-1.0, 1.0);
    (fs / period, (1.0 - b).clamp(0.0, 1.0))
}

/// F1, F2 the way Praat does it: Burg LPC (order fs/1000 + 2) on a pre-emphasised Gaussian-ish
/// window, then the roots of the predictor polynomial; each root with a narrow bandwidth is a
/// formant at its angle. Checked against Praat on LibriSpeech recordings (see tools/test/eval_voice.py).
pub fn formants(win: &[f32], fs: f32) -> Option<(f32, f32)> {
    let n = win.len();
    let order = ((fs / 1000.0).round() as usize + 2).clamp(8, 20);
    if n < order * 4 {
        return None;
    }
    // pre-emphasis from 50 Hz, then a Hann-type window (Praat uses a Gaussian; close enough)
    let alpha = (-2.0 * std::f64::consts::PI * 50.0 / fs as f64).exp();
    let mut x = vec![0.0f64; n];
    let mut prev = 0.0f64;
    for (i, &v) in win.iter().enumerate() {
        let v = v as f64;
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
        x[i] = (v - alpha * prev) * w;
        prev = v;
    }
    let a = burg(&x, order)?;
    let roots = poly_roots(&a)?;
    let mut fr: Vec<f32> = roots
        .iter()
        .filter(|r| r.1 > 0.0)
        .filter_map(|&(re, im)| {
            let mag = (re * re + im * im).sqrt();
            let f = im.atan2(re) * fs as f64 / (2.0 * std::f64::consts::PI);
            let bw = -mag.ln() * fs as f64 / std::f64::consts::PI;
            (f > 90.0 && bw < 600.0).then_some(f as f32)
        })
        .collect();
    fr.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let f1 = *fr.iter().find(|&&f| (200.0..=1100.0).contains(&f))?;
    let f2 = *fr
        .iter()
        .find(|&&f| f > f1 + 100.0 && (500.0..=3300.0).contains(&f))?;
    Some((f1, f2))
}

/// Burg's method: predictor coefficients a[0..=p] with a[0] = 1.
fn burg(x: &[f64], p: usize) -> Option<Vec<f64>> {
    let n = x.len();
    let (mut f, mut b) = (x.to_vec(), x.to_vec());
    let mut a = vec![1.0f64];
    for m in 0..p {
        let (mut num, mut den) = (0.0, 0.0);
        for i in (m + 1)..n {
            num += f[i] * b[i - 1];
            den += f[i] * f[i] + b[i - 1] * b[i - 1];
        }
        if den <= 1e-20 {
            return None;
        }
        let k = -2.0 * num / den;
        let mut a2 = a.clone();
        a2.push(0.0);
        for j in 1..=m + 1 {
            a2[j] += k * a[m + 1 - j];
        }
        a = a2;
        for i in ((m + 1)..n).rev() {
            let fi = f[i];
            f[i] = fi + k * b[i - 1];
            b[i] = b[i - 1] + k * fi;
        }
    }
    Some(a)
}

/// Roots of z^p + a1 z^(p-1) + ... + ap (Durand-Kerner), as (re, im).
fn poly_roots(a: &[f64]) -> Option<Vec<(f64, f64)>> {
    let p = a.len() - 1;
    type C = (f64, f64);
    let mul = |x: C, y: C| (x.0 * y.0 - x.1 * y.1, x.0 * y.1 + x.1 * y.0);
    let div = |x: C, y: C| {
        let d = y.0 * y.0 + y.1 * y.1;
        ((x.0 * y.0 + x.1 * y.1) / d, (x.1 * y.0 - x.0 * y.1) / d)
    };
    let eval = |z: C| {
        a.iter().fold((0.0, 0.0), |acc, &c| {
            let m = mul(acc, z);
            (m.0 + c, m.1)
        })
    };
    let mut z: Vec<C> = (0..p)
        .map(|k| {
            let ang = 2.0 * std::f64::consts::PI * k as f64 / p as f64 + 0.4;
            (0.9 * ang.cos(), 0.9 * ang.sin())
        })
        .collect();
    for _ in 0..200 {
        let mut moved = 0.0f64;
        for i in 0..p {
            let mut den = (1.0, 0.0);
            for j in 0..p {
                if i != j {
                    den = mul(den, (z[i].0 - z[j].0, z[i].1 - z[j].1));
                }
            }
            if den.0 == 0.0 && den.1 == 0.0 {
                continue;
            }
            let step = div(eval(z[i]), den);
            z[i] = (z[i].0 - step.0, z[i].1 - step.1);
            moved = moved.max(step.0.abs() + step.1.abs());
        }
        if moved < 1e-10 {
            break;
        }
    }
    z.iter()
        .all(|c| c.0.is_finite() && c.1.is_finite())
        .then_some(z)
}

// ------------------------------------------------------------------------------------------
// Vowels
// ------------------------------------------------------------------------------------------

/// Where a speaker's three corner vowels sit, as (F1, F2) in Hz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VowelRefs {
    pub ah: (f32, f32),
    pub ee: (f32, f32),
    pub oo: (f32, f32),
}

impl Default for VowelRefs {
    /// Averages across adult voices (F1 nudged up the way LPC reads low formants); calibration
    /// replaces them with yours.
    fn default() -> Self {
        VowelRefs {
            ah: (740.0, 1220.0),
            ee: (340.0, 2250.0),
            oo: (370.0, 900.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vowel {
    Ah,
    Ee,
    Oo,
}

impl VowelRefs {
    /// Nearest corner vowel and how far away it is (in log-frequency units; ~0.1 is "close").
    pub fn nearest(&self, f1: f32, f2: f32) -> (Vowel, f32) {
        let d = |r: (f32, f32)| {
            let a = (f1.ln() - r.0.ln()) * 1.3;
            let b = f2.ln() - r.1.ln();
            (a * a + b * b).sqrt()
        };
        [
            (Vowel::Ah, d(self.ah)),
            (Vowel::Ee, d(self.ee)),
            (Vowel::Oo, d(self.oo)),
        ]
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap()
    }

    /// Position in your own vowel space: `open` 0 = closed jaw (ee/oo) .. 1 = ah; `front` 0 = rounded
    /// (oo) .. 1 = spread (ee). Normalised by your corner vowels, so voice pitch and size cancel out.
    pub fn coords(&self, f1: f32, f2: f32) -> (f32, f32) {
        let closed = (self.ee.0.ln() + self.oo.0.ln()) * 0.5;
        let open = (f1.ln() - closed) / (self.ah.0.ln() - closed).max(0.05);
        let front = (f2.ln() - self.oo.1.ln()) / (self.ee.1.ln() - self.oo.1.ln()).max(0.05);
        (open, front)
    }

    pub fn to_setting(&self) -> String {
        format!(
            "{:.0},{:.0},{:.0},{:.0},{:.0},{:.0}",
            self.ah.0, self.ah.1, self.ee.0, self.ee.1, self.oo.0, self.oo.1
        )
    }

    pub fn from_setting(s: &str) -> Option<Self> {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 6).then(|| VowelRefs {
            ah: (v[0], v[1]),
            ee: (v[2], v[3]),
            oo: (v[4], v[5]),
        })
    }
}

/// Collects (F1, F2) while you hold one vowel; the median is your reference for it.
#[derive(Default)]
pub struct VowelCalibrator {
    f1: Vec<f32>,
    f2: Vec<f32>,
}

impl VowelCalibrator {
    pub fn push(&mut self, v: VoiceFrame) {
        if v.clarity > 0.6 && v.f1 > 0.0 && v.f2 > 0.0 {
            self.f1.push(v.f1);
            self.f2.push(v.f2);
        }
    }
    pub fn finish(&mut self) -> Option<(f32, f32)> {
        if self.f1.len() < 25 {
            return None;
        }
        let med = |v: &mut Vec<f32>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let r = (med(&mut self.f1), med(&mut self.f2));
        self.f1.clear();
        self.f2.clear();
        Some(r)
    }
}

// ------------------------------------------------------------------------------------------
// Prosody: pitch and stress -> body language
// ------------------------------------------------------------------------------------------

#[derive(Default, Debug)]
pub struct ProsodyEvents {
    /// a stressed syllable: nod / hand beat, strength 0..1
    pub beat: Option<f32>,
    /// the phrase that just ended rose in pitch and was followed by a pause: most yes/no questions
    /// do this, but so do lists and "uptalk", so it drives a curious head tilt, not a question mark
    pub question: bool,
}

pub struct Prosody {
    /// running baseline of your pitch (log2 Hz)
    base: f32,
    /// smoothed pitch relative to the baseline, in semitones
    pub rel_st: f32,
    spread: f32,
    t: f32,
    voiced_run: f32,
    silent_run: f32,
    hist: VecDeque<(f32, f32)>,
    last_onsets: u32,
    last_beat: f32,
    high_since: Option<f32>,
    /// pitch is clearly up: raise the brows
    pub brows_up: bool,
    /// 0.6..1.6: how animated the voice is lately (pitch movement), scales head motion
    pub liveliness: f32,
    pub voiced: bool,
    pub pitch_hz: f32,
}

impl Default for Prosody {
    fn default() -> Self {
        Prosody {
            base: 140f32.log2(),
            rel_st: 0.0,
            spread: 2.0,
            t: 0.0,
            voiced_run: 0.0,
            silent_run: 1.0,
            hist: VecDeque::new(),
            last_onsets: 0,
            last_beat: -10.0,
            high_since: None,
            brows_up: false,
            liveliness: 1.0,
            voiced: false,
            pitch_hz: 0.0,
        }
    }
}

impl Prosody {
    /// `db` and `onsets` come from the loudness analysis, `gate_db` / `speech_db` from the lip sync.
    pub fn update(
        &mut self,
        v: VoiceFrame,
        db: f32,
        onsets: u32,
        dt: f32,
        gate_db: f32,
        speech_db: f32,
    ) -> ProsodyEvents {
        let mut ev = ProsodyEvents::default();
        self.t += dt;
        let voiced = db > gate_db && v.pitch_hz > 0.0 && v.clarity > 0.6;
        self.voiced = voiced;
        if voiced {
            self.pitch_hz = v.pitch_hz;
            let lp = v.pitch_hz.log2();
            // baseline settles over ~3 s of talking; first contact jumps straight to it
            if self.voiced_run == 0.0 && self.hist.is_empty() && (self.base - lp).abs() > 1.0 {
                self.base = lp;
            }
            self.base += (1.0 - (-dt / 3.0).exp()) * (lp - self.base);
            let st = 12.0 * (lp - self.base);
            let prev = self.rel_st;
            self.rel_st += (1.0 - (-dt / 0.06).exp()) * (st - self.rel_st);
            self.spread += (1.0 - (-dt / 2.0).exp())
                * ((self.rel_st - prev).abs() / dt.max(1e-3) * 0.1 - self.spread);
            self.liveliness = (0.6 + self.spread / 2.0).clamp(0.6, 1.6);
            // one-frame octave slips are the commonest pitch-tracker error; leave them out
            if self.hist.back().is_none_or(|h| (h.1 - st).abs() < 7.0) {
                self.hist.push_back((self.t, st));
            }
            self.voiced_run += dt;
            self.silent_run = 0.0;
        } else {
            self.silent_run += dt;
            self.rel_st *= (-dt / 0.3).exp();
        }
        while self.hist.front().is_some_and(|h| self.t - h.0 > 1.2) {
            self.hist.pop_front();
        }
        // phrase end: a quarter second of no voicing after at least half a second of voice
        // phrase end: a pause of 0.6 s after at least half a second of voice. Shorter pauses are
        // mostly commas, where read and spoken English rise just as much as in a question
        // (measured on LJSpeech and LibriSpeech recordings), so they don't count.
        if self.silent_run >= 0.6 && self.voiced_run >= 0.5 {
            if let Some(&(end, _)) = self.hist.back() {
                let mean = |a: f32, b: f32| {
                    let s: Vec<f32> = self
                        .hist
                        .iter()
                        .filter(|h| h.0 > end - a && h.0 <= end - b)
                        .map(|h| h.1)
                        .collect();
                    (!s.is_empty()).then(|| s.iter().sum::<f32>() / s.len() as f32)
                };
                if let (Some(tail), Some(prior)) = (mean(0.12, 0.0), mean(0.5, 0.2)) {
                    ev.question = tail - prior > 4.0;
                }
            }
            self.voiced_run = 0.0;
            self.hist.clear();
        }
        // stressed beat: a syllable start that's louder or higher than usual
        if onsets != self.last_onsets {
            self.last_onsets = onsets;
            if db > gate_db && self.t - self.last_beat > 0.3 {
                let s =
                    ((db - (speech_db - 2.0)) / 8.0 + self.rel_st.max(0.0) / 6.0).clamp(0.0, 1.0);
                if s > 0.25 {
                    ev.beat = Some(s);
                    self.last_beat = self.t;
                }
            }
        }
        // brows ride the pitch
        if voiced && self.rel_st > 3.5 {
            let since = *self.high_since.get_or_insert(self.t);
            if self.t - since > 0.15 {
                self.brows_up = true;
            }
        } else {
            self.high_since = None;
            if self.rel_st < 2.0 || self.silent_run > 0.3 {
                self.brows_up = false;
            }
        }
        ev
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Formant synthesis: a glottal pulse train through three resonators, like the classic
    /// Klatt-style synthesiser. Good enough to check that F1/F2 come back out.
    pub fn synth_vowel(fs: f32, f0: f32, f1: f32, f2: f32, secs: f32) -> Vec<f32> {
        synth_glide(fs, f0, f0, f1, f2, secs)
    }

    pub fn synth_glide(fs: f32, f0a: f32, f0b: f32, f1: f32, f2: f32, secs: f32) -> Vec<f32> {
        let n = (secs * fs) as usize;
        let mut phase = 0.0f32;
        let mut src = Vec::with_capacity(n);
        for i in 0..n {
            let f0 = f0a + (f0b - f0a) * i as f32 / n as f32;
            phase += f0 / fs;
            // differentiated glottal pulse: a sharp negative spike each period, then decay
            let p = phase.fract();
            src.push(if p < f0 / fs { -1.0 } else { 0.0 } + 0.0);
            if phase > 1e6 {
                phase = phase.fract();
            }
        }
        let mut y = src;
        for (f, bw) in [(f1, 80.0), (f2, 100.0), (2800.0, 140.0)] {
            let r = (-PI * bw / fs).exp();
            let c = 2.0 * r * (2.0 * PI * f / fs).cos();
            let (mut y1, mut y2) = (0.0f32, 0.0f32);
            for v in y.iter_mut() {
                let o = *v + c * y1 - r * r * y2;
                y2 = y1;
                y1 = o;
                *v = o;
            }
        }
        let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        y.iter().map(|v| v / peak * 0.3).collect()
    }

    fn track(sig: &[f32], fs: u32) -> Vec<VoiceFrame> {
        let mut t = VoiceTracker::new(fs);
        let hop = (fs / 100) as usize;
        let mut out = vec![];
        for (i, &x) in sig.iter().enumerate() {
            t.push(x);
            if (i + 1) % hop == 0 {
                out.push(t.analyse(true));
            }
        }
        out
    }

    #[test]
    fn pitch_is_accurate() {
        for (fs, f0) in [(48000u32, 110.0f32), (44100, 180.0), (16000, 240.0)] {
            let v = track(&synth_vowel(fs as f32, f0, 700.0, 1200.0, 0.5), fs);
            let last = v.last().unwrap();
            assert!(
                (last.pitch_hz - f0).abs() / f0 < 0.02,
                "fs {fs}: {} vs {f0}",
                last.pitch_hz
            );
            assert!(last.clarity > 0.8);
        }
    }

    #[test]
    fn formants_come_back_out() {
        for (f1, f2) in [
            (740.0f32, 1220.0f32),
            (300.0, 2250.0),
            (330.0, 880.0),
            (520.0, 1850.0),
        ] {
            let v = track(&synth_vowel(48000.0, 120.0, f1, f2, 0.5), 48000);
            let last = v.last().unwrap();
            // LPC pulls a low F1 toward the nearest pitch harmonic; 25% still leaves the vowels apart
            println!("F1 {:.0}/{f1} F2 {:.0}/{f2}", last.f1, last.f2);
            assert!((last.f1 - f1).abs() / f1 < 0.25, "F1 {} vs {f1}", last.f1);
            assert!((last.f2 - f2).abs() / f2 < 0.15, "F2 {} vs {f2}", last.f2);
        }
    }

    #[test]
    fn noise_is_unvoiced() {
        let mut s = 99u32;
        let sig: Vec<f32> = (0..24000)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as f32 / u32::MAX as f32 - 0.5) * 0.4
            })
            .collect();
        let v = track(&sig, 48000);
        assert!(v.last().unwrap().clarity < 0.6);
    }

    #[test]
    fn corner_vowels_classify() {
        let r = VowelRefs::default();
        assert_eq!(r.nearest(760.0, 1180.0).0, Vowel::Ah);
        assert_eq!(r.nearest(290.0, 2300.0).0, Vowel::Ee);
        assert_eq!(r.nearest(340.0, 860.0).0, Vowel::Oo);
        assert_eq!(VowelRefs::from_setting(&r.to_setting()), Some(r));
    }

    fn run_prosody(sig: &[f32], fs: u32) -> (usize, usize) {
        let mut t = VoiceTracker::new(fs);
        let mut p = Prosody::default();
        let hop = (fs / 100) as usize;
        let (mut q, mut beats) = (0, 0);
        let mut e = 0.0f32;
        for (i, &x) in sig.iter().enumerate() {
            t.push(x);
            e += x * x;
            if (i + 1) % hop == 0 {
                let db = 10.0 * (e / hop as f32).max(1e-10).log10();
                e = 0.0;
                let v = t.analyse(db > -60.0);
                let ev = p.update(v, db, 0, 0.01, -45.0, -20.0);
                q += ev.question as usize;
                beats += ev.beat.is_some() as usize;
            }
        }
        (q, beats)
    }

    #[test]
    fn rising_phrase_is_a_question_flat_is_not() {
        let fs = 16000u32;
        let mut rise = synth_vowel(fs as f32, 130.0, 600.0, 1400.0, 0.7);
        rise.extend(synth_glide(fs as f32, 130.0, 210.0, 600.0, 1400.0, 0.35));
        rise.extend(vec![0.0; 16000]);
        assert_eq!(run_prosody(&rise, fs).0, 1);
        let mut flat = synth_vowel(fs as f32, 130.0, 600.0, 1400.0, 1.05);
        flat.extend(vec![0.0; 16000]);
        assert_eq!(run_prosody(&flat, fs).0, 0);
        let mut fall = synth_vowel(fs as f32, 160.0, 600.0, 1400.0, 0.7);
        fall.extend(synth_glide(fs as f32, 160.0, 110.0, 600.0, 1400.0, 0.35));
        fall.extend(vec![0.0; 16000]);
        assert_eq!(run_prosody(&fall, fs).0, 0);
    }

    #[test]
    fn calibrator_takes_the_median() {
        let mut c = VowelCalibrator::default();
        for i in 0..40 {
            c.push(VoiceFrame {
                pitch_hz: 120.0,
                clarity: 0.9,
                f1: 700.0 + (i % 5) as f32,
                f2: 1200.0,
            });
        }
        c.push(VoiceFrame {
            pitch_hz: 120.0,
            clarity: 0.9,
            f1: 3000.0,
            f2: 9000.0,
        });
        let (f1, f2) = c.finish().unwrap();
        assert!((700.0..=705.0).contains(&f1) && f2 == 1200.0);
    }
}
