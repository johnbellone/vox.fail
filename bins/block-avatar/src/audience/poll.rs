//! Tug-of-war polls: two options, chat votes `!1` or `!2`, and two little characters pull a rope
//! along the bottom of the picture; the knot moves with the vote share. When time's up the
//! winner celebrates and the loser falls over.
//!
//! Start one from the control page, or `GET /cmd/poll:start:60:Best snack?|Pizza|Tacos`
//! (seconds, then title|option 1|option 2). `poll:end` finishes early, `poll:cancel` drops it.
//! One vote per viewer; voting again changes it.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::puppet::{Puppet, pick_model};

/// Rope group in the main camera's space: centred, low in the frame.
const AT: Vec3 = Vec3::new(0.0, -0.33, -1.2);
const HALF: f32 = 0.27;
const RESULT_S: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Running,
    /// finished at t
    Result(f32),
}

pub struct Poll {
    pub title: String,
    pub options: [String; 2],
    pub votes: HashMap<String, u8>,
    pub ends: f32,
    pub phase: Phase,
    group: Option<Entity>,
    pullers: [Option<Entity>; 2],
    knot: Option<Entity>,
    /// eased knot position, -1 (option 1 winning) .. 1 (option 2)
    pub pos: f32,
}

impl Poll {
    pub fn counts(&self) -> [usize; 2] {
        let a = self.votes.values().filter(|v| **v == 0).count();
        [a, self.votes.len() - a]
    }
    /// option 2's share minus option 1's: -1..1 (0 with no votes)
    pub fn balance(&self) -> f32 {
        let [a, b] = self.counts();
        if a + b == 0 {
            0.0
        } else {
            (b as f32 - a as f32) / (a + b) as f32
        }
    }
    pub fn winner(&self) -> Option<usize> {
        let [a, b] = self.counts();
        match a.cmp(&b) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            _ => None,
        }
    }
    pub fn line(&self, now: f32) -> String {
        let [a, b] = self.counts();
        let tot = (a + b).max(1) as f32;
        let pct = |n: usize| (n as f32 / tot * 100.0).round() as u32;
        match self.phase {
            Phase::Running => {
                let left = (self.ends - now).max(0.0) as u32;
                format!(
                    "{}\n!1 {} {}%   vs   {}% {} !2   ({}:{:02})",
                    self.title,
                    self.options[0],
                    pct(a),
                    pct(b),
                    self.options[1],
                    left / 60,
                    left % 60
                )
            }
            Phase::Result(_) => match self.winner() {
                Some(w) => format!(
                    "{}\n{} wins, {}% of {} votes",
                    self.title,
                    self.options[w],
                    pct([a, b][w]),
                    a + b
                ),
                None => format!("{}\nIt's a tie! ({} votes)", self.title, a + b),
            },
        }
    }
}

/// `!1`, `!2`, `!a`, `!b`, or the option's name after `!`.
pub fn vote_of(text: &str, options: &[String; 2]) -> Option<u8> {
    let w = text
        .split_whitespace()
        .next()?
        .strip_prefix('!')?
        .to_ascii_lowercase();
    match w.as_str() {
        "1" | "a" => Some(0),
        "2" | "b" => Some(1),
        _ => options
            .iter()
            .position(|o| {
                o.split_whitespace()
                    .next()
                    .is_some_and(|f| f.eq_ignore_ascii_case(&w))
            })
            .map(|i| i as u8),
    }
}

#[derive(Resource, Default)]
pub struct Polls {
    pub poll: Option<Poll>,
    start: Option<(String, [String; 2], f32)>,
    end_now: bool,
    cancel: bool,
}

impl Polls {
    /// poll:start:<secs>:<title>|<1>|<2>, poll:end, poll:cancel
    pub fn command(&mut self, arg: &str) -> Option<String> {
        let (verb, rest) = arg.split_once(':').unwrap_or((arg, ""));
        Some(match verb {
            "start" => {
                let (secs, spec) = rest.split_once(':')?;
                let secs = secs.parse::<f32>().ok()?.clamp(10.0, 600.0);
                let parts: Vec<String> = spec
                    .split('|')
                    .map(|s| s.trim().chars().take(40).collect())
                    .collect();
                let (title, a, b) = match parts.as_slice() {
                    [t, a, b] => (t.clone(), a.clone(), b.clone()),
                    [a, b] => ("Poll".into(), a.clone(), b.clone()),
                    _ => return Some("Poll needs a title and two options: title|one|two".into()),
                };
                if a.is_empty() || b.is_empty() {
                    return Some("Poll needs two options".into());
                }
                self.start = Some((title.clone(), [a, b], secs));
                format!("Poll started: {title}")
            }
            "end" => {
                self.end_now = true;
                "Poll ending".into()
            }
            "cancel" => {
                self.cancel = true;
                "Poll cancelled".into()
            }
            _ => return None,
        })
    }
    pub fn json(&self, now: f32) -> String {
        match &self.poll {
            Some(p) => serde_json::json!({"title": p.title, "options": p.options, "counts": p.counts(), "left": (p.ends - now).max(0.0).round(),
                "phase": if p.phase == Phase::Running { "running" } else { "result" }, "line": p.line(now)})
            .to_string(),
            None => "null".into(),
        }
    }
}

