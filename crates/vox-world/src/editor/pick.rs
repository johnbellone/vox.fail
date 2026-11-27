//! Picking in edit mode: what the pointer is on while the Select tool is held. The voxel ray
//! ([`vox_core::ray`]) finds the first solid block; ray tests against the boxes of doorway
//! frames and props find the first entity; the nearer of the two is what the click picked.
//! [`pick`] is the pure part and is tested on its own. [`select`] is the system that builds the
//! ray from the pointer (or from the middle of the view while the mouse is captured) and acts
//! on the answer.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use vox_core::entity::{Axis, Component, EntityId, prop_cell};
use vox_core::ray;
use vox_core::world::Cell;

use super::bridge::{Event, Tool};
use super::{EditorOutbox, EditorState};
use crate::WorldCamera;
use crate::client::WorldClient;
use crate::entities::WorldEntities;
use crate::input::{Action, Actions, UiMode};

/// The ray through the pointer, as (origin in blocks, direction). While the mouse is captured
/// it goes through the middle of the view (the crosshair); otherwise through the pointer's place
/// in the window. The direction is the camera's, so it need not be unit length. None when there
/// is no pointer to read or the camera cannot make a ray from it.
pub fn cursor_ray(w: &WorldClient, win: &Window, cam: &Camera, gt: &GlobalTransform) -> Option<([f32; 3], [f32; 3])> {
    let pos = if w.captured { win.size() * 0.5 } else { win.cursor_position()? };
    let ray = cam.viewport_to_world(gt, pos).ok()?;
    let size = w.block_size();
    Some(((ray.origin / size).to_array(), Vec3::from(ray.direction).to_array()))
}

/// Which cells stop a pick or a ray in the client: the endless floor below `w.floor`, and any
/// solid block of the view. The same rule `client::controls` uses for the cursor's ray.
pub fn solid(w: &WorldClient) -> impl Fn(Cell) -> bool + '_ {
    move |c: Cell| w.floor.is_some_and(|f| c[1] < f) || w.view.solid(c)
}

/// How far a pick reaches, in blocks (100 m): the same reach as the cursor's ray in
/// `client::controls`.
pub const PICK_REACH: f32 = vox_core::in_blocks(100.0);

/// A doorway's frame is this thick, in blocks, through its plane, so a ray finds it from either
/// side and a ray grazing the plane still counts.
pub const FRAME_THICKNESS: f32 = vox_core::in_blocks(0.25);

/// An axis-aligned box, in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// What a pick found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// a solid block, and the face the ray came through (a unit step, as `vox_core::ray` gives it)
    Block { cell: Cell, face: [i32; 3] },
    /// an entity whose box the ray meets first, `dist` blocks along the ray
    Entity { id: EntityId, dist: f32 },
}

/// The box of a doorway: its rectangle (the middle of the sill at `at`, `size` wide and high),
/// thickened to [`FRAME_THICKNESS`] through its plane. An `Axis::Z` doorway lies in a plane of
/// constant z and is wide along x, as `client::portals` draws it; `Axis::X` is the other way round.
pub fn frame_box(at: [f32; 3], axis: Axis, size: [f32; 2]) -> Aabb {
    let (across, through) = match axis {
        Axis::Z => (0, 2),
        Axis::X => (2, 0),
    };
    let mut min = at;
    let mut max = [at[0], at[1] + size[1], at[2]];
    min[across] -= size[0] * 0.5;
    max[across] += size[0] * 0.5;
    min[through] -= FRAME_THICKNESS * 0.5;
    max[through] += FRAME_THICKNESS * 0.5;
    Aabb { min, max }
}

/// The box of a prop: the block it stands in. The catalogue's footprint is not on the client yet,
/// so a prop is picked by its block.
pub fn prop_box(at: [i32; 3]) -> Aabb {
    let c = prop_cell(at).map(|v| v as f32);
    Aabb { min: c, max: [c[0] + 1.0, c[1] + 1.0, c[2] + 1.0] }
}

