//! Graph nodes in edit mode: every Node entity is drawn as a box outline of its footprint, and the
//! selected node's preview, the blocks its recipe would build, is drawn as a translucent ghost.
//!
//! The preview is expanded here with the same code the server builds with
//! ([`vox_graph::entity`] and [`vox_graph::expand`]) and the world's own palette, so what the
//! ghost shows is what Build places. `expand` is one call that cannot be split, so its cost is
//! bounded by caching: it runs only when the node's seq or the palette changes ([`stale`]), never
//! per frame. The ghost's mesh is then built over several frames within a time budget
//! ([`MeshBuild`]), as the client's chunk remeshing is, so a large node does not stall a frame.
//!
//! Over the build cap ([`vox_graph::MAX_BUILD_CELLS`]) there is no ghost and the page is told
//! the block count and that it is too big ([`Event::NodePreview`]); a Build of such a node is
//! refused here, before it is sent ([`build_refusal`]), and the refusal names the count and the cap.

use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use vox_core::entity::{Component, EntityId};
use vox_graph::{ExpandError, MAX_BUILD_CELLS};

use super::bridge::Event;
use super::{EditorOutbox, EditorState};
use crate::client::WorldClient;
use crate::entities::WorldEntities;

/// The most a frame spends building the ghost's mesh: 5 ms on the desktop, 3 ms in the browser,
/// as the chunk remeshing does.
fn budget_secs() -> f32 {
    if cfg!(target_arch = "wasm32") { 0.003 } else { 0.005 }
}

/// Cells meshed between looks at the clock.
const CELLS_PER_CHECK: usize = 256;

/// What a preview was made from: the node (and the seq it had) and the world's palette. When any
/// of these differ the preview is out of date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub node: u32,
    pub seq: u64,
    pub palette: u64,
}

/// Whether the preview made from `held` must be made again for `now`.
pub fn stale(held: Option<&Key>, now: &Key) -> bool {
    held != Some(now)
}

/// A hash of a world's block names in order, to tell when the palette changed without keeping
/// or comparing the names every frame (FNV-1a, with a separator so `ab,c` and `a,bc` differ).
pub fn palette_hash<'a>(names: impl Iterator<Item = &'a str>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for name in names {
        for b in name.bytes().chain(std::iter::once(0xff)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// What expanding a node gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// the block positions it builds
    Cells(Vec<[i32; 3]>),
    /// over the build cap; `cells` is the block count it would build
    TooBig { cells: usize },
    /// the node or its recipe does not build (the page's inspector says why)
    Invalid,
    /// the world's palette is not one vox-art takes
    Unavailable,
}

/// Expands node entity `id` with the world's block names, as the server would.
pub fn preview(id: u32, node: &vox_core::entity::Node, names: &[String]) -> Outcome {
    let Some((palette, _)) = vox_graph::entity::build_palette(names) else { return Outcome::Unavailable };
    let Ok(graph) = vox_graph::entity::from_entity(id, node) else { return Outcome::Invalid };
    match vox_graph::expand(&graph, &palette) {
        Ok(cells) => Outcome::Cells(cells.into_iter().map(|(at, _)| at).collect()),
        Err(ExpandError::TooBig { cells }) => Outcome::TooBig { cells },
        Err(ExpandError::Invalid(_)) => Outcome::Invalid,
    }
}

/// The page's event for an outcome: the block count, none when the node does not build.
pub fn event_for(node: u32, outcome: &Outcome) -> Event {
    match outcome {
        Outcome::Cells(cells) => Event::NodePreview { node, blocks: u32::try_from(cells.len()).ok(), too_big: false },
        Outcome::TooBig { cells } => Event::NodePreview { node, blocks: u32::try_from(*cells).ok(), too_big: true },
        Outcome::Invalid | Outcome::Unavailable => Event::NodePreview { node, blocks: None, too_big: false },
    }
}

/// The refusal for a Build whose preview is over the cap, to be sent to the page instead of the
/// server; None when the build may go on (the server decides everything else).
pub fn build_refusal(outcome: &Outcome) -> Option<Event> {
    let Outcome::TooBig { cells } = outcome else { return None };
    Some(Event::Refused {
        id: 0,
        r#ref: None,
        why: "TooBig".into(),
        text: format!("that builds {cells} blocks; at most {MAX_BUILD_CELLS}"),
    })
}

/// A ghost mesh being built a few cells at a time: a face for each side of a cell that has no
/// cell beside it.
pub struct MeshBuild {
    cells: Vec<[i32; 3]>,
    set: HashSet<[i32; 3]>,
    next: usize,
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    idx: Vec<u32>,
}

impl MeshBuild {
    pub fn new(cells: Vec<[i32; 3]>) -> MeshBuild {
        let set = cells.iter().copied().collect();
        MeshBuild { cells, set, next: 0, pos: Vec::new(), nrm: Vec::new(), idx: Vec::new() }
    }

    /// Meshes up to `limit` more cells; true once every cell is done.
    pub fn step(&mut self, limit: usize) -> bool {
        let end = self.next.saturating_add(limit).min(self.cells.len());
        for i in self.next..end {
            let cell = self.cells[i];
            for dir in 0..6 {
                let axis = dir / 2;
                let positive = dir % 2 == 0;
                let mut beside = cell;
                let Some(moved) = beside[axis].checked_add(if positive { 1 } else { -1 }) else { continue };
                beside[axis] = moved;
                if self.set.contains(&beside) {
                    continue;
                }
                self.face(cell, axis, positive);
            }
        }
        self.next = end;
        self.next >= self.cells.len()
    }

    fn face(&mut self, cell: [i32; 3], axis: usize, positive: bool) {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut base = cell.map(|c| c as f32);
        if positive {
            base[axis] += 1.0;
        }
        let at = |du: f32, dv: f32| {
            let mut p = base;
            p[u] += du;
            p[v] += dv;
            p
        };
        let mut n = [0.0; 3];
        n[axis] = if positive { 1.0 } else { -1.0 };
        let first = self.pos.len() as u32;
        self.pos.extend([at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)]);
        self.nrm.extend([n; 4]);
        // drawn with culling off, so the winding does not matter
        self.idx.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    /// Faces made so far.
    pub fn faces(&self) -> usize {
        self.idx.len() / 6
    }

    /// The mesh, for the GPU only; None when there is no face.
    pub fn finish(self) -> Option<Mesh> {
        if self.idx.is_empty() {
            return None;
        }
        Some(
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
                .with_inserted_indices(Indices::U32(self.idx)),
        )
    }
}

/// Where the selected node's preview stands.
#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    /// meshing, a few cells a frame
    Meshing(MeshBuild),
    /// the mesh is made (or there is nothing to draw)
    Done,
}