#[derive(Component)]
pub struct PollLabel;

pub fn spawn_labels(
    stages: Res<crate::stage::Stages>,
    fonts: Res<AssetServer>,
    mut commands: Commands,
) {
    let font: Handle<Font> = fonts.load("fonts/PressStart2P-Regular.ttf");
    for v in stages.views.iter() {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Percent(2.5),
                justify_content: JustifyContent::Center,
                ..default()
            },
            ChildOf(v.root),
            Visibility::Hidden,
            PollLabel,
            children![(
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.72)),
                Text::new(""),
                TextFont {
                    font: font.clone().into(),
                    font_size: bevy::text::FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                TextLayout::justify(bevy::text::Justify::Center),
                TextShadow {
                    offset: Vec2::new(2.0, 2.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.85)
                },
            )],
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut polls: ResMut<Polls>,
    chat: Res<crate::chat::Chat>,
    av: Res<crate::Avatar>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    cams: Query<Entity, With<crate::MainCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut puppets: Query<(&mut Puppet, &mut Transform), Without<PollKnot>>,
    mut knots: Query<&mut Transform, With<PollKnot>>,
    labels: Query<(&PollLabel, &Children)>,
    mut vis: Query<&mut Visibility, With<PollLabel>>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    if polls.cancel || polls.start.is_some() {
        polls.cancel = false;
        if let Some(g) = polls.poll.take().and_then(|p| p.group) {
            commands.entity(g).despawn();
        }
    }
    if let Some((title, options, secs)) = polls.start.take() {
        let Some(cam) = cams.iter().next() else {
            return;
        };
        let host = av.models[av.model].1.clone();
        let rope = meshes.add(Cuboid::new(HALF * 2.0 + 0.14, 0.012, 0.012));
        let knot = meshes.add(Cuboid::new(0.035, 0.035, 0.035));
        let brown = mats.add(StandardMaterial {
            base_color: Color::srgb_u8(170, 130, 80),
            perceptual_roughness: 0.9,
            ..default()
        });
        let red = mats.add(StandardMaterial {
            base_color: Color::srgb_u8(220, 50, 50),
            perceptual_roughness: 0.9,
            ..default()
        });
        let group = commands
            .spawn((
                Transform::from_translation(AT),
                Visibility::default(),
                ChildOf(cam),
            ))
            .id();
        commands.spawn((
            Mesh3d(rope),
            MeshMaterial3d(brown),
            Transform::from_xyz(0.0, 0.07, 0.0),
            ChildOf(group),
        ));
        let k = commands
            .spawn((
                Mesh3d(knot),
                MeshMaterial3d(red),
                Transform::from_xyz(0.0, 0.07, 0.0),
                PollKnot,
                ChildOf(group),
            ))
            .id();
        let mut pullers = [None, None];
        for (i, s) in [-1.0f32, 1.0].into_iter().enumerate() {
            let mut p = Puppet::new(
                &pick_model(&av.models, &host, (now * 13.0) as u32 + i as u32 * 7),
                &assets,
            );
            p.want = "Brace".into();
            p.face = ["eyes_squint", "brows_angry", "mouth_grit"];
            // facing the middle, leaning back
            let rot = Quat::from_rotation_y(-s * std::f32::consts::FRAC_PI_2)
                * Quat::from_rotation_x(-0.25);
            pullers[i] = Some(
                commands
                    .spawn((
                        p,
                        Transform::from_xyz(s * HALF, -0.05, 0.0)
                            .with_rotation(rot)
                            .with_scale(Vec3::splat(0.13)),
                        ChildOf(group),
                    ))
                    .id(),
            );
        }
        polls.poll = Some(Poll {
            title,
            options,
            votes: HashMap::new(),
            ends: now + secs,
            phase: Phase::Running,
            group: Some(group),
            pullers,
            knot: Some(k),
            pos: 0.0,
        });
    }
    let end_now = std::mem::take(&mut polls.end_now);
    let mut done = false;
    if let Some(p) = polls.poll.as_mut() {
        if p.phase == Phase::Running {
            for m in &chat.inbox {
                if let Some(v) = vote_of(&m.text, &p.options) {
                    p.votes.insert(m.user.to_ascii_lowercase(), v);
                }
            }
            if end_now || now >= p.ends {
                p.phase = Phase::Result(now);
            }
        }
        // the knot drifts toward the vote balance, with a little tug wobble while running
        let target = match p.phase {
            Phase::Running => p.balance() * 0.8 + (now * 9.0).sin() * 0.03,
            Phase::Result(_) => match p.winner() {
                Some(0) => -1.0,
                Some(_) => 1.0,
                None => 0.0,
            },
        };
        p.pos += (1.0 - (-dt * 3.0).exp()) * (target - p.pos);
        let x = p.pos * HALF * 0.45;
        if let Some(mut t) = p.knot.and_then(|k| knots.get_mut(k).ok()) {
            t.translation.x = x;
        }
        for (i, s) in [-1.0f32, 1.0].into_iter().enumerate() {
            let Some((mut pp, mut t)) = p.pullers[i].and_then(|e| puppets.get_mut(e).ok()) else {
                continue;
            };
            t.translation.x = s * HALF + x;
            if let Phase::Result(_) = p.phase {
                let won = p.winner() == Some(i);
                let tie = p.winner().is_none();
                pp.want = if tie {
                    "Idle"
                } else if won {
                    "Victory"
                } else {
                    "Knockout"
                }
                .into();
                pp.face = if won || tie {
                    ["eyes_open", "brows_raised", "mouth_grin"]
                } else {
                    ["eyes_x", "brows_worried", "mouth_o"]
                };
                t.rotation = Quat::from_rotation_y(-s * std::f32::consts::FRAC_PI_2)
                    .slerp(Quat::IDENTITY, 0.6);
            }
        }
        if let Phase::Result(t0) = p.phase {
            done = now - t0 > RESULT_S;
        }
    }
    if done {
        if let Some(g) = polls.poll.take().and_then(|p| p.group) {
            commands.entity(g).despawn();
        }
    }
    // labels
    let line = polls.poll.as_ref().map(|p| p.line(now));
    for (_, kids) in &labels {
        for c in kids.iter() {
            if let Ok(mut t) = texts.get_mut(c) {
                let s = line.clone().unwrap_or_default();
                if t.0 != s {
                    t.0 = s;
                }
            }
        }
    }
    for mut v in &mut vis {
        *v = if line.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[derive(Component)]
pub struct PollKnot;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn votes_and_result() {
        let opts = ["Pizza".to_string(), "Tacos al pastor".to_string()];
        assert_eq!(vote_of("!1", &opts), Some(0));
        assert_eq!(vote_of("!B lol", &opts), Some(1));
        assert_eq!(vote_of("!tacos", &opts), Some(1));
        assert_eq!(vote_of("!pizza", &opts), Some(0));
        assert_eq!(vote_of("!3", &opts), None);
        assert_eq!(vote_of("1", &opts), None);
        let mut p = Poll {
            title: "Snack".into(),
            options: opts,
            votes: HashMap::new(),
            ends: 60.0,
            phase: Phase::Running,
            group: None,
            pullers: [None, None],
            knot: None,
            pos: 0.0,
        };
        assert_eq!(p.balance(), 0.0);
        p.votes.insert("a".into(), 0);
        p.votes.insert("b".into(), 1);
        p.votes.insert("c".into(), 1);
        p.votes.insert("a".into(), 1); // changed their mind
        assert_eq!(p.counts(), [0, 3]);
        assert_eq!(p.winner(), Some(1));
        assert!(p.line(30.0).contains("0:30"));
        p.phase = Phase::Result(60.0);
        assert!(
            p.line(61.0)
                .contains("Tacos al pastor wins, 100% of 3 votes"),
            "{}",
            p.line(61.0)
        );
        let mut ps = Polls::default();
        assert_eq!(
            ps.command("start:60:Best snack?|Pizza|Tacos").as_deref(),
            Some("Poll started: Best snack?")
        );
        assert!(
            ps.command("start:60:only one")
                .unwrap()
                .contains("two options")
        );
    }
}
