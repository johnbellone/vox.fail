//! The chat in the room: everyone who chatted in the last few minutes stands at the edges of
//! the stage as a little character with their name on a sign, waves when they talk, and leaves
//! when they go quiet. Up to 8 at once; the newest pushes out the one quiet longest.
//!
//! - The sign's border is the viewer's platform colour (Twitch purple, YouTube red, Kick green),
//!   and subscribers/members get a star.
//! - `!me dog` (any character's name) picks who you are in the crowd.
//! - The crowd ducks when you get hit, dances for a raid or a sub, and during a tug-of-war poll
//!   everyone who voted walks over to their side.
//! - A viewer banned or timed out on any platform is shown out.
//!
//! `crowd:on` toggles it; `crowd:test` adds a few made-up chatters to see it.

use std::collections::HashMap;

use bevy::{prelude::*, window::PrimaryWindow};

use crate::chat::{ChatEvent, Platform, viewer_key};
use crate::puppet::{Puppet, pick_model};

const MAX: usize = 8;
/// how long a chatter stays after their last message
const STAY: f32 = 180.0;

#[derive(Resource)]
pub struct Crowd {
    pub on: bool,
    members: Vec<Member>,
    test: Vec<String>,
    /// `!me` picks: viewer key -> model path
    picks: HashMap<String, String>,
    /// dancing (a raid or a sub) until
    party_until: f32,
}

struct Member {
    key: String,
    user: String,
    platform: Platform,
    sub: bool,
    model: String,
    entity: Entity,
    labels: Vec<Entity>,
    last: f32,
    slot: usize,
    leaving: Option<f32>,
}

#[derive(Component)]
pub struct Chatter {
    slot: usize,
    t0: f32,
    wave_until: f32,
}

#[derive(Component)]
pub struct NameSign {
    owner: Entity,
    view: usize,
}

impl Crowd {
    pub fn new(on: bool) -> Self {
        Crowd {
            on,
            members: vec![],
            test: vec![],
            picks: HashMap::new(),
            party_until: -100.0,
        }
    }
    /// crowd:on (toggle), crowd:test
    pub fn command(&mut self, arg: &str) -> Option<String> {
        match arg {
            "on" => {
                self.on = !self.on;
                Some(format!("Chat crowd {}", if self.on { "on" } else { "off" }))
            }
            "test" => {
                self.test.extend(
                    [
                        "PixelPal",
                        "tomato_tim",
                        "LurkerLou",
                        "QueenOfChat",
                        "bitsy",
                    ]
                    .map(String::from),
                );
                Some("Test chatters".into())
            }
            _ => None,
        }
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"on": self.on, "count": self.members.iter().filter(|m| m.leaving.is_none()).count()})
    }
}

/// Where slot `i` stands: alternating sides, a little back, filling outward.
pub fn slot_pos(i: usize) -> Vec3 {
    let side = if i % 2 == 0 { -1.0 } else { 1.0 };
    let k = (i / 2) as f32;
    Vec3::new(side * (0.85 + k * 0.42), 0.0, -0.35 - k * 0.12)
}

/// `!me <character>`: the model whose name matches (exactly, else starting with, else
/// containing), from the app's character list.
pub fn pick_of(text: &str, models: &[(String, String)]) -> Option<String> {
    let rest = text.trim().strip_prefix("!me")?.trim().to_lowercase();
    if rest.is_empty() {
        return None;
    }
    let norm = |s: &str| s.to_lowercase().replace(['_', '-'], " ");
    let want = norm(&rest);
    let label = |m: &(String, String)| norm(&m.0);
    models
        .iter()
        .find(|m| label(m) == want)
        .or_else(|| models.iter().find(|m| label(m).starts_with(&want)))
        .or_else(|| models.iter().find(|m| label(m).contains(&want)))
        .map(|m| m.1.clone())
}

/// The sign's text: the name (shortened), with a star for subscribers and members.
pub fn sign_text(user: &str, sub: bool) -> String {
    let name: String = user.chars().take(14).collect();
    if sub { format!("* {name}") } else { name }
}

