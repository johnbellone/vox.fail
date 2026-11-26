//! The simulation clock: everything that moves step by step (particles, the crowd, hit puffs,
//! splats) runs at a fixed 120 Hz in `FixedUpdate`, so it moves the same at any frame rate.
//! Each frame then draws a blend of the last two ticks, so nothing jumps between them.
//!
//! - A moving thing carries [`Interp`] and writes `Interp::curr`, never `Transform`.
//! - `snap_prev` keeps the previous tick's pose at the start of every tick.
//! - `blend` writes `Transform` from the two, just before transforms propagate.
//! - [`Tick`] counts ticks, for anything that wants a whole-number clock.
//!
//! A slow frame (a shader compile, a set loading) runs at most `MAX_CATCH_UP` worth of ticks;
//! after a longer stall the world slows down for that moment instead of jumping ahead.

use std::time::Duration;

use bevy::{prelude::*, transform::TransformSystems};

/// Simulation ticks a second.
pub const HZ: f64 = vox_world::pose::HZ;
/// The most simulated time one frame may catch up on (12 ticks).
pub const MAX_CATCH_UP: Duration = Duration::from_millis(100);

/// Ticks run since start.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick(pub u64);

/// The pose at the last two ticks; the frame shows a blend of them.
#[derive(Component, Clone, Copy, Debug)]
pub struct Interp {
    pub prev: Transform,
    pub curr: Transform,
}

impl Interp {
    /// Both ticks at `t`: a newly spawned thing doesn't streak in from somewhere else.
    pub fn new(t: Transform) -> Self {
        Interp { prev: t, curr: t }
    }
}

/// `a` at 0, `b` at 1: position and size in a straight line, rotation the short way round.
pub fn blend_transform(a: &Transform, b: &Transform, s: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, s),
        rotation: a.rotation.slerp(b.rotation, s),
        scale: a.scale.lerp(b.scale, s),
    }
}

fn count_tick(mut tick: ResMut<Tick>) {
    tick.0 += 1;
}

fn snap_prev(mut q: Query<&mut Interp>) {
    for mut i in &mut q {
        i.prev = i.curr;
    }
}

fn blend(time: Res<Time<Fixed>>, mut q: Query<(&Interp, &mut Transform)>) {
    let s = time.overstep_fraction();
    for (i, mut t) in &mut q {
        *t = blend_transform(&i.prev, &i.curr, s);
    }
}