/// The distance along a ray at which it first meets `b`, or None when it misses, or meets it
/// further than `max` along. `dir` is a unit vector. A ray that starts inside the box meets it
/// at 0. A ray along a face of the box, or through its edge, counts as meeting it. A box or a
/// direction that is not finite meets nothing.
pub fn ray_box(origin: [f32; 3], dir: [f32; 3], b: &Aabb, max: f32) -> Option<f32> {
    let finite = |a: &[f32; 3]| a.iter().all(|v| v.is_finite());
    if !finite(&dir) || !finite(&b.min) || !finite(&b.max) {
        return None;
    }
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for i in 0..3 {
        if dir[i].abs() < 1e-9 {
            // parallel to this pair of faces: it misses unless it is between them
            if origin[i] < b.min[i] || origin[i] > b.max[i] {
                return None;
            }
        } else {
            let a = (b.min[i] - origin[i]) / dir[i];
            let c = (b.max[i] - origin[i]) / dir[i];
            near = near.max(a.min(c));
            far = far.min(a.max(c));
        }
    }
    if near > far || far < 0.0 {
        return None;
    }
    let t = near.max(0.0);
    (t <= max).then_some(t)
}

/// What the ray from `origin` along `dir` (blocks; `dir` need not be unit) picks first, within
/// `max_dist` blocks: the nearest of the first solid block (`solid` says which cells are) and
/// the first entity box it meets. On a tie the block wins. A ray with a non-finite origin or a
/// zero direction picks nothing.
pub fn pick(
    origin: [f32; 3],
    dir: [f32; 3],
    max_dist: f32,
    solid: impl Fn(Cell) -> bool,
    portals: &[(EntityId, Aabb)],
    props: &[(EntityId, Aabb)],
) -> Option<Hit> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if !len.is_finite() || len <= 1e-9 || !origin.iter().all(|v| v.is_finite()) {
        return None;
    }
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];
    let block = ray::raycast_with(&solid, origin, d, max_dist);
    let mut entity: Option<(EntityId, f32)> = None;
    for (id, b) in portals.iter().chain(props) {
        let Some(t) = ray_box(origin, d, b, max_dist) else { continue };
        // nearer wins; at the same distance the smaller id wins, so the pick does not depend on
        // the order the entities came in
        let better = match entity {
            None => true,
            Some((best_id, best)) => t < best || (t == best && *id < best_id),
        };
        if better {
            entity = Some((*id, t));
        }
    }
    match (block, entity) {
        (Some(b), Some((id, t))) if t < b.distance => Some(Hit::Entity { id, dist: t }),
        (Some(b), _) => Some(Hit::Block { cell: b.cell, face: b.normal }),
        (None, Some((id, t))) => Some(Hit::Entity { id, dist: t }),
        (None, None) => None,
    }
}

