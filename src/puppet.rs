//! Puppets: small extra characters for the show (raid crowds, the tug-of-war, the press row).
//! Spawn an entity with `Puppet::new(model)`, a Transform and (optionally) a parent; set
//! `want` to a clip name and `face` to an expression. They load, animate and keep a tidy face
//! (only one pair of eyes, one mouth) on their own.

use std::{collections::HashMap, time::Duration};

use bevy::{
    gltf::Gltf,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

/// Clips puppets use; the rest of the model's clips aren't loaded into their graph.
const CLIPS: &[&str] = &[
    "Idle",
    "Run",
    "Walk",
    "Victory",
    "Jump",
    "Rally chant",
    "Rally dance",
    "Brace",
    "Knockout",
    "Hurt",
    "Talk",
    "Emotes",
];

#[derive(Component)]
pub struct Puppet {
    gltf: Handle<Gltf>,
    graph: Option<Handle<AnimationGraph>>,
    nodes: HashMap<String, AnimationNodeIndex>,
    player: Option<Entity>,
    /// clip to play (looped)
    pub want: String,
    playing: String,
    /// eyes, brows, mouth
    pub face: [&'static str; 3],
    pub speed: f32,
    /// extra head turn on top of the animation (x nod, y turn, z tilt)
    pub head: Vec3,
    pub head_entity: Option<Entity>,
}

impl Puppet {
    pub fn new(model: &str, assets: &AssetServer) -> Puppet {
        Puppet {
            gltf: assets.load::<Gltf>(model.to_string()),
            graph: None,
            nodes: HashMap::new(),
            player: None,
            want: "Idle".into(),
            playing: String::new(),
            face: ["eyes_open", "brows_normal", "mouth_grin"],
            speed: 1.0,
            head: Vec3::ZERO,
            head_entity: None,
        }
    }
}

#[derive(Component)]
pub struct PuppetPart {
    owner: Entity,
    name: &'static str,
}

pub fn load(
    mut q: Query<(Entity, &mut Puppet), Without<WorldAssetRoot>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut commands: Commands,
) {
    for (e, mut p) in &mut q {
        if p.graph.is_some() {
            continue;
        }
        let Some(g) = gltfs.get(&p.gltf) else {
            continue;
        };
        let mut graph = AnimationGraph::new();
        for (name, h) in g.named_animations.iter() {
            if CLIPS.contains(&name.as_ref()) {
                let n = graph.add_clip(h.clone(), 1.0, graph.root);
                p.nodes.insert(name.to_string(), n);
            }
        }
        p.graph = Some(graphs.add(graph));
        if let Some(scene) = g
            .default_scene
            .clone()
            .or_else(|| g.scenes.first().cloned())
        {
            commands
                .entity(e)
                .insert(WorldAssetRoot(scene))
                .observe(ready);
        }
    }
}

fn ready(
    ev: On<WorldInstanceReady>,
    mut q: Query<&mut Puppet>,
    children: Query<&Children>,
    names: Query<&Name>,
    players: Query<(), With<AnimationPlayer>>,
    mut commands: Commands,
) {
    let Ok(mut p) = q.get_mut(ev.entity) else {
        return;
    };
    let Some(graph) = p.graph.clone() else { return };
    for c in children.iter_descendants(ev.entity) {
        if let Ok(n) = names.get(c) {
            if let Some(part) = crate::FACE_PARTS.iter().find(|x| **x == n.as_str()) {
                commands.entity(c).insert(PuppetPart {
                    owner: ev.entity,
                    name: part,
                });
            } else if n.as_str() == "Head" {
                p.head_entity = Some(c);
            }
        }
        if players.get(c).is_ok() {
            commands.entity(c).insert((
                AnimationGraphHandle(graph.clone()),
                AnimationTransitions::new(),
            ));
            p.player = Some(c);
        }
    }
}

pub fn animate(
    mut q: Query<&mut Puppet>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for mut p in &mut q {
        let Some(pe) = p.player else { continue };
        if p.want == p.playing {
            continue;
        }
        let Some(node) = p
            .nodes
            .get(&p.want)
            .or_else(|| p.nodes.get("Idle"))
            .copied()
        else {
            continue;
        };
        if let Ok((mut pl, mut tr)) = players.get_mut(pe) {
            let speed = p.speed;
            tr.play(&mut pl, node, Duration::from_millis(200))
                .repeat()
                .set_speed(speed);
            p.playing = p.want.clone();
        }
    }
}

/// After animation: one face per puppet, and any extra head turn.
pub fn faces(
    owners: Query<&Puppet>,
    mut parts: Query<(&PuppetPart, &mut Transform)>,
    mut heads: Query<&mut Transform, Without<PuppetPart>>,
) {
    for (pp, mut t) in &mut parts {
        let Ok(p) = owners.get(pp.owner) else {
            continue;
        };
        t.scale = if p.face.contains(&pp.name) {
            Vec3::ONE
        } else {
            Vec3::ZERO
        };
    }
    for p in &owners {
        if p.head == Vec3::ZERO {
            continue;
        }
        if let Some(mut t) = p.head_entity.and_then(|h| heads.get_mut(h).ok()) {
            t.rotation *= Quat::from_euler(EulerRot::YXZ, p.head.y, p.head.x, p.head.z);
        }
    }
}

/// A random character for crowds (not the one you're playing).
pub fn pick_model(models: &[(String, String)], avoid: &str, seed: u32) -> String {
    let pool: Vec<&(String, String)> = models.iter().filter(|m| m.1 != avoid).collect();
    if pool.is_empty() {
        return avoid.to_string();
    }
    pool[(crate::hash01(seed, 0.37) * pool.len() as f32) as usize % pool.len()]
        .1
        .clone()
}
