//! Meshing the world's chunks and putting them on screen.

use super::*;

/// How long meshing may take each frame: a world arriving all at once fills in over a few
/// frames, nearest first, instead of stopping the game. (At least one chunk always goes.)
#[cfg(not(target_arch = "wasm32"))]
const MESH_BUDGET_MS: u128 = 8;
#[cfg(target_arch = "wasm32")]
const MESH_BUDGET_MS: u128 = 5;

/// Rebuild the meshes of chunks that changed, nearest to you first, within a budget.
pub fn remesh(
    mut w: ResMut<WorldClient>,
    blocks: Res<crate::blocks::Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut backlog: Local<(usize, f64, f64, u32)>,
) {
    if !w.ready || w.dirty.is_empty() {
        return;
    }
    let Some(pal) = blocks.palette.as_ref() else { return };
    let here = ChunkKey::of(w.body.pos.map(|v| v.floor() as i32));
    let mut keys: Vec<ChunkKey> = w.dirty.iter().copied().collect();
    let d2 = |k: &ChunkKey| (0..3).map(|i| (k.0[i] - here.0[i]).pow(2)).sum::<i32>();
    keys.sort_by_key(d2);
    if backlog.0 == 0 {
        *backlog = (keys.len(), w.now(), 0.0, 0);
    }
    let started = Instant::now();
    let mut grid = Vec::new();
    let mut done = 0;
    for k in keys {
        if done > 0 && started.elapsed().as_millis() >= MESH_BUDGET_MS {
            break;
        }
        w.dirty.remove(&k);
        done += 1;
        if let Some(e) = w.meshes.remove(&k) {
            commands.entity(e).try_despawn();
        }
        // (a chunk the server hasn't got may still hold your own pending blocks)
        if w.view.confirmed.chunk(k).is_none() && w.view.pending() == 0 {
            continue;
        }
        if let Some(e) = spawn_chunk(&w, k, pal, &blocks, &mut meshes, &mut commands, &mut grid) {
            w.meshes.insert(k, e);
        }
    }
    backlog.2 += started.elapsed().as_secs_f64();
    backlog.3 += 1;
    if w.dirty.is_empty() {
        let (count, since, work, frames) = std::mem::take(&mut *backlog);
        if count > 16 {
            crate::note!(
                "[world] meshed {count} chunks: {:.0} ms of work over {frames} frames ({:.0} ms)",
                work * 1000.0,
                (w.now() - since) * 1000.0
            );
        }
    }
}

pub(super) fn spawn_chunk(
    w: &WorldClient,
    k: ChunkKey,
    pal: &crate::blocks::Palette,
    blocks: &crate::blocks::Blocks,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
    grid: &mut Vec<u16>,
) -> Option<Entity> {
    let o = IVec3::from(k.origin());
    let n = vox_core::CHUNK;
    // the chunk and a block all round it, copied out once (chunk by chunk, not block by
    // block): the mesher looks at each block's neighbours many times over
    let side = (n + 2) as usize;
    let at = |l: IVec3| (l.y as usize * side + l.z as usize) * side + l.x as usize;
    w.view.copy_region((o - 1).to_array(), n + 2, grid);
    let mut cells = Vec::new();
    for y in -1..=n {
        for z in -1..=n {
            for x in -1..=n {
                let i = at(IVec3::new(x, y, z) + 1);
                let b = w.local(grid[i]);
                grid[i] = b;
                if b != 0 && (0..n).contains(&x) && (0..n).contains(&y) && (0..n).contains(&z) {
                    cells.push((o + IVec3::new(x, y, z), b));
                }
            }
        }
    }
    if cells.is_empty() {
        return None;
    }
    let get = |p: IVec3| {
        let l = p - o + 1;
        if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(n + 2)).all() {
            grid[at(l)]
        } else {
            w.local(w.view.get(p.to_array()))
        }
    };
    let crate::blocks::Meshes { opaque, glass, water, flame } = crate::blocks::mesh_cells(cells, get, pal, &blocks.rects, true);
    let e = commands
        .spawn((
            ChunkMesh,
            Transform::default(),
            Visibility::default(),
            Name::new(format!("chunk {:?}", k.0)),
        ))
        .id();
    commands.entity(e).with_children(|c| {
        if opaque.faces() > 0 {
            // (its data lives only on the GPU, so its bounds are given, not worked out from it)
            let aabb = opaque.bounds();
            let mut e = c.spawn((Mesh3d(meshes.add(opaque.into_packed_mesh())), MeshMaterial3d(blocks.world_solid.clone()), crate::NoBevel));
            if let Some(b) = aabb {
                e.insert(b);
            }
        }
        if glass.faces() > 0 {
            c.spawn((Mesh3d(meshes.add(glass.into_mesh())), MeshMaterial3d(blocks.glass.clone()), crate::NoBevel));
        }
        if flame.faces() > 0 {
            c.spawn((Mesh3d(meshes.add(flame.into_mesh())), MeshMaterial3d(blocks.flame.clone()), crate::NoBevel));
        }
        if water.faces() > 0 {
            c.spawn((
                Mesh3d(meshes.add(water.into_mesh())),
                MeshMaterial3d(blocks.water.clone()),
                bevy::light::NotShadowCaster,
                crate::NoBevel,
            ));
        }
    });
    Some(e)
}
