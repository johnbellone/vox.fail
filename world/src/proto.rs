//! The wire format between a host, its guest and its viewers (carried by warren-relay).
//!
//! Every binary message is one byte the relay looks at ([`DROPPABLE`] or [`RELIABLE`]) followed
//! by a postcard-encoded [`Msg`]. Poses (and later voice) are droppable: a newer one replaces a
//! lost one. Everything else is reliable.
//!
//! A pose is the avatar's final shape for one tick: every model node that differs from its
//! rest transform, packed small (a rotation in 7 bytes, a position or scale in 6), plus which
//! face parts show and where the camera is. The receiver doesn't re-run the host's animation,
//! face or reactions; it draws exactly what the host drew. Nodes are numbered by their names in
//! sorted order, which both ends read from the same model file.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const VERSION: u16 = 1;
/// First byte: may be skipped for a slow viewer (the relay's `DROPPABLE`).
pub const DROPPABLE: u8 = 0;
/// First byte: every receiver must get it (the relay's `RELIABLE`).
pub const RELIABLE: u8 = 1;

/// Snapshots a second.
pub const POSE_HZ: u64 = 30;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Msg {
    /// who's sending, and the model their poses refer to
    Hello(Hello),
    /// commands that rebuild the scene: model, background, set, lighting, what's worn
    Scene(Vec<String>),
    Pose(Pose),
    Event(Event),
    /// host to everyone: the guest on stage beside the host (their hello), or nobody
    Guest(Option<Hello>),
    Voice(Voice),
}

/// The only voice codec so far: IMA ADPCM, 16 kHz mono.
pub const ADPCM: u8 = 1;

/// 20 ms of someone's voice. Each packet carries the coder's starting state, so it decodes on
/// its own.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Voice {
    pub who: u8,
    pub seq: u32,
    pub codec: u8,
    pub samples: u16,
    pub pred: i16,
    pub index: u8,
    pub data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Hello {
    pub version: u16,
    pub name: String,
    /// model file under assets/models
    pub model: String,
    /// [`names_hash`] of the model's node names, so a mismatched model is spotted
    pub nodes: u32,
}

/// Which avatar a pose is for.
pub const HOST: u8 = 0;
pub const GUEST: u8 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Pose {
    /// the host's simulation tick (a guest uses its own)
    pub tick: u64,
    pub who: u8,
    /// the character's root (hops, crouches, where it stands)
    pub root: Packed,
    /// the main camera, so viewers see the same shot (host only)
    pub camera: Option<Packed>,
    /// bit i set: `FACE_PARTS[i]` shows
    pub face: u32,
    /// nodes that differ from rest
    pub nodes: Vec<Node>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub i: u8,
    pub rot: Option<Rot>,
    pub pos: Option<[i16; 3]>,
    pub scale: Option<[i16; 3]>,
}

/// A rotation as its three smallest components (the largest is rebuilt), 1/32767 steps.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Rot(pub u8, pub [i16; 3]);

