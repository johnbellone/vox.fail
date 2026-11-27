//! Gizmos in edit mode: the handles on the selected doorway or prop, and the drag that moves,
//! turns or resizes it. A handle is pressed with the Break action (the left click) while the
//! Select tool is held. While the button stays down the pointer's ray (see
//! [`pick::cursor_ray`]) gives the new place, drawn as an outline. On release the change is sent
//! as a `Set` with the seq the page last saw (through [`send_edit`], the same path as the page's
//! own commands). The Place action (right click) drops the drag and sends nothing. Escape (Back)
//! is not a cancel: it is also Menu, which opens the pause menu; the mode then becomes Menu, the
//! drag is dropped, and nothing is sent. If the cursor ray is lost, a drag ends when the Break
//! button is up, and sends nothing.
//!
//! Gizmo edits are sent with no `ref`. They only move, turn or resize an entity that already
//! exists, which never charges (only making a new portal does), so a drag refused as Unavailable
//! can simply be made again under a new edit id.
//!
//! What each handle does:
//! - Move (a disc on the ground at the sill, or at a prop's foot): a doorway's sill goes to the
//!   half block under the pointer, a prop to the 5 cm unit under it. Height is kept. A move
//!   reaches at most [`MAX_MOVE`] blocks from where the drag began.
//! - Turn (a ring on the ground around it): a doorway's axis turns a quarter for each 90 degrees
//!   the pointer swings round it. A quarter turn swaps its axis (x and z) and keeps its yaw (the
//!   facing on the far side, absolute), so the arrival point stays the same; a half turn changes
//!   nothing. A prop turns in 15 degree steps, in the way its rotation turns (clockwise from
//!   above when the pointer swings clockwise).
//! - Resize (a knob at the top corner of a doorway): the width is twice the distance from the
//!   sill's middle to the pointer across the doorway, the height is the pointer's height above
//!   the sill, both in whole blocks and clamped to 1 to 8 wide and 1 to 16 high. Props do not
//!   resize.
//!
//! The pure parts ([`snap_move`], [`snap_prop_move`], [`turn_portal`], [`turn_prop`],
//! [`clamp_size`], [`ray_plane`], [`grab`], [`drag_to`]) are tested on their own. [`gizmo`] is the
//! system that runs them with the pointer.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use vox_core::entity::{Axis, Component, EntityId, PROP_UNITS_PER_BLOCK, PROP_YAWS, Portal, Prop};

use super::bridge::{Command, Tool};
use super::pick::{self, PICK_REACH};
use super::{EditorOutbox, EditorState, send_edit};
use crate::WorldCamera;
use crate::client::WorldClient;
use crate::entities::WorldEntities;
use crate::input::{Action, Actions, UiMode};

/// The largest width a resize gives a doorway, blocks.
pub const MAX_WIDTH: f32 = 8.0;
/// The largest height a resize gives a doorway, blocks.
pub const MAX_HEIGHT: f32 = 16.0;
/// The furthest a move takes an entity from where its drag began (ground distance), blocks.
pub const MAX_MOVE: f32 = 64.0;

/// How far from the centre a move disc is grabbed, blocks.
const MOVE_R: f32 = 0.4;
/// How far from the corner a resize knob is grabbed, blocks.
const KNOB_R: f32 = 0.5;
/// The radius of a doorway's turn ring, blocks.
const DOORWAY_RING: f32 = 1.5;
/// The radius of a prop's turn ring, blocks.
const PROP_RING: f32 = 1.0;
/// How far either side of the ring the ring is grabbed, blocks.
const RING_BAND: f32 = 0.25;

/// Which handle a drag moves with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Move,
    Turn,
    Resize,
}

/// A drag in progress: the entity and the seq the page last saw for it, the handle, the
/// component when the drag began, the change it would send now, and (for a turn) the angle the
/// pointer had round the centre when it began.
struct Drag {
    entity: EntityId,
    seq: u64,
    handle: Handle,
    start: Component,
    now: Component,
    angle0: f32,
}

