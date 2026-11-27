//! Packets: cuts microphone audio into the 20 ms ADPCM packets the wire carries, and checks the
//! packets it gets back before it decodes them.
//!
//! [`Packetizer`] takes 16 kHz mono samples (in chunks of any size; the caller resamples first),
//! cuts them into [`PACKET`]-sample packets and drops quiet ones after a short hangover, so
//! silence costs nothing on the wire. Push-to-talk ([`Packetizer::set_ptt`]) bypasses the gate.
//!
//! Two things keep a real microphone from holding the gate open in a quiet room (both found on
//! a laptop mic: a DC offset of -0.26 and a hiss right at [`QUIET`]): a DC blocker (a one-pole
//! high-pass near 13 Hz, below anything a voice makes, so the voice itself is not changed) runs
//! before the gate and the coder, and the gate opens only well above the room's own noise
//! floor, which it follows.
//! [`Packet::decode`] checks every size and state value a packet claims before it decodes it.

use crate::adpcm::Adpcm;
use crate::{PACKET, RATE};

/// Quieter than this (RMS of a packet) counts as silence.
const QUIET: f32 = 0.004;
/// The gate opens at this many times the room's noise floor (12 dB above it).
const OVER_FLOOR: f32 = 4.0;
/// The noise floor follows a quieter packet at once and rises at most this much per packet
/// (doubling in about 2.8 s), so speech, which pauses, never becomes the floor.
const FLOOR_RISE: f32 = 1.005;
/// The floor never rises past this: a room this loud still lets a raised voice through.
const FLOOR_MAX: f32 = 0.05;
/// DC blocker pole: cutoff (1 - R) * 16000 / 2pi, about 13 Hz.
const DC_POLE: f32 = 0.995;
/// Keep sending this many packets after it goes quiet (300 ms), so word ends aren't clipped.
const HANGOVER: u32 = 15;
/// Most audio kept while waiting for a whole packet: 1 s. Older samples are dropped, so a caller
/// that stops reading the packets cannot make this grow without limit.
const MAX_PENDING: usize = RATE as usize;
/// Highest IMA ADPCM step index.
const MAX_INDEX: u8 = 88;

/// One packet on the wire: 320 samples as 160 bytes of ADPCM, with the coder state it starts from.
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub seq: u32,
    pub samples: u16,
    pub pred: i16,
    pub index: u8,
    pub data: Vec<u8>,
}

impl Packet {
    /// Decode to f32 samples in -1..1. Returns `None` for a packet with the wrong sizes or an
    /// index the codec does not have, so a bad packet from the network is dropped, not decoded.
    pub fn decode(&self) -> Option<Vec<f32>> {
        if usize::from(self.samples) != PACKET
            || self.data.len() != PACKET / 2
            || self.index > MAX_INDEX
        {
            return None;
        }
        let mut dec = Adpcm {
            pred: i32::from(self.pred),
            index: i32::from(self.index),
        };
        let pcm = dec.decode(&self.data, PACKET);
        Some(pcm.iter().map(|&s| f32::from(s) / 32768.0).collect())
    }
}

/// Turns a stream of 16 kHz samples into packets, with the silence gate and push-to-talk.
#[derive(Debug)]
pub struct Packetizer {
    pending: Vec<f32>,
    coder: Adpcm,
    seq: u32,
    /// packets since the last loud one; starts past the hangover, so silence at the start sends nothing
    quiet_for: u32,
    ptt: Option<bool>,
    /// the room's noise floor (RMS of its quietest recent packets)
    floor: f32,
    /// DC blocker state: the last input and output (`None` until the first sample, which
    /// seeds it, so a mic's offset does not start with a click)
    dc: Option<(f32, f32)>,
}

impl Default for Packetizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Packetizer {
    pub fn new() -> Packetizer {
        Packetizer {
            pending: Vec::new(),
            coder: Adpcm::default(),
            seq: 0,
            quiet_for: HANGOVER,
            ptt: None,
            floor: QUIET,
            dc: None,
        }
    }