/// A whole transform, packed.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Packed {
    pub rot: Rot,
    pub pos: [i16; 3],
    pub scale: [i16; 3],
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Event {
    pub tick: u64,
    pub what: What,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum What {
    /// a lighting cue: 0 sub, 1 raid, 2 big hit
    Cue(u8),
}

pub fn encode(msg: &Msg) -> Vec<u8> {
    let kind = match msg {
        Msg::Pose(_) | Msg::Voice(_) => DROPPABLE,
        _ => RELIABLE,
    };
    let mut out = vec![kind];
    out.extend(postcard::to_stdvec(msg).expect("encodable"));
    out
}

pub fn decode(bytes: &[u8]) -> Option<Msg> {
    postcard::from_bytes(bytes.get(1..)?).ok()
}

const ROT_SCALE: f32 = 32767.0 * std::f32::consts::SQRT_2;
/// positions in millimetres: ±32 m
const POS_SCALE: f32 = 1000.0;
/// scales in 1/1024: ±32
const SCALE_SCALE: f32 = 1024.0;

pub fn pack_rot(q: Quat) -> Rot {
    let q = q.normalize();
    let c = [q.x, q.y, q.z, q.w];
    let (big, _) = c
        .iter()
        .enumerate()
        .fold((0, -1.0f32), |b, (i, v)| if v.abs() > b.1 { (i, v.abs()) } else { b });
    let sign = if c[big] < 0.0 { -1.0 } else { 1.0 };
    let mut out = [0i16; 3];
    let mut k = 0;
    for (i, v) in c.iter().enumerate() {
        if i != big {
            out[k] = (v * sign * ROT_SCALE).round().clamp(-32767.0, 32767.0) as i16;
            k += 1;
        }
    }
    Rot(big as u8, out)
}

pub fn unpack_rot(r: Rot) -> Quat {
    let small: Vec<f32> = r.1.iter().map(|v| *v as f32 / ROT_SCALE).collect();
    let big = (1.0 - small.iter().map(|v| v * v).sum::<f32>()).max(0.0).sqrt();
    let mut c = [0.0f32; 4];
    let mut k = 0;
    for (i, slot) in c.iter_mut().enumerate() {
        if i == r.0 as usize % 4 {
            *slot = big;
        } else {
            *slot = small[k];
            k += 1;
        }
    }
    Quat::from_xyzw(c[0], c[1], c[2], c[3]).normalize()
}

fn pack3(v: Vec3, s: f32) -> [i16; 3] {
    [v.x, v.y, v.z].map(|c| (c * s).round().clamp(-32767.0, 32767.0) as i16)
}

fn unpack3(v: [i16; 3], s: f32) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) / s
}

pub fn pack(t: &Transform) -> Packed {
    Packed {
        rot: pack_rot(t.rotation),
        pos: pack3(t.translation, POS_SCALE),
        scale: pack3(t.scale, SCALE_SCALE),
    }
}

pub fn unpack(p: &Packed) -> Transform {
    Transform {
        translation: unpack3(p.pos, POS_SCALE),
        rotation: unpack_rot(p.rot),
        scale: unpack3(p.scale, SCALE_SCALE),
    }
}