fn cap_catch_up(mut time: ResMut<Time<Virtual>>) {
    time.set_max_delta(MAX_CATCH_UP);
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(HZ))
            .init_resource::<Tick>()
            .add_systems(Startup, cap_catch_up)
            .add_systems(FixedFirst, (count_tick, snap_prev))
            .add_systems(PostUpdate, blend.before(TransformSystems::Propagate));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;

    /// A bare app on the fixed step whose frames each take `frame` of time.
    pub(crate) fn test_app(frame: Duration) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(TransformPlugin)
            .add_plugins(SimPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app
    }

    #[test]
    fn blend_ends() {
        let a = Transform::from_xyz(0.0, 1.0, 0.0);
        let b = Transform::from_xyz(2.0, 1.0, 4.0)
            .with_rotation(Quat::from_rotation_y(1.2))
            .with_scale(Vec3::splat(3.0));
        assert_eq!(blend_transform(&a, &b, 0.0), a);
        let end = blend_transform(&a, &b, 1.0);
        assert!(end.translation.distance(b.translation) < 1e-6);
        assert!(end.rotation.angle_between(b.rotation) < 1e-5);
        let mid = blend_transform(&a, &b, 0.5);
        assert!(mid.translation.distance(Vec3::new(1.0, 1.0, 2.0)) < 1e-6);
        assert!((mid.rotation.angle_between(a.rotation) - 0.6).abs() < 1e-4, "slerped");
    }

    /// A ball under gravity and drag, stepped once a tick, its height kept for every tick.
    #[derive(Component)]
    struct Ball {
        vel: Vec3,
        trail: Vec<Vec3>,
    }

    fn fall(time: Res<Time>, mut q: Query<(&mut Ball, &mut Interp)>) {
        let dt = time.delta_secs();
        for (mut b, mut i) in &mut q {
            b.vel.y -= 9.8 * dt;
            b.vel *= (-0.5 * dt).exp();
            i.curr.translation += b.vel * dt;
            let p = i.curr.translation;
            b.trail.push(p);
        }
    }

    fn trail_at(fps: f64, seconds: f64) -> Vec<Vec3> {
        let mut app = test_app(Duration::from_secs_f64(1.0 / fps));
        app.add_systems(FixedUpdate, fall);
        let e = app
            .world_mut()
            .spawn((
                Ball {
                    vel: Vec3::new(1.0, 4.0, 0.0),
                    trail: vec![],
                },
                Interp::new(Transform::default()),
                Transform::default(),
            ))
            .id();
        for _ in 0..(fps * seconds) as usize + 2 {
            app.update();
        }
        app.world().get::<Ball>(e).unwrap().trail.clone()
    }

    #[test]
    fn same_motion_any_fps() {
        let runs: Vec<Vec<Vec3>> = [30.0, 60.0, 144.0].iter().map(|f| trail_at(*f, 2.0)).collect();
        let n = runs.iter().map(|r| r.len()).min().unwrap();
        assert!(n >= 230, "about 240 ticks in 2 s, got {n}");
        for r in &runs[1..] {
            for k in 0..n {
                assert!(
                    r[k].distance(runs[0][k]) < 1e-5,
                    "tick {k}: {:?} vs {:?}",
                    r[k],
                    runs[0][k]
                );
            }
        }
    }

    #[test]
    fn one_tick_per_frame_at_120() {
        let mut app = test_app(Duration::from_secs_f64(1.0 / HZ));
        app.update(); // the first frame starts the clock
        let start = app.world().resource::<Tick>().0;
        for _ in 0..50 {
            app.update();
        }
        assert_eq!(app.world().resource::<Tick>().0 - start, 50);
    }

    #[test]
    fn catch_up_is_capped() {
        let mut app = test_app(Duration::from_secs(1));
        app.update();
        app.update();
        let before = app.world().resource::<Tick>().0;
        app.update();
        let ran = app.world().resource::<Tick>().0 - before;
        assert_eq!(ran, 12, "a 1 s frame runs 100 ms of ticks, not 120");
    }

    /// A queue filled per frame and drained by the first tick that runs.
    #[derive(Resource, Default)]
    struct Inbox {
        waiting: Vec<u32>,
        handled: Vec<(u64, u32)>,
        push: Option<u32>,
    }

    fn produce(mut inbox: ResMut<Inbox>) {
        if let Some(v) = inbox.push.take() {
            inbox.waiting.push(v);
        }
    }

    fn consume(tick: Res<Tick>, mut inbox: ResMut<Inbox>) {
        for v in std::mem::take(&mut inbox.waiting) {
            inbox.handled.push((tick.0, v));
        }
    }

    #[test]
    fn queue_survives_zero_ticks_and_runs_once_on_many() {
        // 300 fps: most frames run no tick at all
        let mut app = test_app(Duration::from_secs_f64(1.0 / 300.0));
        app.init_resource::<Inbox>()
            .add_systems(Update, produce)
            .add_systems(FixedUpdate, consume);
        app.update();
        for _ in 0..5 {
            app.update();
        }
        let ticks_before = app.world().resource::<Tick>().0;
        app.world_mut().resource_mut::<Inbox>().push = Some(7);
        app.update();
        // the push lands in Update, after this frame's ticks: nothing handled yet
        assert!(app.world().resource::<Inbox>().handled.is_empty());
        for _ in 0..5 {
            app.update();
        }
        let inbox = app.world().resource::<Inbox>();
        assert_eq!(inbox.handled.len(), 1, "handled exactly once");
        assert!(inbox.handled[0].0 > ticks_before);

        // 20 fps: every frame runs several ticks; still handled once
        let mut app = test_app(Duration::from_secs_f64(1.0 / 20.0));
        app.init_resource::<Inbox>()
            .add_systems(Update, produce)
            .add_systems(FixedUpdate, consume);
        app.update();
        app.update();
        app.world_mut().resource_mut::<Inbox>().push = Some(9);
        app.update();
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Inbox>().handled.len(), 1);
    }
}
