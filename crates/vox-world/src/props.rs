//! Props on the client: a prop entity's blob is fetched by its hash (`data/props/<hash>.voxp`),
//! decoded and turned into one flat-shaded mesh per hash, and each prop entity is shown at its
//! place and turn. A prop that collides also puts its cells into [`PropOccupancy`], which walking
//! reads beside the blocks (`vox_core::body::with_props`).
//!
//! Blobs that arrive wait in a queue and are decoded and meshed within a frame budget, so a room
//! full of new props fills in over a few frames instead of stalling the game. Meshes use Bevy's
//! standard material with per-vertex colours. Coplanar faces of the same colour are merged into
//! one quad per slice (greedy meshing), and faces that touch another filled voxel are left out.
//! A prop whose mesh would have too many quads is refused. Static props are not merged per chunk
//! yet, and colours come from a fixed fallback ramp until the palette file exists (vox-tol.4.14).
//!
//! The behaviours that look (`light` and `motion`, `docs/design/props.md`) are read here too. A
//! prop's lights are handed to `outdoors` to join the nearest lamps, and its motions move the
//! whole mesh each frame from the clock. Named parts are not moved on their own yet. Motion and
//! flicker read each client's own clock (`Time<Real>`) with a per-entity phase, so different
//! clients may show different points of the same cycle. This is looks only: collision, the
//! server and the choice of lamps all use the resting place, so nothing depends on it.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    pbr::StandardMaterial,
    platform::time::Instant,
    prelude::*,
    time::{Real, Time},
};
use vox_art::{
    VoxelGrid,
    prop::{Behaviour, Collide, MotionKind, PropDef, When},
};
use vox_core::body::Occupancy;
use vox_core::entity::{Component, EntityId, PROP_YAWS, Prop};
use vox_format::Hash;
use vox_format::prop::PropBlob;

use crate::client::WorldClient;
use crate::entities::WorldEntities;
use crate::files;
use crate::outdoors::{FIRE_RGB, LAMP_RGB};

/// One prop unit (5 cm, see `vox_core::entity::PROP_UNITS_PER_BLOCK`) in metres.
const UNIT_M: f32 = 0.05;
/// Fetches that may be waiting at once.
const IN_FLIGHT: usize = 4;
/// Seconds a failed or unanswered fetch waits before it is asked for again.
const RETRY_SECS: f64 = 10.0;
/// Failed fetches before a prop's blob is given up on.
const MAX_TRIES: u8 = 3;
/// Most meshes kept at once; the least recently shown are dropped beyond this.
const MAX_MESHES: usize = 256;
/// Most quads one prop's mesh may have. A prop with more is refused (its blob is bad or too busy).
const MAX_QUADS: usize = 50_000;
/// How long decoding and meshing may take each frame. At least one blob goes each frame. (Same
/// budget as the chunks, see `client::chunks`.)
#[cfg(not(target_arch = "wasm32"))]
const BUILD_BUDGET: Duration = Duration::from_millis(5);
#[cfg(target_arch = "wasm32")]
const BUILD_BUDGET: Duration = Duration::from_millis(3);

/// Most props spawned in one frame. The rest wait in the spawn queue for later frames, so a room
/// of new props does not cost one frame a long stall.
const SPAWN_CAP: usize = 64;

/// Colours for voxels, by palette index, until the project palette file exists (vox-tol.4.14
/// will read the palette the blob's `palette_version` names). Index 0 is empty and never stored.
const FALLBACK_RAMP: [[u8; 3]; 8] = [
    [196, 160, 120],
    [150, 110, 80],
    [90, 140, 80],
    [80, 110, 160],
    [190, 80, 70],
    [220, 200, 90],
    [140, 140, 150],
    [240, 240, 235],
];

/// The prop meshes and what collision needs from each definition, by version hash.
#[derive(Resource, Default)]
pub struct PropCache {
    entries: HashMap<[u8; 16], Entry>,
    /// blobs that arrived, waiting to be decoded and meshed, in arrival order
    queue: VecDeque<([u8; 16], Vec<u8>)>,
    /// counts the frames that showed props; each mesh is stamped with it when it is shown
    tick: u64,
    /// a blob was built, so the props on screen need another look
    changed: bool,
}

enum Entry {
    /// asked for at this time (real seconds), with the failures before this ask
    Asked { since: f64, tries: u8 },
    /// arrived, waiting in the build queue
    Queued,
    /// `seen` is the last [`PropCache::tick`] it was shown at
    Ready { mesh: Handle<Mesh>, info: Arc<PropInfo>, seen: u64 },
    /// a failed fetch or read; asked again after `retry_at`, until `MAX_TRIES`
    Failed { tries: u8, retry_at: f64 },
}

/// What collision needs from a definition.
#[derive(Clone, Debug, PartialEq)]
pub struct PropInfo {
    /// the footprint, in 0.5 m blocks from the pivot's floor cell
    pub footprint: Vec<[i16; 2]>,
    pub height: u8,
    /// whether the prop blocks walking (its collide is not `none`)
    pub solid: bool,
    /// the def's state names in order: a prop's `state` is an index into them
    pub states: Vec<String>,
    /// the lights its `light` behaviours give
    pub lights: Vec<PropLight>,
    /// the kinds of its `motion` behaviours
    pub motions: Vec<MotionKind>,
}

/// A light a def's `light` behaviour gives: its socket's place on the prop, in metres before the
/// prop's turn, with the colour (resolved to RGB when the def loads), power and `when` the def sets.
#[derive(Clone, Debug, PartialEq)]
pub struct PropLight {
    pub at: Vec3,
    pub rgb: [f32; 3],
    pub power: f32,
    pub when: When,
}

/// A light that is on, where it is in the world (metres), for `outdoors` to pick among the lamps.
/// `flicker` is the phase of its prop's flicker, when the prop flickers; `night` is set for a
/// light that is on after dark, which fades with the lamps.
#[derive(Clone, Debug, PartialEq)]
pub struct PropSpot {
    pub at: Vec3,
    pub rgb: [f32; 3],
    pub power: f32,
    pub flicker: Option<f32>,
    pub night: bool,
}

/// The motions a prop's def gives it, and the resting transform they move about. Attached only
/// to props with a motion other than flicker (which is light only).
#[derive(Component)]
struct PropMotion {
    base: Transform,
    kinds: Vec<MotionKind>,
    phase: f32,
}

