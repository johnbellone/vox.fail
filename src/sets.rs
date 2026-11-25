//! Block levels as sets: when a level becomes the show's set, read it, mesh it and put it in
//! place, with a sky behind.

use bevy::prelude::*;

use crate::blocks::{Blocks, LevelRoot, Meshes, level_path, load_level, mesh_level};

/// A level that just became the set: read it, mesh it, put it in place (with a sky behind).
pub fn build(
    blocks: Res<Blocks>,
    added: Query<(Entity, &LevelRoot), Added<LevelRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut av: ResMut<crate::Avatar>,
    mut commands: Commands,
) {
    for (e, root) in &added {
        let Some(pal) = blocks.palette.as_ref() else {
            av.status = blocks.status.clone();
            continue;
        };
        let cells = level_path(&root.0)
            .ok_or_else(|| format!("no level called {}", root.0))
            .and_then(|p| std::fs::read_to_string(&p).map_err(|e| e.to_string()))
            .and_then(|t| load_level(&t, pal));
        let cells = match cells {
            Ok(c) => c,
            Err(err) => {
                crate::note!("[blocks] level {}: {err}", root.0);
                av.status = format!("Level {}: {err}", root.0);
                continue;
            }
        };
        let Meshes { opaque, glass: clear, water, flame } = mesh_level(&cells, pal, &blocks.rects);
        crate::note!("[blocks] level {}: {} blocks, {} faces", root.0, cells.len(), opaque.faces() + clear.faces() + water.faces());
        commands.entity(e).with_children(|c| {
            c.spawn((Mesh3d(meshes.add(opaque.into_mesh())), MeshMaterial3d(blocks.solid.clone()), crate::bevel::NoBevel, Name::new("level")));
            if clear.faces() > 0 {
                c.spawn((Mesh3d(meshes.add(clear.into_mesh())), MeshMaterial3d(blocks.glass.clone()), crate::bevel::NoBevel, Name::new("level glass")));
            }
            if water.faces() > 0 {
                c.spawn((Mesh3d(meshes.add(water.into_mesh())), MeshMaterial3d(blocks.water.clone()), crate::bevel::NoBevel, Name::new("level water")));
            }
            if flame.faces() > 0 {
                c.spawn((Mesh3d(meshes.add(flame.into_mesh())), MeshMaterial3d(blocks.flame.clone()), crate::bevel::NoBevel, Name::new("level fire")));
            }
            // a sky: a big gradient far behind, lit by nothing
            c.spawn((
                Mesh3d(meshes.add(crate::blocks::sky_mesh())),
                MeshMaterial3d(mats.add(StandardMaterial { unlit: true, base_color: Color::WHITE, ..default() })),
                Transform::from_xyz(0.0, 4.0, -30.0),
                crate::bevel::NoBevel,
                Name::new("sky"),
            ));
        });
    }
}

