//! Regression tests on real, unmodified human recordings (testdata/real, see its README):
//! six speakers saying digits, and four LJ Speech sentences. They pin the accuracy measured when
//! the voice analysis was tuned, so a change that makes real voices worse fails `cargo test`.

use crate::lipsync::{
    Analyzer, Frame, LipMode, LipSync, Mouth, NoiseFloor, PickerSettings, read_wav,
};
use crate::voice::{Prosody, VoiceFrame};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/real")
}

fn frames(file: &str) -> Vec<Frame> {
    let (rate, ch, samples) = read_wav(dir().join(file).to_str().unwrap()).unwrap();
    let mut a = Analyzer::new(rate);
    let mut out = vec![];
    a.process(&samples, ch, |f| out.push(f));
    out
}

/// gate from the file's own quiet parts, as the app does at startup
fn picker(fr: &[Frame]) -> LipSync {
    let mut nf = NoiseFloor::new();
    fr.iter().for_each(|f| nf.push(f.db));
    let mut s = PickerSettings::default();
    s.gate_db = (nf.finish().unwrap_or(-60.0) + 6.0).clamp(-70.0, -20.0);
    LipSync::new(s, LipMode::Visemes)
}

fn voiced(f: &Frame) -> bool {
    f.pitch_hz > 0.0 && f.clarity > 0.6
}

const SPEAKERS: [&str; 6] = ["george", "jackson", "lucas", "nicolas", "theo", "yweweler"];

#[test]
fn pitch_matches_praat_on_real_voices() {
    // Praat's pitch at the centre of each of our 10 ms frames (testdata/real/praat_pitch.csv)
    let refs = std::fs::read_to_string(dir().join("praat_pitch.csv")).unwrap();
    let mut by_file: std::collections::BTreeMap<String, Vec<(usize, f32)>> = Default::default();
    for line in refs.lines().skip(1) {
        let c: Vec<&str> = line.split(',').collect();
        by_file
            .entry(c[0].to_string())
            .or_default()
            .push((c[1].parse().unwrap(), c[2].parse().unwrap()));
    }
    let (mut both, mut close, mut praat_voiced, mut gross) = (0, 0, 0, 0);
    for (file, refs) in &by_file {
        let fr = frames(file);
        for &(i, praat) in refs {
            praat_voiced += 1;
            let Some(f) = fr.get(i) else { continue };
            if !voiced(f) {
                continue;
            }
            both += 1;
            let err = (f.pitch_hz - praat).abs() / praat;
            close += (err < 0.05) as usize;
            gross += (err > 0.2) as usize;
        }
    }
    let (agree, within5, gross) = (
        both as f32 / praat_voiced as f32,
        close as f32 / both as f32,
        gross as f32 / both as f32,
    );
    println!(
        "pitch vs Praat: voiced in both {:.0} %, within 5 % on {:.0} %, gross errors {:.1} %",
        agree * 100.0,
        within5 * 100.0,
        gross * 100.0
    );
    assert!(
        agree > 0.8 && within5 > 0.85 && gross < 0.06,
        "agree {agree} within5 {within5} gross {gross}"
    );
}

#[test]
fn vowels_on_real_voices() {
    // digit -> vowel in its nucleus, and which part of the voiced stretch that is
    let words = [
        ("3", Mouth::Bright, 0.3, 0.8),
        ("2", Mouth::Round, 0.3, 0.8),
        ("5", Mouth::Open, 0.15, 0.45),
        ("9", Mouth::Open, 0.25, 0.5),
    ];
    let (mut hit, mut total, mut hit_tone, mut open_or_mid) = (0, 0, 0, 0);
    for s in SPEAKERS {
        for (d, want, a, b) in words {
            for i in [25, 27, 29] {
                let fr = frames(&format!("{d}_{s}_{i}.wav"));
                for formants in [true, false] {
                    let mut lip = picker(&fr);
                    lip.use_formants = formants;
                    let mouths: Vec<(Mouth, bool)> = fr
                        .iter()
                        .map(|f| (lip.update(*f, 0.01), voiced(f)))
                        .collect();
                    let v: Vec<Mouth> = mouths.iter().filter(|m| m.1).map(|m| m.0).collect();
                    if v.len() < 4 {
                        continue;
                    }
                    let lo = (v.len() as f32 * a) as usize;
                    let hi = ((v.len() as f32 * b) as usize).max(lo + 1);
                    for m in &v[lo..hi] {
                        if formants {
                            total += 1;
                            hit += (*m == want) as usize;
                            open_or_mid += (want == Mouth::Open && *m == Mouth::Mid) as usize;
                        } else {
                            hit_tone += (*m == want) as usize;
                        }
                    }
                }
            }
        }
    }
    let acc = hit as f32 / total as f32;
    let tone = hit_tone as f32 / total as f32;
    println!(
        "real voices: formants {:.0} %, tone only {:.0} %, ah shown half-open {:.0} %",
        acc * 100.0,
        tone * 100.0,
        open_or_mid as f32 / total as f32 * 100.0
    );
    assert!(acc > 0.55, "vowel accuracy on real voices fell to {acc}");
    assert!(
        acc > tone + 0.25,
        "formants should beat tone-only clearly ({acc} vs {tone})"
    );
}

#[test]
fn statements_dont_tilt_like_questions() {
    // LJ Speech sentences, including ones that end on a comma with a continuation rise
    for f in [
        "LJ001-0002.wav",
        "LJ001-0004.wav",
        "LJ001-0006.wav",
        "LJ001-0008.wav",
    ] {
        let fr = frames(f);
        let lip = picker(&fr);
        let mut p = Prosody::default();
        let mut q = 0;
        // run on, then a second of silence so the phrase end is seen
        let silence = Frame {
            db: -100.0,
            ..Default::default()
        };
        for fm in fr.iter().chain(std::iter::repeat_n(&silence, 100)) {
            let v = VoiceFrame {
                pitch_hz: fm.pitch_hz,
                clarity: fm.clarity,
                f1: fm.f1,
                f2: fm.f2,
            };
            q += p
                .update(
                    v,
                    fm.db,
                    fm.onsets,
                    0.01,
                    lip.settings.gate_db,
                    lip.speech_db,
                )
                .question as usize;
        }
        assert_eq!(q, 0, "{f}");
    }
}
