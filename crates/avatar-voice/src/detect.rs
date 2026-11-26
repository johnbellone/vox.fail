//! Laugh and sing detection from the mic analysis (no speech-to-text needed).
//!
//! - **Laughing** comes in quick, evenly spaced bursts ("ha-ha-ha", 3 to 8 a second), the same
//!   vowel every time and breathy. Speech has syllables too, but unevenly spaced, with changing
//!   vowels and cleaner voicing. Four such bursts in 1.6 s is a laugh.
//! - **Singing** holds notes: the pitch stays on one note for a quarter second or more, again and
//!   again. Speech glides between pitches and rarely holds one that long.
//!
//! Your character cracks up when you laugh, and brings out a microphone and sways while you sing.
//! Both are tuned and tested on real recordings (testdata/real/laugh, testdata/real/sing, and the
//! speech in testdata/real and demo/ as the "shouldn't fire" side).

use std::collections::VecDeque;

use crate::lipsync::Frame;

/// 10 ms frames
const SING_WIN: usize = 300;
const LAUGH_WIN: usize = 160;

fn semis(hz: f32) -> f32 {
    12.0 * (hz / 110.0).log2()
}

/// Frames of held notes in a window: runs of voiced frames that stay within about a semitone of
/// where they started (vibrato allowed), at least `min_run` long.
pub fn held_frames(w: &[Frame], gate: f32, min_run: usize) -> usize {
    let mut held = 0;
    let mut run = 0usize;
    let mut anchor = 0.0f32;
    for f in w {
        let voiced = f.pitch_hz > 0.0 && f.clarity > 0.55 && f.db > gate;
        if voiced {
            let s = semis(f.pitch_hz);
            if run > 0 && (s - anchor).abs() < 1.0 {
                run += 1;
                // follow slow drift, not jumps
                anchor += 0.05 * (s - anchor);
                continue;
            }
            if run >= min_run {
                held += run;
            }
            run = 1;
            anchor = s;
        } else {
            if run >= min_run {
                held += run;
            }
            run = 0;
        }
    }
    if run >= min_run {
        held += run;
    }
    held
}

