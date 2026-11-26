//! Meshing the world's chunks and putting them on screen.

use super::*;

/// Rebuild the meshes of chunks that changed.
pub fn remesh(
    mut w: ResMut<WorldClient>,
    blocks: Res<crate::blocks::Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    if !w.ready || w.dirty.is_empty() {
        return;
    }
    let Some(pal) = blocks.palette.as_ref() else { return };
    let keys: Vec<ChunkKey> = w.dirty.drain().collect();
    let started = Instant::now();
    let count = keys.len();
    for k in keys {
        if let Some(e) = w.meshes.remove(&k) {
            commands.entity(e).try_despawn();
        }
        // (a chunk the server hasn't got may still hold your own pending blocks)
        if w.view.confirmed.chunk(k).is_none() && w.view.pending() == 0 {
            continue;
        }
        if let Some(e) = spawn_chunk(&w, k, pal, &blocks, &mut meshes, &mut commands) {
            w.meshes.insert(k, e);
        }
    }
    if count > 16 {
        crate::note!("[world] meshed {count} chunks in {} ms", started.elapsed().as_millis());
    }
}

pub(super) fn spawn_chunk(
    w: &WorldClient,
    k: ChunkKey,
    pal: &crate::blocks::Palette,
    blocks: &crate::blocks::Blocks,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) -> Option<Entity> {
    let o = IVec3::from(k.origin());
    let n = vox_core::CHUNK;
    // the chunk and a block all round it, copied out once: the mesher looks at each block's
    // neighbours many times over
    let side = (n + 2) as usize;
    let at = |l: IVec3| (l.y as usize * side + l.z as usize) * side + l.x as usize;
    let mut grid = vec![0u16; side * side * side];
    let mut cells = Vec::new();
    for y in -1..=n {
        for z in -1..=n {
            for x in -1..=n {
                let c = o + IVec3::new(x, y, z);
                let b = w.local(w.view.get(c.to_array()));
                grid[at(IVec3::new(x, y, z) + 1)] = b;
                if b != 0 && (0..n).contains(&x) && (0..n).contains(&y) && (0..n).contains(&z) {
                    cells.push((c, b));
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
    let crate::blocks::Meshes { opaque, glass, water, flame } = crate::blocks::mesh_cells(cells, get, pal, &blocks.rects);
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
            c.spawn((Mesh3d(meshes.add(opaque.into_mesh())), MeshMaterial3d(blocks.world_solid.clone()), crate::NoBevel));
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