#[allow(clippy::too_many_arguments)]
fn spawn_member(
    commands: &mut Commands,
    stages: &crate::stage::Stages,
    assets: &AssetServer,
    model: &str,
    slot: usize,
    now: f32,
    user: &str,
    platform: Platform,
    sub: bool,
) -> (Entity, Vec<Entity>) {
    let mut p = Puppet::new(model, assets);
    p.want = "Victory".into();
    let t = Transform::from_translation(slot_pos(slot) + Vec3::Y * 0.6).with_scale(Vec3::splat(0.001));
    let e = commands
        .spawn((
            p,
            t,
            crate::sim::Interp::new(t),
            Chatter {
                slot,
                t0: now,
                wave_until: now + 2.5,
            },
        ))
        .id();
    let [r, g, b] = platform.colour();
    let mut labels = vec![];
    for (vi, v) in stages.views.iter().enumerate() {
        let font: Handle<Font> = assets.load("fonts/PressStart2P-Regular.ttf");
        labels.push(
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.98, 0.96, 0.9, 0.92)),
                    BorderColor::all(Color::srgb_u8(r, g, b)),
                    Text::new(sign_text(user, sub)),
                    TextFont {
                        font: font.into(),
                        font_size: bevy::text::FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(if sub {
                        Color::srgb(0.55, 0.38, 0.0)
                    } else {
                        Color::srgb(0.2, 0.15, 0.1)
                    }),
                    Visibility::Hidden,
                    NameSign { owner: e, view: vi },
                    ChildOf(v.root),
                ))
                .id(),
        );
    }
    (e, labels)
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut crowd: ResMut<Crowd>,
    chat: Res<crate::chat::Chat>,
    mut av: ResMut<crate::Avatar>,
    stages: Res<crate::stage::Stages>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    mut people: Query<&mut Chatter>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    // who spoke: real chat, plus made-up test names
    struct Heard {
        user: String,
        platform: Platform,
        sub: bool,
        text: String,
    }
    let mut heard: Vec<Heard> = chat
        .seen
        .iter()
        .filter(|m| m.user != crate::chat::QUIET)
        .map(|m| Heard {
            user: m.user.clone(),
            platform: m.platform,
            sub: m.sub || m.vip || m.moderator,
            text: m.text.clone(),
        })
        .collect();
    let tests = std::mem::take(&mut crowd.test);
    for (i, t) in tests.into_iter().enumerate() {
        let platform = [Platform::Twitch, Platform::YouTube, Platform::Kick][i % 3];
        heard.push(Heard {
            user: t,
            platform,
            sub: i % 2 == 0,
            text: String::new(),
        });
    }
    if chat
        .events
        .iter()
        .any(|e| matches!(e, ChatEvent::Raid { .. } | ChatEvent::Sub { .. }))
    {
        crowd.party_until = now + 4.0;
    }
    if !crowd.on {
        heard.clear();
        for m in &mut crowd.members {
            m.leaving.get_or_insert(now);
        }
    }
    // banned or timed out anywhere: shown out, and their pick forgotten
    for key in &chat.banned_now {
        crowd.picks.remove(key);
        for m in &mut crowd.members {
            if &m.key == key {
                m.leaving.get_or_insert(now);
            }
        }
    }
    let host = av.models[av.model].1.clone();
    // the host's eyes go to whoever just spoke (one glance at a time)
    let mut look_at: Option<f32> = None;
    for h in heard {
        let key = viewer_key(h.platform, &h.user);
        if let Some(pick) = pick_of(&h.text, &av.models) {
            crowd.picks.insert(key.clone(), pick);
        }
        let seed = key
            .bytes()
            .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
        let model = crowd
            .picks
            .get(&key)
            .cloned()
            .unwrap_or_else(|| pick_model(&av.models, &host, seed));
        if let Some(i) = crowd
            .members
            .iter()
            .position(|m| m.key == key && m.leaving.is_none())
        {
            let m = &mut crowd.members[i];
            m.last = now;
            m.sub |= h.sub;
            if m.model != model {
                // a new !me pick: swap the character in place
                commands.entity(m.entity).despawn();
                for l in std::mem::take(&mut m.labels) {
                    commands.entity(l).despawn();
                }
                let (e, labels) = spawn_member(
                    &mut commands,
                    &stages,
                    &assets,
                    &model,
                    m.slot,
                    now,
                    &m.user,
                    m.platform,
                    m.sub,
                );
                m.entity = e;
                m.labels = labels;
                m.model = model;
            } else if let Ok(mut c) = people.get_mut(m.entity) {
                c.wave_until = now + 2.0;
            }
            look_at = Some(slot_pos(m.slot).x);
            continue;
        }
        // make room: the one quiet longest leaves
        let live: Vec<usize> = (0..crowd.members.len())
            .filter(|i| crowd.members[*i].leaving.is_none())
            .collect();
        if live.len() >= MAX {
            if let Some(&i) = live
                .iter()
                .min_by(|a, b| crowd.members[**a].last.total_cmp(&crowd.members[**b].last))
            {
                crowd.members[i].leaving = Some(now);
            }
        }
        let taken: Vec<usize> = crowd
            .members
            .iter()
            .filter(|m| m.leaving.is_none())
            .map(|m| m.slot)
            .collect();
        let slot = (0..MAX).find(|s| !taken.contains(s)).unwrap_or(0);
        let (e, labels) = spawn_member(
            &mut commands,
            &stages,
            &assets,
            &model,
            slot,
            now,
            &h.user,
            h.platform,
            h.sub,
        );
        look_at = Some(slot_pos(slot).x);
        crowd.members.push(Member {
            key,
            user: h.user,
            platform: h.platform,
            sub: h.sub,
            model,
            entity: e,
            labels,
            last: now,
            slot,
            leaving: None,
        });
    }
    if let Some(x) = look_at {
        if av.glance.is_none() && !av.asleep {
            av.glance = Some((if x < 0.0 { "eyes_left" } else { "eyes_right" }, now + 0.8));
        }
    }
    // quiet too long: leave
    for m in &mut crowd.members {
        if m.leaving.is_none() && now - m.last > STAY {
            m.leaving = Some(now);
        }
    }
}