/// Loudness peaks ("ha"s): (frame index, F2) of local maxima at least `prom` dB above the dips
/// on both sides and at least 90 ms apart.
pub fn bursts(w: &[Frame], gate: f32, prom: f32) -> Vec<(usize, f32)> {
    // smooth 30 ms
    let d: Vec<f32> = (0..w.len())
        .map(|i| {
            let a = i.saturating_sub(1);
            let b = (i + 2).min(w.len());
            w[a..b].iter().map(|f| f.db).sum::<f32>() / (b - a) as f32
        })
        .collect();
    let mut peaks: Vec<(usize, f32)> = vec![];
    for i in 1..d.len().saturating_sub(1) {
        if d[i] < gate || d[i] < d[i - 1] || d[i] < d[i + 1] {
            continue;
        }
        // dips within 150 ms either side
        let lo_l = d[i.saturating_sub(15)..i]
            .iter()
            .cloned()
            .fold(f32::INFINITY, f32::min);
        let lo_r = d[i + 1..(i + 16).min(d.len())]
            .iter()
            .cloned()
            .fold(f32::INFINITY, f32::min);
        if d[i] - lo_l < prom || d[i] - lo_r < prom {
            continue;
        }
        if let Some(last) = peaks.last() {
            if i - last.0 < 9 {
                continue;
            }
        }
        peaks.push((i, w[i].f2));
    }
    peaks
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LaughStats {
    pub n: usize,
    /// spacing regularity, 1 = metronome
    pub regular: f32,
    /// mean spacing, seconds
    pub period: f32,
    /// how much the vowel (F2) changes between bursts, relative
    pub vowel_var: f32,
    /// pitch spread across the bursts, semitones (for tuning)
    #[allow(dead_code)]
    pub pitch_sd: f32,
    /// mean burst width (frames within 6 dB of its peak) (for tuning)
    #[allow(dead_code)]
    pub width: f32,
    /// voicing (clarity) at the peaks
    pub clarity: f32,
}

pub fn laugh_stats(w: &[Frame], gate: f32) -> LaughStats {
    laugh_stats_p(w, gate, PROM)
}

pub const PROM: f32 = 4.0;

pub fn laugh_stats_p(w: &[Frame], gate: f32, prom: f32) -> LaughStats {
    // only bursts within 20 dB of the loudest one count, whatever the room's noise gate is
    let top = w.iter().map(|f| f.db).fold(-200.0f32, f32::max);
    let p = bursts(w, gate.max(top - 20.0), prom);
    if p.len() < 3 {
        return LaughStats {
            n: p.len(),
            ..Default::default()
        };
    }
    let gaps: Vec<f32> = p
        .windows(2)
        .map(|x| (x[1].0 - x[0].0) as f32 * 0.01)
        .collect();
    let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
    let sd = (gaps.iter().map(|g| (g - mean).powi(2)).sum::<f32>() / gaps.len() as f32).sqrt();
    let f2: Vec<f32> = p.iter().map(|x| x.1).filter(|v| *v > 0.0).collect();
    let vowel_var = if f2.len() >= 2 {
        let m = f2.iter().sum::<f32>() / f2.len() as f32;
        (f2.iter().map(|v| (v - m).powi(2)).sum::<f32>() / f2.len() as f32).sqrt() / m.max(1.0)
    } else {
        1.0
    };
    let st: Vec<f32> = p
        .iter()
        .map(|x| w[x.0].pitch_hz)
        .filter(|h| *h > 0.0)
        .map(semis)
        .collect();
    let pitch_sd = if st.len() >= 2 {
        let m = st.iter().sum::<f32>() / st.len() as f32;
        (st.iter().map(|v| (v - m).powi(2)).sum::<f32>() / st.len() as f32).sqrt()
    } else {
        0.0
    };
    let width = p
        .iter()
        .map(|&(i, _)| {
            let pk = w[i].db;
            let l = (0..i).rev().take_while(|&j| w[j].db > pk - 6.0).count();
            let r = (i + 1..w.len()).take_while(|&j| w[j].db > pk - 6.0).count();
            (l + r + 1) as f32
        })
        .sum::<f32>()
        / p.len() as f32;
    let clarity = p.iter().map(|x| w[x.0].clarity).sum::<f32>() / p.len() as f32;
    LaughStats {
        n: p.len(),
        regular: 1.0 - (sd / mean.max(1e-3)).min(1.0),
        period: mean,
        vowel_var,
        pitch_sd,
        width,
        clarity,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Laugh,
    SingStart,
    SingStop,
}

/// Streaming detector: feed it every analysis frame.
pub struct Detector {
    hist: VecDeque<Frame>,
    pub singing: bool,
    since_laugh: usize,
    sing_hold: usize,
    pub gate: f32,
    pub laugh_on: bool,
    pub sing_on: bool,
}

impl Default for Detector {
    fn default() -> Self {
        Detector {
            hist: VecDeque::new(),
            singing: false,
            since_laugh: 1000,
            sing_hold: 0,
            gate: -45.0,
            laugh_on: true,
            sing_on: true,
        }
    }
}

impl Detector {
    pub fn push(&mut self, f: Frame) -> Option<Event> {
        self.hist.push_back(f);
        if self.hist.len() > SING_WIN {
            self.hist.pop_front();
        }
        self.since_laugh += 1;
        let n = self.hist.len();
        // check 10 times a second
        if n < LAUGH_WIN || n % 10 != 0 && self.hist.len() == SING_WIN && self.since_laugh % 10 != 0
        {
            return None;
        }
        let v: Vec<Frame> = self.hist.iter().copied().collect();
        if self.sing_on {
            let held = held_frames(&v, self.gate, 25) as f32 / v.len() as f32;
            // start on the last 2 s, so a song is noticed quickly
            let recent = &v[v.len().saturating_sub(200)..];
            let held_recent = held_frames(recent, self.gate, 25) as f32 / recent.len() as f32;
            if !self.singing && v.len() >= 200 && held_recent > SING_ON {
                self.singing = true;
                self.sing_hold = 0;
                return Some(Event::SingStart);
            }
            if self.singing {
                if held < SING_OFF {
                    self.sing_hold += 1;
                    if self.sing_hold > 8 {
                        self.singing = false;
                        return Some(Event::SingStop);
                    }
                } else {
                    self.sing_hold = 0;
                }
            }
        }
        if self.laugh_on && !self.singing && self.since_laugh > 250 {
            let w = &v[v.len() - LAUGH_WIN..];
            let s = laugh_stats(w, self.gate);
            // held notes mean singing, not laughing
            if is_laugh(&s) && (held_frames(w, self.gate, 25) as f32) < 0.25 * LAUGH_WIN as f32 {
                self.since_laugh = 0;
                return Some(Event::Laugh);
            }
        }
        None
    }
}

pub const SING_ON: f32 = 0.45;
pub const SING_OFF: f32 = 0.2;

pub fn is_laugh(s: &LaughStats) -> bool {
    // laughs are breathy: the bursts are less cleanly voiced than speech syllables
    s.n >= 4
        && s.regular > 0.58
        && (0.12..0.32).contains(&s.period)
        && s.vowel_var < 0.18
        && s.clarity < 0.84
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lipsync::{Analyzer, NoiseFloor, read_wav};
    use std::path::PathBuf;

    fn frames(path: &PathBuf) -> (Vec<Frame>, f32) {
        let (rate, ch, s) = read_wav(path.to_str().unwrap()).unwrap();
        let mut a = Analyzer::new(rate);
        let mut out = vec![];
        a.process(&s, ch, |f| out.push(f));
        let mut nf = NoiseFloor::new();
        out.iter().for_each(|f| nf.push(f.db));
        // as the app's startup calibration in a normal room: never above -40 dB
        let gate = (nf.finish().unwrap_or(-60.0) + 6.0).clamp(-70.0, -40.0);
        (out, gate)
    }

    fn files(sub: &str, ext: &str) -> Vec<PathBuf> {
        let d = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join(sub);
        let mut v: Vec<PathBuf> = std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == ext))
            .collect();
        v.sort();
        v
    }

    fn speech() -> Vec<PathBuf> {
        let mut v = files("testdata/real", "wav")
            .into_iter()
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("LJ"))
            .collect::<Vec<_>>();
        v.extend(files("demo", "wav"));
        v.extend(files("demo/questions", "wav"));
        v
    }

    /// Runs the streaming detector over a file; returns its events.
    fn run(p: &PathBuf) -> Vec<Event> {
        run_gate(p, None)
    }

    fn run_gate(p: &PathBuf, g: Option<f32>) -> Vec<Event> {
        let (fr, gate) = frames(p);
        let mut d = Detector {
            gate: g.unwrap_or(gate),
            ..Default::default()
        };
        fr.into_iter().filter_map(|f| d.push(f)).collect()
    }

    /// Pinned on real recordings: catches most clear "ha-ha-ha" laughs, never fires on speech.
    #[test]
    fn laughs_and_singing_on_real_recordings() {
        let laughs = files("testdata/real/laugh", "wav");
        let caught = laughs
            .iter()
            .filter(|p| run(p).contains(&Event::Laugh))
            .count();
        assert!(caught >= 6, "laughs caught: {caught} of {}", laughs.len());
        for p in files("testdata/real/sing", "wav") {
            let ev = run(&p);
            assert!(ev.contains(&Event::SingStart), "{p:?} {ev:?}");
            assert!(!ev.contains(&Event::Laugh), "{p:?} {ev:?}");
        }
        for p in speech() {
            let ev = run(&p);
            assert!(ev.is_empty(), "speech set something off: {p:?} {ev:?}");
        }
        for p in &laughs {
            assert!(!run(p).contains(&Event::SingStart), "{p:?}");
        }
        // a very quiet room (gate at the bottom of its range) changes nothing
        let caught = laughs
            .iter()
            .filter(|p| run_gate(p, Some(-70.0)).contains(&Event::Laugh))
            .count();
        assert!(caught >= 6, "quiet room: laughs caught {caught}");
        for p in speech()
            .iter()
            .chain(files("testdata/real/sing", "wav").iter())
        {
            assert!(
                !run_gate(p, Some(-70.0)).contains(&Event::Laugh),
                "quiet room: {p:?}"
            );
        }
    }

    #[test]
    #[ignore]
    fn explore() {
        for (label, list) in [
            ("LAUGH", files("testdata/real/laugh", "wav")),
            ("SING", files("testdata/real/sing", "wav")),
            ("SPEECH", speech()),
        ] {
            println!("== {label}");
            for p in list {
                let (fr, gate) = frames(&p);
                let prom: f32 = std::env::var("PROM")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(PROM);
                let mut best = LaughStats::default();
                let mut best_score = -1.0;
                let mut i = 0;
                while i + LAUGH_WIN <= fr.len() {
                    let s = laugh_stats_p(&fr[i..i + LAUGH_WIN], gate, prom);
                    let sc = s.n as f32 * s.regular - s.vowel_var * 4.0;
                    if sc > best_score {
                        best_score = sc;
                        best = s;
                    }
                    i += 10;
                }
                let mut held_max: f32 = 0.0;
                let mut i = 0;
                while i + SING_WIN <= fr.len() {
                    held_max = held_max
                        .max(held_frames(&fr[i..i + SING_WIN], gate, 25) as f32 / SING_WIN as f32);
                    i += 10;
                }
                let mut i = 0;
                while i + LAUGH_WIN <= fr.len() {
                    let s = laugh_stats_p(&fr[i..i + LAUGH_WIN], gate, prom);
                    if is_laugh(&s) {
                        println!("   fires @{:.1}s {:?}", i as f32 * 0.01, s);
                    }
                    i += 10;
                }
                println!(
                    "{:28} gate {:5.1} laugh n {} reg {:.2} per {:.2} vvar {:.2} | held {:.2} | events {:?}",
                    p.file_name().unwrap().to_string_lossy(),
                    gate,
                    best.n,
                    best.regular,
                    best.period,
                    best.vowel_var,
                    held_max,
                    run(&p)
                );
            }
        }
    }
}
