//! Secondary motion: things hanging off the character lag behind and swing back when it moves.
//! Hats and face pieces on the head, and the animals' tails (which also wag a little on their
//! own). Each is a small damped spring driven by how its parent turns and moves.
//!
//! The parent's pose comes from animation clips sampled once a frame, so the springs can't run
//! in `FixedUpdate`. Instead each frame steps the spring in fixed 1/120 s substeps (the same
//! rate as `sim`) and shows a blend of the last two, so it swings the same at any frame rate.

use bevy::prelude::*;

#[derive(Component, Clone, Copy)]
pub struct Jiggle {
    /// spring stiffness and damping ratio
    k: f32,
    damping: f32,
    /// how much turning and moving the parent swings it
    turn_gain: f32,
    move_gain: f32,
    /// idle wag (radians) and speed, for tails
    wag: f32,
    wag_speed: f32,
    /// swing (pitch about x, roll about z) and its rate
    off: Vec2,
    vel: Vec2,
    last: Quat,
    prev: Option<(Quat, Vec3, Vec3)>,
    /// simulated time not yet stepped, and the swing one substep ago (for the blend)
    carry: f32,
    off_prev: Vec2,
    /// last frame's turn and acceleration, eased toward this frame's over the substeps
    input: Option<(Vec3, Vec3)>,
}

/// The spring's substep: the same rate as the simulation clock.
pub const SUBSTEP: f32 = (1.0 / crate::sim::HZ) as f32;
/// The most one frame may catch up on, as for the simulation clock.
const MAX_CARRY: f32 = 0.1;

impl Jiggle {
    pub fn hat() -> Self {
        Jiggle {
            k: 220.0,
            damping: 0.28,
            turn_gain: 0.07,
            move_gain: 0.035,
            wag: 0.0,
            wag_speed: 0.0,
            off: Vec2::ZERO,
            vel: Vec2::ZERO,
            last: Quat::IDENTITY,
            prev: None,
            carry: 0.0,
            off_prev: Vec2::ZERO,
            input: None,
        }
    }
    pub fn tail() -> Self {
        Jiggle {
            k: 90.0,
            damping: 0.22,
            turn_gain: 0.16,
            move_gain: 0.08,
            wag: 0.22,
            wag_speed: 3.4,
            ..Jiggle::hat()
        }
    }

    /// One step of the spring. `turn` is the parent's angular velocity and `accel` its
    /// acceleration, both in the parent's own frame. Returns the swing.
    pub fn step(&mut self, turn: Vec3, accel: Vec3, dt: f32) -> Vec2 {
        // it swings against the turn and against the acceleration, like anything loose
        let target = Vec2::new(
            -turn.x * self.turn_gain + accel.z * self.move_gain * 0.1
                - accel.y * self.move_gain * 0.05,
            -turn.z * self.turn_gain - accel.x * self.move_gain * 0.1,
        );
        let target = target.clamp(Vec2::splat(-0.5), Vec2::splat(0.5));
        let c = 2.0 * self.damping * self.k.sqrt();
        let dt = dt.min(1.0 / 30.0);
        let acc = (target - self.off) * self.k - self.vel * c;
        self.vel += acc * dt;
        self.off += self.vel * dt;
        self.off = self.off.clamp(Vec2::splat(-0.6), Vec2::splat(0.6));
        self.off
    }

    /// A frame of `dt`: as many fixed substeps as are due. The parent's turn and acceleration
    /// ease from last frame's to this frame's across them, so a low frame rate feeds the spring
    /// much the same input as a high one. Returns the swing to show: a blend of the last two
    /// substeps.
    pub fn advance(&mut self, turn: Vec3, accel: Vec3, dt: f32) -> Vec2 {
        let (from_turn, from_accel) = self.input.unwrap_or((turn, accel));
        self.input = Some((turn, accel));
        let start = self.carry;
        self.carry = (self.carry + dt).min(MAX_CARRY);
        let span = (self.carry - start).max(1e-6);
        let mut done = 0.0;
        while self.carry >= SUBSTEP {
            self.off_prev = self.off;
            // the input eases from last frame's to this frame's across the frame
            done += SUBSTEP;
            let u = ((done - start) / span).clamp(0.0, 1.0);
            self.step(from_turn.lerp(turn, u), from_accel.lerp(accel, u), SUBSTEP);
            self.carry -= SUBSTEP;
        }
        self.off_prev.lerp(self.off, self.carry / SUBSTEP)
    }
}

/// Animals' tails get a spring when their model appears.
pub fn attach(q: Query<(Entity, &Name), (Added<Name>, Without<Jiggle>)>, mut commands: Commands) {
    for (e, n) in &q {
        if n.as_str() == "Tail" {
            commands.entity(e).insert(Jiggle::tail());
        }
    }
}

