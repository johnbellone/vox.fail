//! Snapshot mode for visual regression tests: load a character (and a set), let everything
//! settle, freeze time, save one picture of the window and quit.
//!
//!     block_avatar --snapshot out.png --model models/dog.glb --snapshot-set diner
//!     block_avatar --snapshot out.png --snapshot-set diner --snapshot-fx confetti
//!
//! Everything that moves on its own (idle life, voice reactions, blinks, weather) is held still
//! so the same build makes the same picture. Every frame is exactly one simulation tick (see
//! `main`), so with `--snapshot-fx` the burst fired on the first frame is always photographed
//! the same number of ticks later. tools/test/visual_test.py runs a list of these and
//! compares them with the reference pictures in testdata/visual/.

use std::path::PathBuf;

use bevy::{
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
};

#[derive(Resource)]
pub struct Snapshot {
    pub out: PathBuf,
    pub set: Option<String>,
    /// an effect to fire once the character is ready (`fx:<this>`); time then runs until the shot
    pub fx: Option<String>,
    /// frames since the character was ready
    frames: u32,
    shot_at: Option<u32>,
    /// an effect is playing: keep the clock running until the picture is taken
    moving: bool,
}

/// How long to let things settle after the character is ready, and how long to wait for the
/// picture to reach the disk.
const SETTLE: u32 = 40;
const SAVE: u32 = 30;

impl Snapshot {
    pub fn new(out: PathBuf, set: Option<String>, fx: Option<String>) -> Self {
        Snapshot { out, set, fx, frames: 0, shot_at: None, moving: false }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    snap: Option<ResMut<Snapshot>>,
    mut av: ResMut<crate::Avatar>,
    mut inject: ResMut<crate::Inject>,
    mut particles: ResMut<crate::particles::Particles>,
    mut studio: ResMut<crate::lighting::Studio>,
    mut cards: Query<&mut Visibility, With<crate::startup::LoadingCard>>,
    mut time: ResMut<Time<Virtual>>,
    mut players: Query<&mut AnimationPlayer>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut snap) = snap else { return };
    // hold still
    av.idle_life = false;
    av.reactions = false;
    av.hud = false;
    // the camera straight to its spot (no easing that depends on frame times), no blinking
    av.cut = true;
    av.next_blink = f32::MAX;
    av.blink_until = 0.0;
    av.blink_again = -1.0;
    av.auto_talk = false;
    // no mic on a test machine: don't doze off
    av.sleep_after = f32::MAX;
    av.asleep = false;
    particles.weather = Some(None);
    // the same quality every run: High, and no stepping down on a slow test machine
    studio.auto = false;
    studio.level = crate::lighting::Level::High;
    if snap.frames == 0 && snap.shot_at.is_none() {
        if let Some(s) = snap.set.take() {
            inject.0.push(format!("set:{s}"));
        }
    }
    if av.player.is_none() {
        return;
    }
    snap.frames += 1;
    if snap.frames == 1 {
        if let Some(b) = snap.fx.take() {
            inject.0.push(format!("fx:{b}"));
            snap.moving = true;
        }
    }
    // the loading card times itself out on the clock, which stops below
    for mut v in &mut cards {
        *v = Visibility::Hidden;
    }
    // the pose: every clip held at the same moment, and the clock stopped
    if snap.frames >= 3 {
        if !snap.moving || snap.frames >= SETTLE {
            time.pause();
        }
        for mut p in &mut players {
            for (_, a) in p.playing_animations_mut() {
                a.seek_to(0.5).pause();
            }
        }
    }
    match snap.shot_at {
        None if snap.frames >= SETTLE => {
            if let Some(dir) = snap.out.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::remove_file(&snap.out);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(snap.out.clone()));
            snap.shot_at = Some(snap.frames);
        }
        Some(at) if snap.out.exists() || snap.frames > at + SAVE * 10 => {
            // a frame or two for the file to finish writing
            if snap.frames > at + 2 {
                if !snap.out.exists() {
                    eprintln!("snapshot: no picture was saved");
                    std::process::exit(2);
                }
                exit.write(AppExit::Success);
            }
        }
        _ => {}
    }
}
