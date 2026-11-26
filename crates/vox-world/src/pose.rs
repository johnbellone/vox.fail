//! Avatars over the network: a model's rig (its nodes in a fixed order), a stream of poses as
//! they arrive and the clock that plays them back, and writing a blend of two poses onto a
//! model. Shared by the show's networking (host, guest, viewers) and by worlds' players.

use std::collections::VecDeque;

use bevy::{
    ecs::query::QueryFilter,
    gltf::{Gltf, GltfNode},
    prelude::*,
    world_serialization::WorldAssetRoot,
};

use crate::proto::{self, Pose};

/// The simulation's tick rate: poses are stamped in these.
pub const HZ: f64 = 120.0;
/// How far behind the sender a pose is shown.
pub const DELAY: f64 = 0.1;

/// Face parts every character carries as children of `Head`, toggled by scale.
pub const FACE_PARTS: [&str; 23] = [
    "eyes_open",
    "eyes_shut",
    "eyes_x",
    "eyes_wide",
    "eyes_squint",
    "eyes_left",
    "eyes_right",
    "eyes_up",
    "brows_normal",
    "brows_angry",
    "brows_raised",
    "brows_worried",
    "mouth_flat",
    "mouth_grin",
    "mouth_open",
    "mouth_grit",
    "mouth_pout",
    "mouth_sad",
    "mouth_o",
    "mouth_smirk",
    "mouth_tongue",
    "mouth_frown",
    "mouth_wavy",
];

/// A blend of two transforms.
pub fn blend_transform(a: &Transform, b: &Transform, s: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, s),
        rotation: a.rotation.slerp(b.rotation, s),
        scale: a.scale.lerp(b.scale, s),
    }
}

/// With `BLOCK_AVATAR_NET_DEBUG=1`, log what arrives (to stderr).
pub fn debug(f: impl FnOnce() -> String) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("BLOCK_AVATAR_NET_DEBUG").is_some()) {
        eprintln!("net: {}", f());
    }
}

/// A model's nodes in pose order, with their rest transforms from the model file.
pub struct Rig {
    pub hash: u32,
    pub entities: Vec<Option<Entity>>,
    pub rest: Vec<Transform>,
    /// (face part entity, its bit in the face mask)
    pub faces: Vec<(Entity, u32)>,
}

impl Rig {
    pub fn complete(&self) -> bool {
        self.entities.iter().all(|e| e.is_some())
    }
}

/// Reads a model's node names and rest transforms, and finds its entities under `root`.
pub fn rig(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    root: Entity,
    children: &Query<&Children>,
    names: &Query<&Name>,
) -> Option<Rig> {
    let mut named: Vec<(String, Transform)> = gltf
        .named_nodes
        .iter()
        .filter(|(n, _)| !FACE_PARTS.contains(&n.as_ref()))
        .filter_map(|(n, h)| nodes.get(h).map(|g| (n.to_string(), g.transform)))
        .collect();
    named.sort_by(|a, b| a.0.cmp(&b.0));
    let (order, hash) = proto::node_order(named.iter().map(|n| n.0.clone()).collect());
    let mut entities = vec![None; order.len()];
    let mut faces = vec![];
    for e in children.iter_descendants(root) {
        let Ok(n) = names.get(e) else { continue };
        if let Ok(i) = order.binary_search_by(|o| o.as_str().cmp(n.as_str())) {
            entities[i] = Some(e);
        } else if let Some(i) = FACE_PARTS.iter().position(|p| *p == n.as_str()) {
            faces.push((e, 1u32 << i));
        }
    }
    if entities.iter().all(|e| e.is_none()) {
        return None; // not spawned yet
    }
    Some(Rig {
        hash,
        entities,
        rest: named.into_iter().map(|n| n.1).collect(),
        faces,
    })
}

/// One avatar's poses as they arrive, and the clock that picks which to show.
#[derive(Default)]
pub struct Stream {
    pub poses: VecDeque<Pose>,
    pub clock: Playout,
    pub hello: Option<proto::Hello>,
    /// the tick being shown
    pub shown: f64,
}

impl Stream {
    pub fn push(&mut self, p: Pose, arrived: f64) {
        self.clock.heard(p.tick as f64 / HZ, arrived);
        let pos = self.poses.partition_point(|q| q.tick < p.tick);
        if self.poses.get(pos).is_none_or(|q| q.tick != p.tick) {
            self.poses.insert(pos, p);
        }
        while self.poses.len() > 120 {
            self.poses.pop_front();
        }
    }

    /// The poses either side of `tick` and how far between them, dropping older ones.
    pub fn around(&mut self, tick: f64) -> Option<(Pose, Pose, f32)> {
        let after = self.poses.partition_point(|p| (p.tick as f64) <= tick);
        for _ in 0..after.saturating_sub(2) {
            self.poses.pop_front();
        }
        let after = self.poses.partition_point(|p| (p.tick as f64) <= tick);
        let (a, b) = match (after.checked_sub(1).and_then(|i| self.poses.get(i)), self.poses.get(after)) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, a),
            (None, Some(b)) => (b, b),
            (None, None) => return None,
        };
        let s = if b.tick > a.tick {
            ((tick - a.tick as f64) / (b.tick - a.tick) as f64).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        Some((a.clone(), b.clone(), s))
    }

    pub fn clear(&mut self) {
        self.poses.clear();
        self.hello = None;
    }
}

