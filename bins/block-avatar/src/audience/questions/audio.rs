//! The recordings: levelling a voice, resampling, and transcribing with Whisper when it's built in.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
// Audio
// ------------------------------------------------------------------------------------------

// the WAV reader and writer are avatar-voice's (one, hardened, for everything)
pub use avatar_voice::wav::{parse_wav_mono16, wav16_bytes, write_wav16};

/// Levels a recording so every viewer comes out about as loud as the others: one gain for the
/// whole clip (no compression, no effects), set from the speech parts, capped so peaks stay
/// below full scale. None if there's no speech to hear.
pub fn level_voice(s: &[f32]) -> Option<Vec<f32>> {
    let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak < 0.003 {
        return None;
    }
    // speech level: RMS of the 20 ms blocks within 30 dB of the loudest block
    let block = 320.max(s.len() / 2000);
    let db: Vec<f32> = s
        .chunks(block)
        .map(|c| {
            10.0 * ((c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).max(1e-12)).log10()
        })
        .collect();
    let top = db.iter().cloned().fold(-200.0f32, f32::max);
    if top < -55.0 {
        return None;
    }
    let active: Vec<f32> = db.iter().cloned().filter(|d| *d > top - 30.0).collect();
    let mean_pow = active.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>() / active.len() as f32;
    let speech_db = 10.0 * mean_pow.log10();
    let target_db = -20.0;
    let gain_db = (target_db - speech_db).min(24.0);
    let gain = 10f32.powf(gain_db / 20.0).min(0.89 / peak);
    Some(s.iter().map(|x| x * gain).collect())
}

/// Built-in transcription of questions (Whisper builds): fills in each question's bubble text.
#[cfg(feature = "whisper")]
pub fn start_transcriber(
    model: String,
    queue: std::sync::Arc<std::sync::Mutex<Queue>>,
) -> Result<(), String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
        .map_err(|e| format!("can't load Whisper model {model}: {e}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<(u32, PathBuf)>();
    // anything waiting without text (from before a restart) gets a pass too
    {
        let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
        for x in q
            .items
            .iter()
            .filter(|x| x.status.waiting() && x.text.is_empty())
        {
            let _ = tx.send((x.id, q.wav_path(x.id)));
        }
        q.transcribe = Some(tx);
    }
    std::thread::Builder::new()
        .name("ask-whisper".into())
        .spawn(move || {
            let Ok(mut state) = ctx.create_state() else {
                return;
            };
            for (id, path) in rx {
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok((rate, s)) = parse_wav_mono16(&bytes) else {
                    continue;
                };
                let s16 = resample_linear(&s, rate, 16000);
                let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                p.set_language(Some("en"));
                p.set_n_threads(
                    std::thread::available_parallelism()
                        .map(|n| n.get().min(4) as i32)
                        .unwrap_or(2),
                );
                p.set_no_context(true);
                p.set_suppress_blank(true);
                p.set_print_progress(false);
                p.set_print_realtime(false);
                p.set_print_special(false);
                p.set_print_timestamps(false);
                if state.full(p, &s16).is_err() {
                    continue;
                }
                let mut text = String::new();
                for seg in state.as_iter() {
                    if let Ok(t) = seg.to_str_lossy() {
                        text.push_str(&t);
                    }
                }
                let text = crate::bubbles::clean_transcript(&text);
                let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                // the streamer may have typed something already: keep theirs
                if q.get(id).is_some_and(|x| x.text.is_empty()) {
                    q.set_text(id, &text);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
pub fn resample_linear(s: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || s.is_empty() {
        return s.to_vec();
    }
    let step = from as f64 / to as f64;
    let n = ((s.len() as f64) / step) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let f = (p - j as f64) as f32;
            let a = s[j.min(s.len() - 1)];
            let b = s[(j + 1).min(s.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}
