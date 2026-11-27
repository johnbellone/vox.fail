//! Mouths from voice: each speaker's decoded voice goes through the same analysis and mouth
//! picker that Block Avatar uses ([`avatar_voice::lipsync`]), and the mouth it picks is shown
//! as one of the character's face parts (the same names Block Avatar uses, see
//! [`crate::pose::FACE_PARTS`]).
//!
//! Remote players are fed from the voice packets the server sends; you are fed from the packets
//! your own microphone makes. A mouth closes when nothing was heard for [`HOLD_S`]. The state
//! is pure (no Bevy in [`MouthDriver`]); feeding is native only because only the native voice
//! plugin receives or captures audio.

use std::collections::HashMap;

use avatar_voice::lipsync::{Analyzer, LipMode, LipSync, Mouth, PickerSettings};
use bevy::prelude::*;

use crate::pose::{FACE_PARTS, Rig};

/// The analyzer runs at the codec's rate (`vox_voice::RATE`); kept as a plain number so this
/// module has no codec dependency.
pub const RATE: u32 = 16_000;
/// Seconds of silence (no packets) before a mouth closes.
pub const HOLD_S: f64 = 0.3;
/// Most remote speakers with a driver at once. A new speaker past this replaces the one heard
/// longest ago.
pub const MAX_REMOTE: usize = 32;
/// A remote speaker not heard for this long (seconds) loses its driver.
pub const IDLE_S: f64 = 10.0;
/// One analysis frame, in seconds (the analyzer's 10 ms window).
const FRAME_S: f32 = 0.01;

/// The face part for each mouth, by [`Mouth::slot`]: the same table Block Avatar uses for its
/// Neutral expression (`bins/block-avatar/src/character/face.rs`). `Rest` is `None`: the pose's
/// own mouth stays.
const MOUTH_PARTS: [Option<&str>; 7] = [
    None,
    Some("mouth_o"),
    Some("mouth_open"),
    Some("mouth_o"),
    Some("mouth_grin"),
    Some("mouth_grit"),
    Some("mouth_pout"),
];

/// The face part that shows `m`, or `None` for [`Mouth::Rest`].
pub fn mouth_part(m: Mouth) -> Option<&'static str> {
    MOUTH_PARTS[m.slot()]
}

/// One speaker's analysis: 16 kHz audio in, a mouth out.
pub struct MouthDriver {
    analyzer: Analyzer,
    lip: LipSync,
    mouth: Mouth,
}

impl Default for MouthDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MouthDriver {
    pub fn new() -> Self {
        MouthDriver {
            analyzer: Analyzer::new(RATE),
            lip: LipSync::new(PickerSettings::default(), LipMode::Visemes),
            mouth: Mouth::Rest,
        }
    }

    /// Feed mono samples at [`RATE`]. Each 10 ms window updates the mouth.
    pub fn feed(&mut self, samples: &[f32]) {
        let MouthDriver { analyzer, lip, mouth } = self;
        analyzer.process(samples, 1, |f| *mouth = lip.update(f, FRAME_S));
    }

    /// The mouth picked by the last window fed.
    pub fn mouth(&self) -> Mouth {
        self.mouth
    }
}

/// A driver and when it was last fed (`f64::NEG_INFINITY` before the first feed).
struct Lane {
    driver: MouthDriver,
    last_fed: f64,
}

impl Lane {
    fn new() -> Self {
        Lane { driver: MouthDriver::new(), last_fed: f64::NEG_INFINITY }
    }

    fn feed(&mut self, samples: &[f32], now: f64) {
        self.driver.feed(samples);
        self.last_fed = now;
    }

    /// The mouth to show at `now`: closed once nothing has been fed for [`HOLD_S`].
    fn shown(&self, now: f64) -> Mouth {
        if now - self.last_fed > HOLD_S { Mouth::Rest } else { self.driver.mouth() }
    }
}

/// Every mouth driver: one for you, one per remote speaker (capped at [`MAX_REMOTE`]).
#[derive(Resource)]
pub struct MouthDrivers {
    by_player: HashMap<u32, Lane>,
    local: Lane,
}

impl Default for MouthDrivers {
    fn default() -> Self {
        MouthDrivers { by_player: HashMap::new(), local: Lane::new() }
    }
}

impl MouthDrivers {
    /// Feed a remote speaker's decoded voice (their mouth follows even when you mute them).
    pub fn feed_remote(&mut self, player: u32, samples: &[f32], now: f64) {
        if !self.by_player.contains_key(&player) && self.by_player.len() >= MAX_REMOTE {
            // evict the speaker heard longest ago
            let oldest = self
                .by_player
                .iter()
                .min_by(|a, b| a.1.last_fed.total_cmp(&b.1.last_fed))
                .map(|(id, _)| *id);
            if let Some(id) = oldest {
                self.by_player.remove(&id);
            }
        }
        self.by_player.entry(player).or_insert_with(Lane::new).feed(samples, now);
    }

    /// Feed your own microphone's decoded voice.
    pub fn feed_local(&mut self, samples: &[f32], now: f64) {
        self.local.feed(samples, now);
    }

    /// The mouth to show for a remote speaker at `now`.
    pub fn shown_remote(&self, player: u32, now: f64) -> Mouth {
        self.by_player.get(&player).map_or(Mouth::Rest, |l| l.shown(now))
    }