/// The drag in progress, if any. Dropped when the selection changes or the Select tool is put
/// down.
#[derive(Resource, Default)]
pub struct GizmoDrag(Option<Drag>);

/// Snaps a doorway's sill to the half block on the ground: x and z only. The height stays, so a
/// ground drag never raises or lowers the doorway.
pub fn snap_move(at: [f32; 3]) -> [f32; 3] {
    let half = |v: f32| (v * 2.0).round() / 2.0;
    [half(at[0]), at[1], half(at[2])]
}

/// A prop's new position: the ground point (x, z, in blocks) in 5 cm units, with its height in
/// units kept. None when the point is not finite.
pub fn snap_prop_move(ground: [f32; 2], height: i32) -> Option<[i32; 3]> {
    if !ground.iter().all(|v| v.is_finite()) {
        return None;
    }
    let unit = |v: f32| (v * PROP_UNITS_PER_BLOCK as f32).round() as i32;
    Some([unit(ground[0]), height, unit(ground[1])])
}

/// A doorway after `quarters` quarter turns (negative turns the other way). An odd number swaps
/// its axis, since a plane of constant z turned a quarter is one of constant x. The yaw (the
/// facing on the far side, absolute, 0 north) is left as it was, so the arrival point does not
/// change: a half turn gives back the doorway it started with.
pub fn turn_portal(axis: Axis, yaw: f32, quarters: i32) -> (Axis, f32) {
    let axis = match (axis, quarters.rem_euclid(4) % 2) {
        (Axis::Z, 1) => Axis::X,
        (Axis::X, 1) => Axis::Z,
        (kept, _) => kept,
    };
    (axis, yaw)
}

/// A prop after `steps` steps of 15 degrees (negative turns the other way), kept in 0 to 23.
pub fn turn_prop(yaw: u8, steps: i32) -> u8 {
    let n = i32::from(PROP_YAWS);
    ((i32::from(yaw) + steps.rem_euclid(n)) % n) as u8
}