/// The cells of the props that block walking, from the props on screen. Only props whose
/// blobs are loaded count, so a prop blocks once its definition is here. The client passes it
/// to `vox_core::body::with_props` when it moves its body.
#[derive(Resource, Default)]
pub struct PropOccupancy(pub Occupancy);

/// The material every prop mesh uses (vertex colours, no texture).
#[derive(Resource)]
struct PropMaterial(Handle<StandardMaterial>);

/// A prop entity on screen: the prop it shows, its spawned entity, and the definition's info
/// (shared with the cache, so the occupancy can be rebuilt from here without looking anything up).
struct ShownProp {
    prop: Prop,
    entity: Entity,
    info: Arc<PropInfo>,
}

/// What the prop systems know of the prop entities between frames, so a change costs only what
/// changed. `seen` is the seq each prop entity had when last diffed (`WorldEntities` keeps one per
/// entity and has no change log). `dirty` are the entities that changed since `show_props` last
/// ran. `waiting` are the prop entities whose blob is not ready yet (asked for, or not yet).
/// `asked` are the blobs in flight. `spawn` are the entities ready to show, drained by
/// [`SPAWN_CAP`] a frame.
#[derive(Resource, Default)]
struct PropWatch {
    seen: BTreeMap<EntityId, u64>,
    dirty: BTreeSet<EntityId>,
    waiting: BTreeSet<EntityId>,
    asked: Vec<[u8; 16]>,
    spawn: VecDeque<EntityId>,
    /// the [`WorldEntities::generation`] this was built for; another one is a new world
    generation: u64,
    /// the world changed, so `show_props` must take down everything it shows
    reset: bool,
}

pub struct PropPlugin;

impl Plugin for PropPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldEntities>()
            .init_resource::<PropCache>()
            .init_resource::<PropWatch>()
            .init_resource::<PropOccupancy>()
            .add_systems(Startup, setup_material)
            .add_systems(Update, (fetch_props, build_props, show_props).chain().after(crate::client::receive))
            .add_systems(Update, animate_props);
    }
}

fn setup_material(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(PropMaterial(materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() })));
}

/// Where a prop's blob lives: `data/props/<hash>.voxp`, relative to the game's files.
pub(crate) fn blob_path(hash: [u8; 16]) -> std::path::PathBuf {
    files::base().join(format!("data/props/{}.voxp", Hash(hash).hex()))
}

/// Records a failed fetch or read: asked again after `RETRY_SECS`, or given up on after `MAX_TRIES`.
fn fail(entries: &mut HashMap<[u8; 16], Entry>, hash: [u8; 16], tries: u8, now: f64) {
    let entry = if tries >= MAX_TRIES {
        crate::note!("[props] {}: gave up after {tries} tries", Hash(hash).hex());
        Entry::Failed { tries: MAX_TRIES, retry_at: f64::INFINITY }
    } else {
        Entry::Failed { tries, retry_at: now + RETRY_SECS }
    };
    entries.insert(hash, entry);
}

/// What a waiting prop entity's blob needs next.
#[derive(Debug, PartialEq)]
enum Wait {
    /// ask for it now, after this many failures
    Ask(u8),
    /// nothing yet: in flight, queued, or a failure whose retry is not due
    Keep,
    /// stop waiting: it is here, or it was given up on
    Drop,
}

/// What to do about a waiting entity, from its blob's entry. A failure not yet due stays
/// waiting (it is retried when due); only a blob given up on (`MAX_TRIES`) leaves.
fn wait_step(entry: Option<&Entry>, now: f64) -> Wait {
    match entry {
        None => Wait::Ask(0),
        Some(Entry::Failed { tries, .. }) if *tries >= MAX_TRIES => Wait::Drop,
        Some(Entry::Failed { tries, retry_at }) if now >= *retry_at => Wait::Ask(*tries),
        // shown by `show_props` once built, which takes it out of the waiting set
        _ => Wait::Keep,
    }
}

/// Brings the watch up to date with the entities: a new world (another generation) forgets
/// everything, then, when `scan` says the entities changed, the prop entities whose seq differs
/// from the last look, and the ones that are gone, go in `dirty`. A dirty entity whose blob is
/// not `ready` waits for it; the others do not.
fn sync_watch(watch: &mut PropWatch, entities: &WorldEntities, scan: bool, ready: impl Fn(&[u8; 16]) -> bool) {
    let new_world = watch.generation != entities.generation;
    if new_world {
        // an id and seq may come again in the new world with another prop, so nothing carries over
        let generation = entities.generation;
        *watch = PropWatch { generation, reset: true, asked: std::mem::take(&mut watch.asked), ..PropWatch::default() };
    }
    let PropWatch { seen, dirty, waiting, .. } = watch;
    if scan || new_world {
        for (id, (seq, c)) in &entities.map {
            if matches!(c, Component::Prop(_)) && seen.get(id) != Some(seq) {
                seen.insert(*id, *seq);
                dirty.insert(*id);
            }
        }
        seen.retain(|id, _| {
            let here = matches!(entities.map.get(id), Some((_, Component::Prop(_))));
            if !here {
                dirty.insert(*id);
            }
            here
        });
    }
    for id in dirty.iter() {
        match entities.map.get(id) {
            Some((_, Component::Prop(p))) if !ready(&p.prop) => {
                waiting.insert(*id);
            }
            _ => {
                waiting.remove(id);
            }
        }
    }
}

/// Takes down what changed (or everything, after a world change) and queues the changed props
/// that are still here and ready to go back up. Returns the entities to despawn, and whether
/// the occupancy must be rebuilt (a solid prop went: its cells may be shared with another).
fn reconcile(
    shown: &mut BTreeMap<EntityId, ShownProp>,
    watch: &mut PropWatch,
    entities: &WorldEntities,
    ready: impl Fn(&[u8; 16]) -> bool,
) -> (Vec<Entity>, bool) {
    let mut despawn = Vec::new();
    let mut rebuild = false;
    if std::mem::take(&mut watch.reset) {
        rebuild = shown.values().any(|s| s.info.solid);
        despawn.extend(std::mem::take(shown).into_values().map(|s| s.entity));
    }
    for id in std::mem::take(&mut watch.dirty) {
        if let Some(old) = shown.remove(&id) {
            despawn.push(old.entity);
            rebuild |= old.info.solid;
        }
        if let Some((_, Component::Prop(p))) = entities.map.get(&id)
            && ready(&p.prop)
        {
            watch.spawn.push_back(id);
        }
    }
    (despawn, rebuild)
}

