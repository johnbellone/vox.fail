//! The stage camera: framing, the wide shot for big moments, and shake.

use super::*;

pub(crate) fn update_camera(
    mut av: ResMut<Avatar>,
    time: Res<Time>,
    moments: Res<events::Moments>,
    fx: Res<fx::Fx>,
    mut shook: Local<Vec3>,
    mut cams: Query<&mut Transform, With<MainCamera>>,
) {
    let framing = if time.elapsed_secs() < moments.wide_until {
        3
    } else {
        av.framing
    };
    let (eye, target) = match framing {
        0 => (Vec3::new(0.45, 1.6, 2.8), Vec3::new(0.0, 1.42, 0.0)),
        1 => (Vec3::new(0.7, 1.45, 4.2), Vec3::new(0.0, 1.15, 0.0)),
        2 => (Vec3::new(0.9, 1.2, 5.2), Vec3::new(0.0, 0.95, 0.0)),
        // the auto-director's punch-in
        4 => (Vec3::new(0.22, 1.62, 1.95), Vec3::new(0.0, 1.5, 0.0)),
        // the raid crowd in front of you: wide and a little higher
        _ => (Vec3::new(0.3, 1.55, 5.3), Vec3::new(0.0, 0.85, 0.4)),
    };
    // fit whoever is on stage: a viewer's guest on the right, a co-host on the left
    let left = if av.cohost_on || av.remote_guest { cohost::COHOST_X } else { 0.0 };
    let right = if av.two_shot { guest::GUEST_X } else { 0.0 };
    let (mid, span) = ((left + right) / 2.0, right - left);
    let (eye, target) = if span > 0.0 && framing != 3 {
        (
            Vec3::new(mid + 0.15, 1.5, 2.6 + span * 1.05),
            Vec3::new(mid, 1.22, 0.0),
        )
    } else {
        (eye, target)
    };
    let goal = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    let k = if std::mem::take(&mut av.cut) {
        1.0
    } else {
        1.0 - (-time.delta_secs() * 6.0).exp()
    };
    // the shake rides on top of the smooth camera move (last frame's shake taken off first)
    let shake = fx.shake_offset(time.elapsed_secs());
    for mut t in &mut cams {
        let base = t.translation - t.rotation * *shook;
        t.translation = base.lerp(goal.translation, k);
        t.rotation = t.rotation.slerp(goal.rotation, k);
        let r = t.rotation;
        t.translation += r * shake;
    }
    *shook = shake;
}