/// The signed angle from `from` to `to` (radians), in (-pi, pi].
pub fn angle_between(from: f32, to: f32) -> f32 {
    let d = (to - from).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

/// The whole number of `step`s (radians) in a turn of `delta` (radians), to the nearest.
pub fn turn_steps(delta: f32, step: f32) -> i32 {
    (delta / step).round() as i32
}

/// A doorway size after a resize: each side rounded to whole blocks and clamped to 1 to
/// [`MAX_WIDTH`] wide and 1 to [`MAX_HEIGHT`] high. None when a side is not finite.
pub fn clamp_size(size: [f32; 2]) -> Option<[f32; 2]> {
    if !size.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some([size[0].round().clamp(1.0, MAX_WIDTH), size[1].round().clamp(1.0, MAX_HEIGHT)])
}

/// Where the ray meets the plane whose coordinate `axis` (0 x, 1 y, 2 z) is `at`. The ray's
/// direction is of unit length (as [`pick::cursor_ray`] gives it), so the distance along it is in
/// blocks. None when the ray runs parallel to the plane, meets it behind the origin or further
/// than [`PICK_REACH`] (a near-parallel ray does this), or any input is not finite. Horizontal
/// ground is axis 1.
pub fn ray_plane(origin: [f32; 3], dir: [f32; 3], axis: usize, at: f32) -> Option<[f32; 3]> {
    if !at.is_finite() || !origin.iter().chain(&dir).all(|v| v.is_finite()) || dir[axis].abs() < 1e-9 {
        return None;
    }
    let t = (at - origin[axis]) / dir[axis];
    if !(0.0..=PICK_REACH).contains(&t) {
        return None;
    }
    let hit = std::array::from_fn(|i| origin[i] + dir[i] * t);
    hit.iter().all(|v| v.is_finite()).then_some(hit)
}

/// `hit` pulled back onto the circle of [`MAX_MOVE`] round `from` when it lies beyond it, so one
/// drag cannot carry an entity across the map. Ground distance only (x and z); height is kept.
fn clamp_move(hit: [f32; 3], from: [f32; 3]) -> [f32; 3] {
    let d = dist_xz(hit, from);
    if d <= MAX_MOVE {
        return hit;
    }
    let k = MAX_MOVE / d;
    [from[0] + (hit[0] - from[0]) * k, hit[1], from[2] + (hit[2] - from[2]) * k]
}

/// The axis index a doorway's plane is at: z for an `Axis::Z` doorway, x for `Axis::X`.
fn through_axis(axis: Axis) -> usize {
    match axis {
        Axis::Z => 2,
        Axis::X => 0,
    }
}

/// Where a selected entity's handles are, in blocks: the centre (a doorway's sill, a prop's
/// foot), the turn ring's radius, and for a doorway the resize knob with the axis index of the
/// plane it lies in.
struct Layout {
    centre: [f32; 3],
    ring: f32,
    knob: Option<([f32; 3], usize)>,
}

fn layout(c: &Component) -> Option<Layout> {
    match c {
        Component::Portal(p) => {
            let through = through_axis(p.axis);
            // the width runs along the other horizontal axis
            let across = 2 - through;
            let mut corner = p.at;
            corner[across] += p.size[0] * 0.5;
            corner[1] += p.size[1];
            Some(Layout { centre: p.at, ring: DOORWAY_RING, knob: Some((corner, through)) })
        }
        Component::Prop(p) => {
            let unit = PROP_UNITS_PER_BLOCK as f32;
            Some(Layout { centre: p.at.map(|u| u as f32 / unit), ring: PROP_RING, knob: None })
        }
        _ => None,
    }
}

/// The angle of `p` round `centre`, seen from above (x and z).
fn angle_of(centre: [f32; 3], p: [f32; 3]) -> f32 {
    (p[2] - centre[2]).atan2(p[0] - centre[0])
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The handle a ray presses first on an entity, and (for a turn) the pointer's angle round the
/// centre. The knob is tested first, then the move disc, then the ring. None when the ray meets
/// none of them, or the entity has no handles (not a doorway or a prop).
pub fn grab(c: &Component, origin: [f32; 3], dir: [f32; 3]) -> Option<(Handle, f32)> {
    let l = layout(c)?;
    if let Some((corner, through)) = l.knob
        && let Some(hit) = ray_plane(origin, dir, through, corner[through])
        && dist(hit, corner) <= KNOB_R
    {
        return Some((Handle::Resize, 0.0));
    }
    let hit = ray_plane(origin, dir, 1, l.centre[1])?;
    let d = dist_xz(hit, l.centre);
    if d <= MOVE_R {
        return Some((Handle::Move, 0.0));
    }
    if (d - l.ring).abs() <= RING_BAND {
        return Some((Handle::Turn, angle_of(l.centre, hit)));
    }
    None
}

/// The component a drag on `handle` makes now, from the component it began with. `angle0` is the
/// pointer's angle round the centre when the turn began. None when the ray misses the handle's
/// plane or the result would not be a sensible value (the caller then keeps the last change).
pub fn drag_to(handle: Handle, start: &Component, angle0: f32, origin: [f32; 3], dir: [f32; 3]) -> Option<Component> {
    match (handle, start) {
        (Handle::Move, Component::Portal(p)) => {
            let hit = clamp_move(ray_plane(origin, dir, 1, p.at[1])?, layout(start)?.centre);
            Some(Component::Portal(Portal { at: snap_move(hit), ..p.clone() }))
        }
        (Handle::Move, Component::Prop(p)) => {
            let hit = ray_plane(origin, dir, 1, p.at[1] as f32 / PROP_UNITS_PER_BLOCK as f32)?;
            let hit = clamp_move(hit, layout(start)?.centre);
            let at = snap_prop_move([hit[0], hit[2]], p.at[1])?;
            Some(Component::Prop(Prop { at, ..p.clone() }))
        }
        (Handle::Turn, Component::Portal(p)) => {
            let l = layout(start)?;
            let hit = ray_plane(origin, dir, 1, l.centre[1])?;
            let delta = angle_between(angle0, angle_of(l.centre, hit));
            let (axis, yaw) = turn_portal(p.axis, p.yaw, turn_steps(delta, FRAC_PI_2));
            Some(Component::Portal(Portal { axis, yaw, ..p.clone() }))
        }
        (Handle::Turn, Component::Prop(p)) => {
            let l = layout(start)?;
            let hit = ray_plane(origin, dir, 1, l.centre[1])?;
            let delta = angle_between(angle0, angle_of(l.centre, hit));
            // angle_of grows clockwise seen from above, but a prop's yaw turns it counter-clockwise
            // (Quat::from_rotation_y(+yaw)), so the steps go the other way to follow the pointer
            let yaw = turn_prop(p.yaw, -turn_steps(delta, TAU / f32::from(PROP_YAWS)));
            Some(Component::Prop(Prop { yaw, ..p.clone() }))
        }
        (Handle::Resize, Component::Portal(p)) => {
            let through = through_axis(p.axis);
            let across = 2 - through;
            let hit = ray_plane(origin, dir, through, p.at[through])?;
            let size = clamp_size([2.0 * (hit[across] - p.at[across]).abs(), hit[1] - p.at[1]])?;
            Some(Component::Portal(Portal { size, ..p.clone() }))
        }
        _ => None,
    }
}

/// Draws the handles of `c` (blocks scaled to world units by `size`), and the outline of a
/// doorway. A dragged doorway is drawn yellow, a resting one white.
fn draw_handles(draw: &mut Gizmos, c: &Component, size: f32, dragging: bool) {
    let Some(l) = layout(c) else { return };
    let world = |p: [f32; 3]| Vec3::from_array(p) * size;
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    let outline = if dragging { Color::srgba(1.0, 0.9, 0.2, 0.95) } else { Color::srgba(1.0, 1.0, 1.0, 0.8) };
    if let Component::Portal(p) = c {
        let f = pick::frame_box(p.at, p.axis, p.size);
        let centre = [0, 1, 2].map(|i| (f.min[i] + f.max[i]) * 0.5);
        let extent = [0, 1, 2].map(|i| (f.max[i] - f.min[i]) * size);
        draw.cube(Transform::from_translation(world(centre)).with_scale(Vec3::from_array(extent)), outline);
    }
    draw.circle(Isometry3d::new(world(l.centre) + Vec3::Y * 0.02 * size, flat), MOVE_R * size, Color::srgba(1.0, 0.85, 0.2, 0.9));
    draw.circle(Isometry3d::new(world(l.centre), flat), l.ring * size, Color::srgba(0.3, 0.9, 1.0, 0.9));
    if let Some((corner, _)) = l.knob {
        draw.cube(Transform::from_translation(world(corner)).with_scale(Vec3::splat(KNOB_R * size)), Color::srgba(1.0, 0.4, 0.9, 0.9));
    }
}

/// With the Select tool and a doorway or prop selected, draws its handles and runs a drag on
/// them. Runs after the controls (so the click that captures the mouse is known) and before
/// `pick::select`: a click that presses a handle sets `pointer_claimed`, so the click is not also
/// a pick. Only once the world is ready, and not while a menu has the mouse.
#[allow(clippy::too_many_arguments)]
pub fn gizmo(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    w: Res<WorldClient>,
    entities: Res<WorldEntities>,
    mut state: ResMut<EditorState>,
    mut outbox: ResMut<EditorOutbox>,
    mut drag: ResMut<GizmoDrag>,
    mut draw: Gizmos,
    cams: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // the selected entity, if it is a doorway or a prop: the only kinds with handles
    let target = state.selected.map(EntityId).and_then(|id| {
        let (seq, c) = entities.map.get(&id)?;
        matches!(c, Component::Portal(_) | Component::Prop(_)).then(|| (id, *seq, c.clone()))
    });
    let live = state.tool == Some(Tool::Select) && w.active() && w.ready && *mode != UiMode::Menu;
    let Some((id, seq, current)) = target.filter(|_| live) else {
        drag.0 = None;
        return;
    };
    if drag.0.as_ref().is_some_and(|d| d.entity != id) {
        drag.0 = None;
    }
    let (Ok(win), Ok((cam, gt))) = (windows.single(), cams.single()) else {
        drag.0 = None;
        return;
    };
    let Some((origin, dir)) = pick::cursor_ray(&w, win, cam, gt) else {
        // no pointer ray (the cursor left the window): the drag waits while Break is held, and
        // ends with nothing sent once it is up
        if !actions.pressed(Action::Break) {
            drag.0 = None;
        }
        return;
    };

    if let Some(mut d) = drag.0.take() {
        if actions.just_pressed(Action::Place) {
            // cancelled: the drag is dropped and nothing is sent
        } else if actions.pressed(Action::Break) {
            if let Some(now) = drag_to(d.handle, &d.start, d.angle0, origin, dir) {
                d.now = now;
            }
            drag.0 = Some(d);
        } else if d.now != d.start {
            // released: one Set, expecting the seq the page last saw, so a change made by
            // someone else in the meantime is refused rather than overwritten
            send_edit(&w, &entities, &mut state, &mut outbox, &Command::Set { entity: d.entity.0, component: d.now, expect: Some(d.seq), r#ref: None });
        }
    } else if actions.just_pressed(Action::Break)
        && !state.pointer_claimed
        && let Some((handle, angle0)) = grab(&current, origin, dir)
    {
        state.pointer_claimed = true;
        drag.0 = Some(Drag { entity: id, seq, handle, start: current.clone(), now: current.clone(), angle0 });
    }

    let dragging = drag.0.is_some();
    let shown = drag.0.as_ref().map_or(current, |d| d.now.clone());
    draw_handles(&mut draw, &shown, w.block_size(), dragging);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portal(at: [f32; 3], axis: Axis, size: [f32; 2]) -> Component {
        portal_yaw(at, axis, size, 0.0)
    }

    fn portal_yaw(at: [f32; 3], axis: Axis, size: [f32; 2], yaw: f32) -> Component {
        Component::Portal(Portal {
            name: "door".into(),
            at,
            axis,
            size,
            yaw,
            level: "yard".into(),
            to: vox_core::entity::Dest::World { world: "yard".into() },
            key: None,
        })
    }

    fn prop(at: [i32; 3], yaw: u8) -> Component {
        Component::Prop(Prop { prop: [0; 16], at, yaw, variant: 0, state: 0, channel: None, text: None, owner: None })
    }

    #[test]
    fn editor_gizmo_snap_move_rounds_to_half_block_and_keeps_height() {
        assert_eq!(snap_move([1.2, 3.0, -0.3]), [1.0, 3.0, -0.5]);
        assert_eq!(snap_move([4.9, 0.0, 7.1]), [5.0, 0.0, 7.0]);
    }

    #[test]
    fn editor_gizmo_snap_prop_move_uses_five_centimetre_units() {
        assert_eq!(snap_prop_move([1.23, -0.04], 25), Some([12, 25, 0]));
        assert_eq!(snap_prop_move([f32::NAN, 0.0], 25), None);
    }

    #[test]
    fn editor_gizmo_turn_portal_quarter_turn_swaps_axis_keeps_yaw() {
        assert_eq!(turn_portal(Axis::Z, 0.0, 1), (Axis::X, 0.0));
        assert_eq!(turn_portal(Axis::X, 90.0, 1), (Axis::Z, 90.0));
        assert_eq!(turn_portal(Axis::Z, 0.0, -1), (Axis::X, 0.0));
        // four quarters is the doorway again
        assert_eq!(turn_portal(Axis::Z, 90.0, 4), (Axis::Z, 90.0));
        // a half turn changes nothing: the same axis and the same yaw, so the arrival stays put
        assert_eq!(turn_portal(Axis::X, 90.0, 2), (Axis::X, 90.0));
    }

    #[test]
    fn editor_gizmo_turn_prop_wraps_in_fifteen_degree_steps() {
        assert_eq!(turn_prop(23, 1), 0);
        assert_eq!(turn_prop(0, -1), 23);
        assert_eq!(turn_prop(5, 25), 6);
        assert_eq!(turn_prop(5, 24), 5);
    }

    #[test]
    fn editor_gizmo_turn_steps_round_to_the_nearest_step() {
        let quarter = FRAC_PI_2;
        assert_eq!(turn_steps(quarter, quarter), 1);
        assert_eq!(turn_steps(-quarter, quarter), -1);
        // 0.3 rad is just over a 15 degree step (0.2618 rad): one step
        assert_eq!(turn_steps(0.3, TAU / 24.0), 1);
        // a swing from 350 to 10 degrees is +20, not -340
        let delta = angle_between(350f32.to_radians(), 10f32.to_radians());
        assert!((delta - 20f32.to_radians()).abs() < 1e-5, "{delta}");
    }

    #[test]
    fn editor_gizmo_ray_plane_rejects_hits_beyond_pick_reach() {
        // a plane 300 blocks down is past the reach; one 150 blocks down is not
        assert_eq!(ray_plane([0.0, 300.0, 0.0], [0.0, -1.0, 0.0], 1, 0.0), None);
        assert!(ray_plane([0.0, 150.0, 0.0], [0.0, -1.0, 0.0], 1, 0.0).is_some());
        // a near-parallel ray meets the plane millions of blocks off
        assert_eq!(ray_plane([0.0, 0.0, 0.0], [1.0, 1e-6, 0.0], 1, 2.0), None);
    }

    #[test]
    fn editor_gizmo_move_is_clamped_to_sixty_four_blocks() {
        let start = portal([0.0, 0.0, 0.0], Axis::Z, [4.0, 6.0]);
        // a ray straight down at x 100 is pulled back to x 64, from the drag's start
        let moved = drag_to(Handle::Move, &start, 0.0, [100.0, 5.0, 0.0], [0.0, -1.0, 0.0]);
        assert_eq!(moved, Some(portal([64.0, 0.0, 0.0], Axis::Z, [4.0, 6.0])));
        // a ray that misses the reach entirely gives nothing
        assert_eq!(drag_to(Handle::Move, &start, 0.0, [1.3, 300.0, 0.0], [0.0, -1.0, 0.0]), None);
    }

    #[test]
    fn editor_gizmo_clamp_size_limits_a_doorway() {
        assert_eq!(clamp_size([0.2, 20.0]), Some([1.0, 16.0]));
        assert_eq!(clamp_size([8.6, 6.4]), Some([8.0, 6.0]));
        assert_eq!(clamp_size([f32::NAN, 2.0]), None);
    }

    #[test]
    fn editor_gizmo_ray_plane_hits_the_plane_in_front_only() {
        assert_eq!(ray_plane([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 1, 2.0), Some([0.0, 2.0, 0.0]));
        // parallel to the plane
        assert_eq!(ray_plane([0.0, 5.0, 0.0], [1.0, 0.0, 0.0], 1, 2.0), None);
        // the plane is behind the origin
        assert_eq!(ray_plane([0.0, 5.0, 0.0], [0.0, 1.0, 0.0], 1, 2.0), None);
    }

    #[test]
    fn editor_gizmo_drag_moves_a_doorway_to_the_half_block() {
        let start = portal([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]);
        // straight down onto the ground at (1.3, 0, 0): snapped to (1.5, 0, 0)
        let moved = drag_to(Handle::Move, &start, 0.0, [1.3, 5.0, 0.0], [0.0, -1.0, 0.0]);
        assert_eq!(moved, Some(portal([1.5, 0.0, 0.0], Axis::Z, [4.0, 6.0])));
    }

    #[test]
    fn editor_gizmo_drag_resizes_a_doorway_from_its_corner() {
        let start = portal([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]);
        // a pointer at x 3, height 2 on the doorway's plane: width 6, height 2
        let resized = drag_to(Handle::Resize, &start, 0.0, [3.0, 2.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(resized, Some(portal([0.0, 0.0, 10.0], Axis::Z, [6.0, 2.0])));
    }

    #[test]
    fn editor_gizmo_drag_turns_a_doorway_a_quarter() {
        let start = portal([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]);
        // the pointer began due east of the sill (angle 0) and swung to due south-ish (+90 deg,
        // z grows): one quarter, so the axis flips to x and the yaw is kept at 0
        let angle0 = 0.0;
        let to = [0.0, 0.0, 11.5]; // on the ring, angle pi/2
        let turned = drag_to(Handle::Turn, &start, angle0, [to[0], 5.0, to[2]], [0.0, -1.0, 0.0]);
        assert_eq!(turned, Some(portal_yaw([0.0, 0.0, 10.0], Axis::X, [4.0, 6.0], 0.0)));
    }

    #[test]
    fn editor_gizmo_grab_finds_the_handle_under_the_ray() {
        let door = portal([0.0, 0.0, 10.0], Axis::Z, [4.0, 6.0]);
        // the top right corner is (2, 6, 10): a ray straight at it grabs the knob
        assert_eq!(grab(&door, [2.1, 6.0, 0.0], [0.0, 0.0, 1.0]).map(|g| g.0), Some(Handle::Resize));
        // the sill's disc: straight down beside the middle
        assert_eq!(grab(&door, [0.1, 5.0, 10.0], [0.0, -1.0, 0.0]).map(|g| g.0), Some(Handle::Move));
        // the ring, 1.5 blocks out on the ground
        assert_eq!(grab(&door, [1.5, 5.0, 10.0], [0.0, -1.0, 0.0]).map(|g| g.0), Some(Handle::Turn));
        // away from every handle: nothing
        assert_eq!(grab(&door, [5.0, 5.0, 10.0], [0.0, -1.0, 0.0]), None);
    }

    #[test]
    fn editor_gizmo_prop_move_and_turn_drags() {
        // a prop at block (-3, 2, 4): (-30, 25, 40) in units, its ground at y 2.5 blocks
        let p = prop([-30, 25, 40], 0);
        // the ground point (-2.5, 4.2) snaps to units (-25, 42), height kept
        let moved = drag_to(Handle::Move, &p, 0.0, [-2.5, 9.0, 4.2], [0.0, -1.0, 0.0]);
        assert_eq!(moved, Some(prop([-25, 25, 42], 0)));
        // a clockwise swing (seen from above) of one ring step, 15 degrees from the start angle 0,
        // turns the prop clockwise: yaw goes down, since +yaw is counter-clockwise
        let step = TAU / 24.0;
        let ring = [-3.0 + PROP_RING * step.cos(), 2.5, 4.0 + PROP_RING * step.sin()];
        let turned = drag_to(Handle::Turn, &p, 0.0, [ring[0], 9.0, ring[2]], [0.0, -1.0, 0.0]);
        assert_eq!(turned, Some(prop([-30, 25, 40], 23)));
        // and a counter-clockwise swing turns it counter-clockwise: yaw goes up
        let ccw = [-3.0 + PROP_RING * step.cos(), 2.5, 4.0 - PROP_RING * step.sin()];
        let turned = drag_to(Handle::Turn, &p, 0.0, [ccw[0], 9.0, ccw[2]], [0.0, -1.0, 0.0]);
        assert_eq!(turned, Some(prop([-30, 25, 40], 1)));
    }

    #[test]
    fn editor_gizmo_props_have_no_resize_and_other_entities_no_handles() {
        let p = prop([0, 0, 0], 0);
        assert!(drag_to(Handle::Resize, &p, 0.0, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]).is_none());
        assert!(grab(&Component::Spawn(vox_core::entity::Spawn { at: [0.0; 3], yaw: 0.0 }), [0.0; 3], [0.0, 1.0, 0.0]).is_none());
    }
}