/// One fixed tick (`FixedUpdate`): pop in, walk over to a poll side and back, wave, shrink away.
pub fn step(
    mut crowd: ResMut<Crowd>,
    av: Res<crate::Avatar>,
    polls: Res<crate::poll::Polls>,
    time: Res<Time>,
    mut people: Query<(&Chatter, &mut Puppet, &mut crate::sim::Interp)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    // a running poll: voters stand on their option's side (option 1 on the left)
    let votes = polls
        .poll
        .as_ref()
        .filter(|p| matches!(p.phase, crate::poll::Phase::Running))
        .map(|p| &p.votes);
    let hit = now - av.last_hit < 0.9;
    let party = now < crowd.party_until;
    // pop in, idle, wave when talking, shrink away when leaving
    let mut gone = vec![];
    for (i, m) in crowd.members.iter().enumerate() {
        let Ok((c, mut p, mut ip)) = people.get_mut(m.entity) else {
            continue;
        };
        let t = &mut ip.curr;
        let mut target = slot_pos(c.slot);
        if let Some(v) = votes.and_then(|v| v.get(&m.user.to_ascii_lowercase())) {
            let side = if *v == 0 { -1.0 } else { 1.0 };
            // crossing over: stand between the ones already on that side, not on top of them
            if side != target.x.signum() {
                target.x = side * (target.x.abs() + 0.21);
                target.z -= 0.06;
            }
        }
        let grow = ((now - c.t0) / 0.4).clamp(0.0, 1.0);
        let (scale, y) = match m.leaving {
            Some(l) => {
                let k = ((now - l) / 0.5).clamp(0.0, 1.0);
                if k >= 1.0 {
                    gone.push(i);
                }
                (0.4 * (1.0 - k), target.y)
            }
            None => (
                0.4 * (1.0 - (1.0 - grow).powi(3)),
                target.y + 0.6 * (1.0 - grow),
            ),
        };
        // walk over (to a poll side and back) rather than jump
        let to = Vec3::new(target.x, y, target.z);
        let away = to.x - t.translation.x;
        let walking = away.abs() > 0.03 && grow >= 1.0;
        if walking {
            t.translation.x += away.signum() * (1.4 * dt).min(away.abs());
            t.translation.y = y;
            t.translation.z = target.z;
        } else {
            t.translation = to;
        }
        t.scale = Vec3::splat(scale.max(0.001));
        // facing the host a little, or the way they walk
        t.rotation = if walking {
            Quat::from_rotation_y(away.signum() * std::f32::consts::FRAC_PI_2)
        } else {
            Quat::from_rotation_y(-t.translation.x.signum() * 0.5)
        };
        p.want = if walking {
            "Walk"
        } else if hit {
            "Brace"
        } else if party {
            "Rally dance"
        } else if now < c.wave_until {
            "Victory"
        } else {
            "Idle"
        }
        .into();
        p.face = if hit {
            ["eyes_wide", "brows_raised", "mouth_o"]
        } else {
            ["eyes_open", "brows_normal", "mouth_grin"]
        };
    }
    for i in gone.into_iter().rev() {
        let m = crowd.members.remove(i);
        commands.entity(m.entity).despawn();
        for l in m.labels {
            commands.entity(l).despawn();
        }
    }
}

