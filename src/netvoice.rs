//! Voices over the network: your mic to the show, and the other voices played back.
//!
//! The voice is sent as it is (no voice changing): mono, 16 kHz, packed with IMA ADPCM (4 bits a
//! sample, about 8 KB/s) in 20 ms packets. Every packet carries the coder's state, so a lost one
//! costs 20 ms of silence and nothing after it. Nothing is sent while you're quiet.
//!
//! Playback holds about 120 ms before starting (the same order as the 100 ms the poses are held,
//! so mouths and voices line up) and mixes up to two voices (host and guest) into the default
//! output, or the one named by `net_output=` in the settings (a virtual cable for OBS).
//! Use headphones: a voice from the speakers would reach your mic and echo back.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::proto::{self, Msg};

/// Samples a second on the wire.
pub const RATE: u32 = 16_000;
/// Samples per packet (20 ms).
pub const PACKET: usize = 320;
/// Held before playing starts (and again after running dry): 120 ms.
const PREBUFFER: usize = (RATE as usize) * 12 / 100;
/// Most held before old audio is dropped: 400 ms.
const MAX_HELD: usize = (RATE as usize) * 4 / 10;
/// Quieter than this (RMS of a packet) counts as silence.
const QUIET: f32 = 0.004;
/// Keep sending this many packets after it goes quiet (300 ms), so word ends aren't clipped.
const HANGOVER: u32 = 15;

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408,
    449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066,
    2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630,
    9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// IMA ADPCM, one channel. `state` is (predicted sample, step index).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adpcm {
    pub pred: i32,
    pub index: i32,
}

impl Adpcm {
    fn nibble(&mut self, sample: i32) -> u8 {
        let step = STEPS[self.index as usize];
        let mut diff = sample - self.pred;
        let mut code = 0u8;
        if diff < 0 {
            code = 8;
            diff = -diff;
        }
        let mut delta = step >> 3;
        if diff >= step {
            code |= 4;
            diff -= step;
            delta += step;
        }
        if diff >= step >> 1 {
            code |= 2;
            diff -= step >> 1;
            delta += step >> 1;
        }
        if diff >= step >> 2 {
            code |= 1;
            delta += step >> 2;
        }
        self.pred = if code & 8 != 0 { self.pred - delta } else { self.pred + delta }.clamp(-32768, 32767);
        self.index = (self.index + INDEX[code as usize]).clamp(0, 88);
        code
    }

    fn sample(&mut self, code: u8) -> i16 {
        let step = STEPS[self.index as usize];
        let mut delta = step >> 3;
        if code & 4 != 0 {
            delta += step;
        }
        if code & 2 != 0 {
            delta += step >> 1;
        }
        if code & 1 != 0 {
            delta += step >> 2;
        }
        self.pred = if code & 8 != 0 { self.pred - delta } else { self.pred + delta }.clamp(-32768, 32767);
        self.index = (self.index + INDEX[code as usize]).clamp(0, 88);
        self.pred as i16
    }

    /// Pack samples, two to a byte.
    pub fn encode(&mut self, pcm: &[i16]) -> Vec<u8> {
        pcm.chunks(2)
            .map(|p| {
                let lo = self.nibble(p[0] as i32);
                let hi = p.get(1).map(|s| self.nibble(*s as i32)).unwrap_or(0);
                lo | (hi << 4)
            })
            .collect()
    }

    /// Unpack `n` samples.
    pub fn decode(&mut self, data: &[u8], n: usize) -> Vec<i16> {
        let mut out = Vec::with_capacity(n);
        for b in data {
            for code in [b & 15, b >> 4] {
                if out.len() < n {
                    out.push(self.sample(code));
                }
            }
        }
        out
    }
}

/// Straight-line resampling of a continuous stream.
#[derive(Clone, Debug)]
pub struct Resampler {
    from: u32,
    to: u32,
    /// position of the next output sample, in input samples after `last`
    pos: f64,
    last: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        Resampler { from, to, pos: 0.0, last: 0.0 }
    }

    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let step = self.from as f64 / self.to as f64;
        // positions run over [last, input...]: index 0 is `last`
        let len = input.len() as f64;
        while self.pos <= len - 1.0 + 1.0 && !input.is_empty() {
            let i = self.pos.floor();
            if i + 1.0 > len {
                break;
            }
            let a = if i == 0.0 { self.last } else { input[i as usize - 1] };
            let b = input[i as usize];
            let f = (self.pos - i) as f32;
            out.push(a + (b - a) * f);
            self.pos += step;
        }
        if let Some(l) = input.last() {
            self.last = *l;
            self.pos -= len;
        }
    }
}

/// Voice waiting to be played: fills from packets, drains into the speakers.
#[derive(Default, Debug)]
pub struct Jitter {
    buf: VecDeque<f32>,
    playing: bool,
    last_seq: Option<u32>,
}

