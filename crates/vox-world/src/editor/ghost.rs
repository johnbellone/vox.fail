//! Ghosts in edit mode: what the Portal tool would place, drawn translucent where the pointer
//! is. The ghost sits on the top face of the block the pointer hits and faces the nearer of the
//! camera's two horizontal axes. [`vox_core::edit::check_put`] is run on a doorway to this world
//! at that place: the ghost is red, with the reason as a HUD notice, where the rules refuse it,
//! and neutral where they allow it. A click on a neutral ghost sends [`Event::Spot`] to the page,
//! which asks the player for the doorway's details and makes it; the client makes no edit itself.
//!
//! The tint is neutral, not green, on purpose: green would say the player may build here, and the
//! client does not yet know the player's role (the server tells it later). Until then a green
//! ghost would promise what only the server can grant.
//!
//! [`ghost_for`], [`tint`] and [`check_portal`] are pure and tested on their own. [`ghost`] is the
//! system that picks the block under the pointer (with [`pick::cursor_ray`]), draws the ghost and
//! sends the click.
//!
//! What the client cannot know is filled in conservatively, because the server still decides:
//! - plots and the player's plot doorways are not on the wire, so no plots are given. Only the
//!   Editor role is checked (a Builder's plot rule would refuse most doorways here), so a neutral
//!   ghost outside a builder's plot is refused by the server, and the page hears why.
//! - the account is not on the client; the Editor rule for a portal reads only whether there
//!   is one, so a placeholder number stands in for it when the client has a sign-in token.
//! - the zone's entity count is not on the client, so it is given as 0. The server counts the
//!   zone's entities and refuses a doorway in a full zone.
//! - props are not ghosts, so the prop rules are given values that refuse anything about a prop.

use std::collections::BTreeMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use vox_core::edit::{Ctx, Role};
use vox_core::entity::{Axis, Component, Dest, EntityId, EntityWhy, Portal, Prop};
use vox_core::world::{Bounds, Cell};

use super::bridge::{Event, Tool};
use super::pick::{self, FRAME_THICKNESS, Hit, PICK_REACH};
use super::{EditorOutbox, EditorState};
use crate::WorldCamera;
use crate::client::WorldClient;
use crate::entities::WorldEntities;
use crate::input::{Action, Actions, UiMode};

/// A new doorway's size, blocks (width, height): the size a Portal create asks for, and the
/// default the page's form shows.
pub const GHOST_SIZE: [f32; 2] = [4.0, 6.0];

/// Where a ghost doorway would be: its sill's middle, which way it faces, and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ghost {
    pub at: [f32; 3],
    pub axis: Axis,
    pub size: [f32; 2],
}

/// The ghost for a block the pointer hits: centred on the block, its sill on the block's top
/// face. The doorway faces the camera's nearer horizontal axis, so it is seen face on: a view
/// along z gives a doorway in a plane of constant z ([`Axis::Z`]), a view along x one of constant
/// x. `view_dir` need not be unit; a tie goes to z.
pub fn ghost_for(cell: Cell, view_dir: [f32; 3]) -> Ghost {
    let axis = if view_dir[2].abs() >= view_dir[0].abs() { Axis::Z } else { Axis::X };
    Ghost {
        at: [cell[0] as f32 + 0.5, cell[1] as f32 + 1.0, cell[2] as f32 + 0.5],
        axis,
        size: GHOST_SIZE,
    }
}

/// How the ghost is drawn: neutral where the rules allow the doorway (see the module doc on why
/// it is not green), red with the reason where they don't.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tint {
    Neutral,
    Red(EntityWhy),
}

/// The tint for a rules check (`check_portal`'s answer).
pub fn tint(check: Result<(), EntityWhy>) -> Tint {
    match check {
        Ok(()) => Tint::Neutral,
        Err(why) => Tint::Red(why),
    }
}

/// What the client knows of the world, for the rules: the player's feet, whether it has a
/// sign-in (a guest may edit nothing), the world's blocks and entities, and whether a cell is
/// solid.
pub struct Known<'a> {
    pub at: Option<[f32; 3]>,
    pub signed_in: bool,
    pub bounds: Bounds,
    pub entities: &'a BTreeMap<EntityId, (u64, Component)>,
    pub solid: &'a dyn Fn(Cell) -> bool,
}

