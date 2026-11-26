//! Feel: sound effects, camera shake and dust puffs.
//!
//! Sounds are real recordings in `assets/sounds/` (credits in the README there); a .wav of the
//! same name in your files folder's `sounds/` replaces one. They play on the default output and
//! go into the shorts' soundtrack like everything else the app plays.
//!
//! Squash and stretch, the eye glances and the blinks live in main.rs's `drive_face`, which asks
//! for the sounds and puffs through here.

use std::{collections::HashMap, sync::Arc};

use bevy::prelude::*;

pub const SOUNDS: [&str; 6] = ["splat", "pop", "knock", "thud", "applause", "firework"];

#[derive(Resource)]
pub struct Fx {
    pub sounds_on: bool,
    pub volume: f32,
    clips: HashMap<&'static str, (u32, Arc<Vec<f32>>)>,
    playing: Vec<(crate::guest::Playback, u32)>,
    last: HashMap<&'static str, f32>,
    /// (sound, loudness 0..1) to play this frame
    pub queue: Vec<(&'static str, f32)>,
    /// camera shake: (start, strength)
    pub shake: (f32, f32),
    /// dust puffs to make: (where on the floor, size)
    pub dust: Vec<(Vec3, f32)>,
    puff_mesh: Option<Handle<Mesh>>,
    puff_mat: Option<Handle<StandardMaterial>>,
}

impl Fx {
    pub fn new(sounds_on: bool, volume: f32) -> Self {
        let mut clips = HashMap::new();
        for name in SOUNDS {
            let mine = crate::applog::user_dir()
                .join("sounds")
                .join(format!("{name}.wav"));
            let bundled = crate::asset_base()
                .join("assets/sounds")
                .join(format!("{name}.wav"));
            let path = if mine.exists() { mine } else { bundled };
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| crate::lipsync::decode_wav_mono(&b))
            {
                Ok((rate, s)) => {
                    clips.insert(name, (rate, Arc::new(s)));
                }
                Err(e) => crate::note!("[fx] no {name} sound ({}: {e})", path.display()),
            }
        }
        Fx {
            sounds_on,
            volume: volume.clamp(0.0, 1.0),
            clips,
            playing: vec![],
            last: HashMap::new(),
            queue: vec![],
            shake: (-100.0, 0.0),
            dust: vec![],
            puff_mesh: None,
            puff_mat: None,
        }
    }

    pub fn play(&mut self, name: &'static str, loud: f32) {
        self.queue.push((name, loud));
    }

    pub fn shake(&mut self, now: f32, strength: f32) {
        // a new shake only takes over if it's stronger than what's left of the old one
        if strength >= self.shake_left(now) {
            self.shake = (now, strength);
        }
    }

    fn shake_left(&self, now: f32) -> f32 {
        self.shake.1 * (-(now - self.shake.0) * 7.0).exp()
    }

    /// Camera offset for the shake right now (decays in about half a second).
    pub fn shake_offset(&self, now: f32) -> Vec3 {
        let age = now - self.shake.0;
        if age > 0.7 {
            return Vec3::ZERO;
        }
        let a = self.shake_left(now);
        Vec3::new(
            (age * 61.0).sin() * a,
            (age * 47.0 + 1.3).sin() * a * 0.8,
            0.0,
        )
    }

    /// sfx:on (toggle), sfx:vol:<0-100>
    pub fn command(&mut self, arg: &str) -> Option<String> {
        if arg == "on" {
            self.sounds_on = !self.sounds_on;
            return Some(format!(
                "Sound effects {}",
                if self.sounds_on { "on" } else { "off" }
            ));
        }
        let v: f32 = arg.strip_prefix("vol:")?.trim().parse().ok()?;
        self.volume = (v / 100.0).clamp(0.0, 1.0);
        Some(format!("Sound effects at {:.0}%", self.volume * 100.0))
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"on": self.sounds_on, "volume": (self.volume * 100.0).round()})
    }
}

#[derive(Component)]
pub struct Puff {
    vel: Vec3,
    t0: f32,
    size: f32,
}