    /// `None`: the mic is open and the silence gate decides. `Some(true)`: send everything (push to
    /// talk held). `Some(false)`: send nothing, and start the hangover again from silence.
    pub fn set_ptt(&mut self, held: Option<bool>) {
        self.ptt = held;
        if held == Some(false) {
            self.pending.clear();
            self.quiet_for = HANGOVER;
        }
    }

    /// Add samples and append the packets they complete to `out`. Non-finite samples count as 0.
    pub fn push(&mut self, mono16k: &[f32], out: &mut Vec<Packet>) {
        if self.ptt == Some(false) {
            return;
        }
        for &v in mono16k {
            let x = if v.is_finite() { v } else { 0.0 };
            let (px, py) = self.dc.unwrap_or((x, 0.0));
            let y = x - px + DC_POLE * py;
            self.dc = Some((x, y));
            self.pending.push(y);
        }
        if self.pending.len() > MAX_PENDING {
            let drop = self.pending.len() - MAX_PENDING;
            self.pending.drain(..drop);
        }
        while self.pending.len() >= PACKET {
            let chunk: Vec<f32> = self.pending.drain(..PACKET).collect();
            self.seq = self.seq.wrapping_add(1);
            let rms = (chunk.iter().map(|v| v * v).sum::<f32>() / PACKET as f32).sqrt();
            self.quiet_for = if rms >= QUIET.max(self.floor * OVER_FLOOR) {
                0
            } else {
                self.quiet_for.saturating_add(1)
            };
            self.floor = if rms < self.floor {
                rms
            } else {
                (self.floor * FLOOR_RISE).min(FLOOR_MAX)
            };
            let send = self.ptt == Some(true) || self.quiet_for <= HANGOVER;
            if !send {
                continue;
            }
            let pcm: Vec<i16> = chunk
                .iter()
                .map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            // each packet carries the coder state it starts from, so it decodes on its own
            let start = self.coder;
            let data = self.coder.encode(&pcm);
            out.push(Packet {
                seq: self.seq,
                samples: PACKET as u16,
                pred: start.pred as i16,
                index: start.index as u8,
                data,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (i as f32 / RATE as f32 * 440.0 * std::f32::consts::TAU).sin())
            .collect()
    }

    #[test]
    fn quiet_input_sends_nothing() {
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.push(&vec![0.0; PACKET * 5], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn speech_sends_then_hangover_ends_after_15_packets() {
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.push(&tone(PACKET * 10, 0.3), &mut out);
        assert_eq!(out.len(), 10);
        out.clear();
        p.push(&vec![0.0; PACKET * 20], &mut out);
        assert_eq!(
            out.len(),
            15,
            "15 packets of hangover, then the gate closes"
        );
        assert_eq!(out[0].seq, 11, "sequence numbers count gated packets too");
        assert_eq!(out[14].seq, 25);
    }

    /// A quiet room through an offset mic: hiss just over the fixed threshold, on -0.26 of DC.
    /// A deterministic noise (xorshift), so the test does not depend on luck.
    fn offset_hiss(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed.max(1);
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                // uniform in -0.0085..0.0085: RMS about 0.0049, over QUIET
                -0.26 + (s as f32 / u32::MAX as f32 - 0.5) * 0.017
            })
            .collect()
    }

    #[test]
    fn an_offset_hissing_mic_in_a_quiet_room_soon_stops_sending() {
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.push(&offset_hiss(PACKET * 250, 7), &mut out);
        // the floor starts at QUIET and has to find the room first; after that, nothing
        assert!(out.len() < 40, "sent {} of 250 quiet packets", out.len());
        out.clear();
        p.push(&offset_hiss(PACKET * 100, 9), &mut out);
        assert!(out.is_empty(), "once the floor is known a quiet room sends nothing");
        // and speech over the same room still gets through, with the offset gone
        let x: Vec<f32> = tone(PACKET * 10, 0.2).iter().zip(offset_hiss(PACKET * 10, 11)).map(|(a, b)| a + b).collect();
        p.push(&x, &mut out);
        assert!(out.len() >= 9, "speech sent: {} of 10", out.len());
        let s = out.last().unwrap().decode().unwrap();
        let mean = s.iter().sum::<f32>() / s.len() as f32;
        assert!(mean.abs() < 0.01, "the DC offset is removed: mean {mean}");
    }

    #[test]
    fn push_to_talk_sends_silence_while_held_and_nothing_when_released() {
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.set_ptt(Some(true));
        p.push(&vec![0.0; PACKET * 5], &mut out);
        assert_eq!(out.len(), 5);
        out.clear();
        p.set_ptt(Some(false));
        p.push(&tone(PACKET * 5, 0.3), &mut out);
        assert!(out.is_empty());
        p.set_ptt(None);
        p.push(&vec![0.0; PACKET * 5], &mut out);
        assert!(
            out.is_empty(),
            "release resets the hangover, so quiet after it sends nothing"
        );
    }

    #[test]
    fn decode_round_trips_speech() {
        let x = tone(PACKET * 4, 0.3);
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.push(&x, &mut out);
        assert_eq!(out.len(), 4);
        let mut got = vec![];
        for pk in &out {
            let s = pk.decode().expect("a packet we made decodes");
            assert_eq!(s.len(), PACKET);
            got.extend(s);
        }
        let sig: f64 = x.iter().map(|v| f64::from(*v).powi(2)).sum();
        let noise: f64 = x
            .iter()
            .zip(&got)
            .map(|(a, b)| f64::from(a - b).powi(2))
            .sum();
        let snr = 10.0 * (sig / noise).log10();
        assert!(snr > 20.0, "signal to noise {snr:.1} dB");
    }

    #[test]
    fn decode_rejects_bad_sizes_and_index() {
        let good = Packet {
            seq: 1,
            samples: PACKET as u16,
            pred: 0,
            index: 0,
            data: vec![0; PACKET / 2],
        };
        assert!(good.decode().is_some());
        let mut bad = good.clone();
        bad.samples = 319;
        assert!(bad.decode().is_none());
        let mut bad = good.clone();
        bad.samples = 0;
        assert!(bad.decode().is_none());
        let mut bad = good.clone();
        bad.data.pop();
        assert!(bad.decode().is_none());
        let mut bad = good.clone();
        bad.data.push(0);
        assert!(bad.decode().is_none());
        let mut bad = good.clone();
        bad.index = 89;
        assert!(bad.decode().is_none());
        let mut edge = good;
        edge.index = 88;
        edge.pred = i16::MIN;
        assert!(edge.decode().is_some());
    }

    #[test]
    fn non_finite_samples_count_as_silence() {
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.set_ptt(Some(true));
        let bad: Vec<f32> = (0..PACKET)
            .map(|i| match i % 3 {
                0 => f32::NAN,
                1 => f32::INFINITY,
                _ => f32::NEG_INFINITY,
            })
            .collect();
        p.push(&bad, &mut out);
        assert_eq!(out.len(), 1);
        let s = out[0].decode().unwrap();
        assert!(s.iter().all(|v| v.abs() < 0.001));
    }

    #[test]
    fn pending_audio_is_capped_at_one_second() {
        let mut p = Packetizer::new();
        let mut out = vec![];
        p.set_ptt(Some(true));
        p.push(&vec![0.0; RATE as usize * 3], &mut out);
        assert_eq!(out.len(), 50, "1 s is 50 packets; the rest was dropped");
    }

    #[test]
    fn seq_wraps_at_u32_max() {
        let mut p = Packetizer::new();
        p.seq = u32::MAX - 1;
        let mut out = vec![];
        p.set_ptt(Some(true));
        p.push(&vec![0.0; PACKET * 3], &mut out);
        let seqs: Vec<u32> = out.iter().map(|k| k.seq).collect();
        assert_eq!(seqs, vec![u32::MAX, 0, 1]);
    }
}