/// Records a prop as shown and adds its cells to the occupancy.
fn show_one(shown: &mut BTreeMap<EntityId, ShownProp>, occupancy: &mut Occupancy, id: EntityId, prop: Prop, entity: Entity, info: Arc<PropInfo>) {
    add_cells(occupancy, &prop, &info);
    shown.insert(id, ShownProp { prop, entity, info });
}

/// Asks for the blobs of the props that are on screen, and takes in the ones that arrived (they
/// wait in the build queue). Only the entities that changed (by their seq) and the ones still
/// waiting for a blob are looked at, not every entity each frame.
fn fetch_props(entities: Res<WorldEntities>, time: Res<Time<Real>>, mut cache: ResMut<PropCache>, mut watch: ResMut<PropWatch>) {
    let now = time.elapsed_secs_f64();
    {
        let entries = &cache.entries;
        sync_watch(&mut watch, &entities, entities.is_changed(), |h| matches!(entries.get(h), Some(Entry::Ready { .. })));
    }
    let PropWatch { waiting, asked, .. } = &mut *watch;

    // arrivals and timeouts first, so the ones still waiting count against the limit
    for hash in std::mem::take(asked) {
        let Some(Entry::Asked { since, tries }) = cache.entries.get(&hash) else { continue };
        let (since, tries) = (*since, *tries);
        // an empty file is a browser fetch that failed (`files::put` keeps the empty bytes), so
        // it is not here yet
        match files::read(&blob_path(hash)).filter(|b| !b.is_empty()) {
            Some(bytes) => {
                cache.queue.push_back((hash, bytes));
                cache.entries.insert(hash, Entry::Queued);
            }
            // in the browser it is still on its way, unless it has taken too long; on the
            // desktop a missing file stays missing, so it is failed for now at once
            None if cfg!(target_arch = "wasm32") && now - since < RETRY_SECS => asked.push(hash),
            None => fail(&mut cache.entries, hash, tries + 1, now),
        }
    }

    // then ask for the ones waiting that are not here yet (and their retry is due)
    let mut gone = Vec::new();
    for id in waiting.iter() {
        if asked.len() >= IN_FLIGHT {
            break;
        }
        let Some((_, Component::Prop(p))) = entities.map.get(id) else {
            gone.push(*id);
            continue;
        };
        let tries = match wait_step(cache.entries.get(&p.prop), now) {
            Wait::Ask(tries) => tries,
            Wait::Keep => continue,
            Wait::Drop => {
                gone.push(*id);
                continue;
            }
        };
        files::want(&blob_path(p.prop));
        cache.entries.insert(p.prop, Entry::Asked { since: now, tries });
        asked.push(p.prop);
    }
    for id in gone {
        waiting.remove(&id);
    }
}

/// Decodes and meshes the queued blobs in arrival order, within [`BUILD_BUDGET`] a frame.
fn build_props(mut cache: ResMut<PropCache>, mut meshes: ResMut<Assets<Mesh>>) {
    if cache.queue.is_empty() {
        return;
    }
    let started = Instant::now();
    while let Some((hash, bytes)) = cache.queue.pop_front() {
        let tick = cache.tick;
        let entry = match load(&bytes, hash) {
            Ok((mesh, info)) => Entry::Ready { mesh: meshes.add(mesh), info: Arc::new(info), seen: tick },
            Err(why) => {
                crate::note!("[props] {}: {why}", Hash(hash).hex());
                Entry::Failed { tries: MAX_TRIES, retry_at: f64::INFINITY }
            }
        };
        cache.entries.insert(hash, entry);
        cache.changed = true;
        if started.elapsed() >= BUILD_BUDGET {
            break;
        }
    }
}

/// Decodes a blob under the hash it was asked for, and makes its mesh.
fn load(bytes: &[u8], hash: [u8; 16]) -> Result<(Mesh, PropInfo), String> {
    let blob = PropBlob::decode_named(bytes, &Hash(hash)).map_err(|e| e.to_string())?;
    let voxel = blob.def.scale.scale().voxel_size();
    let parts = mesh_parts(&blob.grid, blob.def.pivot, voxel)?;
    let info = PropInfo {
        footprint: blob.def.place.footprint.clone(),
        height: blob.def.place.height,
        solid: blob.def.collide != Collide::None,
        states: blob.def.states.clone(),
        lights: socket_lights(&blob.def, voxel),
        motions: blob.def.behaviours.iter().filter_map(|b| if let Behaviour::Motion(m) = b { Some(m.kind) } else { None }).collect(),
    };
    Ok((parts.into_mesh(), info))
}

/// Whether a prop blocks walking, and so adds its cells to the occupancy.
fn add_cells(occupancy: &mut Occupancy, prop: &Prop, info: &PropInfo) {
    if info.solid {
        occupancy.add_prop(prop.at, prop.yaw, &info.footprint, info.height);
    }
}

/// The occupancy of the props shown, rebuilt from scratch. Used when a shown prop that blocks is
/// removed or moved: `Occupancy` can add cells but not take them away, and two props may share one.
fn occupancy_of(shown: &BTreeMap<EntityId, ShownProp>) -> Occupancy {
    let mut cells = Occupancy::default();
    for s in shown.values() {
        add_cells(&mut cells, &s.prop, &s.info);
    }
    cells
}

/// The first spawns for this frame: up to [`SPAWN_CAP`] off the queue, the rest stay for later.
fn take_spawns(queue: &mut VecDeque<EntityId>) -> Vec<EntityId> {
    let n = queue.len().min(SPAWN_CAP);
    queue.drain(..n).collect()
}

