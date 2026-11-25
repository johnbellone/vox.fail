//! Twitch moments on stage:
//! - **Raid:** a crowd of little characters (one per 10 raiders, 3 to 16) runs in from both
//!   sides, cheers in front of you for a few seconds and runs off; the camera goes wide.
//! - **Sub / resub / gift subs:** confetti, your character celebrates, and a thank-you line.
//! - **Bits** are in chat.rs (bigger throws).
//!
//! Control page: "Test raid" and "Test sub". Settings: `raid_crowd=false`, `sub_party=false`.

use bevy::prelude::*;

use crate::chat::ChatEvent;
use crate::puppet::{Puppet, pick_model};

const RUN_IN: f32 = 2.0;
const CHEER: f32 = 6.0;
const RUN_OUT: f32 = 2.2;

#[derive(Resource)]
pub struct Moments {
    pub raid_crowd: bool,
    pub sub_party: bool,
    pub max_crowd: u32,
    /// the camera stays wide until then
    pub wide_until: f32,
    pub last: String,
    test: Vec<ChatEvent>,
}

impl Moments {
    pub fn new(raid_crowd: bool, sub_party: bool) -> Self {
        Moments {
            raid_crowd,
            sub_party,
            max_crowd: 16,
            wide_until: 0.0,
            last: String::new(),
            test: vec![],
        }
    }
    /// event:raid, event:sub, event:gift, event:crowd (toggle), event:subs (toggle)
    pub fn command(&mut self, arg: &str) -> Option<String> {
        Some(match arg {
            "raid" => {
                self.test.push(ChatEvent::Raid {
                    from: "TestRaider".into(),
                    viewers: 60,
                });
                "Test raid".into()
            }
            "sub" => {
                self.test.push(ChatEvent::Sub {
                    user: "TestFan".into(),
                    months: 3,
                    gifts: 0,
                });
                "Test sub".into()
            }
            "gift" => {
                self.test.push(ChatEvent::Sub {
                    user: "TestSanta".into(),
                    months: 0,
                    gifts: 5,
                });
                "Test gift subs".into()
            }
            "crowd" => {
                self.raid_crowd = !self.raid_crowd;
                format!("Raid crowd {}", if self.raid_crowd { "on" } else { "off" })
            }
            "subs" => {
                self.sub_party = !self.sub_party;
                format!(
                    "Sub celebrations {}",
                    if self.sub_party { "on" } else { "off" }
                )
            }
            _ => return None,
        })
    }
}

#[derive(Component)]
pub struct Raider {
    from: Vec3,
    to: Vec3,
    out: Vec3,
    t0: f32,
    hop: f32,
}

