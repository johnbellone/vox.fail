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
//!   is one, so a placeholder number stands in for it for everyone, guests included (see
//!   [`Known`]). Only the server knows whether a world lets guests edit (`--editors-all`).
//! - the zone's entity count is not on the client, so it is given as 0. The server counts the
//!   zone's entities and refuses a doorway in a full zone.
//!
//! The Prop tool's ghost is the chosen prop's own mesh, read from its blob as the client shows
//! props, snapped to 5 cm on the top of the block under the pointer ([`prop_ghost`]). The same
//! rules run on it ([`check_prop`]), with its footprint checked against the solid blocks and the
//! props already here. A click on a neutral one sends [`Event::PropSpot`]. Props are given no turn
//! (yaw 0): nothing in the editor turns a prop while it is placed; the gizmo turns it afterwards.
//! The server still says NoSuchProp for a hash it does not accept, since the catalogue is not on
//! the client.

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use vox_core::body::{Occupancy, prop_footprint_cells, with_props};
use vox_art::prop::Surface;
use vox_core::edit::{Around, Ctx, Role, Support, prop_stands};
use vox_core::entity::{Axis, Component, Dest, EntityId, EntityWhy, PROP_UNITS_PER_BLOCK, PROP_YAWS, Portal, Prop};
use vox_core::world::{Bounds, Cell};
use vox_format::Hash;
use vox_format::prop::PropBlob;

use super::bridge::{Event, Tool};
use super::gizmo::snap_prop_move;
use super::pick::{self, FRAME_THICKNESS, Hit, PICK_REACH};
use super::{EditorOutbox, EditorState};
use crate::WorldCamera;
use crate::client::WorldClient;
use crate::entities::WorldEntities;
use crate::files;
use crate::props::{PropOccupancy, mesh_parts};
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

/// What the client knows of the world, for the rules: the player's feet, the world's blocks and
/// entities, and whether a cell is solid. Who may edit is not here: a guest is not refused on it
/// either, since a world may let guests edit and only the server knows which (see
/// [`check_portal`]).
pub struct Known<'a> {
    pub at: Option<[f32; 3]>,
    pub bounds: Bounds,
    pub entities: &'a BTreeMap<EntityId, (u64, Component)>,
    pub solid: &'a dyn Fn(Cell) -> bool,
}

