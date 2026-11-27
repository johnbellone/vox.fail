//! The edit camera: in edit mode the player flies (no gravity, up and down on the jump and
//! crouch actions, at [`EDIT_FLY`] metres a second, which the server's move check allows), and
//! a fly-to moves the player to the front of an entity. Walking, looking and the view stay the
//! client's (`client::controls` and `client::camera`); this module says how fast the edit
//! flight goes and where a fly-to ends.

use bevy::prelude::*;
use vox_core::body::{Occupancy, with_props};
use vox_core::entity::{Component, EntityId, PROP_UNITS_PER_BLOCK};

use super::EditorState;
use crate::client::WorldClient;
use crate::entities::WorldEntities;

/// Present while the client is in edit mode. `client::controls` reads it: the player then
/// flies at [`EDIT_FLY`] and nothing turns the flight off.
#[derive(Resource, Default)]
pub struct EditFly;

/// The edit flight's speed, metres a second: the client's flying speed. The server's move
/// check allows [`vox_core::moves::MAX_SPEED`] sideways and [`vox_core::moves::MAX_RISE`] up,
/// and the test keeps this well under both, so lag and rounding fit.
pub const EDIT_FLY: f32 = crate::client::FLY;

/// How far in front of an entity a fly-to ends, blocks.
pub const FLY_TO_DISTANCE: f32 = 6.0;

/// A fly-to ends this close to its target, blocks: it stops short rather than push into the
/// entity's own cell.
pub const FLY_TO_STOP: f32 = 1.5;

/// A fly-to gives up after this many frames in a row in which the player moved less than a
/// tenth of the step they were sent: something solid is in the way.
const FLY_TO_STUCK_FRAMES: u32 = 5;

/// Where a fly-to ends for an entity: [`FLY_TO_DISTANCE`] blocks in front of it, at its feet.
/// "In front" is the way the entity faces (its yaw, degrees, as the client's own facing
/// uses it). An objective has no facing, so its front is +z. None for the settings, which
/// have no place in the world.
pub fn fly_target(component: &Component) -> Option<[f32; 3]> {
    let (at, yaw_deg) = match component {
        Component::Portal(p) => (p.at, p.yaw),
        Component::Spawn(s) => (s.at, s.yaw),
        Component::Objective(o) => (o.at, 0.0),
        // a prop's position is in units of a tenth of a block; its facing is in 15 degree steps
        Component::Prop(p) => (p.at.map(|c| c as f32 / PROP_UNITS_PER_BLOCK as f32), p.yaw as f32 * 15.0),
        Component::Settings(_) => return None,
    };
    let yaw = yaw_deg.to_radians();
    Some([at[0] + FLY_TO_DISTANCE * yaw.sin(), at[1], at[2] + FLY_TO_DISTANCE * yaw.cos()])
}

/// One step of the flight toward `to`: the new position, at most `max` blocks from `from` and
/// never past `to`. `arrived` is true when it reached `to`. A target that is not a finite
/// place stays where it is and counts as arrived, so nothing is ever set to NaN.
pub fn fly_step(from: [f32; 3], to: [f32; 3], max: f32) -> ([f32; 3], bool) {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !dist.is_finite() {
        return (from, true);
    }
    let max = max.max(0.0);
    if dist <= max {
        return (to, true);
    }
    let k = max / dist;
    ([from[0] + d[0] * k, from[1] + d[1] * k, from[2] + d[2] * k], false)
}

