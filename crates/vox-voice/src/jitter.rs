//! The playout buffer: packets arrive unevenly, and this turns them into one steady stream of
//! samples for the speakers.
//!
//! It waits for [`PREBUFFER`] samples before it plays, fills a lost packet's place with silence so
//! the timing holds, drops late and repeated packets, and never holds more than [`MAX_HELD`]
//! samples. Sequence numbers are `u32` and wrap, so every comparison goes through `wrapping_sub`.

use std::collections::VecDeque;

use crate::{MAX_HELD, PACKET, PREBUFFER};

/// How far ahead of the expected packet one is still taken: 2 s (100 packets of 20 ms).
/// Anything further is ignored, so a bad sequence number cannot make the stream jump forever.
const MAX_JUMP: u32 = (2 * crate::RATE as usize / PACKET) as u32;
/// Most silent packets put in for a gap. Longer gaps keep the timing only up to this point.
const MAX_FILL: u32 = 10;
/// Half the `u32` range: a difference at or above this means the packet is behind, not ahead.
const HALF: u32 = 1 << 31;

/// Voice waiting to be played: fills from packets, drains into the speakers.
#[derive(Default, Debug)]
pub struct Jitter {
    buf: VecDeque<f32>,
    playing: bool,
    last_seq: Option<u32>,
}

impl Jitter {
    /// Add one packet. Late and repeated packets are ignored while audio is held. A far-ahead
    /// packet is ignored while playing, but before playback has started (or after it ran dry)
    /// it restarts the stream: the stale partial buffer is dropped, so a sender that restarted
    /// its sequence numbers is heard again. When nothing is held the packet is the new start.
    pub fn push(&mut self, seq: u32, samples: &[f32]) {
        if let (Some(last), false) = (self.last_seq, self.buf.is_empty()) {
            let ahead = seq.wrapping_sub(last);
            if ahead == 0 || ahead >= HALF {
                return; // late or repeated
            }
            if ahead > MAX_JUMP {
                if self.playing {
                    return; // absurd jump mid-stream: keep waiting for the stream we were on
                }
                // never started playing: a stale partial buffer is replaced by the restart
                self.buf.clear();
            } else if self.playing {
                // a packet lost in between: fill its gap with silence, so the timing holds
                let missing = (ahead - 1).min(MAX_FILL) as usize;
                self.buf.extend(std::iter::repeat_n(0.0, missing * PACKET));
            }
        }
        self.last_seq = Some(seq);
        // a single packet never holds more than the cap, however long the slice is
        let keep = &samples[samples.len().saturating_sub(MAX_HELD)..];
        self.buf.extend(keep);
        let over = self.buf.len().saturating_sub(MAX_HELD);
        self.buf.drain(..over);
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

    /// Samples held and not yet played.
    pub fn held(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn seq_wraps_at_u32_max() {
        let mut j = Jitter::default();
        let p = vec![0.5f32; PACKET];
        let start = u32::MAX - 2;
        for s in 0..6 {
            j.push(start.wrapping_add(s), &p);
        }
        assert_eq!(j.held(), 6 * PACKET);
        assert_eq!(j.pull(), 0.5, "playing");
        // start + 5 is 2 after the wrap; 3 is the next packet and must count as ahead
        let before = j.held();
        j.push(3, &p);
        assert_eq!(j.held(), before + PACKET, "no gap, no silence");
        // 2 is behind (it was the packet before 3): ignored
        j.push(2, &p);
        assert_eq!(j.held(), before + PACKET);
        // 0 was already seen in the first six (start..start+5 wraps through 0), so 4 is next
        j.push(4, &p);
        assert_eq!(j.held(), before + 2 * PACKET);
    }

    #[test]
    fn far_future_seq_does_not_grow_memory() {
        let mut j = Jitter::default();
        let p = vec![0.5f32; PACKET];
        for s in 1..=6 {
            j.push(s, &p);
        }
        assert_eq!(j.pull(), 0.5, "playing: far-ahead packets are rejected from here on");
        let held = j.held();
        // ten minutes of packets ahead (50 a second): ignored while audio is held
        j.push(7 + 30_000, &p);
        assert_eq!(j.held(), held);
        // a flood of them: still bounded and still ignored
        for s in 0..10_000u32 {
            j.push(u32::MAX / 2 + s * 1000, &p);
        }
        assert_eq!(j.held(), held);
        // the stream itself carries on from where it was
        j.push(7, &p);
        assert_eq!(j.held(), held + PACKET);
    }

    #[test]
    fn far_ahead_restart_before_playing_replaces_partial_buffer() {
        let mut j = Jitter::default();
        let p = vec![0.5f32; PACKET];
        // 60 ms: not enough to start, so nothing has played yet
        for s in 1..=3 {
            j.push(s, &p);
        }
        assert_eq!(j.pull(), 0.0, "still filling");
        // the sender restarts 1000 packets later: the stale partial buffer is replaced
        j.push(3 + 1000, &p);
        assert_eq!(j.held(), PACKET);
        // enough packets of the new run, and the audio plays
        for s in 1004..=1008 {
            j.push(s, &p);
        }
        assert_eq!(j.held(), 6 * PACKET);
        assert_eq!(j.pull(), 0.5, "120 ms of the new run held: playing");
    }

    #[test]
    fn restart_after_dry_is_taken_and_held_is_capped() {
        let mut j = Jitter::default();
        let p = vec![0.5f32; PACKET];
        for s in 1..=6 {
            j.push(s, &p);
        }
        while j.held() > 0 {
            j.pull();
        }
        // nothing held: a far-future sequence starts the stream again
        j.push(1_000_000, &p);
        assert_eq!(j.held(), PACKET);
        // one huge slice is cut to the cap
        let big = vec![0.25f32; 100_000];
        j.push(1_000_001, &big);
        assert_eq!(j.held(), MAX_HELD);
    }
}