/// The preview of the selected node, kept in [`EditorState`]: what it was made from, and the
/// mesh being built.
#[derive(Default)]
pub struct Preview {
    key: Option<Key>,
    stage: Stage,
    /// the cached outcome's block count when it is over the cap, for a Build of the same node and seq
    too_big: Option<usize>,
}

impl Preview {
    /// Forget it (the world changed, or nothing is selected), so selecting again tells the page.
    pub fn clear(&mut self) {
        *self = Preview::default();
    }

    /// The block count of the preview held for exactly `key`, when it is over the cap.
    fn over_cap(&self, key: &Key) -> Option<usize> {
        if self.key.as_ref() == Some(key) { self.too_big } else { None }
    }
}

/// The world's block names in order.
fn names(w: &WorldClient) -> Vec<String> {
    w.block_ids().map(|id| w.block_name(id).to_string()).collect()
}

fn hash_of(w: &WorldClient) -> u64 {
    palette_hash(w.block_ids().map(|id| w.block_name(id)))
}

/// The refusal for Build of `node`, if its preview is over the cap. The cached preview is used
/// when it is of this node as it is now; otherwise it is expanded here. That is one synchronous
/// expand, bounded by the recipe caps: acceptable because a Build is rare, and cheaper than
/// keeping a preview for every node the page might build.
pub fn refuse_build(state: &EditorState, w: &WorldClient, entities: &WorldEntities, node: u32) -> Option<Event> {
    let (seq, Component::Node(n)) = entities.map.get(&EntityId(node))? else { return None };
    let key = Key { node, seq: *seq, palette: hash_of(w) };
    if let Some(cells) = state.preview.over_cap(&key) {
        return build_refusal(&Outcome::TooBig { cells });
    }
    if state.preview.key.as_ref() == Some(&key) {
        return None;
    }
    build_refusal(&preview(node, n, &names(w)))
}

/// Draws the outline of every node's footprint, the selected one brighter.
pub fn outlines(w: Res<WorldClient>, entities: Res<WorldEntities>, state: Res<EditorState>, mut gizmos: Gizmos) {
    if !w.active() || !w.ready {
        return;
    }
    let size = w.block_size();
    for (id, (_, c)) in &entities.map {
        let Component::Node(n) = c else { continue };
        let on = state.selected == Some(id.0);
        let colour = if on { Color::srgba(1.0, 0.85, 0.3, 0.95) } else { Color::srgba(0.7, 0.8, 1.0, 0.5) };
        let side = n.size.map(f32::from);
        let centre = [0, 1, 2].map(|i| (n.at[i] as f32 + side[i] * 0.5) * size);
        gizmos.cube(Transform::from_translation(Vec3::from(centre)).with_scale(Vec3::from(side) * size), colour);
    }
}

