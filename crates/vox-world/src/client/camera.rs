//! The views: first person, behind the shoulder, from above, fixed shots and the watcher's
//! drift.

use super::*;

/// The camera's forward direction for a turn and tilt.
pub(super) fn facing(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

/// The behind camera: behind and a little right of the head, pulled in if a block is in the
/// way. Metres.
pub(super) fn behind_spot(w: &WorldClient) -> Vec3 {
    let size = w.block_size;
    let f = facing(w.cam_yaw, w.cam_pitch);
    let right = Vec3::new(w.cam_yaw.cos(), 0.0, -w.cam_yaw.sin());
    let head = (Vec3::from(w.body.pos) + Vec3::Y * (EYES + vox_core::in_blocks(0.2))) * size;
    let back = (-f * CAM_DIST + right * 0.45).normalize();
    let dist = (CAM_DIST * CAM_DIST + 0.45 * 0.45).sqrt();
    let hit = ray::raycast_with(|c| w.solid_at(c), (head / size).to_array(), back.to_array(), dist / size);
    let d = hit.map_or(dist, |h| (h.distance * size - 0.15).max(0.2));
    head + back * d
}

/// The camera: Diablo-style from above (zooming in and out, the tilt steepening as it goes
/// out), behind you, or a world's fixed shot for where you are, blended smoothly between.
/// Gizmo lines go round the block you'd build on.
pub fn camera(
    mut w: ResMut<WorldClient>,
    real: Res<Time<Real>>,
    mut cams: Query<&mut Transform, With<crate::WorldCamera>>,
    mut gizmos: Gizmos,
    mut clear: ResMut<ClearColor>,
) {
    if !w.active() || !w.ready {
        return;
    }
    // a sky instead of the show's background
    let sky = Color::srgb(0.55, 0.74, 0.95);
    if clear.0 != sky {
        clear.0 = sky;
    }
    let dt = real.delta_secs().min(0.25);
    let size = w.block_size;
    if w.watch {
        // a slow drift round where people arrive, high enough to see them come and go
        let t = w.now() as f32;
        let centre = Vec3::from(w.spawn) * size + Vec3::Y * 1.5;
        let angle = t * 0.045;
        let radius = 15.0 + 2.5 * (t * 0.07).sin();
        let at = centre + Vec3::new(angle.cos() * radius, 6.5 + 1.5 * (t * 0.05).sin(), angle.sin() * radius);
        let dir = (centre - at).normalize_or(Vec3::NEG_Z);
        w.cam_at = at;
        w.cam_dir = dir;
        for mut c in &mut cams {
            *c = Transform::from_translation(at).looking_to(dir, Vec3::Y);
        }
        return;
    }
    let ease = |rate: f32| 1.0 - (-dt * rate).exp();
    let chest = (Vec3::from(w.body.pos) + Vec3::Y * vox_core::in_blocks(1.2)) * size;
    let focus = match w.focus {
        Some(f) if f.distance(chest) < 6.0 => f.lerp(chest, ease(9.0)),
        _ => chest,
    };
    w.focus = Some(focus);
    w.zoom_now += (w.zoom - w.zoom_now) * ease(8.0);
    // ---- the follow camera
    let (follow_at, follow_dir) = match w.cam_mode {
        CamMode::Diablo => {
            // steeper the further out: 38° looking down close in, 62° far out
            let t = ((w.zoom_now - 4.0) / 41.0).clamp(0.0, 1.0);
            let tilt = (38.0 + 24.0 * t).to_radians();
            let dir = facing(w.cam_yaw, -tilt);
            (focus - dir * w.zoom_now, dir)
        }
        CamMode::Behind => (behind_spot(&w), facing(w.cam_yaw, w.cam_pitch)),
        CamMode::First => ((Vec3::from(w.body.pos) + Vec3::Y * EYES) * size, facing(w.cam_yaw, w.cam_pitch)),
    };
    // ---- a fixed shot for where you are
    let here = w.body.pos;
    // (fixed shots are for the views from outside you)
    let zone = if w.cam_mode == CamMode::First && w.forced_shot.is_none() {
        None
    } else {
        w.forced_shot.or_else(|| w.shots.iter().position(|s| s.contains(here)))
    };
    if zone.is_some() {
        if zone != w.shot {
            w.last_shot = w.shot.or(w.last_shot);
        }
        w.shot = zone;
    } else if w.shot.is_some() {
        w.last_shot = w.shot.take();
    }
    let want = if w.shot.is_some() { 1.0 } else { 0.0 };
    w.shot_blend += (want - w.shot_blend) * ease(2.6);
    let shot_view = |i: usize| {
        let s = &w.shots[i];
        let look = s.look.unwrap_or(chest);
        (s.at, (look - s.at).normalize_or(Vec3::NEG_Z))
    };
    let (at, dir) = match w.shot.or(w.last_shot) {
        Some(i) if w.shot_blend > 0.001 => {
            let (sa, sd) = shot_view(i);
            let k = w.shot_blend * w.shot_blend * (3.0 - 2.0 * w.shot_blend);
            (follow_at.lerp(sa, k), follow_dir.lerp(sd, k).normalize_or(sd))
        }
        _ => (follow_at, follow_dir),
    };
    w.cam_at = at;
    w.cam_dir = dir;
    for mut t in &mut cams {
        *t = Transform::from_translation(at).looking_to(dir, Vec3::Y);
    }
    if let Some(h) = w.target {
        let centre = (IVec3::from(h.cell).as_vec3() + Vec3::splat(0.5)) * size;
        gizmos.cube(Transform::from_translation(centre).with_scale(Vec3::splat(size * 1.02)), Color::srgba(1.0, 1.0, 1.0, 0.8));
    }
    if let Some(g) = w.goal {
        let p = Vec3::new(g.x, g.y.floor(), g.z) * size + Vec3::Y * 0.02;
        gizmos.circle(Isometry3d::new(p, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.25, Color::srgba(1.0, 1.0, 1.0, 0.7));
    }
}
