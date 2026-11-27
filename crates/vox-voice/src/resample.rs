//! A linear resampler: turns microphone audio at one rate into the wire rate (see [`crate::RATE`]).
//!
//! It keeps the last input sample and the fractional position between calls, so a stream can be
//! fed in chunks of any size and the output has no seams.

/// The largest input rate taken as real. Anything above it is a bad value, not audio.
const MAX_RATE: u32 = 1_000_000;

/// Turns a stream from `from` Hz into `to` Hz.
#[derive(Clone, Debug)]
pub struct Resampler {
    from: u32,
    to: u32,
    /// position of the next output sample, in input samples after `last`
    pos: f64,
    last: f32,
}

/// A non-finite sample (NaN or infinity) would spread through the interpolation, so it counts as silence.
fn clean(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

impl Resampler {
    /// A bad rate (`from` of 0 or above 1 MHz, or `to` of 0) makes the resampler pass through:
    /// `from` and `to` both become the target rate, or 1 Hz when `to` is 0. Such a value comes
    /// from a bad device or header, and a step from it would be zero, huge or infinite, so the
    /// step between samples is always finite and at least one sample is kept.
    pub fn new(from: u32, to: u32) -> Resampler {
        let (from, to) = if from == 0 || from > MAX_RATE || to == 0 {
            let rate = to.max(1);
            (rate, rate)
        } else {
            (from, to)
        };
        Resampler {
            from,
            to,
            pos: 0.0,
            last: 0.0,
        }
    }

    /// Append the resampled `input` to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        let step = self.from as f64 / self.to as f64;
        let len = input.len() as f64;
        // stream index 0 is `last`, index k >= 1 is input[k - 1]
        let last = self.last;
        let sample = |k: usize| clean(if k == 0 { last } else { input[k - 1] });
        // `pos` is in [0, len) inside the loop, so `i + 1 <= len` always holds
        while self.pos < len {
            let i = self.pos.floor() as usize;
            let a = sample(i);
            let b = sample(i + 1);
            let f = (self.pos - i as f64) as f32;
            out.push(a + (b - a) * f);
            self.pos += step;
        }
        // the next call starts where this one stopped; the new `last` is index len
        self.pos -= len;
        self.last = input[input.len() - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RATE;

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
    fn resampling_keeps_length_and_pitch() {
        let x = tone(48_000, 48_000.0);
        let mut r = Resampler::new(48_000, RATE);
        let mut out = vec![];
        for c in x.chunks(441) {
            r.push(c, &mut out);
        }
        assert!((out.len() as i32 - 16_000).abs() <= 2, "{}", out.len());
        // compare with the tone made at 16 kHz directly
        let want = tone(16_000, 16_000.0);
        let err: f32 = out
            .iter()
            .zip(&want)
            .skip(10)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(err < 0.05, "worst error {err}");
    }

    #[test]
    fn chunking_does_not_change_the_output() {
        let x = tone(9_600, 48_000.0);
        let mut whole = vec![];
        Resampler::new(48_000, RATE).push(&x, &mut whole);
        let mut split = vec![];
        let mut r = Resampler::new(48_000, RATE);
        for c in x.chunks(7) {
            r.push(c, &mut split);
        }
        assert_eq!(whole.len(), split.len());
        for (a, b) in whole.iter().zip(&split) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn non_finite_input_becomes_silence() {
        let x = [0.5f32, f32::NAN, f32::INFINITY, -0.5, 0.25, 0.0];
        let mut out = vec![];
        Resampler::new(RATE, RATE).push(&x, &mut out);
        assert_eq!(out.len(), x.len());
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn absurd_input_rate_passes_through() {
        let mut r = Resampler::new(2_000_000, RATE);
        let mut out = vec![];
        r.push(&[0.5f32; 100], &mut out);
        assert_eq!(out.len(), 100, "same length: no resampling");
        assert!(out[1..].iter().all(|v| *v == 0.5));
    }

    #[test]
    fn zero_rates_do_not_hang_or_divide_by_zero() {
        let mut out = vec![];
        Resampler::new(0, 0).push(&[0.1, 0.2, 0.3], &mut out);
        assert!(!out.is_empty() && out.iter().all(|v| v.is_finite()));
    }
}
