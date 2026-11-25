//! Making a one-person stream look produced:
//!
//! - **Sets**: voxel backdrops that replace the green screen (rally stage, press room, diner,
//!   hospital, newsroom, night city; built by tools/build_sets.py). Pick one on the control page,
//!   or "auto" to match the character (the medic gets the hospital, the cook the diner...).
//!   An OBS scene switcher or Stream Deck can change it with `GET /cmd/set:diner`.
//! - **Auto-director**: cuts between close-up, waist-up and wide on its own: wide after a long
//!   silence, back in when you talk, a punch-in on a splat or a laugh, and a change of angle every
//!   so often while you talk. Raids, guests and the co-host still get their own shots.
//! - **Press conference**: a podium in front of you, and the approved viewer questions waiting
//!   as a row of little reporters along the bottom (each in the character they picked); the next
//!   one waves their arms to be picked. Call on them with **A** or Air next.

use bevy::{gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAssetRoot};

use crate::puppet::Puppet;
use crate::questions::Status;

pub const SETS: [&str; 7] = [
    "rally",
    "pressroom",
    "diner",
    "hospital",
    "newsroom",
    "night",
    "yard",
];

/// The set that suits a character, for "auto".
pub fn set_for(model: &str) -> Option<&'static str> {
    let m = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .trim_end_matches(".glb");
    Some(match m {
        "roasted_medic" => "hospital",
        "roasted_cook" => "diner",
        "roasted_garbage" | "roasted_miami" | "riot_a" | "riot_b" | "roasted_riot" | "officer"
        | "plainclothes" => "night",
        "reporter" | "staffer" => "newsroom",
        "scene_all" | "rally_hat" | "rally_sign" | "roasted_army" => "rally",
        _ if m.starts_with("senator")
            || m.starts_with("rep")
            || m.starts_with("agent")
            || ["president", "vice_president", "judge", "executive"].contains(&m) =>
        {
            "pressroom"
        }
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shot {
    Close,
    Medium,
    Wide,
    Punch,
}

impl Shot {
    pub fn framing(self) -> usize {
        match self {
            Shot::Close => 0,
            Shot::Medium => 1,
            Shot::Wide => 2,
            Shot::Punch => 4,
        }
    }
}

#[derive(Resource)]
pub struct Show {
    /// "none", a set name, or "auto"
    pub set: String,
    set_entity: Option<(String, Entity)>,
    pub director: bool,
    pub shot: Shot,
    punch_until: f32,
    last_cut: f32,
    talk_since: Option<f32>,
    quiet_since: f32,
    seen_hit: f32,
    seen_laugh: f32,
    pub press: bool,
    podium: Option<Entity>,
    row: Vec<(u32, Entity)>,
    row_group: Option<Entity>,
}

impl Show {
    pub fn new(set: String, director: bool, press: bool) -> Self {
        let set = if set == "auto" || SETS.contains(&set.as_str()) {
            set
        } else {
            "none".into()
        };
        Show {
            set,
            set_entity: None,
            director,
            shot: Shot::Close,
            punch_until: 0.0,
            last_cut: 0.0,
            talk_since: None,
            quiet_since: 0.0,
            seen_hit: 0.0,
            seen_laugh: 0.0,
            press,
            podium: None,
            row: vec![],
            row_group: None,
        }
    }

    /// The set showing for this character right now.
    /// The set on stage right now, if any.
    pub fn live_set(&self) -> Option<&str> {
        self.set_entity.as_ref().map(|(n, _)| n.as_str())
    }

    pub fn current_set(&self, model: &str) -> Option<String> {
        match self.set.as_str() {
            "none" | "" => None,
            "auto" => set_for(model).map(|s| s.to_string()),
            s => Some(s.to_string()),
        }
    }

    /// set:<name|none|auto|next>, director, press
    pub fn command(&mut self, k: &str, v: &str) -> Option<String> {
        Some(match k {
            "set" => {
                let v = v.trim().to_ascii_lowercase();
                self.set = match v.as_str() {
                    "next" => {
                        let i = SETS
                            .iter()
                            .position(|s| *s == self.set)
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        SETS.get(i)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "none".into())
                    }
                    "none" | "off" | "green" => "none".into(),
                    "auto" => "auto".into(),
                    // the built-in sets, and any block level (yours too, from your levels folder)
                    s if SETS.contains(&s) || crate::blocks::level_path(s).is_some() => s.to_string(),
                    _ => return Some(format!("No set called {v}; sets: {}", SETS.join(", "))),
                };
                format!("Set: {}", self.set)
            }
            "director" => {
                self.director = !self.director;
                format!("Auto-director {}", if self.director { "on" } else { "off" })
            }
            "press" => {
                self.press = !self.press;
                format!("Press conference {}", if self.press { "on" } else { "off" })
            }
            _ => return None,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut show: ResMut<Show>,
    mut av: ResMut<crate::Avatar>,
    mut stages: ResMut<crate::stage::Stages>,
    guest: Res<crate::guest::Guest>,
    polls: Res<crate::poll::Polls>,
    moments: Res<crate::events::Moments>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    cams: Query<Entity, With<crate::MainCamera>>,
    mut puppets: Query<&mut Puppet>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let model = av.models[av.model].1.clone();
    // ---- set
    let want = show.current_set(&model);
    if show.set_entity.as_ref().map(|s| &s.0) != want.as_ref() {
        if let Some((_, e)) = show.set_entity.take() {
            commands.entity(e).try_despawn();
        }
        if let Some(name) = want.clone() {
            // a block level (data/levels) or a modelled set (assets/sets)
            let e = if crate::blocks::level_path(&name).is_some() {
                commands
                    .spawn((crate::blocks::LevelRoot(name.clone()), Transform::default(), Visibility::default()))
                    .id()
            } else {
                commands
                    .spawn((
                        WorldAssetRoot(
                            assets.load(
                                GltfAssetLabel::Scene(0).from_asset(format!("sets/{name}.glb")),
                            ),
                        ),
                        Transform::default(),
                    ))
                    .id()
            };
            show.set_entity = Some((name, e));
        }
    }
    stages.set_model = want.map(|s| format!("sets/{s}.glb"));

    // ---- auto-director
    if show.director {
        let speaking = av.mouth != crate::lipsync::Mouth::Rest;
        if speaking {
            show.talk_since.get_or_insert(now);
            show.quiet_since = now;
        } else if now - show.quiet_since > 0.8 {
            show.talk_since = None;
        }
        let special = av.two_shot || av.cohost_on || now < moments.wide_until;
        let mut next = show.shot;
        if av.last_hit > show.seen_hit {
            show.seen_hit = av.last_hit;
            show.punch_until = now + 1.6;
        }
        if av.laugh_until > now && show.seen_laugh < av.laugh_until - 2.0 {
            show.seen_laugh = av.laugh_until;
            show.punch_until = now + 1.8;
        }
        if now < show.punch_until {
            next = Shot::Punch;
        } else if now - show.quiet_since > 8.0 {
            next = Shot::Wide;
        } else if speaking && matches!(show.shot, Shot::Wide | Shot::Punch) {
            next = Shot::Close;
        } else if show.talk_since.is_some_and(|t| now - t > 6.0) && now - show.last_cut > 9.0 {
            next = if show.shot == Shot::Close {
                Shot::Medium
            } else {
                Shot::Close
            };
        }
        if next != show.shot && !special {
            show.shot = next;
            show.last_cut = now;
            av.cut = true;
        }
        if !special {
            av.framing = show.shot.framing();
        }
    }

    // ---- press conference: podium, and the approved questions as a row of reporters
    let podium_on = show.press;
    match (podium_on, show.podium) {
        (true, None) => {
            let e =
                commands
                    .spawn((
                        WorldAssetRoot(assets.load(
                            GltfAssetLabel::Scene(0).from_asset("props/cast/pres_podium.glb"),
                        )),
                        Transform::from_xyz(0.0, 0.0, 0.42),
                    ))
                    .id();
            show.podium = Some(e);
        }
        (false, Some(e)) => {
            commands.entity(e).try_despawn();
            show.podium = None;
        }
        _ => {}
    }
    let row_visible = show.press && polls.poll.is_none();
    let wanted: Vec<(u32, String)> = if row_visible {
        let q = guest.queue.lock().unwrap();
        q.waiting()
            .into_iter()
            .filter(|x| x.status == Status::Approved)
            .take(5)
            .map(|x| (x.id, x.model.clone()))
            .collect()
    } else {
        vec![]
    };
    let ids: Vec<u32> = wanted.iter().map(|w| w.0).collect();
    if show.row.iter().map(|r| r.0).collect::<Vec<_>>() != ids {
        if let Some(g) = show.row_group.take() {
            commands.entity(g).try_despawn();
        }
        show.row.clear();
        if !wanted.is_empty() {
            if let Some(cam) = cams.iter().next() {
                let g = commands
                    .spawn((
                        Transform::from_xyz(0.0, -0.40, -1.2),
                        Visibility::default(),
                        ChildOf(cam),
                    ))
                    .id();
                let n = wanted.len() as f32;
                for (i, (id, m)) in wanted.iter().enumerate() {
                    let x = (i as f32 - (n - 1.0) / 2.0) * 0.16;
                    let mut p = Puppet::new(m, &assets);
                    p.face = ["eyes_open", "brows_raised", "mouth_flat"];
                    // facing you (away from the camera), turned a little toward the middle
                    let rot = Quat::from_rotation_y(std::f32::consts::PI - x * 1.2);
                    let e = commands
                        .spawn((
                            p,
                            Transform::from_xyz(x, 0.0, 0.0)
                                .with_rotation(rot)
                                .with_scale(Vec3::splat(0.1)),
                            ChildOf(g),
                        ))
                        .id();
                    show.row.push((*id, e));
                }
                show.row_group = Some(g);
            }
        }
    }
    // the next one up waves to be picked; the rest wait, now and then shuffling
    let next = show.row.first().map(|r| r.1);
    for (k, (_, e)) in show.row.iter().enumerate() {
        if let Ok(mut p) = puppets.get_mut(*e) {
            p.want = if Some(*e) == next {
                "Rally chant"
            } else if ((now * 0.3 + k as f32) as u32) % 5 == 0 {
                "Emotes"
            } else {
                "Idle"
            }
            .into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_and_commands() {
        for s in SETS.iter().filter(|s| crate::blocks::level_path(s).is_none()) {
            assert!(
                crate::asset_base()
                    .join("assets/sets")
                    .join(format!("{s}.glb"))
                    .exists(),
                "{s}"
            );
        }
        assert_eq!(set_for("models/roasted_medic.glb"), Some("hospital"));
        assert_eq!(set_for("models/senator_plum.glb"), Some("pressroom"));
        assert_eq!(set_for("models/dog.glb"), None);
        let mut s = Show::new("none".into(), false, false);
        assert_eq!(s.current_set("models/roasted_cook.glb"), None);
        s.command("set", "auto");
        assert_eq!(
            s.current_set("models/roasted_cook.glb").as_deref(),
            Some("diner")
        );
        s.command("set", "next");
        assert_eq!(s.set, "rally");
        s.command("set", "night");
        assert_eq!(s.current_set("models/dog.glb").as_deref(), Some("night"));
        assert!(s.command("set", "moon").unwrap().contains("No set"));
        assert_eq!(
            s.command("director", "").as_deref(),
            Some("Auto-director on")
        );
    }
}