/// Runs after animation and before transforms are worked out, so it rides on whatever pose the
/// clip gave (taking its own last swing off first, for parts no clip moves).
pub fn update(
    time: Res<Time>,
    mut q: Query<(&mut Jiggle, &mut Transform, &ChildOf)>,
    parents: Query<&GlobalTransform>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let now = time.elapsed_secs();
    for (mut j, mut t, child_of) in &mut q {
        let Ok(p) = parents.get(child_of.parent()) else {
            continue;
        };
        let (_, rot, pos) = p.to_scale_rotation_translation();
        let (turn, accel, vel) = match j.prev {
            Some((prot, ppos, pvel)) => {
                let d = rot * prot.inverse();
                let (axis, angle) = d.to_axis_angle();
                let w = if angle.is_finite() && angle.abs() > 1e-6 {
                    axis * angle / dt
                } else {
                    Vec3::ZERO
                };
                let v = (pos - ppos) / dt;
                (rot.inverse() * w, rot.inverse() * ((v - pvel) / dt), v)
            }
            None => (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
        };
        j.prev = Some((rot, pos, vel));
        let swing = j.advance(turn, accel, dt);
        let wag = if j.wag > 0.0 {
            (now * j.wag_speed).sin() * j.wag
        } else {
            0.0
        };
        let new = Quat::from_euler(EulerRot::XYZ, swing.x, wag, swing.y);
        // remove last frame's swing, add this one's
        let base = t.rotation * j.last.inverse();
        t.rotation = base * new;
        j.last = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swings_then_settles() {
        let mut j = Jiggle::hat();
        // a quick head turn about z: the hat rolls the other way
        let mut peak: f32 = 0.0;
        for _ in 0..10 {
            peak = peak.max(-j.step(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, 1.0 / 60.0).y);
        }
        assert!(peak > 0.05, "swings: {peak}");
        // then stillness: it settles back
        let mut last = Vec2::ZERO;
        for _ in 0..240 {
            last = j.step(Vec3::ZERO, Vec3::ZERO, 1.0 / 60.0);
        }
        assert!(last.length() < 0.01, "settles: {last}");
    }

    /// A head turning back and forth (yaw) and bobbing, seen at a frame rate: returns the
    /// swing shown at 0.5 s, 1.0 s, 1.5 s and 2.0 s, and the largest swing seen.
    fn swing_at(fps: f32, substepped: bool) -> (Vec<Vec2>, f32) {
        let mut j = Jiggle::hat();
        let dt = 1.0 / fps;
        let frames = (2.0 * fps).round() as usize;
        let marks: Vec<usize> = [0.5f32, 1.0, 1.5, 2.0].iter().map(|s| (s * fps).round() as usize).collect();
        let (mut out, mut peak) = (vec![], 0.0f32);
        let mut pvel = Vec3::ZERO;
        for f in 1..=frames {
            let t = f as f32 * dt;
            let turn = Vec3::new(0.0, 0.0, 4.0 * (t * 5.0).sin());
            let vel = Vec3::new(0.0, 0.6 * (t * 7.0).cos(), 0.0);
            let accel = (vel - pvel) / dt;
            pvel = vel;
            let s = if substepped { j.advance(turn, accel, dt) } else { j.step(turn, accel, dt) };
            peak = peak.max(s.length());
            if marks.contains(&f) {
                out.push(s);
            }
        }
        (out, peak)
    }

    #[test]
    fn substeps_match_across_frame_rates() {
        let (base, peak) = swing_at(144.0, true);
        assert!(peak > 0.1, "it swings: {peak}");
        let worst = |runs: &[(Vec<Vec2>, f32)], base: &[Vec2]| {
            runs.iter()
                .flat_map(|(r, _)| r.iter().zip(base).map(|(a, b)| a.distance(*b)))
                .fold(0.0f32, f32::max)
        };
        let fixed = worst(&[swing_at(30.0, true), swing_at(60.0, true)], &base);
        // the old way (one step per frame) for comparison
        let (old_base, _) = swing_at(144.0, false);
        let old = worst(&[swing_at(30.0, false), swing_at(60.0, false)], &old_base);
        eprintln!("spring difference across 30/60/144 fps: substepped {fixed:.4}, one step a frame {old:.4}, peak swing {peak:.4}");
        assert!(fixed < peak * 0.01, "within 1% of the swing: off by {fixed}, peak {peak}");
        assert!(fixed < old, "closer than one step a frame: {fixed} vs {old}");
    }

    #[test]
    fn stable_on_a_slow_frame() {
        let mut j = Jiggle::hat();
        let s = j.advance(Vec3::new(0.0, 0.0, 40.0), Vec3::new(30.0, -50.0, 10.0), 0.5);
        assert!(s.is_finite() && s.length() <= 0.6 * 2f32.sqrt(), "{s}");
        assert!(j.carry < SUBSTEP, "a long frame runs at most 100 ms of substeps");
        for _ in 0..600 {
            j.advance(Vec3::ZERO, Vec3::ZERO, 1.0 / 60.0);
        }
        assert!(j.advance(Vec3::ZERO, Vec3::ZERO, 1.0 / 60.0).length() < 0.01, "settles");
    }
}
