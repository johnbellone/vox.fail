//! How far a player can really move: the server's check on the positions clients report.
//!
//! A client says where it stands with every pose; a modified one could say anywhere, and
//! where you stand decides what you can reach and where you can claim a plot. So the server
//! keeps a budget of distance per player, refilled at the fastest a player can go (flying,
//! with room for lag) and capped at a couple of seconds' worth, and spends it on each move.
//! A move the budget can't pay for is refused: the player is told to go back to their last
//! good position. Falling is allowed to be fast; climbing and going sideways aren't.
//!
//! Positions are in blocks; speeds in metres a second, divided by the block size.

/// Sideways: flying is 7 m/s and running 6; this leaves room for lag and rounding.
pub const MAX_SPEED: f32 = 11.0;
/// Up: flying up is 7 m/s and a jump about 5.5.
pub const MAX_RISE: f32 = 9.0;
/// Down: a long fall gets fast.
pub const MAX_FALL: f32 = 70.0;
/// How much unspent movement may be saved up: lag bunches moves together.
pub const SLACK_SECONDS: f32 = 2.0;
/// Arriving within a metre of the spawn is always allowed (falling out of the world puts you
/// back there). In blocks.
pub const SPAWN_RADIUS: f32 = crate::in_blocks(1.0);

/// One player's movement so far.
#[derive(Clone, Debug, Default)]
pub struct Moves {
    last: Option<[f32; 3]>,
    at: f64,
    sideways: f32,
    up: f32,
}

impl Moves {
    /// Where they last stood, as accepted.
    pub fn last(&self) -> Option<[f32; 3]> {
        self.last
    }

    /// Start them at `at` (time `now`, seconds): where a ticket says they arrive, or the spawn.
    /// Their first reported position is then checked against it like any other move.
    pub fn start(&mut self, at: [f32; 3], now: f64) {
        self.accept(at, now);
    }

    /// A reported position at time `now` (seconds). `Ok` if they could have got there; `Err`
    /// with where they should be instead. Without [`Moves::start`], the first position is taken
    /// as it is (a world with no sign-in, where a portal's arrival point isn't known).
    pub fn check(&mut self, to: [f32; 3], now: f64, block_size: f32, spawn: [f32; 3]) -> Result<(), [f32; 3]> {
        if !to.iter().all(|v| v.is_finite()) {
            return Err(self.last.unwrap_or(spawn));
        }
        let elapsed = (now - self.at).max(0.0) as f32;
        let Some(from) = self.last else {
            self.accept(to, now);
            return Ok(());
        };
        let near_spawn = (0..3).map(|i| (to[i] - spawn[i]).powi(2)).sum::<f32>().sqrt() <= SPAWN_RADIUS;
        if near_spawn {
            self.accept(to, now);
            return Ok(());
        }
        let per_block = 1.0 / block_size.max(0.01);
        let cap = SLACK_SECONDS * per_block;
        let sideways = (self.sideways + MAX_SPEED * per_block * elapsed).min(MAX_SPEED * cap);
        let up = (self.up + MAX_RISE * per_block * elapsed).min(MAX_RISE * cap);
        let flat = ((to[0] - from[0]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
        let rise = to[1] - from[1];
        let fall_limit = MAX_FALL * per_block * (elapsed + 0.5);
        if flat > sideways || rise > up || -rise > fall_limit {
            // refused: keep what was saved up, so a later honest move isn't refused too
            self.sideways = sideways;
            self.up = up;
            self.at = now;
            return Err(from);
        }
        self.sideways = sideways - flat;
        self.up = up - rise.max(0.0);
        self.last = Some(to);
        self.at = now;
        Ok(())
    }

    fn accept(&mut self, to: [f32; 3], now: f64) {
        self.last = Some(to);
        self.at = now;
        self.sideways = 0.0;
        self.up = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: f32 = crate::BLOCK_M;
    const SPAWN: [f32; 3] = [0.5, 0.0, 0.5];

    fn walk(m: &mut Moves, from: [f32; 3], speed_ms: f32, secs: f64, step: f64) -> Result<(), [f32; 3]> {
        let mut t = 0.0;
        let mut x = from[0];
        while t < secs {
            t += step;
            x += speed_ms / B * step as f32;
            m.check([x, from[1], from[2]], 100.0 + t, B, SPAWN)?;
        }
        Ok(())
    }

    #[test]
    fn walking_running_and_flying_are_fine() {
        for speed in [3.2, 6.0, 7.0] {
            let mut m = Moves::default();
            m.check([10.0, 0.0, 0.0], 100.0, B, SPAWN).unwrap();
            assert_eq!(walk(&mut m, [10.0, 0.0, 0.0], speed, 10.0, 1.0 / 30.0), Ok(()), "{speed} m/s");
        }
    }

    #[test]
    fn going_too_fast_is_sent_back() {
        let mut m = Moves::default();
        m.check([10.0, 0.0, 0.0], 100.0, B, SPAWN).unwrap();
        assert!(walk(&mut m, [10.0, 0.0, 0.0], 30.0, 5.0, 1.0 / 30.0).is_err());
        // a teleport across the world is refused, back to where they were
        let mut m = Moves::default();
        m.check([10.0, 0.0, 0.0], 100.0, B, SPAWN).unwrap();
        assert_eq!(m.check([500.0, 0.0, 0.0], 100.1, B, SPAWN), Err([10.0, 0.0, 0.0]));
        assert_eq!(m.last(), Some([10.0, 0.0, 0.0]));
    }

    #[test]
    fn a_lag_spike_is_saved_up_for() {
        let mut m = Moves::default();
        m.check([10.0, 0.0, 0.0], 100.0, B, SPAWN).unwrap();
        // a second and a half with no poses, then the whole flight at once
        assert_eq!(m.check([10.0 + 7.0 / B * 1.5, 0.0, 0.0], 101.5, B, SPAWN), Ok(()));
    }

    #[test]
    fn falls_are_fast_but_climbs_are_not() {
        let mut m = Moves::default();
        m.check([10.0, 200.0, 0.0], 100.0, B, SPAWN).unwrap();
        assert_eq!(m.check([10.0, 150.0, 0.0], 100.5, B, SPAWN), Ok(()), "a long fall");
        assert!(m.check([10.0, 250.0, 0.0], 100.6, B, SPAWN).is_err(), "no rocketing up");
    }

    #[test]
    fn the_spawn_is_always_reachable_and_nonsense_isnt() {
        let mut m = Moves::default();
        m.check([300.0, -60.0, 0.0], 100.0, B, SPAWN).unwrap();
        assert_eq!(m.check(SPAWN, 100.1, B, SPAWN), Ok(()), "fell out, back at the start");
        assert!(m.check([f32::NAN, 0.0, 0.0], 100.2, B, SPAWN).is_err());
    }
}
