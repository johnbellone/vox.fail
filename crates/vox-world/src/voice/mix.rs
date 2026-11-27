//! Voice mixing rules, with no engine and no device in them: how loud one voice plays in each
//! ear, from where the speaker stands relative to the listener. Pure, so it also builds for
//! the browser.
//!
//! Yaw convention (the same as Bevy and the camera): yaw 0 faces -Z, and a larger yaw turns
//! the listener to the left. So forward is `(-sin yaw, 0, -cos yaw)` and right is
//! `(cos yaw, 0, -sin yaw)`. Positions are in blocks; the Y axis only affects distance.

use vox_core::wire::{VOICE_NEARBY, VOICE_PARTY};

/// Distance (blocks) inside which a nearby voice plays at full volume.
const FULL_VOLUME: f32 = 2.0;

/// Left and right gains (0..=1) for one voice, from where it is and which channel it is on.
///
/// `NEARBY` falls off linearly from 1.0 at [`FULL_VOLUME`] blocks to 0.0 at `radius`, and is
/// panned by its bearing from the listener's facing (equal power, so a voice in front plays at
/// about 0.71 in each ear). `PARTY` plays at full volume in both ears wherever the speaker is.
/// Non-finite input, a radius of zero or less, or an unknown channel gives silence `(0.0, 0.0)`.
pub fn gains(
    listener_at: [f32; 3],
    listener_yaw: f32,
    speaker_at: [f32; 3],
    channel: u8,
    radius: f32,
) -> (f32, f32) {
    let all = listener_at
        .iter()
        .chain(speaker_at.iter())
        .chain([&listener_yaw, &radius]);
    if !all.copied().all(f32::is_finite) || radius <= 0.0 {
        return (0.0, 0.0);
    }
    match channel {
        VOICE_PARTY => (1.0, 1.0),
        VOICE_NEARBY => {
            let [dx, dy, dz] = [
                speaker_at[0] - listener_at[0],
                speaker_at[1] - listener_at[1],
                speaker_at[2] - listener_at[2],
            ];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            // Checked before dividing: with radius <= FULL_VOLUME the middle branch never runs,
            // so the division by (radius - FULL_VOLUME) is always positive.
            let volume = if dist >= radius {
                0.0
            } else if dist <= FULL_VOLUME {
                1.0
            } else {
                (radius - dist) / (radius - FULL_VOLUME)
            };
            if volume <= 0.0 {
                return (0.0, 0.0);
            }
            let (sin, cos) = listener_yaw.sin_cos();
            // Components of the horizontal offset along the listener's forward and right.
            let ahead = -dx * sin - dz * cos;
            let right = dx * cos - dz * sin;
            let span = (ahead * ahead + right * right).sqrt();
            // Bearing as a pan in -1 (hard left) ..= 1 (hard right). Straight above or below
            // has no bearing, so it sits in the middle.
            let pan = if span > 0.0 {
                (right / span).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            // Equal power: the angle runs from 0 (all left) to pi/2 (all right).
            let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            (volume * angle.cos(), volume * angle.sin())
        }
        _ => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;
    const ORIGIN: [f32; 3] = [0.0, 0.0, 0.0];
    const SQRT_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS
    }

    #[test]
    fn voice_ahead_is_centred() {
        // yaw 0 faces -Z
        let g = gains(ORIGIN, 0.0, [0.0, 0.0, -5.0], VOICE_NEARBY, 20.0);
        let v = 15.0 / 18.0;
        assert!(near(g, (v * SQRT_HALF, v * SQRT_HALF)), "{g:?}");
    }

    #[test]
    fn voice_behind_is_centred() {
        let g = gains(ORIGIN, 0.0, [0.0, 0.0, 5.0], VOICE_NEARBY, 20.0);
        let v = 15.0 / 18.0;
        assert!(near(g, (v * SQRT_HALF, v * SQRT_HALF)), "{g:?}");
    }

    #[test]
    fn voice_on_the_left_plays_in_the_left_ear() {
        // facing -Z, the left hand is -X
        let g = gains(ORIGIN, 0.0, [-5.0, 0.0, 0.0], VOICE_NEARBY, 20.0);
        let v = 15.0 / 18.0;
        assert!(near(g, (v, 0.0)), "{g:?}");
    }

    #[test]
    fn voice_on_the_right_plays_in_the_right_ear() {
        let g = gains(ORIGIN, 0.0, [5.0, 0.0, 0.0], VOICE_NEARBY, 20.0);
        let v = 15.0 / 18.0;
        assert!(near(g, (0.0, v)), "{g:?}");
    }

    #[test]
    fn bearing_turns_with_the_listener() {
        // yaw pi/2 faces -X, so a voice at -X is ahead and a voice at +Z is on the left
        let ahead = gains(
            ORIGIN,
            std::f32::consts::FRAC_PI_2,
            [-5.0, 0.0, 0.0],
            VOICE_NEARBY,
            20.0,
        );
        let left = gains(
            ORIGIN,
            std::f32::consts::FRAC_PI_2,
            [0.0, 0.0, 5.0],
            VOICE_NEARBY,
            20.0,
        );
        assert!((ahead.0 - ahead.1).abs() < EPS, "{ahead:?}");
        assert!(left.0 > left.1, "{left:?}");
    }

    #[test]
    fn full_volume_up_to_two_blocks() {
        let g = gains(ORIGIN, 0.0, [0.0, 0.0, -2.0], VOICE_NEARBY, 10.0);
        assert!(near(g, (SQRT_HALF, SQRT_HALF)), "{g:?}");
    }

    #[test]
    fn silent_at_the_radius_and_beyond() {
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, 0.0, -10.0], VOICE_NEARBY, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, 0.0, -12.0], VOICE_NEARBY, 10.0),
            (0.0, 0.0)
        );
    }

    #[test]
    fn falls_off_linearly_between() {
        // 6 blocks of a 10-block radius: (10 - 6) / (10 - 2) = 0.5 of full volume
        let g = gains(ORIGIN, 0.0, [-6.0, 0.0, 0.0], VOICE_NEARBY, 10.0);
        assert!(near(g, (0.5, 0.0)), "{g:?}");
    }

    #[test]
    fn party_is_full_in_both_ears_at_any_distance() {
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, 0.0, -100.0], VOICE_PARTY, 10.0),
            (1.0, 1.0)
        );
        assert_eq!(
            gains(ORIGIN, 1.0, [3.0, 0.0, 0.0], VOICE_PARTY, 10.0),
            (1.0, 1.0)
        );
    }

    #[test]
    fn non_finite_or_empty_radius_is_silent() {
        let nan = f32::NAN;
        let inf = f32::INFINITY;
        assert_eq!(
            gains([nan, 0.0, 0.0], 0.0, [0.0, 0.0, -1.0], VOICE_NEARBY, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, inf, [0.0, 0.0, -1.0], VOICE_NEARBY, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, inf, -1.0], VOICE_NEARBY, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, nan, [0.0, 0.0, -1.0], VOICE_PARTY, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, 0.0, -1.0], VOICE_NEARBY, nan),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, 0.0, -1.0], VOICE_NEARBY, 0.0),
            (0.0, 0.0)
        );
        assert_eq!(
            gains(ORIGIN, 0.0, [0.0, 0.0, -1.0], VOICE_NEARBY, -4.0),
            (0.0, 0.0)
        );
    }

    #[test]
    fn unknown_channel_is_silent() {
        assert_eq!(gains(ORIGIN, 0.0, [0.0, 0.0, -1.0], 7, 10.0), (0.0, 0.0));
    }
}