/// Keeps each name sign above its chatter's head, in every view.
pub fn signs(
    stages: Res<crate::stage::Stages>,
    webcam: Res<crate::webcam::Webcam>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform)>,
    owners: Query<(Entity, &GlobalTransform, &Transform), With<Chatter>>,
    mut signs: Query<(&NameSign, &mut Node, &mut Visibility, &ComputedNode)>,
) {
    // neighbours take turns high and low (by where they stand now, so it holds when they move)
    let mut order: Vec<(Entity, f32)> = owners
        .iter()
        .map(|(e, g, _)| (e, g.translation().x))
        .collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    let high: std::collections::HashSet<Entity> = order
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, (e, _))| *e)
        .collect();
    for (s, mut node, mut vis, computed) in &mut signs {
        let Some(v) = stages.views.get(s.view) else {
            continue;
        };
        let Ok((_, g, t)) = owners.get(s.owner) else {
            continue;
        };
        let final_size = if v.window {
            windows
                .single()
                .map(|w| Vec2::new(w.width(), w.height()))
                .unwrap_or(Vec2::splat(900.0))
        } else {
            v.image_size
                .map(|s| s.as_vec2())
                .unwrap_or(Vec2::new(webcam.width as f32, webcam.height as f32))
        };
        let above = g.translation() + Vec3::Y * (1.75 * t.scale.y + 0.05);
        let p = cams
            .get(v.cam3d)
            .ok()
            .and_then(|(cam, tf)| crate::stage::to_final(v, cam, tf, above, final_size));
        match p {
            Some(p) if t.scale.y > 0.2 => {
                let size = computed.size() * computed.inverse_scale_factor();
                node.left = Val::Px(p.x - size.x / 2.0);
                let lift = if high.contains(&s.owner) {
                    size.y * 1.2
                } else {
                    0.0
                };
                node.top = Val::Px(p.y - size.y - lift);
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            _ => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fill_outward_on_both_sides() {
        assert!(slot_pos(0).x < 0.0 && slot_pos(1).x > 0.0);
        assert!(slot_pos(2).x < slot_pos(0).x && slot_pos(3).x > slot_pos(1).x);
        assert!(slot_pos(0).x.abs() > 0.6, "clear of the host");
        let mut c = Crowd::new(true);
        assert_eq!(c.command("on").as_deref(), Some("Chat crowd off"));
        assert_eq!(c.command("test").as_deref(), Some("Test chatters"));
        assert_eq!(c.test.len(), 5);
    }

    #[test]
    fn me_picks_a_character_and_signs_show_subs() {
        let models: Vec<(String, String)> = [
            ("Lead", "models/scene_all.glb"),
            ("Dog", "models/dog.glb"),
            ("Senator bob", "models/senator_bob.glb"),
            ("Senator", "models/senator.glb"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(
            pick_of("!me dog", &models).as_deref(),
            Some("models/dog.glb")
        );
        assert_eq!(
            pick_of("!me Senator", &models).as_deref(),
            Some("models/senator.glb"),
            "an exact name beats a longer one"
        );
        assert_eq!(
            pick_of("!me senator_bob", &models).as_deref(),
            Some("models/senator_bob.glb")
        );
        assert_eq!(
            pick_of("!me bo", &models).as_deref(),
            Some("models/senator_bob.glb")
        );
        assert_eq!(pick_of("!me", &models), None);
        assert_eq!(pick_of("!me unicorn", &models), None);
        assert_eq!(pick_of("hello !me dog", &models), None);
        assert_eq!(sign_text("QueenOfChat", true), "* QueenOfChat");
        assert_eq!(
            sign_text("averyveryverylongname", false).chars().count(),
            14
        );
        assert_eq!(viewer_key(Platform::Kick, "Bob"), "kick:bob");
        assert_ne!(
            viewer_key(Platform::Kick, "bob"),
            viewer_key(Platform::Twitch, "bob")
        );
    }
}
