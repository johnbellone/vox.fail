//! Crossings between worlds (ADR 0004, `docs/decisions/0004-seamless-transitions.md`): how two
//! worlds' coordinates meet ([`Frame`]), how a crossing carries you on moving as you were
//! ([`Motion`], checked by [`Motion::sane`]), and what a doorway shows about where it leads
//! ([`DoorState`]).
//!
//! The world you leave works out the frame and your motion; the directory signs your motion
//! into the ticket ([`crate::auth::Ticket`]); the client is told both in
//! [`crate::wire::ToClient::Travel`] (and, ahead of time, the frame in
//! [`crate::wire::ToClient::Prepare`]), so it can draw the far side in place and arrive still
//! moving. Positions are in blocks; velocities in blocks a second.

use serde::{Deserialize, Serialize};

use crate::moves::{MAX_FALL, MAX_RISE, MAX_SPEED};

/// Farthest a frame may move a point, blocks (a billion: far past any world, but finite, so a
/// hostile frame can't make positions infinite).
pub const MAX_OFFSET: f32 = 1.0e9;

/// How the world you are in meets another: a point `p` here is `turn(p) + offset` there, where
/// `turn` is a rotation about y by `turn` radians (as `glam::Quat::from_rotation_y` applies it).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub offset: [f32; 3],
    /// radians about y
    pub turn: f32,
}

impl Frame {
    /// The two worlds share their coordinates.
    pub const SAME: Frame = Frame { offset: [0.0; 3], turn: 0.0 };

    /// Just moved, not turned (an atlas's tiles, a stack of worlds).
    pub fn moved(offset: [f32; 3]) -> Frame {
        Frame { offset, turn: 0.0 }
    }

    /// Finite, and moving points no farther than [`MAX_OFFSET`]: a frame from the network is
    /// checked with this before it is used.
    pub fn valid(&self) -> bool {
        self.turn.is_finite() && self.offset.iter().all(|v| v.is_finite() && v.abs() <= MAX_OFFSET)
    }

    /// A direction here (a velocity) as the same direction there.
    pub fn direction(&self, v: [f32; 3]) -> [f32; 3] {
        let (s, c) = self.turn.sin_cos();
        [c * v[0] + s * v[2], v[1], -s * v[0] + c * v[2]]
    }

    /// A point here as the same point there.
    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let d = self.direction(p);
        [d[0] + self.offset[0], d[1] + self.offset[1], d[2] + self.offset[2]]
    }

    /// A facing here (radians; forward is `(sin yaw, cos yaw)` on x and z, as the client has
    /// it) as the same facing there.
    pub fn facing(&self, yaw: f32) -> f32 {
        yaw + self.turn
    }
}

impl Default for Frame {
    fn default() -> Self {
        Frame::SAME
    }
}

/// How you were moving as you crossed: carried into the world you arrive in.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    /// blocks a second
    pub velocity: [f32; 3],
    /// which way you face, radians (forward is `(sin yaw, cos yaw)` on x and z)
    pub yaw: f32,
}

impl Motion {
    /// Standing still, facing `yaw`.
    pub fn still(yaw: f32) -> Motion {
        Motion { velocity: [0.0; 3], yaw }
    }

    /// Within what a player can really do (the same limits as the server's move check,
    /// [`crate::moves`]), in a world of blocks `block_size` metres across: sideways at most
    /// [`MAX_SPEED`], up at most [`MAX_RISE`], down at most [`MAX_FALL`] (metres a second). Not
    /// finite is still; the facing is brought into 0..2π. Motion from a client, or carried in a
    /// ticket, goes through this before anything uses it.
    pub fn sane(self, block_size: f32) -> Motion {
        let per_block = 1.0 / block_size.max(0.01);
        let v = if self.velocity.iter().all(|x| x.is_finite()) { self.velocity } else { [0.0; 3] };
        let flat = (v[0] * v[0] + v[2] * v[2]).sqrt();
        let most = MAX_SPEED * per_block;
        let k = if flat > most { most / flat } else { 1.0 };
        let y = v[1].clamp(-MAX_FALL * per_block, MAX_RISE * per_block);
        let yaw = if self.yaw.is_finite() { self.yaw.rem_euclid(std::f32::consts::TAU) } else { 0.0 };
        Motion { velocity: [v[0] * k, y, v[2] * k], yaw }
    }

    /// This motion as it is on the other side of `frame`.
    pub fn through(self, frame: &Frame) -> Motion {
        Motion { velocity: frame.direction(self.velocity), yaw: frame.facing(self.yaw) }
    }
}

/// What a doorway shows about where it leads (ADR 0004 decision 11). Each looks different in
/// colour and in motion. It describes only that doorway's own destination.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DoorState {
    /// not checked yet: today's faint look
    #[default]
    Unknown,
    /// running, and it will take you
    Open,
    /// stopped and starting (or its host is): you are held at the sill until it opens
    Waking,
    /// nothing answers: you bump back, "the way is closed"
    Unlinked,
    /// there, but not for you; the reason in general terms
    Restricted(Restriction),
}