/// On a click with the Select tool held: picks what the pointer is on. An entity becomes the
/// selection and the page hears it; a block is reported as picked; nothing clears the selection.
/// A click while a menu has the mouse does nothing. The click is the same action that breaks a
/// block, so the block it would otherwise break is the one this reports.
#[allow(clippy::too_many_arguments)]
pub fn select(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    w: Res<WorldClient>,
    entities: Res<WorldEntities>,
    mut state: ResMut<EditorState>,
    mut outbox: ResMut<EditorOutbox>,
    cams: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if state.tool != Some(Tool::Select) || !actions.just_pressed(Action::Break) {
        return;
    }
    // the click that captured the mouse (behind camera) is not a pick; the controls set this
    // and run before this system
    if state.pointer_claimed || !w.active() || !w.ready || *mode == UiMode::Menu {
        return;
    }
    let (Ok(win), Ok((cam, gt))) = (windows.single(), cams.single()) else { return };
    let Some((origin, dir)) = cursor_ray(&w, win, cam, gt) else { return };

    let mut portals = Vec::new();
    let mut props = Vec::new();
    for (id, (_, c)) in &entities.map {
        match c {
            Component::Portal(p) => portals.push((*id, frame_box(p.at, p.axis, p.size))),
            Component::Prop(p) => props.push((*id, prop_box(p.at))),
            _ => {}
        }
    }
    match pick(origin, dir, PICK_REACH, solid(&w), &portals, &props) {
        Some(Hit::Entity { id, .. }) => {
            state.selected = Some(id.0);
            outbox.0.push(Event::Selected { entity: Some(id.0) });
        }
        Some(Hit::Block { cell, .. }) => outbox.0.push(Event::Picked { cell }),
        None => {
            state.selected = None;
            outbox.0.push(Event::Selected { entity: None });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: [f32; 3] = [0.0, 1.0, 0.0];
    const FWD: [f32; 3] = [0.0, 0.0, 1.0];

    fn no_blocks(_: Cell) -> bool {
        false
    }

    /// A doorway in the plane z = 10, 4 wide and 6 high, its sill at y = 0.
    fn gate(id: u32) -> (EntityId, Aabb) {
        (EntityId(id), frame_box([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]))
    }

    #[test]
    fn editor_pick_portal_frame_front_edge_and_miss() {
        let b = frame_box([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]);
        // front: straight at the middle of the frame, it meets the near face half a frame
        // thickness before the plane z = 10
        let t = ray_box([0.0, 3.0, 0.0], FWD, &b, 100.0).expect("the front is met");
        assert!((t - (10.0 - FRAME_THICKNESS / 2.0)).abs() < 1e-5, "{t}");
        // edge: along the frame's side at x = 2 (its right edge) and at its top at y = 6
        assert!(ray_box([2.0, 3.0, 0.0], FWD, &b, 100.0).is_some(), "the edge counts");
        assert!(ray_box([0.0, 6.0, 0.0], FWD, &b, 100.0).is_some(), "the top edge counts");
        // miss: just past the right edge, just above the top, and behind the ray
        assert!(ray_box([2.01, 3.0, 0.0], FWD, &b, 100.0).is_none());
        assert!(ray_box([0.0, 6.01, 0.0], FWD, &b, 100.0).is_none());
        assert!(ray_box([0.0, 3.0, 20.0], FWD, &b, 100.0).is_none());
        // and not within reach
        assert!(ray_box([0.0, 3.0, 0.0], FWD, &b, 5.0).is_none());
    }

    #[test]
    fn editor_pick_portal_frame_of_an_x_doorway() {
        let b = frame_box([5.0, 0.0, 0.0], Axis::X, [4.0, 6.0]);
        let t = ray_box([0.0, 3.0, 0.0], [1.0, 0.0, 0.0], &b, 100.0).expect("the front is met");
        assert!((t - (5.0 - FRAME_THICKNESS / 2.0)).abs() < 1e-5, "{t}");
        // the frame is 4 wide along z, so z = 2.01 is past it
        assert!(ray_box([0.0, 3.0, 2.01], [1.0, 0.0, 0.0], &b, 100.0).is_none());
    }

    #[test]
    fn editor_pick_nearest_wins_between_a_block_and_a_portal() {
        let wall = |c: Cell| c == [0, 3, 5];
        // the wall (at z = 5) is nearer than the doorway (at z = 10): the block is picked
        let hit = pick([0.5, 3.5, 0.5], FWD, 100.0, wall, &[gate(1)], &[]);
        assert_eq!(hit, Some(Hit::Block { cell: [0, 3, 5], face: [0, 0, -1] }));
        // a doorway in front of the wall is nearer: the portal is picked, at its near face
        let door = (EntityId(2), frame_box([0.0, 2.0, 3.0], Axis::Z, [4.0, 6.0]));
        match pick([0.5, 3.5, 0.5], FWD, 100.0, wall, &[door], &[]) {
            Some(Hit::Entity { id, dist }) => {
                assert_eq!(id, EntityId(2));
                // from z = 0.5 to the near face of a frame at z = 3
                assert!((dist - (2.5 - FRAME_THICKNESS / 2.0)).abs() < 1e-5, "{dist}");
            }
            other => panic!("expected the doorway, got {other:?}"),
        }
        // with no block in the way, the doorway is what the ray picks
        assert!(matches!(pick([0.5, 3.5, 0.5], FWD, 100.0, no_blocks, &[gate(1)], &[]), Some(Hit::Entity { id, .. }) if id == EntityId(1)));
        // nothing within reach
        assert_eq!(pick([0.5, 3.5, 0.5], FWD, 5.0, no_blocks, &[gate(1)], &[]), None);
    }

    #[test]
    fn editor_pick_prop_box_is_hit_by_its_block() {
        // a prop at block (-3, 2, 4): its position is in tenths of a block
        let prop = (EntityId(7), prop_box([-30, 25, 40]));
        match pick([-2.5, 2.5, 0.5], FWD, 100.0, no_blocks, &[], &[prop]) {
            Some(Hit::Entity { id, dist }) => {
                assert_eq!(id, EntityId(7));
                assert!((dist - 3.5).abs() < 1e-5, "{dist}");
            }
            other => panic!("expected the prop, got {other:?}"),
        }
        // beside it, the ray misses
        assert_eq!(pick([-1.5, 2.5, 0.5], FWD, 100.0, no_blocks, &[], &[prop]), None);
    }

    #[test]
    fn editor_pick_equal_distances_pick_the_smaller_id() {
        // two props at the same distance: the smaller id wins whichever comes first
        let a = (EntityId(3), prop_box([-30, 25, 40]));
        let b = (EntityId(9), prop_box([-30, 25, 40]));
        for props in [[a, b], [b, a]] {
            match pick([-2.5, 2.5, 0.5], FWD, 100.0, no_blocks, &[], &props) {
                Some(Hit::Entity { id, .. }) => assert_eq!(id, EntityId(3)),
                other => panic!("expected entity 3, got {other:?}"),
            }
        }
    }

    #[test]
    fn editor_pick_non_finite_boxes_and_directions_meet_nothing() {
        let b = frame_box([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]);
        assert_eq!(ray_box([0.0, 3.0, 0.0], [f32::NAN, 0.0, 1.0], &b, 100.0), None);
        assert_eq!(ray_box([0.0, 3.0, 0.0], [0.0, 0.0, f32::INFINITY], &b, 100.0), None);
        let bad = Aabb { min: [f32::NAN, 0.0, 0.0], max: [1.0, 1.0, 1.0] };
        assert_eq!(ray_box([0.0, 0.5, -5.0], FWD, &bad, 100.0), None);
    }

    #[test]
    fn editor_pick_degenerate_rays_pick_nothing() {
        let solid = |c: Cell| c == [0, 0, 5];
        assert_eq!(pick([0.5, 0.5, 0.5], [0.0; 3], 100.0, solid, &[], &[]), None);
        assert_eq!(pick([f32::NAN, 0.5, 0.5], FWD, 100.0, solid, &[], &[]), None);
        // a direction need not be unit: the same pick as the unit one
        let a = pick([0.5, 0.5, 0.5], FWD, 100.0, solid, &[], &[]);
        let b = pick([0.5, 0.5, 0.5], [0.0, 0.0, 7.0], 100.0, solid, &[], &[]);
        assert_eq!(a, b);
        assert!(matches!(a, Some(Hit::Block { cell: [0, 0, 5], .. })));
        // and a ray that goes up misses a block in front of it
        assert_eq!(pick([0.5, 0.5, 0.5], UP, 100.0, solid, &[], &[]), None);
    }
}