/// Shows each prop on screen whose blob is loaded, at its place and turn, and removes the ones
/// that went or changed. Only the entities that changed since the last run are touched: a change
/// is taken down and up again, a new blob's props are queued, and at most [`SPAWN_CAP`] new props
/// are spawned a frame, the rest on later frames. Runs while anything is dirty, a blob was built,
/// or spawns are still queued.
fn show_props(
    mut commands: Commands,
    entities: Res<WorldEntities>,
    mut cache: ResMut<PropCache>,
    mut watch: ResMut<PropWatch>,
    mut shown: Local<BTreeMap<EntityId, ShownProp>>,
    material: Res<PropMaterial>,
    mut occupancy: ResMut<PropOccupancy>,
) {
    if watch.dirty.is_empty() && !watch.reset && !cache.changed && watch.spawn.is_empty() {
        return;
    }
    let blobs_arrived = std::mem::take(&mut cache.changed);
    cache.tick += 1;
    let tick = cache.tick;

    // what changed comes down; a changed prop that is still here goes back up (queued)
    let (despawn, rebuild) = {
        let entries = &cache.entries;
        reconcile(&mut shown, &mut watch, &entities, |h| matches!(entries.get(h), Some(Entry::Ready { .. })))
    };
    for e in despawn {
        commands.entity(e).try_despawn();
    }
    // a removed cell may be shared with another prop, so the occupancy is rebuilt once here
    if rebuild {
        occupancy.0 = occupancy_of(&shown);
    }
    let PropWatch { waiting, spawn, .. } = &mut *watch;

    // props still waiting whose blob has just been built are ready to show
    if blobs_arrived {
        let entries = &cache.entries;
        waiting.retain(|id| match entities.map.get(id) {
            Some((_, Component::Prop(p))) if matches!(entries.get(&p.prop), Some(Entry::Ready { .. })) => {
                spawn.push_back(*id);
                false
            }
            _ => true,
        });
    }

    for id in take_spawns(spawn) {
        if shown.contains_key(&id) {
            continue;
        }
        let Some((_, Component::Prop(p))) = entities.map.get(&id) else { continue };
        let Some(Entry::Ready { mesh, info, seen }) = cache.entries.get_mut(&p.prop) else {
            waiting.insert(id);
            continue;
        };
        *seen = tick;
        let (mesh, info) = (mesh.clone(), info.clone());

        let at = Vec3::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32) * UNIT_M;
        let base = Transform::from_translation(at).with_rotation(turn_of(p.yaw));
        let mut spawned = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.0.clone()),
            base,
            Visibility::default(),
            Name::new(format!("prop {}", id.0)),
        ));
        let kinds = moving(&info.motions);
        if !kinds.is_empty() {
            spawned.insert(PropMotion { base, kinds, phase: phase_of(id) });
        }
        let entity = spawned.id();
        show_one(&mut shown, &mut occupancy.0, id, p.clone(), entity, info);
    }

    // the cache keeps its meshes within the cap; the ones on screen are marked so they stay, and
    // the entities whose mesh was dropped go back to waiting for it
    if cache.entries.len() > MAX_MESHES {
        for s in shown.values() {
            if let Some(Entry::Ready { seen, .. }) = cache.entries.get_mut(&s.prop.prop) {
                *seen = tick;
            }
        }
        let dropped = evict(&mut cache.entries, tick);
        if !dropped.is_empty() {
            for (id, (_, c)) in &entities.map {
                if let Component::Prop(p) = c
                    && dropped.contains(&p.prop)
                    && !shown.contains_key(id)
                {
                    waiting.insert(*id);
                }
            }
        }
    }
}

/// The turn a prop's yaw gives it. Yaw is in 15 degree steps, a full turn is `PROP_YAWS` of them.
fn turn_of(yaw: u8) -> Quat {
    Quat::from_rotation_y(f32::from(yaw % PROP_YAWS) * TAU / f32::from(PROP_YAWS))
}

/// A prop's motion phase, from its entity id: the same on every client, spread round the turn.
fn phase_of(id: EntityId) -> f32 {
    (f64::from(id.0) * 0.618_033_988_749_894_9).fract() as f32 * TAU
}

/// The motions that move the mesh (flicker only lights it, so it is left out).
fn moving(kinds: &[MotionKind]) -> Vec<MotionKind> {
    kinds.iter().copied().filter(|k| *k != MotionKind::Flicker).collect()
}

/// The place a prop's lights are, in metres, for each light on in the world now. `night` is
/// outdoors' lamps level (0 by day, 1 after dark). Only props whose blobs are loaded count.
pub fn light_spots(cache: &PropCache, entities: &WorldEntities, night: f32) -> Vec<PropSpot> {
    let mut spots = Vec::new();
    for (id, (_, c)) in &entities.map {
        let Component::Prop(p) = c else { continue };
        let Some(Entry::Ready { info, .. }) = cache.entries.get(&p.prop) else { continue };
        if info.lights.is_empty() {
            continue;
        }
        let state = info.states.get(usize::from(p.state)).map(String::as_str);
        let place = Vec3::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32) * UNIT_M;
        let turn = turn_of(p.yaw);
        let flicker = info.motions.contains(&MotionKind::Flicker).then(|| phase_of(*id));
        for light in &info.lights {
            if light_is_on(&light.when, state, night) {
                let night = matches!(light.when, When::Night);
                spots.push(PropSpot { at: place + turn * light.at, rgb: light.rgb, power: light.power, flicker, night });
            }
        }
    }
    spots
}

/// Moves each prop that has motions, from the clock: the motions of its behaviours, on top of
/// its resting place. Sway leans it towards the wind, which is read from the world's air.
fn animate_props(time: Res<Time<Real>>, world: Option<Res<WorldClient>>, mut props: Query<(&PropMotion, &mut Transform)>) {
    let clock = time.elapsed_secs_f64();
    let wind = world.and_then(|w| w.air()).map_or(Vec3::ZERO, |a| Vec3::new(a.wind[0], 0.0, a.wind[2]));
    for (motion, mut tf) in &mut props {
        // the wind in the prop's own frame, so a prop leans into it whatever way it faces
        let local = motion.base.rotation.inverse() * wind;
        let (mut offset, mut turn) = (Vec3::ZERO, Quat::IDENTITY);
        for kind in &motion.kinds {
            let (o, r) = motion_offset(*kind, clock, motion.phase, Vec2::new(local.x, local.z));
            offset += o;
            turn *= r;
        }
        tf.translation = motion.base.translation + offset;
        tf.rotation = motion.base.rotation * turn;
    }
}

/// Seconds for one full turn of a spinning prop.
const SPIN_SECS: f64 = 8.0;
/// Seconds for one full swing of a swinging prop.
const SWING_SECS: f64 = 4.0;
/// Widest swing, in radians (about 20 degrees either way).
const SWING_RAD: f32 = 0.35;
/// Seconds for one bob up and back.
const BOB_SECS: f64 = 3.0;
/// Highest a bobbing prop rises or sinks, in metres.
const BOB_M: f32 = 0.05;
/// Seconds for one sway.
const SWAY_SECS: f64 = 5.0;
/// Widest sway, in radians (about 7 degrees), reached in a wind of [`SWAY_FULL_WIND`].
const SWAY_RAD: f32 = 0.12;
/// Wind speed (m/s) at which a swaying prop leans its full amount. In still air it leans a fifth.
const SWAY_FULL_WIND: f32 = 8.0;