/// How many little characters a raid of this size brings.
pub fn crowd_size(viewers: u32, max: u32) -> u32 {
    viewers.div_ceil(10).clamp(3, max.max(3))
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut m: ResMut<Moments>,
    mut chat: ResMut<crate::chat::Chat>,
    mut inject: ResMut<crate::Inject>,
    av: Res<crate::Avatar>,
    mut fx: ResMut<crate::fx::Fx>,
    mut cues: ResMut<crate::lighting::Cues>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut raiders: Query<(Entity, &Raider, &mut Puppet, &mut Transform)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let mut evs = std::mem::take(&mut m.test);
    evs.extend(chat.events.iter().cloned());
    for ev in evs {
        match ev {
            ChatEvent::Raid { from, viewers } => {
                m.last = format!("Raid from {from} ({viewers})");
                fx.play("applause", 1.0);
                cues.0.push(crate::lighting::Cue::Raid);
                chat.credit = Some((format!("RAID! {from} brought {viewers} friends"), now));
                chat.queue
                    .push((crate::chat::Throw::Confetti, crate::chat::QUIET.into()));
                inject
                    .0
                    .extend(["play:Victory".to_string(), "face:1".into()]);
                if !m.raid_crowd {
                    continue;
                }
                m.wide_until = now + RUN_IN + CHEER + 0.8;
                let n = crowd_size(viewers, m.max_crowd);
                let host = av.models[av.model].1.clone();
                for i in 0..n {
                    let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                    let row = (i / 8) as f32;
                    let in_row = (n - (i / 8) * 8).min(8);
                    let k = if in_row <= 1 {
                        0.5
                    } else {
                        (i % 8) as f32 / (in_row - 1) as f32
                    };
                    // a loose arc in front of you, back row a little higher up the frame
                    let to = Vec3::new(
                        (k - 0.5) * 2.6 + (crate::hash01(i, now) - 0.5) * 0.15,
                        0.0,
                        1.25 - row * 0.45 + (crate::hash01(i + 50, now) - 0.5) * 0.15,
                    );
                    let from = Vec3::new(side * (3.2 + crate::hash01(i + 9, now)), 0.0, to.z);
                    let out = Vec3::new(-side * 3.6, 0.0, to.z);
                    let mut p =
                        Puppet::new(&pick_model(&av.models, &host, i * 31 + now as u32), &assets);
                    p.want = "Run".into();
                    p.speed = 1.3;
                    commands.spawn((
                        p,
                        Transform::from_translation(from)
                            .with_scale(Vec3::splat(0.42))
                            .with_rotation(Quat::from_rotation_y(
                                -side * std::f32::consts::FRAC_PI_2,
                            )),
                        Raider {
                            from,
                            to,
                            out,
                            t0: now + i as f32 * 0.08,
                            hop: crate::hash01(i + 77, now) * 6.0,
                        },
                    ));
                }
            }
            ChatEvent::Sub {
                user,
                months,
                gifts,
            } => {
                let line = if gifts > 0 {
                    format!(
                        "{user} gifted {gifts} sub{}!",
                        if gifts == 1 { "" } else { "s" }
                    )
                } else if months > 1 {
                    format!("{user} subscribed for {months} months!")
                } else {
                    format!("{user} just subscribed!")
                };
                m.last = line.clone();
                chat.credit = Some((line, now));
                fx.play("firework", 0.8);
                cues.0.push(crate::lighting::Cue::Sub);
                if m.sub_party {
                    chat.queue
                        .push((crate::chat::Throw::Confetti, crate::chat::QUIET.into()));
                    inject.0.extend([
                        "play:Victory".to_string(),
                        "face:1".into(),
                        "emote:hop".into(),
                    ]);
                }
            }
            _ => {}
        }
    }
    // run in, cheer, run off
    for (e, r, mut p, mut t) in &mut raiders {
        let s = now - r.t0;
        if s < 0.0 {
            continue;
        }
        if s < RUN_IN {
            let u = s / RUN_IN;
            let u = 1.0 - (1.0 - u) * (1.0 - u);
            t.translation = r.from.lerp(r.to, u);
            p.want = "Run".into();
        } else if s < RUN_IN + CHEER {
            t.translation = r.to;
            t.rotation = t.rotation.slerp(Quat::IDENTITY, 0.15);
            let c = ((s - RUN_IN + r.hop) as u32) % 3;
            p.want = ["Victory", "Rally chant", "Jump"][c as usize].into();
            p.speed = 1.0;
            p.face = ["eyes_wide", "brows_raised", "mouth_open"];
        } else if s < RUN_IN + CHEER + RUN_OUT {
            let u = (s - RUN_IN - CHEER) / RUN_OUT;
            t.translation = r.to.lerp(r.out, u * u);
            let dir = (r.out - r.to).x.signum();
            t.rotation = Quat::from_rotation_y(dir * std::f32::consts::FRAC_PI_2);
            p.want = "Run".into();
            p.speed = 1.3;
            p.face = ["eyes_open", "brows_normal", "mouth_grin"];
        } else {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crowd_sizes() {
        assert_eq!(super::crowd_size(1, 16), 3);
        assert_eq!(super::crowd_size(42, 16), 5);
        assert_eq!(super::crowd_size(5000, 16), 16);
    }
}
