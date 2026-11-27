//! Playback of other players' voices on this machine: one playout buffer per speaker, mixed
//! into the sound card with each voice's stereo gains (from [`super::mix::gains`]) and a
//! master volume. Native only; the browser has no voice output yet.
//!
//! Voices are decoded at the wire rate (16 kHz) and stepped through at the device's rate with
//! linear interpolation, the same way Block Avatar plays its voices.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use vox_voice::jitter::Jitter;
use vox_voice::{Packet, RATE};

/// Most speakers mixed at once. A new speaker past this evicts the one idle longest, so a
/// crowd cannot grow the mix without bound.
const MAX_SPEAKERS: usize = 16;

/// One speaker's playout buffer and the gains it plays at.
#[derive(Default)]
struct Voice {
    id: u32,
    jitter: Jitter,
    gains: (f32, f32),
    /// Value of the mixer's tick when this speaker last sent a packet (for eviction).
    last_tick: u64,
}

/// What the audio callback and the network thread share.
#[derive(Default)]
struct Shared {
    voices: Vec<Voice>,
    volume: f32,
    tick: u64,
}

impl Shared {
    /// The next stereo sample at the wire rate: every voice's next sample times its gains,
    /// summed, scaled by the master volume and clamped to -1..1.
    fn mix_step(&mut self) -> (f32, f32) {
        mix_voices(&mut self.voices, self.volume)
    }
}

/// Mixes one sample from each voice. A free function so it can be tested without a device.
fn mix_voices(voices: &mut [Voice], volume: f32) -> (f32, f32) {
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for v in voices.iter_mut() {
        let s = v.jitter.pull();
        l += s * v.gains.0;
        r += s * v.gains.1;
    }
    ((l * volume).clamp(-1.0, 1.0), (r * volume).clamp(-1.0, 1.0))
}

/// Takes the locked state, recovering from a poisoned lock so one panicking thread does not
/// silence the app.
fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The voices this machine plays. Holds the output stream open for as long as it lives.
pub struct Playback {
    shared: Arc<Mutex<Shared>>,
    _stream: cpal::Stream,
}

// cpal streams are not Send on every platform. The playback is kept as a resource and its
// stream is only dropped, never touched, from another thread; the shared state is behind a
// mutex. Same reasoning as Block Avatar's `Player`.
unsafe impl Send for Playback {}
unsafe impl Sync for Playback {}

impl Playback {
    /// Opens the output device named `device` (a case-insensitive part of its name), or the
    /// default output when it is `None` or not found. Fails if there is no output at all.
    pub fn start(device: Option<&str>) -> Result<Playback, String> {
        let shared = Arc::new(Mutex::new(Shared {
            volume: 1.0,
            ..Default::default()
        }));
        let stream = open_stream(shared.clone(), device)?;
        Ok(Playback {
            shared,
            _stream: stream,
        })
    }

    /// A packet from `speaker` arrived, to be played at `gains` (left, right). Packets that
    /// fail [`Packet::decode`] are dropped. Gains are clamped to 0..=1 and non-finite gains
    /// become silence, so nothing from the network can make the mix loud or NaN.
    pub fn hear(&self, speaker: u32, packet: &Packet, gains: (f32, f32)) {
        let Some(pcm) = packet.decode() else { return };
        let gains = (sane_gain(gains.0), sane_gain(gains.1));
        let mut st = lock(&self.shared);
        st.tick = st.tick.wrapping_add(1);
        let tick = st.tick;
        let idx = match st.voices.iter().position(|v| v.id == speaker) {
            Some(i) => i,
            None => {
                if st.voices.len() >= MAX_SPEAKERS {
                    let oldest = st
                        .voices
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, v)| v.last_tick)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    st.voices.swap_remove(oldest);
                }
                st.voices.push(Voice {
                    id: speaker,
                    ..Default::default()
                });
                st.voices.len() - 1
            }
        };
        let v = &mut st.voices[idx];
        v.gains = gains;
        v.last_tick = tick;
        v.jitter.push(packet.seq, &pcm);
    }

    /// Master volume, 0 (silent) to 1 (full). Other values are clamped; non-finite is ignored.
    pub fn set_volume(&self, volume: f32) {
        if volume.is_finite() {
            lock(&self.shared).volume = volume.clamp(0.0, 1.0);
        }
    }
}