/// The angle of a motion `period` seconds long, `phase` ahead, at clock time `t`, in radians. The
/// clock is reduced in f64 first, so a long session keeps its precision.
fn cycle(t: f64, period: f64, phase: f32) -> f32 {
    (t / period).rem_euclid(1.0) as f32 * TAU + phase
}

/// What one motion does to a prop at clock time `t`: a lift of its place, and a turn about it. A
/// spin turns about the prop's own up; a swing about its x; a bob lifts it; a sway leans it along
/// the wind (`wind` in the prop's own frame, x and z). A flicker does neither.
pub fn motion_offset(kind: MotionKind, t: f64, phase: f32, wind: Vec2) -> (Vec3, Quat) {
    match kind {
        MotionKind::Spin => (Vec3::ZERO, Quat::from_rotation_y(cycle(t, SPIN_SECS, phase))),
        MotionKind::Swing => (Vec3::ZERO, Quat::from_rotation_x(SWING_RAD * cycle(t, SWING_SECS, phase).sin())),
        MotionKind::Bob => (Vec3::Y * (BOB_M * cycle(t, BOB_SECS, phase).sin()), Quat::IDENTITY),
        MotionKind::Sway => {
            let speed = wind.length();
            // the axis across the wind, so the prop leans along it
            let axis = if speed > 1e-3 { Vec3::new(-wind.y, 0.0, wind.x) / speed } else { Vec3::X };
            let gust = 0.2 + 0.8 * (speed / SWAY_FULL_WIND).min(1.0);
            (Vec3::ZERO, Quat::from_axis_angle(axis, SWAY_RAD * gust * cycle(t, SWAY_SECS, phase).sin()))
        }
        // flicker is light only: the mesh stays put (see `flicker`)
        MotionKind::Flicker => (Vec3::ZERO, Quat::IDENTITY),
    }
}

/// How bright a flickering light is at clock time `t`, as a share of its power: between 0.6 and 1.
pub fn flicker(t: f64, phase: f32) -> f32 {
    let p = f64::from(phase);
    (0.8 + 0.12 * (t * 9.0 + p).sin() + 0.08 * (t * 23.0 + p * 1.7).sin()) as f32
}

/// Whether a light is on: always; after dark (`night` is outdoors' lamps level, 0 by day and 1
/// after dark, lit above 0.01 as the lamps are); or while the prop is in the state its `when`
/// names. `state` is the prop's current state's name, if it has one.
pub fn light_is_on(when: &When, state: Option<&str>, night: f32) -> bool {
    match when {
        When::Always => true,
        When::Night => night > 0.01,
        When::State(name) => state == Some(name.as_str()),
    }
}

/// The place of a socket's voxel centre on a prop, in metres before the prop's turn. It uses the
/// grid `mesh_parts` draws on: x and z centred on the pivot's column, y up from the floor.
fn socket_local(at: [u16; 3], pivot: [u16; 3], voxel: f32) -> Vec3 {
    let d = |i: usize| f32::from(at[i]) - f32::from(pivot[i]);
    Vec3::new(d(0) * voxel, (d(1) + 0.5) * voxel, d(2) * voxel)
}

/// The lights a def gives, each at its socket. A light naming no socket is left out (validation
/// refuses such a def).
fn socket_lights(def: &PropDef, voxel: f32) -> Vec<PropLight> {
    def.behaviours
        .iter()
        .filter_map(|b| {
            let Behaviour::Light(light) = b else { return None };
            let socket = def.sockets.iter().find(|s| s.name == light.socket)?;
            Some(PropLight {
                at: socket_local(socket.at, def.pivot, voxel),
                rgb: light_rgb(&light.colour),
                power: light.power,
                when: light.when.clone(),
            })
        })
        .collect()
}

/// The RGB of a prop light's colour name: the lamp's for "warm", the fire's for "fire", and warm
/// white for any other name (no other colour is in the lamp and fire palette). Called once when
/// the def loads, so a frame never looks a name up.
fn light_rgb(name: &str) -> [f32; 3] {
    match name {
        "warm" => LAMP_RGB,
        "fire" => FIRE_RGB,
        _ => [1.0, 0.95, 0.85],
    }
}

/// Drops the least recently shown meshes beyond [`MAX_MESHES`], and returns their hashes. Meshes
/// shown this pass carry `tick` and are never dropped, so the cache may stay over the cap while
/// more props than that are on screen; the dropped ones are asked for again when they come back
/// into view.
fn evict(entries: &mut HashMap<[u8; 16], Entry>, tick: u64) -> Vec<[u8; 16]> {
    let mut ready: Vec<(u64, [u8; 16])> = entries
        .iter()
        .filter_map(|(hash, e)| match e {
            Entry::Ready { seen, .. } => Some((*seen, *hash)),
            _ => None,
        })
        .collect();
    let mut dropped = Vec::new();
    if ready.len() <= MAX_MESHES {
        return dropped;
    }
    ready.sort_unstable();
    let excess = ready.len() - MAX_MESHES;
    for (seen, hash) in ready.into_iter().take(excess) {
        if seen == tick {
            break;
        }
        entries.remove(&hash);
        dropped.push(hash);
    }
    dropped
}

/// Flat faces for a grid: one quad per face that touches no filled voxel, with coplanar faces of
/// one colour merged into one quad (greedy, per slice). Each voxel is a cube `voxel` metres
/// across.
///
/// The pivot voxel is centred on the prop's place: the origin is the middle of the pivot voxel's
/// footprint on x and z, and its bottom on y. So the prop sits on its place and turns about it.
///
/// Fails when the mesh would have more than [`MAX_QUADS`] quads.
#[derive(Default, Debug)]
pub struct MeshParts {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

impl MeshParts {
    /// The quads as one mesh for the GPU (its data is not kept on the CPU).
    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx))
    }

    fn quads(&self) -> usize {
        self.idx.len() / 6
    }
}

