//! One small seeded random generator (splitmix64), the same on every target.
//!
//! It replaces the separate copies that used to live in the avatar app and the bevel code.
//! The state is a plain `u64` (never `usize`), so wasm32 and native give the same numbers.

/// A deterministic generator: the same seed always gives the same sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator starting from `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`. Returns 0 when `n` is 0.
    ///
    /// Uses the high bits of the output scaled by `n`, which avoids the bias a modulo gives.
    pub fn below(&mut self, n: u32) -> u32 {
        let high = self.next_u64() >> 32;
        ((high * u64::from(n)) >> 32) as u32
    }

    /// A number in `[0, 1)`, with 24 bits of precision.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_seven_first_three_outputs() {
        let mut rng = Rng::new(7);
        // Reference values from an independent splitmix64 implementation.
        assert_eq!(rng.next_u64(), 0x63CB_E1E4_5932_0DD7);
        assert_eq!(rng.next_u64(), 0x044C_3CD7_F43C_661C);
        assert_eq!(rng.next_u64(), 0xE698_4080_BAB1_2A02);
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::new(42);
        for _ in 0..1000 {
            assert!(rng.below(6) < 6);
        }
        assert_eq!(rng.below(0), 0);
    }

    #[test]
    fn unit_is_in_zero_to_one() {
        let mut rng = Rng::new(42);
        for _ in 0..1000 {
            let x = rng.unit();
            assert!((0.0..1.0).contains(&x));
        }
    }
}
