//! Your avatar in the world, and everyone else's: attaching the model, sending poses, playing
//! back the others'.

use super::*;

/// Put your character under a walker that moves through the world (again after a model
/// change), and find its nodes for sending poses.
pub fn attach(
    mut w: ResMut<WorldClient>,
    av: Res<crate::LocalAvatar>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<bevy::gltf::GltfNode>>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if !w.active() {
        return;
    }
    let walker = match w.walker {
        Some(e) => e,
        None => {
            let e = commands.spawn((Walker, Transform::default(), Visibility::default(), Name::new("walker"))).id();
            w.walker = Some(e);
            e
        }
    };
    let Some(root) = av.root else { return };
    if parents.get(root).map(|p| p.parent()) != Ok(walker) {
        commands.entity(root).insert(ChildOf(walker));
    }
    if w.rig.as_ref().is_some_and(|(r, g)| *r == root && g.complete()) {
        return;
    }
    let (true, Some(gh)) = (av.ready, av.gltf.as_ref()) else { return };
    let Some(gltf) = gltfs.get(gh) else { return };
    if let Some(rig) = net::rig(gltf, &nodes, root, &children, &names) {
        w.rig = Some((root, rig));
    }
}

/// Your own mouth, from your microphone (see [`crate::voice::lipsync`]). It runs before the
/// pose is sent, so the others see it.
pub fn local_mouth(
    w: Res<WorldClient>,
    drivers: Res<crate::voice::lipsync::MouthDrivers>,
    mut saved: Local<Vec<(Entity, Vec3)>>,
    mut tf: Query<&mut Transform>,
) {
    let Some((_, rig)) = &w.rig else { return };
    let mouth = drivers.shown_local(w.now());
    crate::voice::lipsync::local_face(rig, mouth, &mut saved, &mut tf);
}

/// Send your pose 30 times a second: where you stand, which way you face, and the body.
pub fn send_pose(mut w: ResMut<WorldClient>, tf: Query<&Transform>) {
    if !w.ready || w.link.is_none() || w.watch {
        return;
    }
    let now = w.now();
    if now - w.last_pose < POSE_EVERY {
        return;
    }
    w.last_pose = now;
    let Some((root, rig)) = &w.rig else { return };
    let tick = (now * HZ) as u64;
    let pose = net::capture(tick, net::WORLD_PLAYER, *root, rig, None, &tf);
    let at = Vec3::from(w.body.pos) * w.block_size;
    let blob = postcard::to_stdvec(&Placed { at: at.to_array(), yaw: w.yaw, pose }).unwrap_or_default();
    let pos = w.body.pos;
    w.send(&ToServer::Pose { at: pos, blob });
}

/// Everyone else, 100 ms behind, blended between their nearest poses.
pub fn apply_others(
    mut w: ResMut<WorldClient>,
    time: Res<Time<Real>>,
    remotes: Query<&Remote>,
    mouths: Option<Res<crate::voice::lipsync::MouthDrivers>>,
    mut tf: Query<&mut Transform, Without<crate::WorldCamera>>,
) {
    if !w.ready {
        return;
    }
    let now = w.now();
    let dt = time.delta_secs_f64();
    for (id, o) in w.others.iter_mut() {
        let Some(e) = o.entity else { continue };
        let Some(t) = o.stream.clock.at(now, dt) else { continue };
        let tick = t * HZ;
        let Some((a, b, s)) = o.stream.around(tick) else { continue };
        let place = |tick: u64| o.places.iter().find(|p| p.0 == tick).map(|p| (p.1, p.2));
        if let (Some(pa), Some(pb)) = (place(a.tick), place(b.tick)) {
            if let Ok(mut t) = tf.get_mut(e) {
                t.translation = pa.0.lerp(pb.0, s);
                t.rotation = Quat::from_rotation_y(pa.1).slerp(Quat::from_rotation_y(pb.1), s);
            }
        }
        while o.places.len() > 2 && o.places[1].0 < a.tick {
            o.places.pop_front();
        }
        if let Ok(r) = remotes.get(e) {
            if let (Some(inner), Some(rig)) = (r.inner, &r.rig) {
                net::put(rig, inner, &a, &b, s, &mut tf);
                // the mouth from their voice goes on after the pose's own face mask
                let mouth = mouths.as_ref().map_or(avatar_voice::lipsync::Mouth::Rest, |m| m.shown_remote(*id, now));
                crate::voice::lipsync::set_face(rig, mouth, &mut tf);
            }
        }
    }
}
