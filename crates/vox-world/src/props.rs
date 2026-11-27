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

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    pbr::StandardMaterial,
    platform::time::Instant,
    prelude::*,
    time::{Real, Time},
};
use vox_art::{VoxelGrid, prop::Collide};
use vox_core::body::Occupancy;
use vox_core::entity::{Component, EntityId, PROP_YAWS, Prop};
use vox_format::Hash;
use vox_format::prop::PropBlob;

use crate::entities::WorldEntities;
use crate::files;

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
    Ready { mesh: Handle<Mesh>, info: PropInfo, seen: u64 },
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
}

/// The cells of the props that block walking, from the props on screen. Only props whose
/// blobs are loaded count, so a prop blocks once its definition is here. The client passes it
/// to `vox_core::body::with_props` when it moves its body.
#[derive(Resource, Default)]
pub struct PropOccupancy(pub Occupancy);

/// The material every prop mesh uses (vertex colours, no texture).
#[derive(Resource)]
struct PropMaterial(Handle<StandardMaterial>);

/// The prop entities on screen, by the entity's id, with the prop they show.
#[derive(Default)]
struct Shown(BTreeMap<EntityId, (Prop, Entity)>);

pub struct PropPlugin;

impl Plugin for PropPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldEntities>()
            .init_resource::<PropCache>()
            .init_resource::<PropOccupancy>()
            .add_systems(Startup, setup_material)
            .add_systems(Update, (fetch_props, build_props, show_props).chain().after(crate::client::receive));
    }
}

fn setup_material(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(PropMaterial(materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() })));
}

/// Where a prop's blob lives: `data/props/<hash>.voxp`, relative to the game's files.
fn blob_path(hash: [u8; 16]) -> std::path::PathBuf {
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

/// Asks for the blobs of the props that are on screen, and takes in the ones that arrived (they
/// wait in the build queue).
fn fetch_props(entities: Res<WorldEntities>, time: Res<Time<Real>>, mut cache: ResMut<PropCache>) {
    let now = time.elapsed_secs_f64();

    // arrivals and timeouts first, so the ones still waiting count against the limit
    let asked: Vec<[u8; 16]> = cache.entries.iter().filter(|(_, e)| matches!(e, Entry::Asked { .. })).map(|(h, _)| *h).collect();
    for hash in asked {
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
            None if cfg!(target_arch = "wasm32") && now - since < RETRY_SECS => {}
            None => fail(&mut cache.entries, hash, tries + 1, now),
        }
    }

    // then ask for the ones on screen that are not here yet
    let mut in_flight = cache.entries.values().filter(|e| matches!(e, Entry::Asked { .. })).count();
    for (_, c) in entities.map.values() {
        if in_flight >= IN_FLIGHT {
            break;
        }
        let Component::Prop(p) = c else { continue };
        let tries = match cache.entries.get(&p.prop) {
            None => 0,
            Some(Entry::Failed { tries, retry_at }) if *tries < MAX_TRIES && now >= *retry_at => *tries,
            _ => continue,
        };
        files::want(&blob_path(p.prop));
        cache.entries.insert(p.prop, Entry::Asked { since: now, tries });
        in_flight += 1;
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
            Ok((mesh, info)) => Entry::Ready { mesh: meshes.add(mesh), info, seen: tick },
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
    };
    Ok((parts.into_mesh(), info))
}

/// Shows each prop on screen whose blob is loaded, at its place and turn, and removes the ones
/// that went or changed. Runs when the entities or the blobs changed.
fn show_props(
    mut commands: Commands,
    entities: Res<WorldEntities>,
    mut cache: ResMut<PropCache>,
    mut shown: Local<Shown>,
    material: Res<PropMaterial>,
    mut occupancy: ResMut<PropOccupancy>,
) {
    if !entities.is_changed() && !cache.changed {
        return;
    }
    cache.changed = false;
    cache.tick += 1;
    let tick = cache.tick;

    let mut wanted: BTreeMap<EntityId, (Prop, Handle<Mesh>)> = BTreeMap::new();
    let mut cells = Occupancy::default();
    for (id, (_, c)) in &entities.map {
        let Component::Prop(p) = c else { continue };
        let Some(Entry::Ready { mesh, info, seen }) = cache.entries.get_mut(&p.prop) else { continue };
        *seen = tick;
        if info.solid {
            cells.add_prop(p.at, p.yaw, &info.footprint, info.height);
        }
        wanted.insert(*id, (p.clone(), mesh.clone()));
    }
    evict(&mut cache.entries, tick);
    occupancy.0 = cells;

    // what went or changed comes down; what is new goes up
    shown.0.retain(|id, (p, e)| {
        let keep = wanted.get(id).is_some_and(|(w, _)| w == p);
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });
    for (id, (p, mesh)) in wanted {
        if shown.0.contains_key(&id) {
            continue;
        }
        let at = Vec3::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32) * UNIT_M;
        // yaw is in 15 degree steps, a full turn is PROP_YAWS of them
        let turn = Quat::from_rotation_y(f32::from(p.yaw % PROP_YAWS) * std::f32::consts::TAU / f32::from(PROP_YAWS));
        let e = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.0.clone()),
                Transform::from_translation(at).with_rotation(turn),
                Visibility::default(),
                Name::new(format!("prop {}", id.0)),
            ))
            .id();
        shown.0.insert(id, (p, e));
    }
}

/// Drops the least recently shown meshes beyond [`MAX_MESHES`]. Meshes shown this pass carry
/// `tick` and are never dropped, so the cache may stay over the cap while more props than that
/// are on screen; the dropped ones are asked for again when they come back into view.
fn evict(entries: &mut HashMap<[u8; 16], Entry>, tick: u64) {
    let mut ready: Vec<(u64, [u8; 16])> = entries
        .iter()
        .filter_map(|(hash, e)| match e {
            Entry::Ready { seen, .. } => Some((*seen, *hash)),
            _ => None,
        })
        .collect();
    if ready.len() <= MAX_MESHES {
        return;
    }
    ready.sort_unstable();
    let excess = ready.len() - MAX_MESHES;
    for (seen, hash) in ready.into_iter().take(excess) {
        if seen == tick {
            break;
        }
        entries.remove(&hash);
    }
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
}