pub fn mesh_parts(grid: &VoxelGrid, pivot: [u16; 3], voxel: f32) -> Result<MeshParts, String> {
    let size = grid.size().map(usize::from);
    // dense occupancy: the palette index of each voxel, 0 where it is empty (never stored)
    let mut occ = vec![0u8; size[0] * size[1] * size[2]];
    for (pos, index) in grid.iter() {
        let [x, y, z] = pos.map(usize::from);
        occ[x + size[0] * (y + size[1] * z)] = index;
    }
    // the palette index at a position, 0 outside the grid
    let at = |p: [isize; 3]| -> u8 {
        if p.iter().zip(size).any(|(&c, side)| c < 0 || c >= side as isize) {
            return 0;
        }
        let [x, y, z] = p.map(|c| c as usize);
        occ[x + size[0] * (y + size[1] * z)]
    };
    let pivot = pivot.map(f32::from);
    // a point on the grid's lines, to metres from the prop's place (see `mesh_parts`)
    let world = |q: [f32; 3]| [0, 1, 2].map(|i| (q[i] - pivot[i]) * voxel - if i == 1 { 0.0 } else { voxel / 2.0 });

    let mut out = MeshParts::default();
    for d in 0..3 {
        let (ua, va) = ((d + 1) % 3, (d + 2) % 3);
        let (du, dv) = (size[ua], size[va]);
        let mut pos_mask = vec![0u8; du * dv];
        let mut neg_mask = vec![0u8; du * dv];
        // the boundary at `b` along `d` lies between the voxels at b-1 and b
        for b in 0..=size[d] as isize {
            for v in 0..dv {
                for u in 0..du {
                    let mut hi_at = [0isize; 3];
                    hi_at[d] = b;
                    hi_at[ua] = u as isize;
                    hi_at[va] = v as isize;
                    let mut lo_at = hi_at;
                    lo_at[d] = b - 1;
                    let (lo, hi) = (at(lo_at), at(hi_at));
                    pos_mask[u + du * v] = if lo != 0 && hi == 0 { lo } else { 0 };
                    neg_mask[u + du * v] = if hi != 0 && lo == 0 { hi } else { 0 };
                }
            }
            for (mask, sign) in [(&mut pos_mask, 1i32), (&mut neg_mask, -1i32)] {
                merge_mask(mask, du, dv, |u, v, w, h, colour| {
                    let corner = |s: usize, t: usize| {
                        let mut q = [0.0f32; 3];
                        q[d] = b as f32;
                        q[ua] = (u + s) as f32;
                        q[va] = (v + t) as f32;
                        world(q)
                    };
                    let quad = [corner(0, 0), corner(w, 0), corner(w, h), corner(0, h)];
                    let mut normal = [0.0f32; 3];
                    normal[d] = sign as f32;
                    push_quad(&mut out, quad, normal, fallback_colour(colour));
                });
            }
            if out.quads() > MAX_QUADS {
                return Err(format!("its mesh would have more than {MAX_QUADS} quads"));
            }
        }
    }
    Ok(out)
}

/// Greedy merge of one slice's face mask (the colour of each cell, 0 for none): from each cell
/// that has a face, the run of equal colours grows along `u`, then the rows below it grow as long
/// as they match the whole run. `emit` gets `(u, v, width, height, colour)` for each rectangle,
/// and the cells it covers are cleared.
fn merge_mask(mask: &mut [u8], du: usize, dv: usize, mut emit: impl FnMut(usize, usize, usize, usize, u8)) {
    for v in 0..dv {
        let mut u = 0;
        while u < du {
            let colour = mask[u + du * v];
            if colour == 0 {
                u += 1;
                continue;
            }
            let mut w = 1;
            while u + w < du && mask[u + w + du * v] == colour {
                w += 1;
            }
            let mut h = 1;
            while v + h < dv && (0..w).all(|k| mask[u + k + du * (v + h)] == colour) {
                h += 1;
            }
            for dy in 0..h {
                for k in 0..w {
                    mask[u + k + du * (v + dy)] = 0;
                }
            }
            emit(u, v, w, h, colour);
            u += w;
        }
    }
}

/// Two triangles for a quad (corners in order around it), wound counter-clockwise seen from the
/// side `normal` points to.
fn push_quad(out: &mut MeshParts, quad: [[f32; 3]; 4], normal: [f32; 3], colour: [f32; 4]) {
    let facing = cross(sub(quad[1], quad[0]), sub(quad[2], quad[0]));
    let order = if dot(facing, normal) < 0.0 { [0, 2, 1, 0, 3, 2] } else { [0, 1, 2, 0, 2, 3] };
    let start = out.pos.len() as u32;
    out.pos.extend(quad);
    out.nrm.extend([normal; 4]);
    out.col.extend([colour; 4]);
    out.idx.extend(order.map(|i| start + i));
}