/// Would `vox_core::edit::check_put` allow this new portal? The rules are the server's, run on
/// what the client knows (see the module doc for each value given conservatively). The Editor
/// role is used: the plot rule that a Builder would also face is the server's to apply.
pub fn check_portal(known: &Known, portal: &Component) -> Result<(), EntityWhy> {
    let spawns = known.entities.values().filter(|(_, c)| matches!(c, Component::Spawn(_))).count();
    // a placeholder account for everyone: the Editor rule for a new portal reads only that there
    // is one. A guest gets it too, so the rule does not refuse on rights the client cannot see:
    // a world started with --editors-all lets guests edit, and the server decides.
    let account = Some(0);
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
        // a portal has no footprint cells of a prop's
        prop_cells: &|_: &Prop| Vec::new(),
        known_prop: &no_prop_kind,
        prop_supported: &no_prop,
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

/// A translucent, unlit material: how both ghosts are drawn.
fn translucent(c: Color) -> StandardMaterial {
    StandardMaterial { base_color: c, alpha_mode: AlphaMode::Blend, unlit: true, cull_mode: None, ..default() }
}

/// One 5 cm prop unit in metres: the same scale `props.rs` shows a prop at, so the ghost sits
/// where the prop will.
const UNIT_M: f32 = 0.05;

/// What the Prop tool's ghost shows: the prop's mesh, and the footprint and height its rules
/// need (read from its blob).
#[derive(Clone, Debug, PartialEq)]
struct PropShape {
    mesh: Handle<Mesh>,
    footprint: Vec<[i16; 2]>,
    height: u8,
    /// the surfaces the def lets it stand on
    on: Vec<Support>,
}

/// Where a prop's blob stands for the ghost. Pending: not here yet. Failed: the fetch found
/// nothing (an empty file) or the bytes do not decode; asking again would get the same, so the
/// blob is never asked for again. Ready: read and meshed.
#[derive(Clone, Debug, PartialEq)]
enum Blob<T> {
    Pending,
    Failed,
    Ready(T),
}

/// The blob's state for one frame, given the bytes the file store has for it (None while it is
/// not here yet). Pure, so the rules for each state are tested without Bevy.
fn read_blob<T>(bytes: Option<&[u8]>, decode: impl FnOnce(&[u8]) -> Option<T>) -> Blob<T> {
    match bytes {
        None => Blob::Pending,
        Some([]) => Blob::Failed,
        Some(b) => decode(b).map_or(Blob::Failed, Blob::Ready),
    }
}

/// The Prop tool's ghost entity and what it shows.
#[derive(Default)]
pub struct PropLook {
    ghost: Option<Entity>,
    neutral: Option<Handle<StandardMaterial>>,
    red: Option<Handle<StandardMaterial>>,
    /// the prop the ghost is for, and where its blob stands
    prop: Option<([u8; 16], Blob<PropShape>)>,
    /// the mesh the ghost entity wears now
    worn: Option<Handle<Mesh>>,
    /// the reason shown as a notice, kept while it stays the same
    why: Option<EntityWhy>,
}

/// A prop's blob as the ghost needs it.
struct Decoded {
    mesh: Mesh,
    footprint: Vec<[i16; 2]>,
    height: u8,
    on: Vec<Support>,
}

/// Decodes a prop's blob (`props.rs` keeps it at [`crate::props::blob_path`]) into its mesh,
/// footprint and height. None when it does not decode.
fn decode_prop(bytes: &[u8], hash: [u8; 16]) -> Option<Decoded> {
    let blob = PropBlob::decode_named(bytes, &Hash(hash)).ok()?;
    let voxel = blob.def.scale.scale().voxel_size();
    let parts = mesh_parts(&blob.grid, blob.def.pivot, voxel).ok()?;
    let on = blob
        .def
        .place
        .on
        .iter()
        .map(|s| match s {
            Surface::Floor => Support::Floor,
            Surface::Wall => Support::Wall,
            Surface::Ceiling => Support::Ceiling,
            Surface::Table => Support::Table,
            Surface::Water => Support::Water,
        })
        .collect();
    Some(Decoded { mesh: parts.into_mesh(), footprint: blob.def.place.footprint.clone(), height: blob.def.place.height, on })
}

/// The shape of the prop the ghost shows, brought to `hash` this frame. A prop not shown before
/// has its blob asked for once (in the browser the client fetches it then) and the mesh of the
/// one it replaces let go. Once the blob is read it is meshed once; once it has failed it is not
/// asked for again. None while there is no shape to show.
fn prop_shape(look: &mut PropLook, hash: [u8; 16], meshes: &mut Assets<Mesh>) -> Option<PropShape> {
    if look.prop.as_ref().is_none_or(|(h, _)| *h != hash) {
        if let Some((_, Blob::Ready(old))) = look.prop.take() {
            meshes.remove(old.mesh.id());
            // the ghost wore that mesh: forget it, so the next one is put on afresh
            look.worn = None;
        }
        files::want(&crate::props::blob_path(hash));
        look.prop = Some((hash, Blob::Pending));
    }
    let (_, slot) = look.prop.as_mut()?;
    if matches!(slot, Blob::Pending) {
        let bytes = files::read(&crate::props::blob_path(hash));
        // on the desktop a file is there or it is not (nothing is on its way), so a missing
        // blob has failed now rather than being read again every frame
        let bytes = if cfg!(target_arch = "wasm32") { bytes } else { Some(bytes.unwrap_or_default()) };
        *slot = read_blob(bytes.as_deref(), |b| {
            let d = decode_prop(b, hash)?;
            Some(PropShape { mesh: meshes.add(d.mesh), footprint: d.footprint, height: d.height, on: d.on })
        });
        if matches!(slot, Blob::Failed) {
            crate::note!("prop {} could not be read: its ghost is not shown", Hash(hash).hex());
        }
    }
    match slot {
        Blob::Ready(shape) => Some(shape.clone()),
        _ => None,
    }
}

/// Where a prop's `at` is for a pointer hit on the top of `cell`: the hit's point on the ground
/// (`ground`, x and z in blocks) snapped to 5 cm, and kept inside `cell` in x and z. Without the
/// clamp a hit on the +x or +z side face rounds to the next block's edge, which puts the prop over
/// a cell it does not stand on (see `prop_cell`). `top` is the top of the block in units.
fn prop_spot(cell: Cell, ground: [f32; 2], top: i32) -> Option<[i32; 3]> {
    let [x, _, z] = snap_prop_move(ground, top)?;
    Some([within(x, cell[0]), top, within(z, cell[2])])
}

/// `unit` moved to the nearest unit inside block `block` along one axis.
fn within(unit: i32, block: i32) -> i32 {
    let lo = block.saturating_mul(PROP_UNITS_PER_BLOCK);
    let hi = lo.saturating_add(PROP_UNITS_PER_BLOCK - 1);
    unit.clamp(lo, hi)
}

/// Would `vox_core::edit::check_put` allow this new prop? The page's catalogue choice is
/// `prop.prop`. The owner is stamped with the same placeholder account as [`check_portal`] uses,
/// so the owner rule passes and the server, which stamps the real owner, decides; `footprint_free`
/// says whether the prop's cells are clear and `supported` whether it has a surface to stand on
/// (the server's own rule, `vox_core::edit::prop_stands`, run on what the client knows).
pub fn check_prop(
    known: &Known,
    prop: &Prop,
    cells: &dyn Fn(&Prop) -> Vec<Cell>,
    footprint_free: &dyn Fn(&Prop) -> bool,
    supported: &dyn Fn(&Prop) -> bool,
) -> Result<(), EntityWhy> {
    let spawns = known.entities.values().filter(|(_, c)| matches!(c, Component::Spawn(_))).count();
    let account = Some(0);
    let stamped = Prop { owner: account, ..prop.clone() };
    // the client has the prop's blob, so the server's catalogue is the only other word on it
    let known_prop = |_: &[u8; 16]| true;
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
        prop_footprint_free: footprint_free,
        // every footprint cell must be in the world (and, for a builder, the server checks the plot)
        prop_cells: cells,
        known_prop: &known_prop,
        prop_supported: supported,
    };
    vox_core::edit::check_put(&ctx, None, &Component::Prop(stamped))
}