/// Why a doorway is restricted, in general terms only.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restriction {
    /// a dungeon for a raid party you are not in
    Party,
    /// the world is full
    Full,
    /// the world needs you signed in
    SignIn,
}

impl DoorState {
    /// Whether walking into the doorway takes you through now.
    pub fn lets_through(self) -> bool {
        matches!(self, DoorState::Open | DoorState::Unknown)
    }

    /// The line shown close to the doorway, for a destination called `name`; None for an open
    /// or unchecked one.
    pub fn line(self, name: &str) -> Option<String> {
        match self {
            DoorState::Open | DoorState::Unknown => None,
            DoorState::Waking => Some(format!("The way to {name} is opening")),
            DoorState::Unlinked => Some(format!("The way to {name} is closed")),
            DoorState::Restricted(Restriction::Party) => Some(format!("The way to {name} is for a raid party")),
            DoorState::Restricted(Restriction::Full) => Some(format!("{name} is full")),
            DoorState::Restricted(Restriction::SignIn) => Some(format!("Sign in to go to {name}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3)
    }

    #[test]
    fn a_frame_moves_and_turns_points_and_turns_directions_only() {
        let f = Frame { offset: [10.0, 0.0, 0.0], turn: FRAC_PI_2 };
        // a quarter turn takes +x to -z, as glam's rotation about y does
        assert!(close(f.direction([1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
        assert!(close(f.point([1.0, 2.0, 0.0]), [10.0, 2.0, -1.0]));
        assert!(close(Frame::SAME.point([3.0, 4.0, 5.0]), [3.0, 4.0, 5.0]));
        assert!(close(Frame::moved([0.0, -5.0, 0.0]).direction([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]), "a move doesn't change a velocity");
    }

    #[test]
    fn a_facing_turns_with_the_frame_as_its_forward_direction_does() {
        for turn in [0.3, FRAC_PI_2, PI, -2.0] {
            let f = Frame { offset: [0.0; 3], turn };
            for yaw in [0.0f32, 1.0, -2.5] {
                let fwd = [yaw.sin(), 0.0, yaw.cos()];
                let there = f.facing(yaw);
                assert!(close(f.direction(fwd), [there.sin(), 0.0, there.cos()]), "turn {turn} yaw {yaw}");
            }
        }
    }

    #[test]
    fn a_frame_from_the_network_must_be_finite_and_near() {
        assert!(Frame::SAME.valid());
        assert!(!Frame { offset: [f32::NAN, 0.0, 0.0], turn: 0.0 }.valid());
        assert!(!Frame { offset: [0.0; 3], turn: f32::INFINITY }.valid());
        assert!(!Frame::moved([0.0, 2.0e9, 0.0]).valid());
    }

    #[test]
    fn motion_is_held_to_what_a_player_can_do() {
        let b = crate::BLOCK_M;
        let per = 1.0 / b;
        // a fast fall is kept up to the most a fall may be
        let fall = Motion { velocity: [0.0, -500.0 * per, 0.0], yaw: 0.0 }.sane(b);
        assert!((fall.velocity[1] + MAX_FALL * per).abs() < 1e-3);
        // sideways is scaled down along its own direction
        let side = Motion { velocity: [30.0 * per, 0.0, 40.0 * per], yaw: 0.0 }.sane(b);
        let flat = (side.velocity[0].powi(2) + side.velocity[2].powi(2)).sqrt();
        assert!((flat - MAX_SPEED * per).abs() < 1e-2);
        assert!((side.velocity[0] / side.velocity[2] - 0.75).abs() < 1e-4, "same direction");
        // rising is capped
        assert!((Motion { velocity: [0.0, 100.0 * per, 0.0], yaw: 0.0 }.sane(b).velocity[1] - MAX_RISE * per).abs() < 1e-3);
        // a walk is left alone
        let walk = Motion { velocity: [3.2 * per, 0.0, 0.0], yaw: 1.0 };
        assert_eq!(walk.sane(b), walk);
        // nonsense is still
        let bad = Motion { velocity: [f32::NAN, 1.0, 1.0], yaw: f32::INFINITY }.sane(b);
        assert_eq!(bad, Motion::still(0.0));
        assert!((Motion::still(-FRAC_PI_2).sane(b).yaw - 3.0 * FRAC_PI_2).abs() < 1e-5, "facing in 0..2π");
    }

    #[test]
    fn doorway_states_say_whether_you_go_through_and_why_not() {
        assert!(DoorState::Open.lets_through() && DoorState::Unknown.lets_through());
        assert!(!DoorState::Waking.lets_through() && !DoorState::Unlinked.lets_through());
        assert!(!DoorState::Restricted(Restriction::Full).lets_through());
        assert_eq!(DoorState::Open.line("the yard"), None);
        assert_eq!(DoorState::Unlinked.line("the yard").as_deref(), Some("The way to the yard is closed"));
        assert_eq!(DoorState::Restricted(Restriction::Party).line("the crypt").as_deref(), Some("The way to the crypt is for a raid party"));
    }
}