fn fallback_colour(index: u8) -> [f32; 4] {
    let [r, g, b] = FALLBACK_RAMP[usize::from(index) % FALLBACK_RAMP.len()];
    [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, 1.0]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_art::Scale;

    /// A grid of `size` with every voxel `filled` says filled, in palette index 1.
    fn grid(size: [u16; 3], filled: impl Fn([u16; 3]) -> bool) -> VoxelGrid {
        let mut g = VoxelGrid::new(Scale::Prop, size).unwrap();
        for z in 0..size[2] {
            for y in 0..size[1] {
                for x in 0..size[0] {
                    if filled([x, y, z]) {
                        g.set([x, y, z], 1).unwrap();
                    }
                }
            }
        }
        g
    }

    /// A checkerboard: no two filled voxels share a face, so no face can merge.
    fn checker(p: [u16; 3]) -> bool {
        (p[0] + p[1] + p[2]).is_multiple_of(2)
    }

    fn bounds(pos: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
        pos.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            ([0, 1, 2].map(|i| lo[i].min(p[i])), [0, 1, 2].map(|i| hi[i].max(p[i])))
        })
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn two_touching_voxels_merge_their_sides() {
        let parts = mesh_parts(&grid([2, 1, 1], |_| true), [0, 0, 0], 0.1).unwrap();
        // the two ends, and the top, bottom, front and back, each one quad across both voxels
        assert_eq!(parts.quads(), 6);
        assert_eq!(parts.pos.len(), 24);
        assert!(parts.nrm.iter().all(|n| (dot(*n, *n) - 1.0).abs() < 1e-6));
    }

    #[test]
    fn a_full_cube_is_six_quads() {
        let parts = mesh_parts(&grid([16, 16, 16], |_| true), [0, 0, 0], 0.1).unwrap();
        assert_eq!(parts.quads(), 6);
    }

    #[test]
    fn a_checkerboard_keeps_every_face_and_is_under_the_cap() {
        // 2048 filled voxels, each with six exposed faces, none merged: 12288 quads < 50000
        let parts = mesh_parts(&grid([16, 16, 16], checker), [0, 0, 0], 0.1).unwrap();
        assert_eq!(parts.quads(), 6 * 2048);
    }

    #[test]
    fn a_checkerboard_over_the_cap_is_refused() {
        // 16384 filled voxels, 98304 exposed faces > 50000 quads
        assert!(mesh_parts(&grid([32, 32, 32], checker), [0, 0, 0], 0.1).is_err());
    }

    #[test]
    fn a_single_voxel_is_centred_on_its_place() {
        let parts = mesh_parts(&grid([1, 1, 1], |_| true), [0, 0, 0], 0.1).unwrap();
        let (lo, hi) = bounds(&parts.pos);
        // x and z run from -half to +half about the place; y starts at the place (the floor)
        assert!(close(lo, [-0.05, 0.0, -0.05]), "lo {lo:?}");
        assert!(close(hi, [0.05, 0.1, 0.05]), "hi {hi:?}");
    }

    /// Whether two turns are the same turn (their dot product is 1 up to sign).
    fn same_turn(a: Quat, b: Quat) -> bool {
        a.dot(b).abs() > 1.0 - 1e-5
    }

    #[test]
    fn spin_turns_once_about_y_per_period() {
        let phase = 0.5;
        let (_, start) = motion_offset(MotionKind::Spin, 0.0, phase, Vec2::ZERO);
        assert!(same_turn(start, Quat::from_rotation_y(phase)));
        let (_, quarter) = motion_offset(MotionKind::Spin, SPIN_SECS / 4.0, phase, Vec2::ZERO);
        assert!(same_turn(quarter, Quat::from_rotation_y(phase + TAU / 4.0)));
        let (_, again) = motion_offset(MotionKind::Spin, SPIN_SECS, phase, Vec2::ZERO);
        assert!(same_turn(again, Quat::from_rotation_y(phase)));
    }

    #[test]
    fn bob_stays_within_its_height() {
        for step in 0..600 {
            let (offset, turn) = motion_offset(MotionKind::Bob, f64::from(step) * 0.1, 1.0, Vec2::ZERO);
            assert!(offset.y.abs() <= BOB_M + 1e-6, "offset {offset:?}");
            assert_eq!((offset.x, offset.z), (0.0, 0.0));
            assert_eq!(turn, Quat::IDENTITY);
        }
    }

    #[test]
    fn sway_leans_no_further_than_its_widest_swing_in_any_wind() {
        for wind in [Vec2::ZERO, Vec2::new(3.0, 0.0), Vec2::new(0.0, -20.0)] {
            for step in 0..500 {
                let (_, turn) = motion_offset(MotionKind::Sway, f64::from(step) * 0.07, 0.3, wind);
                let angle = 2.0 * turn.w.abs().min(1.0).acos();
                assert!(angle <= SWAY_RAD + 1e-4, "wind {wind:?}: angle {angle}");
            }
        }
    }

    #[test]
    fn flicker_stays_between_six_tenths_and_all_of_its_power() {
        for step in 0..1000 {
            let f = flicker(f64::from(step) * 0.013, 2.0);
            assert!((0.6..=1.0).contains(&f), "flicker {f}");
        }
    }

    #[test]
    fn a_light_is_on_by_its_when() {
        assert!(light_is_on(&When::Always, None, 0.0));
        assert!(!light_is_on(&When::Night, None, 0.0));
        assert!(light_is_on(&When::Night, None, 1.0));
        assert!(light_is_on(&When::State("lit".to_string()), Some("lit"), 0.0));
        assert!(!light_is_on(&When::State("lit".to_string()), Some("off"), 1.0));
        assert!(!light_is_on(&When::State("lit".to_string()), None, 1.0));
    }

    #[test]
    fn a_light_colour_name_resolves_to_its_rgb() {
        assert_eq!(light_rgb("warm"), LAMP_RGB);
        assert_eq!(light_rgb("fire"), FIRE_RGB);
        // any other name is warm white
        assert_eq!(light_rgb("cold"), [1.0, 0.95, 0.85]);
        assert_eq!(light_rgb(""), [1.0, 0.95, 0.85]);
    }

    #[test]
    fn a_socket_on_the_pivot_is_half_a_voxel_up() {
        assert!(close(socket_local([4, 2, 4], [4, 2, 4], 0.1).to_array(), [0.0, 0.05, 0.0]));
    }

    fn info(solid: bool) -> Arc<PropInfo> {
        Arc::new(PropInfo { footprint: vec![[0, 0], [1, 0], [0, 1]], height: 2, solid, states: Vec::new(), lights: Vec::new(), motions: Vec::new() })
    }

    fn prop_at(at: [i32; 3], yaw: u8) -> Prop {
        Prop { prop: [7; 16], at, yaw, variant: 0, state: 0, channel: None, text: None, owner: None }
    }

    fn put(shown: &mut BTreeMap<EntityId, ShownProp>, occ: &mut Occupancy, id: EntityId, prop: Prop, info: Arc<PropInfo>) {
        show_one(shown, occ, id, prop, Entity::PLACEHOLDER, info);
    }

    fn lift(shown: &mut BTreeMap<EntityId, ShownProp>, occ: &mut Occupancy, id: EntityId) {
        if let Some(old) = shown.remove(&id)
            && old.info.solid
        {
            *occ = occupancy_of(shown);
        }
    }

    fn change(id: u32, seq: u64, prop: Option<Prop>) -> vox_core::wire::EntityChange {
        vox_core::wire::EntityChange { entity: EntityId(id), seq, component: prop.map(Component::Prop) }
    }

    /// One frame of the prop systems over plain data: the watch is synced, then reconciled, then
    /// everything queued is shown (every blob is ready). Returns the entities despawned.
    fn frame(watch: &mut PropWatch, shown: &mut BTreeMap<EntityId, ShownProp>, occ: &mut Occupancy, ents: &WorldEntities, solid: &Arc<PropInfo>) -> Vec<Entity> {
        sync_watch(watch, ents, true, |_| true);
        let (despawn, rebuild) = reconcile(shown, watch, ents, |_| true);
        if rebuild {
            *occ = occupancy_of(shown);
        }
        for id in take_spawns(&mut watch.spawn) {
            if let Some((_, Component::Prop(p))) = ents.map.get(&id) {
                show_one(shown, occ, id, p.clone(), Entity::PLACEHOLDER, solid.clone());
            }
        }
        despawn
    }

    #[test]
    fn occupancy_after_adds_moves_and_removes_equals_a_full_rebuild() {
        let (mut shown, mut occ) = (BTreeMap::new(), Occupancy::default());
        let solid = info(true);
        put(&mut shown, &mut occ, EntityId(1), prop_at([0, 0, 0], 0), solid.clone());
        // the same place as the first, so every one of its cells is shared
        put(&mut shown, &mut occ, EntityId(2), prop_at([0, 0, 0], 0), solid.clone());
        put(&mut shown, &mut occ, EntityId(3), prop_at([40, 0, 0], 0), info(false));
        assert_eq!(occ, occupancy_of(&shown));

        // move the first away: the cells it shared stay blocked by the second
        lift(&mut shown, &mut occ, EntityId(1));
        put(&mut shown, &mut occ, EntityId(1), prop_at([-5, 0, 4], 12), solid.clone());
        assert_eq!(occ, occupancy_of(&shown));
        for c in vox_core::body::prop_footprint_cells([0, 0, 0], 0, &solid.footprint, solid.height) {
            assert!(occ.contains(c), "cell {c:?} of the second prop is still blocked");
        }

        // remove the second and the non-solid third
        lift(&mut shown, &mut occ, EntityId(2));
        lift(&mut shown, &mut occ, EntityId(3));
        assert_eq!(occ, occupancy_of(&shown));
        assert_eq!(shown.len(), 1);
    }

    #[test]
    fn the_frame_logic_follows_add_move_and_remove() {
        let (mut watch, mut shown, mut occ, mut ents) = (PropWatch::default(), BTreeMap::new(), Occupancy::default(), WorldEntities::default());
        let solid = info(true);
        ents.apply(&[change(1, 1, Some(prop_at([0, 0, 0], 0))), change(2, 2, Some(prop_at([0, 0, 0], 0)))]);
        assert!(frame(&mut watch, &mut shown, &mut occ, &ents, &solid).is_empty());
        assert_eq!(shown.len(), 2);
        assert_eq!(occ, occupancy_of(&shown));

        // a frame with nothing changed leaves everything as it is
        assert!(frame(&mut watch, &mut shown, &mut occ, &ents, &solid).is_empty());
        assert_eq!(shown.len(), 2);

        // move entity 1 (a newer seq): it is taken down and shown at its new place
        ents.apply(&[change(1, 3, Some(prop_at([-5, 0, 4], 12)))]);
        assert_eq!(frame(&mut watch, &mut shown, &mut occ, &ents, &solid).len(), 1);
        assert_eq!(shown[&EntityId(1)].prop.at, [-5, 0, 4]);
        assert_eq!(occ, occupancy_of(&shown));

        // remove entity 2
        ents.apply(&[change(2, 4, None)]);
        assert_eq!(frame(&mut watch, &mut shown, &mut occ, &ents, &solid).len(), 1);
        assert_eq!(shown.len(), 1);
        assert_eq!(occ, occupancy_of(&shown));
    }

    #[test]
    fn a_world_change_with_the_same_id_and_seq_shows_the_new_prop() {
        let (mut watch, mut shown, mut occ, mut ents) = (PropWatch::default(), BTreeMap::new(), Occupancy::default(), WorldEntities::default());
        let solid = info(true);
        ents.apply(&[change(1, 5, Some(prop_at([0, 0, 0], 0)))]);
        frame(&mut watch, &mut shown, &mut occ, &ents, &solid);
        assert_eq!(shown[&EntityId(1)].prop.at, [0, 0, 0]);

        // the new world's entities arrive in the same run as the clear: same id, same seq
        ents.clear();
        ents.apply(&[change(1, 5, Some(prop_at([100, 0, 100], 0)))]);
        let despawn = frame(&mut watch, &mut shown, &mut occ, &ents, &solid);
        assert_eq!(despawn.len(), 1);
        assert_eq!(shown[&EntityId(1)].prop.at, [100, 0, 100]);
        assert_eq!(occ, occupancy_of(&shown));
        assert!(!occ.contains(vox_core::body::prop_footprint_cells([0, 0, 0], 0, &solid.footprint, solid.height)[0]));

        // a new world with no props takes everything down
        ents.clear();
        frame(&mut watch, &mut shown, &mut occ, &ents, &solid);
        assert!(shown.is_empty());
        assert_eq!(occ, Occupancy::default());
    }

    #[test]
    fn a_failed_fetch_keeps_waiting_until_due_and_is_given_up_at_the_cap() {
        let failed = |tries, retry_at| Entry::Failed { tries, retry_at };
        assert_eq!(wait_step(None, 0.0), Wait::Ask(0));
        // a first failure that is not due yet stays waiting
        assert_eq!(wait_step(Some(&failed(1, 10.0)), 5.0), Wait::Keep);
        // due: asked again, with its failures counted
        assert_eq!(wait_step(Some(&failed(1, 10.0)), 10.0), Wait::Ask(1));
        // the cap: given up on, however late
        assert_eq!(wait_step(Some(&failed(MAX_TRIES, f64::INFINITY)), 1e9), Wait::Drop);
        // in flight or queued: nothing to do
        assert_eq!(wait_step(Some(&Entry::Asked { since: 0.0, tries: 0 }), 1.0), Wait::Keep);
        assert_eq!(wait_step(Some(&Entry::Queued), 1.0), Wait::Keep);
    }

    #[test]
    fn a_spawn_cap_leaves_the_rest_for_later_frames_and_completes() {
        let mut queue: VecDeque<EntityId> = (0..150).map(EntityId).collect();
        let (mut frames, mut got) = (Vec::new(), Vec::new());
        while !queue.is_empty() {
            let batch = take_spawns(&mut queue);
            frames.push(batch.len());
            got.extend(batch);
        }
        assert_eq!(frames, vec![64, 64, 22]);
        assert_eq!(got, (0..150).map(EntityId).collect::<Vec<_>>());
    }
}
