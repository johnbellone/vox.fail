//! Voices over the network: your mic to the show, and the other voices played back.
//!
//! The voice is sent as it is (no voice changing): mono, 16 kHz, packed with IMA ADPCM (4 bits a
//! sample, about 8 KB/s) in 20 ms packets. Every packet carries the coder's state, so a lost one
//! costs 20 ms of silence and nothing after it. Nothing is sent while you're quiet.
//!
//! The codec, resampler, playout buffer and silence gate live in `vox-voice`, shared with the
//! world's voice chat. This module is the glue: the cpal device code, and the mapping between
//! the relay's `proto::Voice` and `vox_voice::Packet`.
//!
//! Playback holds about 120 ms before starting (the same order as the 100 ms the poses are held,
//! so mouths and voices line up) and mixes up to two voices (host and guest) into the default
//! output, or the one named by `net_output=` in the settings (a virtual cable for OBS).
//! Use headphones: a voice from the speakers would reach your mic and echo back.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use vox_voice::{Packet, Packetizer, RATE, jitter::Jitter, resample::Resampler};

use crate::proto::{self, Msg};

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
        // a packet with the wrong sizes or an index the codec does not have is dropped, not decoded
        let Some(pcm) = from_wire(v).decode() else {
            return;
        };
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

/// The relay's message for one packet. `who` is 0 for the host, 1 for a guest.
fn to_wire(who: u8, p: Packet) -> proto::Voice {
    proto::Voice {
        who,
        seq: p.seq,
        codec: proto::ADPCM,
        samples: p.samples,
        pred: p.pred,
        index: p.index,
        data: p.data,
    }
}

/// A received message as a packet, for `Packet::decode`.
fn from_wire(v: &proto::Voice) -> Packet {
    Packet {
        seq: v.seq,
        samples: v.samples,
        pred: v.pred,
        index: v.index,
        data: v.data.clone(),
    }
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
                // the resampler is rebuilt when the mic's rate changes; the rate is kept beside it
                let mut rs: Option<(u32, Resampler)> = None;
                let mut resampled: Vec<f32> = vec![];
                let mut packetizer = Packetizer::new();
                let mut packets: Vec<Packet> = vec![];
                while !s2.load(Ordering::Relaxed) {
                    let Ok((rate, mono)) = tap.recv_timeout(std::time::Duration::from_millis(200)) else {
                        continue;
                    };
                    let (from, r) = rs.get_or_insert_with(|| (rate, Resampler::new(rate, RATE)));
                    if *from != rate {
                        *from = rate;
                        *r = Resampler::new(rate, RATE);
                    }
                    r.push(&mono, &mut resampled);
                    packetizer.push(&resampled, &mut packets);
                    resampled.clear();
                    for p in packets.drain(..) {
                        if out.send(proto::encode(&Msg::Voice(to_wire(who, p)))).is_err() {
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
    use vox_voice::{PACKET, adpcm::Adpcm};

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

    #[test]
    fn packets_survive_the_wire_and_decode_on_the_other_side() {
        let mut p = Packetizer::new();
        p.set_ptt(Some(true));
        let mut packets = vec![];
        p.push(&tone(PACKET * 2, 16_000.0), &mut packets);
        assert_eq!(packets.len(), 2);
        for pk in packets {
            let bytes = proto::encode(&Msg::Voice(to_wire(proto::GUEST, pk.clone())));
            let Some(Msg::Voice(v)) = proto::decode(&bytes) else {
                panic!("a voice message decodes");
            };
            assert_eq!(v.who, proto::GUEST);
            assert_eq!(from_wire(&v), pk);
            assert_eq!(from_wire(&v).decode().map(|s| s.len()), Some(PACKET));
        }
    }
}