/// Flies the player toward the entity the page asked for (`EditorState::fly_to`), a step a
/// frame at the edit flight's speed, through the same collision as walking. The request ends
/// within [`FLY_TO_STOP`] of the target, when the entity is gone, or when the player is stuck
/// against something solid for [`FLY_TO_STUCK_FRAMES`] frames. Runs before the controls, which
/// add no motion of their own while it runs (see `client::controls`).
pub fn fly_to(
    real: Res<Time<Real>>,
    entities: Res<WorldEntities>,
    mut state: ResMut<EditorState>,
    mut w: ResMut<WorldClient>,
    props: Option<Res<crate::props::PropOccupancy>>,
    mut stuck: Local<u32>,
) {
    let Some(id) = state.fly_to else {
        *stuck = 0;
        return;
    };
    if !w.active() || !w.ready {
        return;
    }
    let Some(target) = entities.map.get(&EntityId(id)).and_then(|(_, c)| fly_target(c)) else {
        state.fly_to = None;
        *stuck = 0;
        return;
    };
    // blocks: the speed is metres a second, a block is block_size metres
    let frame = real.delta_secs().min(0.25);
    let from = w.body.pos;
    let (pos, _) = fly_step(from, target, EDIT_FLY * frame / w.block_size());
    let delta = [pos[0] - from[0], pos[1] - from[1], pos[2] - from[2]];
    let intended = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    let mut body = w.body;
    {
        // the same solid test as walking: blocks, and the props on screen (none when absent)
        let no_props = Occupancy::default();
        let occupancy = props.as_ref().map_or(&no_props, |p| &p.0);
        let wr: &WorldClient = &w;
        body.travel(with_props(|c| wr.stands_on(c), occupancy), delta, 0.0);
    }
    w.body = body;
    w.fly = true;
    let p = w.body.pos;
    let left = ((target[0] - p[0]).powi(2) + (target[1] - p[1]).powi(2) + (target[2] - p[2]).powi(2)).sqrt();
    if left <= FLY_TO_STOP {
        state.fly_to = None;
        *stuck = 0;
        return;
    }
    let moved = ((p[0] - from[0]).powi(2) + (p[1] - from[1]).powi(2) + (p[2] - from[2]).powi(2)).sqrt();
    if intended > 1e-4 && moved < intended * 0.1 {
        *stuck += 1;
        if *stuck >= FLY_TO_STUCK_FRAMES {
            state.fly_to = None;
            *stuck = 0;
        }
    } else {
        *stuck = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::Spawn;
    use vox_core::moves;

    #[test]
    fn editor_camera_flight_is_under_the_move_budget() {
        // the server refuses a move it cannot pay for; flying must stay under it with room
        // for lag (a quarter more than the flight speed)
        const {
            assert!(EDIT_FLY * 1.25 <= moves::MAX_SPEED, "sideways: flying is too fast for the move budget");
            assert!(EDIT_FLY * 1.25 <= moves::MAX_RISE, "up: flying is too fast for the move budget");
        }
    }

    #[test]
    fn editor_camera_step_approaches_without_overshoot() {
        let to = [3.0, 0.0, 4.0];
        let mut pos = [0.0, 0.0, 0.0];
        let mut last = f32::MAX;
        let mut arrived = false;
        for _ in 0..100 {
            let (next, done) = fly_step(pos, to, 0.7);
            let left = ((to[0] - next[0]).powi(2) + (to[2] - next[2]).powi(2)).sqrt();
            assert!(left <= last, "moved away from the target");
            let moved = ((next[0] - pos[0]).powi(2) + (next[2] - pos[2]).powi(2)).sqrt();
            assert!(moved <= 0.7 + 1e-5, "a step longer than the budget: {moved}");
            pos = next;
            last = left;
            if done {
                arrived = true;
                break;
            }
        }
        assert!(arrived, "reached the target");
        assert_eq!(pos, to, "exactly on the target, not past it");
    }

    #[test]
    fn editor_camera_fly_to_passes_the_server_move_check() {
        // a fly-to at the frame rates a player can have (4 fps, the clamp, and 60 fps): every
        // frame's position must be one the server accepts, or the player is sent back
        const BLOCK: f32 = 0.5;
        let spawn = [1000.0, 0.0, 1000.0];
        for dt in [0.25_f32, 1.0 / 60.0] {
            let mut pos = [0.0, 10.0, 0.0];
            let to = [60.0, 14.0, 40.0];
            let mut check = moves::Moves::default();
            check.start(pos, 0.0);
            let mut now = 0.0_f64;
            let mut arrived = false;
            for frame in 0..100_000 {
                let (next, done) = fly_step(pos, to, EDIT_FLY * dt / BLOCK);
                now += dt as f64;
                assert!(check.check(next, now, BLOCK, spawn).is_ok(), "refused at frame {frame}, dt {dt}, to {next:?}");
                pos = next;
                if done {
                    arrived = true;
                    break;
                }
            }
            assert!(arrived, "reached the target at dt {dt}");
        }
    }

    #[test]
    fn editor_camera_step_to_a_non_finite_target_stays_put() {
        assert_eq!(fly_step([1.0, 1.0, 1.0], [f32::NAN, 0.0, 0.0], 1.0), ([1.0, 1.0, 1.0], true));
    }

    #[test]
    fn editor_camera_target_is_six_blocks_in_front() {
        // yaw 0 faces +z, yaw 90 faces +x (the client's own facing)
        let front = |yaw| {
            fly_target(&Component::Spawn(Spawn { at: [10.0, 2.0, -4.0], yaw })).expect("a spawn has a place")
        };
        let a = front(0.0);
        assert!((a[0] - 10.0).abs() < 1e-4 && (a[1] - 2.0).abs() < 1e-4 && (a[2] - 2.0).abs() < 1e-4, "{a:?}");
        let b = front(90.0);
        assert!((b[0] - 16.0).abs() < 1e-4 && (b[2] + 4.0).abs() < 1e-4, "{b:?}");
    }
}