/// Node names in the order poses number them, and a hash of that list.
pub fn node_order(mut names: Vec<String>) -> (Vec<String>, u32) {
    names.sort();
    names.dedup();
    let mut h: u32 = 0x811c9dc5;
    for n in &names {
        for b in n.bytes().chain(std::iter::once(0)) {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
    }
    (names, h)
}

/// The angle between two rotations, accurate for small angles (unlike `angle_between`, whose
/// `acos` in f32 can't tell anything under about 0.001 rad from zero).
pub fn angle(a: Quat, b: Quat) -> f32 {
    let d = a.inverse() * b;
    2.0 * Vec3::new(d.x, d.y, d.z).length().min(1.0).asin()
}

/// The nodes of `now` that differ from `rest` (same order), packed.
pub fn diff(rest: &[Transform], now: &[Transform]) -> Vec<Node> {
    rest.iter()
        .zip(now)
        .enumerate()
        .take(256)
        .filter_map(|(i, (r, n))| {
            let rot = (angle(r.rotation, n.rotation) > 1e-4).then(|| pack_rot(n.rotation));
            let pos = (r.translation.distance(n.translation) > 5e-4)
                .then(|| pack3(n.translation, POS_SCALE));
            let scale = (r.scale.distance(n.scale) > 1e-3).then(|| pack3(n.scale, SCALE_SCALE));
            (rot.is_some() || pos.is_some() || scale.is_some()).then_some(Node {
                i: i as u8,
                rot,
                pos,
                scale,
            })
        })
        .collect()
}

/// Rest with `nodes` applied: the full pose.
pub fn apply(rest: &[Transform], nodes: &[Node]) -> Vec<Transform> {
    let mut out = rest.to_vec();
    for n in nodes {
        let Some(t) = out.get_mut(n.i as usize) else {
            continue;
        };
        if let Some(r) = n.rot {
            t.rotation = unpack_rot(r);
        }
        if let Some(p) = n.pos {
            t.translation = unpack3(p, POS_SCALE);
        }
        if let Some(s) = n.scale {
            t.scale = unpack3(s, SCALE_SCALE);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_survive_packing() {
        let mut worst = 0.0f32;
        for i in 0..2000 {
            let f = i as f32;
            let q = Quat::from_euler(EulerRot::YXZ, f * 0.37, (f * 0.11).sin() * 1.5, f * 0.71);
            let back = unpack_rot(pack_rot(q));
            worst = worst.max(angle(q, back));
        }
        assert!(worst < 1e-4, "worst error {worst} rad");
        // the identity and its negative are the same rotation
        assert!(angle(unpack_rot(pack_rot(-Quat::IDENTITY)), Quat::IDENTITY) < 1e-4);
    }

    #[test]
    fn transforms_survive_packing() {
        let t = Transform::from_xyz(1.234, -0.5, 3.0)
            .with_rotation(Quat::from_rotation_y(0.8))
            .with_scale(Vec3::new(1.2, 0.6, 1.0));
        let b = unpack(&pack(&t));
        assert!(b.translation.distance(t.translation) < 1e-3);
        assert!(b.scale.distance(t.scale) < 1e-3);
        assert!(angle(b.rotation, t.rotation) < 1e-4);
    }

    #[test]
    fn only_changed_nodes_are_sent() {
        let rest: Vec<Transform> = (0..40).map(|i| Transform::from_xyz(i as f32 * 0.1, 0.0, 0.0)).collect();
        let mut now = rest.clone();
        now[3].rotation = Quat::from_rotation_x(0.4);
        now[7].scale = Vec3::ZERO;
        now[9].translation.y += 0.2;
        let d = diff(&rest, &now);
        assert_eq!(d.iter().map(|n| n.i).collect::<Vec<_>>(), vec![3, 7, 9]);
        assert!(d[0].rot.is_some() && d[0].pos.is_none() && d[0].scale.is_none());
        let back = apply(&rest, &d);
        for (a, b) in back.iter().zip(&now) {
            assert!(a.translation.distance(b.translation) < 1e-3);
            assert!(angle(a.rotation, b.rotation) < 1e-4);
            assert!(a.scale.distance(b.scale) < 1e-3);
        }
    }

    #[test]
    fn messages_round_trip_and_stay_small() {
        let nodes: Vec<Node> = (0..15)
            .map(|i| Node {
                i,
                rot: Some(pack_rot(Quat::from_rotation_z(i as f32 * 0.1))),
                pos: None,
                scale: None,
            })
            .chain((15..22).map(|i| Node {
                i,
                rot: None,
                pos: None,
                scale: Some([0, 0, 0]),
            }))
            .collect();
        let pose = Msg::Pose(Pose {
            tick: 123_456,
            who: HOST,
            root: pack(&Transform::from_xyz(0.0, 0.1, 0.0)),
            camera: Some(pack(&Transform::from_xyz(0.0, 1.4, 3.2))),
            face: 0b1010_0110,
            nodes,
        });
        let bytes = encode(&pose);
        assert_eq!(bytes[0], DROPPABLE);
        assert_eq!(decode(&bytes), Some(pose));
        // 15 turned bones, 7 hidden props, the root and the camera: about 7 KB/s at 30 a second
        assert!(bytes.len() < 250, "a typical pose is {} bytes", bytes.len());

        let scene = Msg::Scene(vec!["model:Dog".into(), "set:diner".into()]);
        let b = encode(&scene);
        assert_eq!(b[0], RELIABLE);
        assert_eq!(decode(&b), Some(scene));
        assert_eq!(decode(&[RELIABLE, 200, 1, 2]), None, "garbage is refused, not a panic");
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn node_order_is_sorted_and_hashed() {
        let (a, ha) = node_order(vec!["Head".into(), "Arm".into(), "Hips".into()]);
        let (b, hb) = node_order(vec!["Hips".into(), "Head".into(), "Arm".into()]);
        assert_eq!(a, vec!["Arm", "Head", "Hips"]);
        assert_eq!(a, b);
        assert_eq!(ha, hb);
        let (_, hc) = node_order(vec!["Head".into(), "Arm".into()]);
        assert_ne!(ha, hc);
    }
}