impl Jitter {
    pub fn push(&mut self, seq: u32, samples: &[f32]) {
        // a packet lost in between: fill its gap with silence, so the timing holds
        if let Some(l) = self.last_seq {
            if seq <= l {
                return; // late or repeated
            }
            let missing = (seq - l - 1).min(10) as usize;
            if self.playing {
                self.buf.extend(std::iter::repeat_n(0.0, missing * PACKET));
            }
        }
        self.last_seq = Some(seq);
        self.buf.extend(samples);
        while self.buf.len() > MAX_HELD {
            self.buf.pop_front();
        }
    }

    /// The next sample at the wire rate (silence while filling up).
    pub fn pull(&mut self) -> f32 {
        if !self.playing {
            if self.buf.len() < PREBUFFER {
                return 0.0;
            }
            self.playing = true;
        }
        match self.buf.pop_front() {
            Some(v) => v,
            None => {
                self.playing = false;
                0.0
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn held(&self) -> usize {
        self.buf.len()
    }
}

/// Everything waiting to be played, one buffer per voice (host, guest).
pub type Mix = Arc<Mutex<[Jitter; 2]>>;

/// The speakers: an output stream pulling from the mix.
pub struct Player {
    pub mix: Mix,
    _stream: Option<cpal::Stream>,
    pub status: String,
}

// cpal streams aren't Send on every platform; the player stays on the main thread's resource
// but is only touched there.
unsafe impl Send for Player {}
unsafe impl Sync for Player {}

impl Player {
    #[allow(deprecated)]
    pub fn open(want: Option<&str>) -> Player {
        let mix: Mix = Arc::new(Mutex::new(Default::default()));
        let (stream, status) = match open_stream(mix.clone(), want) {
            Ok((s, name)) => (Some(s), format!("Voices play on {name}")),
            Err(e) => (None, format!("No sound output for voices: {e}")),
        };
        Player { mix, _stream: stream, status }
    }

    /// A voice packet arrived.
    pub fn hear(&self, v: &proto::Voice) {
        if v.codec != proto::ADPCM || v.who > 1 {
            return;
        }
        let mut st = Adpcm { pred: v.pred as i32, index: v.index as i32 };
        let pcm: Vec<f32> = st
            .decode(&v.data, v.samples as usize)
            .into_iter()
            .map(|s| s as f32 / 32768.0)
            .collect();
        if let Ok(mut m) = self.mix.lock() {
            m[v.who as usize].push(v.seq, &pcm);
        }
    }
}

#[allow(deprecated)]
fn open_stream(mix: Mix, want: Option<&str>) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let device = match want {
        Some(w) => host
            .output_devices()
            .ok()
            .and_then(|mut it| {
                it.find(|d| d.name().map(|n| n.to_lowercase().contains(&w.to_lowercase())).unwrap_or(false))
            })
            .or_else(|| host.default_output_device()),
        None => host.default_output_device(),
    }
    .ok_or("no output device")?;
    let name = device.name().unwrap_or_else(|_| "output".into());
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let ch = config.channels() as usize;
    let out_rate = config.sample_rate();
    // the mix runs at the wire rate; step through it at the device's
    let step = RATE as f64 / out_rate as f64;
    let mut at = 0.0f64;
    let (mut a, mut b) = (0.0f32, 0.0f32);
    let mut next = move || -> f32 {
        at += step;
        while at >= 1.0 {
            at -= 1.0;
            a = b;
            b = mix
                .lock()
                .map(|mut m| (m[0].pull() + m[1].pull()).clamp(-1.0, 1.0))
                .unwrap_or(0.0);
        }
        a + (b - a) * at as f32
    };
    let err = |e: cpal::StreamError| crate::problem!("[voice] output error: {e}");
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
        other => return Err(format!("unsupported output format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

/// Your mic to the network: a thread takes the mic's mono copy, brings it to 16 kHz, cuts
/// 20 ms packets, skips silence, and hands them to the network thread.
pub struct Sender {
    stop: Arc<AtomicBool>,
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Sender {
    pub fn start(
        who: u8,
        tap: mpsc::Receiver<(u32, Vec<f32>)>,
        out: mpsc::Sender<Vec<u8>>,
    ) -> Sender {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        std::thread::Builder::new()
            .name("voice-out".into())
            .spawn(move || {
                let mut rs: Option<Resampler> = None;
                let mut pending: Vec<f32> = vec![];
                let mut coder = Adpcm::default();
                let mut seq = 0u32;
                let mut quiet_for = HANGOVER;
                while !s2.load(Ordering::Relaxed) {
                    let Ok((rate, mono)) = tap.recv_timeout(std::time::Duration::from_millis(200)) else {
                        continue;
                    };
                    let r = rs.get_or_insert_with(|| Resampler::new(rate, RATE));
                    if r.from != rate {
                        *r = Resampler::new(rate, RATE);
                    }
                    r.push(&mono, &mut pending);
                    while pending.len() >= PACKET {
                        let chunk: Vec<f32> = pending.drain(..PACKET).collect();
                        seq = seq.wrapping_add(1);
                        let rms = (chunk.iter().map(|v| v * v).sum::<f32>() / PACKET as f32).sqrt();
                        quiet_for = if rms >= QUIET { 0 } else { quiet_for.saturating_add(1) };
                        if quiet_for > HANGOVER {
                            continue;
                        }
                        let pcm: Vec<i16> = chunk.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                        let start = coder;
                        let data = coder.encode(&pcm);
                        let msg = Msg::Voice(proto::Voice {
                            who,
                            seq,
                            codec: proto::ADPCM,
                            samples: PACKET as u16,
                            pred: start.pred as i16,
                            index: start.index as u8,
                            data,
                        });
                        if out.send(proto::encode(&msg)).is_err() {
                            return;
                        }
                    }
                }
            })
            .expect("voice thread");
        Sender { stop }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, rate: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                // a voice-ish mix: 220 Hz and 1.1 kHz, rising and falling
                0.4 * (t * 220.0 * std::f32::consts::TAU).sin() * (t * 3.0).sin().abs()
                    + 0.15 * (t * 1100.0 * std::f32::consts::TAU).sin()
            })
            .collect()
    }

    #[test]
    fn adpcm_keeps_the_voice() {
        let x: Vec<i16> = tone(16_000, 16_000.0).iter().map(|v| (v * 32767.0) as i16).collect();
        let mut enc = Adpcm::default();
        let mut out = vec![];
        let mut sent = 0usize;
        // packet by packet, each decoded from its own starting state
        for p in x.chunks(PACKET) {
            let start = enc;
            let data = enc.encode(p);
            sent += data.len();
            let mut dec = start;
            out.extend(dec.decode(&data, p.len()));
        }
        assert_eq!(out.len(), x.len());
        let sig: f64 = x.iter().map(|v| (*v as f64).powi(2)).sum();
        let noise: f64 = x.iter().zip(&out).map(|(a, b)| (*a as f64 - *b as f64).powi(2)).sum();
        let snr = 10.0 * (sig / noise).log10();
        assert!(snr > 20.0, "signal to noise {snr:.1} dB");
        assert_eq!(sent, 8000, "4 bits a sample: 8 KB a second");
    }

    #[test]
    fn resampling_keeps_length_and_pitch() {
        let x = tone(48_000, 48_000.0);
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = vec![];
        for c in x.chunks(441) {
            r.push(c, &mut out);
        }
        assert!((out.len() as i32 - 16_000).abs() <= 2, "{}", out.len());
        // compare with the tone made at 16 kHz directly
        let want = tone(16_000, 16_000.0);
        let err: f32 = out.iter().zip(&want).skip(10).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.05, "worst error {err}");
    }

    #[test]
    fn jitter_buffer_waits_fills_gaps_and_refills() {
        let mut j = Jitter::default();
        let p = vec![0.5f32; PACKET];
        j.push(1, &p);
        assert_eq!(j.pull(), 0.0, "still filling: 20 ms of 120");
        for s in 2..=6 {
            j.push(s, &p);
        }
        assert_eq!(j.held(), 6 * PACKET);
        assert_eq!(j.pull(), 0.5, "120 ms held: playing");
        // packet 7 lost, 8 arrives: 20 ms of silence keeps the timing
        let before = j.held();
        j.push(8, &p);
        assert_eq!(j.held(), before + 2 * PACKET);
        // late packet ignored
        j.push(7, &p);
        assert_eq!(j.held(), before + 2 * PACKET);
        // run dry, then wait for 120 ms again
        while j.held() > 0 {
            j.pull();
        }
        assert_eq!(j.pull(), 0.0);
        j.push(9, &p);
        assert_eq!(j.pull(), 0.0, "refilling");
    }

    #[test]
    fn voice_messages_are_droppable_and_small() {
        let mut enc = Adpcm::default();
        let pcm: Vec<i16> = tone(PACKET, 16_000.0).iter().map(|v| (v * 32767.0) as i16).collect();
        let msg = Msg::Voice(proto::Voice {
            who: proto::GUEST,
            seq: 7,
            codec: proto::ADPCM,
            samples: PACKET as u16,
            pred: 0,
            index: 0,
            data: enc.encode(&pcm),
        });
        let b = proto::encode(&msg);
        assert_eq!(b[0], proto::DROPPABLE);
        assert!(b.len() < 175, "{} bytes per 20 ms", b.len());
        assert_eq!(proto::decode(&b), Some(msg));
    }
}