    /// The mouth to show for you at `now`.
    pub fn shown_local(&self, now: f64) -> Mouth {
        self.local.shown(now)
    }

    /// Drops remote drivers not fed for [`IDLE_S`].
    pub fn prune(&mut self, now: f64) {
        self.by_player.retain(|_, l| now - l.last_fed <= IDLE_S);
    }
}

/// Shows `mouth` on `rig`: its `mouth_*` face part is shown and the others hidden. `Rest`
/// changes nothing, so the pose's own mouth stays.
pub fn set_face<F: bevy::ecs::query::QueryFilter>(
    rig: &Rig,
    mouth: Mouth,
    tf: &mut Query<&mut Transform, F>,
) {
    let Some(part) = mouth_part(mouth) else { return };
    for (e, bit) in &rig.faces {
        // `bit` is `1 << index` into FACE_PARTS (see `pose::rig`)
        let Some(name) = FACE_PARTS.get(bit.trailing_zeros() as usize) else { continue };
        if !name.starts_with("mouth_") {
            continue;
        }
        if let Ok(mut t) = tf.get_mut(*e) {
            t.scale = if *name == part { Vec3::ONE } else { Vec3::ZERO };
        }
    }
}

/// Your own mouth on `rig`. Your face is not driven by a pose, so when the mouth closes the
/// mouth parts go back to what they were before the first change (`saved`).
pub fn local_face(
    rig: &Rig,
    mouth: Mouth,
    saved: &mut Vec<(Entity, Vec3)>,
    tf: &mut Query<&mut Transform>,
) {
    if mouth == Mouth::Rest {
        for (e, scale) in saved.drain(..) {
            if let Ok(mut t) = tf.get_mut(e) {
                t.scale = scale;
            }
        }
        return;
    }
    if saved.is_empty() {
        for (e, _) in &rig.faces {
            if let Ok(t) = tf.get(*e) {
                saved.push((*e, t.scale));
            }
        }
    }
    set_face(rig, mouth, tf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use avatar_voice::lipsync::decode_wav_mono;
    use vox_voice::resample::Resampler;

    /// A real recording from testdata/real, at [`RATE`] mono, as 20 ms packets' worth of samples.
    fn real_speech(file: &str) -> Vec<f32> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/real/");
        let bytes = std::fs::read(format!("{path}{file}")).unwrap();
        let (rate, mono) = decode_wav_mono(&bytes).unwrap();
        let mut out = vec![];
        Resampler::new(rate, RATE).push(&mono, &mut out);
        out
    }

    #[test]
    fn mouth_part_maps_each_mouth_to_a_face_part() {
        assert_eq!(mouth_part(Mouth::Rest), None);
        assert_eq!(mouth_part(Mouth::Mid), Some("mouth_o"));
        assert_eq!(mouth_part(Mouth::Open), Some("mouth_open"));
        assert_eq!(mouth_part(Mouth::Round), Some("mouth_o"));
        assert_eq!(mouth_part(Mouth::Bright), Some("mouth_grin"));
        assert_eq!(mouth_part(Mouth::Teeth), Some("mouth_grit"));
        assert_eq!(mouth_part(Mouth::Closed), Some("mouth_pout"));
        for m in MOUTH_PARTS.iter().flatten() {
            assert!(FACE_PARTS.contains(m), "{m} is not a face part");
        }
    }

    #[test]
    fn speech_opens_the_mouth_and_silence_closes_it() {
        let speech = real_speech("LJ001-0002.wav");
        let mut d = MouthDriver::new();
        let (mut open, mut loud) = (0, 0);
        for chunk in speech.chunks(320) {
            d.feed(chunk);
            // count only chunks with real sound in them (not the recording's quiet ends)
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
            if rms > 0.02 {
                loud += 1;
                if d.mouth() != Mouth::Rest {
                    open += 1;
                }
            }
        }
        assert!(loud > 50, "the recording should have speech ({loud} loud chunks)");
        assert!(open * 10 > loud * 7, "mouth open in {open} of {loud} speech chunks");

        // one second of silence in 20 ms chunks
        for _ in 0..50 {
            d.feed(&[0.0; 320]);
        }
        assert_eq!(d.mouth(), Mouth::Rest, "closed after 1 s of silence");
    }

    #[test]
    fn a_mouth_closes_when_nothing_is_fed_for_the_hold_time() {
        let mut m = MouthDrivers::default();
        m.feed_remote(7, &real_speech("LJ001-0002.wav"), 10.0);
        assert_ne!(m.shown_remote(7, 10.1), Mouth::Rest, "open just after feeding");
        assert_eq!(m.shown_remote(7, 10.0 + HOLD_S + 0.01), Mouth::Rest, "closed after the hold");
        assert_eq!(m.shown_remote(8, 10.0), Mouth::Rest, "nobody fed: closed");
        assert_eq!(m.shown_local(10.0), Mouth::Rest, "you: closed before any packet");
        m.prune(10.0 + IDLE_S + 1.0);
        assert!(m.by_player.is_empty(), "idle speakers are dropped");
    }

    #[test]
    fn remote_drivers_are_capped() {
        let mut m = MouthDrivers::default();
        for id in 0..(MAX_REMOTE as u32 + 5) {
            m.feed_remote(id, &[0.0; 320], id as f64);
        }
        assert_eq!(m.by_player.len(), MAX_REMOTE);
        assert!(!m.by_player.contains_key(&0), "the oldest speaker was dropped");
    }
}