/// Would `vox_core::edit::check_put` allow this new portal? The rules are the server's, run on
/// what the client knows (see the module doc for each value given conservatively). The Editor
/// role is used: the plot rule that a Builder would also face is the server's to apply.
pub fn check_portal(known: &Known, portal: &Component) -> Result<(), EntityWhy> {
    let spawns = known.entities.values().filter(|(_, c)| matches!(c, Component::Spawn(_))).count();
    // a placeholder account: the Editor rule for a new portal reads only that there is one
    let account = known.signed_in.then_some(0);
    let no_prop = |_: &Prop| false;
    let no_prop_kind = |_: &[u8; 16]| false;
    let ctx = Ctx {
        role: Role::Editor,
        account,
        at: known.at,
        bounds: known.bounds,
        plots: &[],
        zone_entities: 0,
        moves_zone: false,
        plot_portals: 0,
        spawns,
        ground: known.solid,
        props_in_plot: 0,
        prop_footprint_free: &no_prop,
        known_prop: &no_prop_kind,
    };
    vox_core::edit::check_put(&ctx, None, portal)
}

/// The doorway a ghost stands for, as the rules are checked on it: in this world's level, with
/// no key (the directory gives one to a doorway it charges for), leading to another world on
/// this host. The world's own id stands in for that destination, a valid world id by
/// construction. Only the check uses it; the page makes the real doorway.
fn doorway(ghost: &Ghost, world: &str) -> Component {
    Component::Portal(Portal {
        name: "portal".into(),
        at: ghost.at,
        axis: ghost.axis,
        size: ghost.size,
        yaw: 0.0,
        level: world.to_string(),
        to: Dest::World { world: world.to_string() },
        key: None,
    })
}

/// The ghost's materials and its entity, made the first time they are needed and again if the
/// entity has gone.
#[derive(Default)]
pub struct Look {
    ghost: Option<Entity>,
    neutral: Option<Handle<StandardMaterial>>,
    red: Option<Handle<StandardMaterial>>,
}