/// The ghost's entity and what it wears.
#[derive(Default)]
pub struct Look {
    ghost: Option<Entity>,
    material: Option<Handle<StandardMaterial>>,
    worn: Option<Handle<Mesh>>,
}

/// Keeps the selected node's preview: expands it when its seq or the palette changes, tells the
/// page the block count, and builds the ghost's mesh within the frame budget.
#[allow(clippy::too_many_arguments)]
pub fn node_preview(
    w: Res<WorldClient>,
    entities: Res<WorldEntities>,
    mut state: ResMut<EditorState>,
    mut outbox: ResMut<EditorOutbox>,
    mut look: Local<Look>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut shown: Query<&mut Visibility>,
) {
    if look.ghost.is_none_or(|e| !shown.contains(e)) {
        let material = mats.add(StandardMaterial {
            base_color: Color::srgba(0.8, 0.9, 1.0, 0.35),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        });
        let empty = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD));
        look.ghost = Some(
            commands
                .spawn((Mesh3d(empty), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, NotShadowCaster))
                .id(),
        );
        look.material = Some(material);
        look.worn = None;
        return;
    }
    let Some(id) = look.ghost else { return };
    let Ok(mut visible) = shown.get_mut(id) else { return };

    let selected = state.selected.and_then(|s| match entities.map.get(&EntityId(s)) {
        Some((seq, Component::Node(n))) => Some((s, *seq, n)),
        _ => None,
    });
    let (Some((node, seq, n)), true) = (selected, w.active() && w.ready) else {
        *visible = Visibility::Hidden;
        state.preview.clear();
        return;
    };

    let now = Key { node, seq, palette: hash_of(&w) };
    if stale(state.preview.key.as_ref(), &now) {
        let outcome = preview(node, n, &names(&w));
        outbox.0.push(event_for(node, &outcome));
        let p = &mut state.preview;
        p.key = Some(now);
        p.too_big = match outcome {
            Outcome::TooBig { cells } => Some(cells),
            _ => None,
        };
        p.stage = match outcome {
            Outcome::Cells(cells) if !cells.is_empty() => Stage::Meshing(MeshBuild::new(cells)),
            _ => Stage::Done,
        };
        // the old preview is not shown while the new one is made
        *visible = Visibility::Hidden;
        if let Some(old) = look.worn.take() {
            meshes.remove(old.id());
            commands.entity(id).insert(Mesh3d(meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD))));
        }
    }

    if let Stage::Meshing(build) = &mut state.preview.stage {
        let started = Instant::now();
        let mut done = false;
        while !done && started.elapsed().as_secs_f32() < budget_secs() {
            done = build.step(CELLS_PER_CHECK);
        }
        if done && let Stage::Meshing(build) = std::mem::take(&mut state.preview.stage) {
            if let Some(mesh) = build.finish() {
                let handle = meshes.add(mesh);
                commands.entity(id).insert(Mesh3d(handle.clone()));
                look.worn = Some(handle);
            }
            state.preview.stage = Stage::Done;
        }
    }
    let size = w.block_size();
    commands.entity(id).insert(Transform::from_scale(Vec3::splat(size)));
    *visible = if look.worn.is_some() && matches!(state.preview.stage, Stage::Done) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(node: u32, seq: u64, palette: u64) -> Key {
        Key { node, seq, palette }
    }

    #[test]
    fn editor_preview_is_made_again_only_when_its_inputs_change() {
        let k = key(4, 10, 7);
        assert!(stale(None, &k));
        assert!(!stale(Some(&k), &k));
        assert!(stale(Some(&k), &key(4, 11, 7)));
        assert!(stale(Some(&k), &key(5, 10, 7)));
        assert!(stale(Some(&k), &key(4, 10, 8)));
    }

    #[test]
    fn editor_palette_hash_follows_names_and_order() {
        let h = |v: &[&str]| palette_hash(v.iter().copied());
        assert_eq!(h(&["stone", "brick"]), h(&["stone", "brick"]));
        assert_ne!(h(&["stone", "brick"]), h(&["brick", "stone"]));
        assert_ne!(h(&["ab", "c"]), h(&["a", "bc"]));
        assert_ne!(h(&["stone"]), h(&["stone", "brick"]));
    }

    fn node(recipe: Option<&str>, size: [u16; 3]) -> vox_core::entity::Node {
        vox_core::entity::Node {
            kind: "house".into(),
            detail: 3,
            at: [10, 0, 20],
            size,
            parent: None,
            recipe: recipe.map(String::from),
            seed: 1,
        }
    }

    fn palette() -> Vec<String> {
        ["brick", "wood"].map(String::from).to_vec()
    }

    /// A hollow brick box with a wooden gable roof and a door: 482 voxels (as vox-graph's own
    /// expansion test pins).
    const HOUSE: &str = r#"{
        "version": 1, "scale": "world", "seed": 1, "size": [10, 12, 8],
        "ops": [
            { "box": { "from": [0, 0, 0], "to": [9, 5, 7], "mat": "brick", "tone": 0, "hollow": true } },
            { "roof": { "over": [0, 0, 9, 7], "at_y": 6, "style": "gable", "along_x": true, "mat": "wood", "tone": 0 } },
            { "cut": { "from": [4, 0, 0], "to": [5, 2, 0] } }
        ]
    }"#;

    #[test]
    fn editor_preview_expands_with_the_servers_conversion() {
        // a node with no recipe builds nothing; a bad kind or a bad recipe does not build
        assert_eq!(preview(1, &node(None, [3, 3, 3]), &palette()), Outcome::Cells(Vec::new()));
        let mut bad = node(None, [3, 3, 3]);
        bad.kind = "castle".into();
        assert_eq!(preview(1, &bad, &palette()), Outcome::Invalid);
        assert_eq!(preview(1, &node(Some("not json"), [3, 3, 3]), &palette()), Outcome::Invalid);
        // a recipe of the footprint's size builds at the footprint
        let Outcome::Cells(cells) = preview(1, &node(Some(HOUSE), [10, 12, 8]), &palette()) else {
            panic!("the house builds")
        };
        assert_eq!(cells.len(), 482);
        assert!(cells.iter().all(|c| (10..20).contains(&c[0]) && (0..12).contains(&c[1]) && (20..28).contains(&c[2])));
        // a palette vox-art refuses is unavailable
        let long = vec!["x".repeat(500)];
        assert_eq!(preview(1, &node(None, [3, 3, 3]), &long), Outcome::Unavailable);
    }

    #[test]
    fn editor_build_over_the_cap_is_refused_before_sending() {
        let refusal = build_refusal(&Outcome::TooBig { cells: 36_864 }).expect("refused");
        match refusal {
            Event::Refused { id, r#ref, why, text } => {
                assert_eq!((id, r#ref, why.as_str()), (0, None, "TooBig"));
                assert_eq!(text, "that builds 36864 blocks; at most 32768");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(build_refusal(&Outcome::Cells(vec![[0, 0, 0]])).is_none());
        assert!(build_refusal(&Outcome::Invalid).is_none());
        assert!(build_refusal(&Outcome::Unavailable).is_none());
    }

    #[test]
    fn editor_the_page_hears_the_block_count() {
        assert_eq!(event_for(3, &Outcome::Cells(vec![[0; 3]; 5])), Event::NodePreview { node: 3, blocks: Some(5), too_big: false });
        assert_eq!(event_for(3, &Outcome::TooBig { cells: 36_864 }), Event::NodePreview { node: 3, blocks: Some(36_864), too_big: true });
        assert_eq!(event_for(3, &Outcome::Invalid), Event::NodePreview { node: 3, blocks: None, too_big: false });
    }

    #[test]
    fn editor_cached_cap_verdict_belongs_to_one_key() {
        let k = key(1, 2, 3);
        let mut p = Preview::default();
        assert_eq!(p.over_cap(&k), None);
        p.key = Some(k);
        p.too_big = Some(36_864);
        assert_eq!(p.over_cap(&k), Some(36_864));
        assert_eq!(p.over_cap(&key(1, 3, 3)), None);
        p.clear();
        assert_eq!(p.over_cap(&k), None);
    }

    #[test]
    fn editor_mesh_hides_the_faces_cells_share() {
        let mut one = MeshBuild::new(vec![[0, 0, 0]]);
        assert!(one.step(10));
        assert_eq!(one.faces(), 6);
        let mut two = MeshBuild::new(vec![[0, 0, 0], [1, 0, 0]]);
        assert!(two.step(10));
        assert_eq!(two.faces(), 10);
        assert!(MeshBuild::new(Vec::new()).finish().is_none());
    }

    #[test]
    fn editor_mesh_is_built_a_few_cells_at_a_time() {
        let cells: Vec<[i32; 3]> = (0..5).map(|x| [x * 2, 0, 0]).collect();
        let mut build = MeshBuild::new(cells);
        assert!(!build.step(2));
        assert_eq!(build.faces(), 12);
        assert!(!build.step(2));
        assert!(build.step(2));
        assert_eq!(build.faces(), 30);
        // past the end it stays done and adds nothing
        assert!(build.step(2));
        assert_eq!(build.faces(), 30);
    }

    #[test]
    fn editor_mesh_does_not_overflow_at_the_edge_of_the_range() {
        let mut build = MeshBuild::new(vec![[i32::MAX, i32::MIN, 0]]);
        assert!(build.step(1));
        assert!(build.faces() >= 4);
    }
}
