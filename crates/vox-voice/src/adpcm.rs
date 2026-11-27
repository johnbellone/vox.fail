//! IMA ADPCM, one channel: 4 bits a sample, so 16 kHz voice is about 8 KB a second.
//!
//! Each packet carries the coder's state (`Adpcm`), so a packet decodes on its own. The decoder
//! takes bytes from the network, so it clamps every state value it looks up and never trusts a
//! length it did not check against the data.

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// IMA ADPCM, one channel. `state` is (predicted sample, step index).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adpcm {
    pub pred: i32,
    pub index: i32,
}

impl Adpcm {
    /// Bring the state back into range. The fields are public, so a caller (or a packet) can
    /// hand us anything; this keeps the table lookups in bounds and the arithmetic from overflowing.
    fn tame(&mut self) {
        self.index = self.index.clamp(0, 88);
        self.pred = self.pred.clamp(-32768, 32767);
    }

    fn nibble(&mut self, sample: i32) -> u8 {
        self.tame();
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
        self.pred = if code & 8 != 0 {
            self.pred - delta
        } else {
            self.pred + delta
        }
        .clamp(-32768, 32767);
        self.index = (self.index + INDEX[code as usize]).clamp(0, 88);
        code
    }

    fn sample(&mut self, code: u8) -> i16 {
        self.tame();
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
        self.pred = if code & 8 != 0 {
            self.pred - delta
        } else {
            self.pred + delta
        }
        .clamp(-32768, 32767);
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

    /// Unpack up to `n` samples. Never returns more than two samples per byte of `data`, so a
    /// huge `n` from the network allocates no more than the bytes it was sent.
    pub fn decode(&mut self, data: &[u8], n: usize) -> Vec<i16> {
        let n = n.min(data.len().saturating_mul(2));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples per packet: 20 ms at 16 kHz (the same as the network packets).
    const PACKET: usize = 320;

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
        let x: Vec<i16> = tone(16_000, 16_000.0)
            .iter()
            .map(|v| (v * 32767.0) as i16)
            .collect();
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
        let noise: f64 = x
            .iter()
            .zip(&out)
            .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
            .sum();
        let snr = 10.0 * (sig / noise).log10();
        assert!(snr > 20.0, "signal to noise {snr:.1} dB");
        assert_eq!(sent, 8000, "4 bits a sample: 8 KB a second");
    }

    #[test]
    fn hostile_state_and_length_do_not_panic() {
        // xorshift: a fixed seed keeps the run the same every time, with no dependency
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let extremes = [i32::MIN, i32::MAX, -1, 0, 88, 89, 1000, -32769, 32768];
        for round in 0..2000 {
            let len = (next() % 600) as usize;
            let data: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let mut st = Adpcm {
                pred: if round % 3 == 0 {
                    next() as i32
                } else {
                    extremes[(next() % extremes.len() as u64) as usize]
                },
                index: if round % 2 == 0 {
                    next() as i32
                } else {
                    extremes[(next() % extremes.len() as u64) as usize]
                },
            };
            let n = match round % 4 {
                0 => usize::MAX,
                1 => 0,
                2 => (next() % 2000) as usize,
                _ => len * 2 + 1,
            };
            let out = st.decode(&data, n);
            assert!(out.len() <= n.min(len * 2));
            // the state stays in range whatever the caller handed in
            assert!((0..=88).contains(&st.index) || out.is_empty());
        }
        // and the encoder takes any starting state too
        let mut bad = Adpcm {
            pred: i32::MAX,
            index: i32::MIN,
        };
        let pcm: Vec<i16> = (0..101).map(|i| (i * 397 % 65536 - 32768) as i16).collect();
        assert_eq!(bad.encode(&pcm).len(), 51);
    }
}