/// Draws the Portal tool's ghost and sends a click on a neutral ghost to the page as a
/// [`Event::Spot`]. Runs after the controls (so the click that captures the mouse is known) and
/// after the editor's own systems. It follows the same rules as `pick::select` for when a click
/// counts: not while a menu has the mouse, not while the pointer is captured by the view, and
/// only once the world is ready.
#[allow(clippy::too_many_arguments)]
pub fn ghost(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    mut w: ResMut<WorldClient>,
    entities: Res<WorldEntities>,
    state: Res<EditorState>,
    mut outbox: ResMut<EditorOutbox>,
    cams: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut look: Local<Look>,
    mut shown: Local<Option<EntityWhy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut ghosts: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
) {
    if look.ghost.is_none_or(|e| !ghosts.contains(e)) {
        let translucent = |c: Color| StandardMaterial {
            base_color: c,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        };
        let neutral = mats.add(translucent(Color::srgba(0.8, 0.9, 1.0, 0.4)));
        let red = mats.add(translucent(Color::srgba(0.95, 0.25, 0.2, 0.4)));
        let mesh = meshes.add(Cuboid::new(GHOST_SIZE[0], GHOST_SIZE[1], FRAME_THICKNESS));
        look.ghost = Some(
            commands
                .spawn((Mesh3d(mesh), MeshMaterial3d(neutral.clone()), Transform::default(), Visibility::Hidden, NotShadowCaster))
                .id(),
        );
        look.neutral = Some(neutral);
        look.red = Some(red);
        // the entity exists from the next frame; nothing to draw until then
        return;
    }
    let (Some(id), Some(neutral), Some(red)) = (look.ghost, look.neutral.clone(), look.red.clone()) else { return };
    let Ok((mut transform, mut material, mut visible)) = ghosts.get_mut(id) else { return };

    // hidden unless the Portal tool holds a block under the pointer; the reason shown is kept
    // while it stays the same, and forgotten once there is no ghost
    *visible = Visibility::Hidden;
    if state.tool != Some(Tool::Portal) || !w.active() || !w.ready || *mode == UiMode::Menu {
        *shown = None;
        return;
    }
    let (Ok(win), Ok((cam, gt))) = (windows.single(), cams.single()) else {
        *shown = None;
        return;
    };
    let Some((origin, dir)) = pick::cursor_ray(&w, win, cam, gt) else {
        *shown = None;
        return;
    };
    let size = w.block_size();
    // the same solid test as pick::select and client::controls
    let solid = pick::solid(&w);
    // blocks only: the ghost stands on the block the pointer is on
    let Some(Hit::Block { cell, .. }) = pick::pick(origin, dir, PICK_REACH, &solid, &[], &[]) else {
        *shown = None;
        return;
    };
    let g = ghost_for(cell, dir);
    let frame = pick::frame_box(g.at, g.axis, g.size);
    let centre = [0, 1, 2].map(|i| (frame.min[i] + frame.max[i]) * 0.5);
    transform.translation = Vec3::from(centre) * size;
    transform.scale = Vec3::splat(size);
    transform.rotation = match g.axis {
        Axis::Z => Quat::IDENTITY,
        Axis::X => Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    };

    let known = Known {
        at: Some(w.body.pos),
        signed_in: w.token.is_some(),
        bounds: w.view.confirmed.bounds,
        entities: &entities.map,
        solid: &solid,
    };
    let verdict = tint(check_portal(&known, &doorway(&g, &w.world)));
    // the solid test borrows the world, and the reason below is a notice on it
    drop(solid);
    *visible = Visibility::Visible;
    match verdict {
        Tint::Neutral => {
            *material = MeshMaterial3d(neutral);
            *shown = None;
            if !state.pointer_claimed && actions.just_pressed(Action::Break) {
                outbox.0.push(Event::Spot { at: g.at, axis: g.axis, size: g.size });
            }
        }
        Tint::Red(why) => {
            *material = MeshMaterial3d(red);
            // the reason is shown when it changes, not every frame
            if *shown != Some(why) {
                *shown = Some(why);
                w.notice(why.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_ghost_sits_on_the_top_face_of_the_picked_block() {
        let g = ghost_for([2, -1, 7], [0.0, 0.0, 1.0]);
        assert_eq!(g.at, [2.5, 0.0, 7.5]);
        assert_eq!(g.size, GHOST_SIZE);
    }

    #[test]
    fn editor_ghost_faces_the_nearer_horizontal_axis() {
        // looking along z: a doorway in a plane of constant z, seen face on
        assert_eq!(ghost_for([0, 0, 0], [0.3, -0.5, 0.9]).axis, Axis::Z);
        // looking along x: the other way round
        assert_eq!(ghost_for([0, 0, 0], [0.9, 0.0, -0.2]).axis, Axis::X);
        // a tie goes to z
        assert_eq!(ghost_for([0, 0, 0], [0.7, 0.0, 0.7]).axis, Axis::Z);
    }

    #[test]
    fn editor_ghost_tint_follows_the_rules_check() {
        assert_eq!(tint(Ok(())), Tint::Neutral);
        assert_eq!(tint(Err(EntityWhy::TooFar)), Tint::Red(EntityWhy::TooFar));
    }

    #[test]
    fn editor_ghost_check_allows_a_doorway_in_reach_and_refuses_a_guest() {
        let empty = BTreeMap::new();
        let no_blocks = |_: Cell| false;
        let portal = doorway(&ghost_for([0, 0, 0], [0.0, 0.0, 1.0]), "yard");
        let known = |signed_in| Known {
            at: Some([0.5, 0.0, -2.0]),
            signed_in,
            bounds: Bounds::default(),
            entities: &empty,
            solid: &no_blocks,
        };
        assert_eq!(check_portal(&known(true), &portal), Ok(()));
        // a guest may edit nothing
        assert_eq!(check_portal(&known(false), &portal), Err(EntityWhy::NotYours));
        // out of reach: the doorway is 20 blocks off from the feet
        let far = doorway(&ghost_for([0, 0, 18], [0.0, 0.0, 1.0]), "yard");
        assert_eq!(check_portal(&known(true), &far), Err(EntityWhy::TooFar));
    }

    #[test]
    fn editor_ghost_outside_the_world_is_refused() {
        let empty = BTreeMap::new();
        let no_blocks = |_: Cell| false;
        let known = Known {
            at: Some([300.0, 0.0, 0.0]),
            signed_in: true,
            bounds: Bounds::default(),
            entities: &empty,
            solid: &no_blocks,
        };
        // the default bounds end at 256 blocks along x, so a doorway at x = 300 is outside
        let out = doorway(&ghost_for([300, 0, 0], [0.0, 0.0, 1.0]), "yard");
        assert_eq!(check_portal(&known, &out), Err(EntityWhy::Outside));
        assert_eq!(tint(check_portal(&known, &out)), Tint::Red(EntityWhy::Outside));
    }

    #[test]
    fn editor_ghost_doorway_leads_to_another_world_here() {
        let g = ghost_for([4, 2, -3], [1.0, 0.0, 0.0]);
        match doorway(&g, "yard") {
            Component::Portal(p) => {
                assert_eq!(p.name, "portal");
                assert_eq!(p.at, [4.5, 3.0, -2.5]);
                assert_eq!(p.axis, Axis::X);
                assert_eq!(p.size, GHOST_SIZE);
                assert_eq!(p.yaw, 0.0);
                assert_eq!(p.level, "yard");
                assert_eq!(p.to, Dest::World { world: "yard".into() });
                assert_eq!(p.key, None);
            }
            other => panic!("expected a portal, got {other:?}"),
        }
    }
}