/// The Prop tool's ghost: the chosen prop's own mesh on the top of the block under the pointer,
/// snapped to 5 cm, tinted with its rules, and a click on a neutral one sent to the page. Hidden
/// whenever the Prop tool is not holding a prop under the pointer. It follows the same rules as
/// the portal ghost for when a click counts.
#[allow(clippy::too_many_arguments)]
fn prop_ghost(
    look: &mut PropLook,
    actions: &Actions,
    mode: &UiMode,
    state: &EditorState,
    w: &mut WorldClient,
    entities: &WorldEntities,
    occupancy: Option<&PropOccupancy>,
    outbox: &mut EditorOutbox,
    cams: &Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    windows: &Query<&Window, With<PrimaryWindow>>,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    commands: &mut Commands,
    ghosts: &mut Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
) {
    if look.ghost.is_none_or(|e| !ghosts.contains(e)) {
        let neutral = mats.add(translucent(Color::srgba(0.8, 0.9, 1.0, 0.4)));
        look.red = Some(mats.add(translucent(Color::srgba(0.95, 0.25, 0.2, 0.4))));
        look.neutral = Some(neutral.clone());
        // nothing to draw until a prop's mesh is read; the entity needs a mesh to start with
        let empty = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD));
        look.ghost = Some(
            commands
                .spawn((Mesh3d(empty), MeshMaterial3d(neutral), Transform::default(), Visibility::Hidden, NotShadowCaster))
                .id(),
        );
        // the new entity wears the empty mesh, so the prop's mesh is put on again
        look.worn = None;
        return;
    }
    let (Some(id), Some(neutral), Some(red)) = (look.ghost, look.neutral.clone(), look.red.clone()) else { return };
    let Ok((mut transform, mut material, mut visible)) = ghosts.get_mut(id) else { return };

    *visible = Visibility::Hidden;
    let hidden = |look: &mut PropLook| look.why = None;
    if state.tool != Some(Tool::Prop) {
        hidden(look);
        return;
    }
    let Some(hash) = state.prop else { return hidden(look) };
    if !w.active() || !w.ready || *mode == UiMode::Menu {
        return hidden(look);
    }
    // until the blob is read there is no ghost; if it cannot be read there never is
    let Some(shape) = prop_shape(look, hash, meshes) else { return hidden(look) };
    if look.worn.as_ref() != Some(&shape.mesh) {
        commands.entity(id).insert(Mesh3d(shape.mesh.clone()));
        look.worn = Some(shape.mesh.clone());
    }

    let (Ok(win), Ok((cam, gt))) = (windows.single(), cams.single()) else { return hidden(look) };
    let Some((origin, dir)) = pick::cursor_ray(w, win, cam, gt) else { return hidden(look) };
    let solid = pick::solid(w);
    let Some(Hit::Block { cell, .. }) = pick::pick(origin, dir, PICK_REACH, &solid, &[], &[]) else {
        return hidden(look);
    };
    // the point the pointer is on in the block it hits, moved up to the block's top: the prop
    // stands on the block, snapped to 5 cm across it
    let len = dir.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !(len.is_finite() && len > 1e-9) {
        return hidden(look);
    }
    let d = dir.map(|v| v / len);
    let block = pick::Aabb { min: cell.map(|c| c as f32), max: cell.map(|c| c as f32 + 1.0) };
    let Some(t) = pick::ray_box(origin, d, &block, PICK_REACH) else { return hidden(look) };
    let p = [0, 1, 2].map(|i| origin[i] + d[i] * t);
    let top = cell[1].saturating_add(1).saturating_mul(PROP_UNITS_PER_BLOCK);
    let Some(at) = prop_spot(cell, [p[0], p[2]], top) else { return hidden(look) };
    let yaw = 0;
    let prop = Prop { prop: hash, at, yaw, variant: 0, state: 0, channel: None, text: None, owner: None };

    // the footprint is clear when none of its cells is a solid block or under another solid prop;
    // the rules are checked in a scope of their own, so the borrows of the world end before the
    // notice below
    let verdict = {
        let empty = Occupancy::default();
        let props = occupancy.map_or(&empty, |o| &o.0);
        let blocked = with_props(&solid, props);
        let cells = |p: &Prop| prop_footprint_cells(p.at, p.yaw, &shape.footprint, shape.height);
        let footprint_free = |p: &Prop| cells(p).into_iter().all(|c| !blocked(c));
        let known = Known {
            at: Some(w.body.pos),
            bounds: w.view.confirmed.bounds,
            entities: &entities.map,
            solid: &solid,
        };
        // what holds a prop up here: what the client walks on (not water), water by its block's
        // flag, and for a table any prop, since the client does not know the server's tags (the
        // server decides)
        let water = |c: Cell| w.is_liquid(w.view.get(c));
        let table = |c: Cell| props.contains(c);
        let around = Around { solid: &|c| w.stands_on(c), water: &water, table: &table };
        let supported = |p: &Prop| prop_stands(&shape.on, p.at, p.yaw, &shape.footprint, shape.height, &around);
        tint(check_prop(&known, &prop, &cells, &footprint_free, &supported))
    };
    // the world's solid test borrows the client, and the notice below needs it
    drop(solid);

    transform.translation = Vec3::from(at.map(|u| u as f32)) * UNIT_M;
    transform.rotation = Quat::from_rotation_y(f32::from(yaw) * TAU / f32::from(PROP_YAWS));
    transform.scale = Vec3::ONE;
    *visible = Visibility::Visible;
    match verdict {
        Tint::Neutral => {
            *material = MeshMaterial3d(neutral);
            look.why = None;
            if !state.pointer_claimed && actions.just_pressed(Action::Break) {
                outbox.0.push(Event::PropSpot { prop: hash, at, yaw });
            }
        }
        Tint::Red(why) => {
            *material = MeshMaterial3d(red);
            if look.why != Some(why) {
                look.why = Some(why);
                w.notice(why.to_string());
            }
        }
    }
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
    mut prop_look: Local<PropLook>,
    occupancy: Option<Res<PropOccupancy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut ghosts: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
) {
    // the Prop tool's ghost runs first; it hides itself unless that tool is holding a prop
    prop_ghost(
        &mut prop_look,
        &actions,
        &mode,
        &state,
        &mut w,
        &entities,
        occupancy.as_deref(),
        &mut outbox,
        &cams,
        &windows,
        &mut meshes,
        &mut mats,
        &mut commands,
        &mut ghosts,
    );
    if look.ghost.is_none_or(|e| !ghosts.contains(e)) {
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
    fn editor_ghost_check_allows_a_doorway_in_reach_and_leaves_a_guest_to_the_server() {
        let empty = BTreeMap::new();
        let no_blocks = |_: Cell| false;
        let portal = doorway(&ghost_for([0, 0, 0], [0.0, 0.0, 1.0]), "yard");
        let known = Known {
            at: Some([0.5, 0.0, -2.0]),
            bounds: Bounds::default(),
            entities: &empty,
            solid: &no_blocks,
        };
        // a guest is not refused here: a world started with --editors-all lets guests edit, and
        // the client cannot tell that world from the others, so the server decides
        assert_eq!(check_portal(&known, &portal), Ok(()));
        // out of reach: the doorway is 20 blocks off from the feet
        let far = doorway(&ghost_for([0, 0, 18], [0.0, 0.0, 1.0]), "yard");
        assert_eq!(check_portal(&known, &far), Err(EntityWhy::TooFar));
    }

    #[test]
    fn editor_ghost_outside_the_world_is_refused() {
        let empty = BTreeMap::new();
        let no_blocks = |_: Cell| false;
        let known = Known {
            at: Some([300.0, 0.0, 0.0]),
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

    /// A prop at `at` (5 cm units) with no owner yet, as the ghost makes it.
    fn ghost_prop(at: [i32; 3]) -> Prop {
        Prop { prop: [7; 16], at, yaw: 0, variant: 0, state: 0, channel: None, text: None, owner: None }
    }

    #[test]
    fn editor_prop_ghost_allows_a_prop_in_reach_and_leaves_a_guest_to_the_server() {
        let empty = BTreeMap::new();
        let no_blocks = |_: Cell| false;
        let free = |_: &Prop| true;
        let pivot = |p: &Prop| prop_footprint_cells(p.at, p.yaw, &[[0, 0]], 1);
        let held = |_: &Prop| true;
        // the feet are at (0.5, 0, -2) blocks, and the prop at [5, 0, -20] units (5 cm each) is there too
        let known = Known {
            at: Some([0.5, 0.0, -2.0]),
            bounds: Bounds::default(),
            entities: &empty,
            solid: &no_blocks,
        };
        assert_eq!(check_prop(&known, &ghost_prop([5, 0, -20]), &pivot, &free, &held), Ok(()));
        // out of reach: 20 blocks from the feet
        assert_eq!(check_prop(&known, &ghost_prop([5, 0, 180]), &pivot, &free, &held), Err(EntityWhy::TooFar));
    }

    #[test]
    fn editor_prop_ghost_is_refused_where_its_footprint_is_not_clear() {
        let empty = BTreeMap::new();
        let no_blocks = |_: Cell| false;
        let known = Known {
            at: Some([0.5, 0.0, -2.0]),
            bounds: Bounds::default(),
            entities: &empty,
            solid: &no_blocks,
        };
        let blocked = |_: &Prop| false;
        let pivot = |p: &Prop| prop_footprint_cells(p.at, p.yaw, &[[0, 0]], 1);
        let held = |_: &Prop| true;
        let verdict = check_prop(&known, &ghost_prop([5, 0, -20]), &pivot, &blocked, &held);
        assert_eq!(verdict, Err(EntityWhy::Blocked));
        assert_eq!(tint(verdict), Tint::Red(EntityWhy::Blocked));
    }

    #[test]
    fn editor_prop_ghost_is_red_with_nothing_to_stand_on() {
        let empty = BTreeMap::new();
        // grass at y = -1 only
        let ground = |c: Cell| c[1] == -1;
        let known = Known { at: Some([0.5, 0.0, -2.0]), bounds: Bounds::default(), entities: &empty, solid: &ground };
        let pivot = |p: &Prop| prop_footprint_cells(p.at, p.yaw, &[[0, 0]], 1);
        let free = |_: &Prop| true;
        let none = |_: Cell| false;
        let around = Around { solid: &ground, water: &none, table: &none };
        // the ghost's own closure, as `prop_ghost` makes it, for a floor prop and a wall prop
        let on_floor = |p: &Prop| prop_stands(&[Support::Floor], p.at, p.yaw, &[[0, 0]], 1, &around);
        let on_wall = |p: &Prop| prop_stands(&[Support::Wall], p.at, p.yaw, &[[0, 0]], 1, &around);
        assert_eq!(check_prop(&known, &ghost_prop([5, 0, -20]), &pivot, &free, &on_floor), Ok(()));
        // over air: a floor of 1 block lower is not under it
        let verdict = check_prop(&known, &ghost_prop([5, 10, -20]), &pivot, &free, &on_floor);
        assert_eq!(verdict, Err(EntityWhy::NoSupport));
        assert_eq!(tint(verdict), Tint::Red(EntityWhy::NoSupport));
        // no wall behind it
        assert_eq!(check_prop(&known, &ghost_prop([5, 0, -20]), &pivot, &free, &on_wall), Err(EntityWhy::NoSupport));
    }

    #[test]
    fn editor_prop_spot_snaps_to_5cm_inside_the_hit_cell() {
        // 2.36 blocks is 23.6 units: 24 units, in cell 2; 7.02 blocks in cell 7 is 70 units
        assert_eq!(prop_spot([2, -1, 7], [2.36, 7.02], 0), Some([24, 0, 70]));
        // negative: -2.96 blocks is -29.6 units, which rounds to -30 in cell -3; -0.04 in cell -1
        // rounds to 0 units, which is cell 0, so it is clamped to the cell's last unit, -1
        assert_eq!(prop_spot([-3, 0, -1], [-2.96, -0.04], 10), Some([-30, 10, -1]));
        assert_eq!(prop_spot([0, 0, 0], [0.0, 0.0], 10), Some([0, 10, 0]));
        // no finite ground, no spot
        assert_eq!(prop_spot([0, 0, 0], [f32::NAN, 0.0], 10), None);
    }

    #[test]
    fn editor_prop_spot_clamps_a_side_face_hit_into_its_own_cell() {
        // a hit on the +x face of cell 4 is at exactly 5.0 blocks, which would round to unit 50,
        // the first unit of cell 5; the same for z
        assert_eq!(prop_spot([4, 0, 4], [5.0, 5.0], 10), Some([49, 10, 49]));
        // and on the -x face of cell 4 it stays at 4.0 blocks, unit 40
        assert_eq!(prop_spot([4, 0, 4], [4.0, 4.0], 10), Some([40, 10, 40]));
    }

    #[test]
    fn editor_prop_spot_is_over_the_cell_it_stands_on() {
        use vox_core::entity::prop_cell;
        // the at a hit gives must sit in the cell hit, one block up (on its top), as the rules
        // and the footprint read it: prop_cell is where the prop's footprint starts
        for (cell, ground) in [([4, 0, 4], [5.0, 4.3]), ([-3, 0, -1], [-3.0, -0.04]), ([0, 0, 0], [0.0, 0.0])] {
            let top = (cell[1] + 1) * PROP_UNITS_PER_BLOCK;
            let at = prop_spot(cell, ground, top).expect("a finite spot");
            let floor = [cell[0], cell[1] + 1, cell[2]];
            assert_eq!(prop_cell(at), floor, "at {at:?} for cell {cell:?}");
            assert_eq!(prop_footprint_cells(at, 0, &[[0, 0]], 1), vec![floor]);
        }
    }

    #[test]
    fn editor_prop_blob_is_pending_until_it_arrives_then_fails_or_reads() {
        let decode = |b: &[u8]| (b.len() == 2).then_some(b.len());
        // not here yet: nothing decoded, and nothing to ask for again
        assert_eq!(read_blob(None, decode), Blob::Pending);
        // an empty file is a fetch that found nothing: failed, not retried
        let empty: &[u8] = &[];
        assert_eq!(read_blob(Some(empty), decode), Blob::Failed);
        // bytes that do not decode are a failure too
        assert_eq!(read_blob(Some(&[1, 2, 3]), decode), Blob::Failed);
        // bytes that decode are ready
        assert_eq!(read_blob(Some(&[1, 2]), decode), Blob::Ready(2));
    }
}