fn sane_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Names of the output devices on this machine, for a settings menu.
#[allow(deprecated)]
pub fn output_devices() -> Vec<String> {
    cpal::default_host()
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

#[allow(deprecated)]
fn open_stream(shared: Arc<Mutex<Shared>>, want: Option<&str>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = match want {
        Some(w) => {
            let w = w.to_lowercase();
            host.output_devices()
                .ok()
                .and_then(|mut it| {
                    it.find(|d| {
                        d.name()
                            .map(|n| n.to_lowercase().contains(&w))
                            .unwrap_or(false)
                    })
                })
                .or_else(|| host.default_output_device())
        }
        None => host.default_output_device(),
    }
    .ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let ch = config.channels() as usize;
    let out_rate = config.sample_rate();
    let cfg: cpal::StreamConfig = config.config();
    // Voices run at the wire rate; the callback steps through them at the device's rate.
    let step = RATE as f64 / out_rate as f64;
    let err = |e: cpal::StreamError| crate::note!("[voice] output error: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &cfg, ch, step, shared, |v| v, err),
        cpal::SampleFormat::I16 => build::<i16>(
            &device,
            &cfg,
            ch,
            step,
            shared,
            |v| (v * 32767.0) as i16,
            err,
        ),
        other => return Err(format!("unsupported output format {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Builds an output stream of sample type `T`. `conv` turns a mixed f32 sample into `T`.
fn build<T: cpal::SizedSample + Send + 'static>(
    device: &cpal::Device,
    cfg: &cpal::StreamConfig,
    ch: usize,
    step: f64,
    shared: Arc<Mutex<Shared>>,
    conv: fn(f32) -> T,
    err: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String> {
    let mut at = 0.0f64;
    let (mut a, mut b) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
    device
        .build_output_stream(
            cfg,
            move |d: &mut [T], _: &cpal::OutputCallbackInfo| {
                // One lock per callback, not per sample.
                let mut st = lock(&shared);
                for fr in d.chunks_mut(ch) {
                    at += step;
                    while at >= 1.0 {
                        at -= 1.0;
                        a = b;
                        b = st.mix_step();
                    }
                    let t = at as f32;
                    let l = a.0 + (b.0 - a.0) * t;
                    let r = a.1 + (b.1 - a.1) * t;
                    for (i, x) in fr.iter_mut().enumerate() {
                        let v = if ch == 1 {
                            (l + r) * 0.5
                        } else if i % 2 == 0 {
                            l
                        } else {
                            r
                        };
                        *x = conv(v);
                    }
                }
            },
            err,
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_applies_gains_and_volume_without_a_device() {
        // Seven packets of 0.5 fill the 120 ms prebuffer, then play.
        let mut loud = Voice {
            id: 1,
            gains: (1.0, 0.0),
            ..Default::default()
        };
        let mut quiet = Voice {
            id: 2,
            gains: (0.5, 0.5),
            ..Default::default()
        };
        for seq in 0..7 {
            loud.jitter.push(seq, &vec![0.5; 320]);
            quiet.jitter.push(seq, &vec![0.5; 320]);
        }
        let mut voices = vec![loud, quiet];
        // Prebuffer is 1920 samples; pull past it.
        let mut last = (0.0, 0.0);
        for _ in 0..1921 {
            last = mix_voices(&mut voices, 1.0);
        }
        // 0.5 * 1.0 + 0.5 * 0.5 on the left, 0.5 * 0.5 on the right.
        assert!((last.0 - 0.75).abs() < 1e-6, "{last:?}");
        assert!((last.1 - 0.25).abs() < 1e-6, "{last:?}");
        let half = mix_voices(&mut voices, 0.5);
        assert!(
            (half.0 - 0.375).abs() < 1e-6 && (half.1 - 0.125).abs() < 1e-6,
            "{half:?}"
        );
    }

    #[test]
    fn mix_is_silent_before_prebuffer_and_clamped() {
        let mut v = Voice {
            id: 1,
            gains: (1.0, 1.0),
            ..Default::default()
        };
        v.jitter.push(0, &vec![1.0; 320]);
        assert_eq!(mix_voices(std::slice::from_mut(&mut v), 1.0), (0.0, 0.0));
        let mut hot = Voice {
            id: 1,
            gains: (1.0, 1.0),
            ..Default::default()
        };
        for seq in 0..7 {
            hot.jitter.push(seq, &vec![1.0; 320]);
        }
        let mut hot = [
            hot,
            Voice {
                id: 2,
                gains: (1.0, 1.0),
                ..Default::default()
            },
        ];
        for _ in 0..1920 {
            mix_voices(&mut hot, 1.0);
        }
        let (l, _) = mix_voices(&mut hot, 1.0);
        assert!(l <= 1.0, "clamped to 1: {l}");
    }

    #[test]
    fn sane_gain_rejects_non_finite() {
        assert_eq!(sane_gain(f32::NAN), 0.0);
        assert_eq!(sane_gain(f32::INFINITY), 0.0);
        assert_eq!(sane_gain(2.0), 1.0);
        assert_eq!(sane_gain(-1.0), 0.0);
        assert_eq!(sane_gain(0.25), 0.25);
    }
}