pub fn setup(
    mut fx: ResMut<Fx>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    fx.puff_mesh = Some(meshes.add(Cuboid::new(1.0, 1.0, 1.0)));
    fx.puff_mat = Some(mats.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.83, 0.77),
        unlit: true,
        ..default()
    }));
}

/// Plays queued sounds and makes dust puffs (`step` moves them).
pub fn update(
    mut fx: ResMut<Fx>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    // sounds
    fx.playing.retain(|(p, rate)| !p.finished(*rate));
    for (name, loud) in std::mem::take(&mut fx.queue) {
        // the same sound twice within 60 ms (a volley) plays once; long ones don't restart
        let gap = if matches!(name, "applause" | "firework") {
            2.0
        } else {
            0.06
        };
        if !fx.sounds_on
            || fx.last.get(name).is_some_and(|t| now - t < gap)
            || fx.playing.len() >= 8
        {
            continue;
        }
        let Some((rate, s)) = fx.clips.get(name).cloned() else {
            continue;
        };
        fx.last.insert(name, now);
        let g = fx.volume * loud.clamp(0.0, 1.0);
        let samples: Vec<f32> = s.iter().map(|v| v * g).collect();
        fx.playing
            .push((crate::guest::Playback::quiet(Arc::new(samples), rate), rate));
    }
    // dust
    let (Some(mesh), Some(mat)) = (fx.puff_mesh.clone(), fx.puff_mat.clone()) else {
        return;
    };
    for (at, size) in std::mem::take(&mut fx.dust) {
        for i in 0..7u32 {
            let a = i as f32 / 7.0 * std::f32::consts::TAU + crate::hash01(i, now) * 0.6;
            let speed = 0.35 + crate::hash01(i + 9, now) * 0.3;
            let t = Transform::from_translation(at + Vec3::new(a.cos() * 0.12, 0.03, a.sin() * 0.08))
                .with_scale(Vec3::splat(0.001));
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                t,
                crate::sim::Interp::new(t),
                Puff {
                    vel: Vec3::new(
                        a.cos() * speed,
                        0.18 + crate::hash01(i + 3, now) * 0.15,
                        a.sin() * speed * 0.5,
                    ) * size,
                    t0: now,
                    size: size * (0.06 + crate::hash01(i + 5, now) * 0.04),
                },
            ));
        }
    }
}

/// One fixed tick (`FixedUpdate`): dust puffs billow out, slow down and shrink away.
pub fn step(
    time: Res<Time>,
    mut puffs: Query<(Entity, &Puff, &mut crate::sim::Interp)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (e, p, mut ip) in &mut puffs {
        let t = &mut ip.curr;
        let age = (now - p.t0) / 0.65;
        if age >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        // billow out, slow down, shrink away
        t.translation += p.vel * dt * (1.0 - age);
        let s = p.size * (age * 4.0).min(1.0) * (1.0 - age).sqrt();
        t.scale = Vec3::splat(s.max(0.001));
        t.rotate_y(dt * 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_load_and_commands() {
        let mut fx = Fx::new(true, 0.8);
        for name in SOUNDS {
            let (rate, s) = fx
                .clips
                .get(name)
                .unwrap_or_else(|| panic!("{name}.wav missing"));
            assert!(
                *rate >= 16000 && s.len() > (*rate as usize) / 10,
                "{name} too short"
            );
            let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.3 && peak <= 1.0, "{name} peak {peak}");
        }
        assert_eq!(fx.command("on").as_deref(), Some("Sound effects off"));
        assert_eq!(
            fx.command("vol:40").as_deref(),
            Some("Sound effects at 40%")
        );
        assert!(fx.command("vol:x").is_none());
        // shakes: a weak one doesn't cut a strong one short
        fx.shake(0.0, 0.08);
        fx.shake(0.05, 0.01);
        assert_eq!(fx.shake.1, 0.08);
        assert!(fx.shake_offset(0.03).length() > 0.0);
        assert_eq!(fx.shake_offset(2.0), Vec3::ZERO);
    }
}