/// An avatar drawn from the network: the guest on a host's or viewer's screen, the host on a
/// guest's. The entity is where it stands; its model hangs below it.
#[derive(Component)]
pub struct Remote {
    pub who: u8,
    pub model: String,
    pub gltf: Handle<Gltf>,
    pub inner: Option<Entity>,
    pub rig: Option<Rig>,
}

/// `who` for a player in a Vox world (they have their own streams, in `world`).
pub const WORLD_PLAYER: u8 = 255;

impl Remote {
    /// A world player's avatar (the world module plays their poses).
    pub fn world_player(model: String, gltf: Handle<Gltf>) -> Remote {
        Remote { who: WORLD_PLAYER, model, gltf, inner: None, rig: None }
    }
}

// ------------------------------------------------------------------------------------------
// Playout clock

/// Turns pose arrival times into a smooth "sender time to show now", [`DELAY`] behind.
#[derive(Default, Debug)]
pub struct Playout {
    /// sender time minus local time, from the fastest recent arrivals
    offset: Option<f64>,
    /// sender time being shown
    play: Option<f64>,
}

impl Playout {
    /// A pose for sender time `sent` arrived at local time `now`.
    pub fn heard(&mut self, sent: f64, now: f64) {
        let o = sent - now;
        self.offset = Some(match self.offset {
            None => o,
            // the earliest-arriving poses carry the least delay; drift down slowly so a lasting
            // rise in delay is followed within a few seconds
            Some(p) if o > p => o,
            Some(p) => p - (p - o).min(0.0005),
        });
    }

    /// Sender time to show at local time `now`, a frame of `dt` after the last call. Runs up to
    /// 1% fast or slow to close a gap; jumps only if more than 250 ms off.
    pub fn at(&mut self, now: f64, dt: f64) -> Option<f64> {
        let target = now + self.offset? - DELAY;
        let p = match self.play {
            None => target,
            Some(p) => {
                let p = p + dt;
                let err = target - p;
                if err.abs() > 0.25 {
                    target
                } else {
                    p + err.clamp(-0.01 * dt, 0.01 * dt)
                }
            }
        };
        self.play = Some(p);
        Some(p)
    }
}

/// A pose of `rig` as it stands now.
pub fn capture(
    tick: u64,
    who: u8,
    root: Entity,
    rig: &Rig,
    camera: Option<Transform>,
    tf: &Query<&Transform>,
) -> Pose {
    let now: Vec<Transform> = rig
        .entities
        .iter()
        .zip(&rig.rest)
        .map(|(e, r)| e.and_then(|e| tf.get(e).ok().copied()).unwrap_or(*r))
        .collect();
    let face = rig
        .faces
        .iter()
        .filter(|(e, _)| tf.get(*e).is_ok_and(|t| t.scale.x > 0.5))
        .fold(0, |m, (_, bit)| m | bit);
    Pose {
        tick,
        who,
        root: proto::pack(&tf.get(root).copied().unwrap_or_default()),
        camera: camera.as_ref().map(proto::pack),
        face,
        nodes: proto::diff(&rig.rest, &now),
    }
}

/// Loads a network avatar's model and finds its rig once it has spawned.
pub fn load_remotes(
    mut remotes: Query<(Entity, &mut Remote)>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut commands: Commands,
) {
    for (e, mut r) in &mut remotes {
        if r.rig.as_ref().is_some_and(|g| g.complete()) {
            continue;
        }
        let Some(gltf) = gltfs.get(&r.gltf) else { continue };
        match r.inner {
            None => {
                let Some(scene) = gltf.default_scene.clone().or_else(|| gltf.scenes.first().cloned()) else {
                    continue;
                };
                let inner = commands.spawn((WorldAssetRoot(scene), Transform::default(), ChildOf(e))).id();
                r.inner = Some(inner);
            }
            Some(inner) => {
                if let Some(rig) = rig(gltf, &nodes, inner, &children, &names) {
                    debug(|| format!("remote {} rig: {} nodes", r.model, rig.entities.len()));
                    r.rig = Some(rig);
                }
            }
        }
    }
}

/// Write a blend of two poses onto a rig.
pub fn put<F: QueryFilter>(
    rig: &Rig,
    root: Entity,
    a: &Pose,
    b: &Pose,
    s: f32,
    tf: &mut Query<&mut Transform, F>,
) {
    let pa = proto::apply(&rig.rest, &a.nodes);
    let pb = proto::apply(&rig.rest, &b.nodes);
    for ((e, x), y) in rig.entities.iter().zip(&pa).zip(&pb) {
        if let Some(mut t) = e.and_then(|e| tf.get_mut(e).ok()) {
            *t = blend_transform(x, y, s);
        }
    }
    if let Ok(mut t) = tf.get_mut(root) {
        *t = blend_transform(&proto::unpack(&a.root), &proto::unpack(&b.root), s);
    }
    // face parts switch at the pose's own tick
    let mask = if s < 1.0 { a.face } else { b.face };
    for (e, bit) in &rig.faces {
        if let Ok(mut t) = tf.get_mut(*e) {
            t.scale = if mask & bit != 0 { Vec3::ONE } else { Vec3::ZERO };
        }
    }
}

