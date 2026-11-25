//! The face and body on top of the animation clips: expressions, emotes and their poses, idle
//! life, and `drive_face`, which sets the face parts and layers the poses every frame.

use super::*;

pub(crate) use warren_world::pose::FACE_PARTS;
/// Gestures on F1-F8, looked up by clip name; missing ones are skipped. F9-F12 play the
/// character's own clips (the ones after the 100 shared clips).
pub(crate) const GESTURES: [&str; 8] = [
    "Emotes",
    "Victory",
    "Rally chant",
    "Rally dance",
    "Hit reaction (heavy)",
    "Knockdown and get up",
    "Jab",
    "Cover up",
];

// ------------------------------------------------------------------------------------------
// Expressions: eyes + brows, and which mouth shape each viseme uses
// ------------------------------------------------------------------------------------------
pub(crate) struct Expression {
    pub(crate) name: &'static str,
    pub(crate) eyes: &'static str,
    pub(crate) brows: &'static str,
    /// rest, mid, open, round, bright, teeth (s/sh/f), closed (m/b/p)
    pub(crate) mouths: [&'static str; 7],
    pub(crate) blink: bool,
    pub(crate) dart: bool,
}

pub(crate) const EXPRESSIONS: [Expression; 10] = [
    Expression {
        name: "Neutral",
        eyes: "eyes_open",
        brows: "brows_normal",
        mouths: [
            "mouth_flat",
            "mouth_o",
            "mouth_open",
            "mouth_o",
            "mouth_grin",
            "mouth_grit",
            "mouth_pout",
        ],
        blink: true,
        dart: false,
    },
    Expression {
        name: "Happy",
        eyes: "eyes_open",
        brows: "brows_raised",
        mouths: [
            "mouth_grin",
            "mouth_grin",
            "mouth_open",
            "mouth_o",
            "mouth_grin",
            "mouth_grit",
            "mouth_smirk",
        ],
        blink: true,
        dart: false,
    },
    Expression {
        name: "Angry",
        eyes: "eyes_open",
        brows: "brows_angry",
        mouths: [
            "mouth_grit",
            "mouth_grit",
            "mouth_open",
            "mouth_open",
            "mouth_grit",
            "mouth_grit",
            "mouth_frown",
        ],
        blink: true,
        dart: false,
    },
    Expression {
        name: "Sad",
        eyes: "eyes_open",
        brows: "brows_worried",
        mouths: [
            "mouth_sad",
            "mouth_sad",
            "mouth_open",
            "mouth_o",
            "mouth_frown",
            "mouth_wavy",
            "mouth_pout",
        ],
        blink: true,
        dart: false,
    },
    Expression {
        name: "Surprised",
        eyes: "eyes_wide",
        brows: "brows_raised",
        mouths: [
            "mouth_o",
            "mouth_o",
            "mouth_open",
            "mouth_o",
            "mouth_open",
            "mouth_grit",
            "mouth_pout",
        ],
        blink: true,
        dart: false,
    },
    Expression {
        name: "Smug",
        eyes: "eyes_squint",
        brows: "brows_angry",
        mouths: [
            "mouth_smirk",
            "mouth_smirk",
            "mouth_grin",
            "mouth_pout",
            "mouth_grin",
            "mouth_grit",
            "mouth_smirk",
        ],
        blink: true,
        dart: false,
    },
    Expression {
        name: "Suspicious",
        eyes: "eyes_squint",
        brows: "brows_angry",
        mouths: [
            "mouth_flat",
            "mouth_flat",
            "mouth_open",
            "mouth_o",
            "mouth_flat",
            "mouth_grit",
            "mouth_pout",
        ],
        blink: true,
        dart: true,
    },
    Expression {
        name: "Nervous",
        eyes: "eyes_open",
        brows: "brows_worried",
        mouths: [
            "mouth_wavy",
            "mouth_wavy",
            "mouth_open",
            "mouth_o",
            "mouth_wavy",
            "mouth_grit",
            "mouth_flat",
        ],
        blink: true,
        dart: true,
    },
    Expression {
        name: "Rage",
        eyes: "eyes_wide",
        brows: "brows_angry",
        mouths: [
            "mouth_grit",
            "mouth_grit",
            "mouth_open",
            "mouth_open",
            "mouth_grit",
            "mouth_grit",
            "mouth_frown",
        ],
        blink: false,
        dart: false,
    },
    Expression {
        name: "KO",
        eyes: "eyes_x",
        brows: "brows_normal",
        mouths: [
            "mouth_tongue",
            "mouth_open",
            "mouth_open",
            "mouth_o",
            "mouth_open",
            "mouth_wavy",
            "mouth_tongue",
        ],
        blink: false,
        dart: false,
    },
];
pub(crate) const SURPRISED: usize = 4;
pub(crate) const RAGE: usize = 8;

// ------------------------------------------------------------------------------------------
// Procedural emotes: short moves layered on top of whatever body clip is playing
// ------------------------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Emote {
    Nod,
    Shake,
    Tilt,
    Hop,
    Spin,
    Bow,
    Shiver,
    LeanIn,
    Jolt,
    // idle life: played on their own during long quiet stretches
    LookAround,
    WeightShift,
    Stretch,
    Watch,
}

/// (emote, name, key hint)
pub(crate) const EMOTES: [(Emote, &str); 8] = [
    (Emote::Nod, "nod"),
    (Emote::Shake, "shake"),
    (Emote::Tilt, "tilt"),
    (Emote::Hop, "hop"),
    (Emote::Spin, "spin"),
    (Emote::Bow, "bow"),
    (Emote::Shiver, "shiver"),
    (Emote::LeanIn, "lean"),
];

impl Emote {
    pub(crate) fn duration(self) -> f32 {
        match self {
            Emote::Nod => 0.9,
            Emote::Shake => 1.0,
            Emote::Tilt => 1.6,
            Emote::Hop => 0.7,
            Emote::Spin => 0.9,
            Emote::Bow => 1.8,
            Emote::Shiver => 1.0,
            Emote::LeanIn => 2.2,
            Emote::Jolt => 0.45,
            Emote::LookAround => 3.2,
            Emote::WeightShift => 2.4,
            Emote::Stretch => 2.8,
            Emote::Watch => 2.6,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Emote::Jolt => "jolt",
            Emote::LookAround => "look",
            Emote::WeightShift => "shift",
            Emote::Stretch => "stretch",
            Emote::Watch => "watch",
            _ => EMOTES
                .iter()
                .find(|e| e.0 == self)
                .map(|e| e.1)
                .unwrap_or("jolt"),
        }
    }
}

/// Offsets one emote adds at progress `t` (0..1).
#[derive(Default)]
pub(crate) struct Pose {
    pub(crate) head: Vec3,   // euler x (nod forward), y (turn), z (tilt)
    pub(crate) torso_x: f32, // forward bend at the waist, legs stay put
    pub(crate) root_y: f32,
    pub(crate) root_z: f32,
    pub(crate) root_x: f32,
    pub(crate) root_spin: f32,
    /// squash (-) and stretch (+) of the whole body, from the feet
    pub(crate) stretch: f32,
    pub(crate) arm_l: f32, // forward lift added to the upper arms (talk beats)
    pub(crate) arm_r: f32,
}

/// The hop's take-off and landing, as fractions of the emote.
pub(crate) const HOP_UP: f32 = 0.16;
pub(crate) const HOP_LAND: f32 = 0.8;

pub(crate) fn emote_pose(e: Emote, t: f32) -> Pose {
    use std::f32::consts::{PI, TAU};
    let t = t.clamp(0.0, 1.0);
    let env = (PI * t).sin();
    // rises fast, holds, falls: for moves that should "stick" for a beat
    let hold = (t.min(1.0 - t) * 5.0).min(1.0);
    let hold = hold * hold * (3.0 - 2.0 * hold);
    let mut p = Pose::default();
    match e {
        Emote::Nod => p.head.x = (t * TAU * 2.0).sin().max(-0.3) * 0.28 * env,
        Emote::Shake => p.head.y = (t * TAU * 3.0).sin() * 0.38 * env,
        Emote::Tilt => {
            p.head.z = 0.32 * hold;
            p.head.x = -0.05 * hold;
        }
        Emote::Hop => {
            // crouch, spring up stretched, land squashed
            let (a, b) = (HOP_UP, HOP_LAND);
            if t < a {
                p.stretch = -0.10 * (PI * t / a).sin();
            } else if t < b {
                let u = (t - a) / (b - a);
                p.root_y = 0.32 * (PI * u).sin();
                p.head.x = -0.12 * (PI * u).sin();
                p.stretch = 0.07 * (1.0 - u * 2.0).max(0.0) - 0.03 * ((u - 0.85) / 0.15).max(0.0);
            } else {
                p.stretch = -0.13 * (PI * (t - b) / (1.0 - b)).sin();
                p.head.x = 0.06 * (PI * (t - b) / (1.0 - b)).sin();
            }
        }
        Emote::Spin => {
            let s = t * t * (3.0 - 2.0 * t);
            p.root_spin = TAU * s;
            p.root_y = 0.08 * env;
        }
        Emote::Bow => {
            p.torso_x = 0.55 * hold;
            p.head.x = 0.15 * hold;
        }
        Emote::Shiver => {
            p.root_x = (t * 95.0).sin() * 0.018 * env;
            p.head.z = (t * 80.0).sin() * 0.05 * env;
        }
        Emote::LeanIn => {
            p.torso_x = 0.22 * hold;
            p.root_z = 0.25 * hold;
            p.head.x = -0.08 * hold;
            p.head.z = 0.08 * hold;
        }
        Emote::Jolt => {
            p.head.x = -0.35 * env;
            p.torso_x = -0.12 * env;
            p.root_z = -0.06 * env;
        }
        // a slow look left, then right, then back
        Emote::LookAround => {
            p.head.y = (t * TAU).sin() * 0.5 * hold;
            p.head.x = -0.06 * hold;
        }
        // weight onto one foot and back
        Emote::WeightShift => {
            p.root_x = 0.035 * env;
            p.root_spin = 0.1 * env;
            p.head.z = -0.06 * env;
            p.torso_x = 0.03 * env;
        }
        // arms up overhead, lean back, head up
        Emote::Stretch => {
            p.arm_l = 2.5 * hold;
            p.arm_r = 2.5 * hold;
            p.torso_x = -0.14 * hold;
            p.head.x = -0.22 * hold;
            p.stretch = 0.04 * hold;
        }
        // raise the left arm and glance down at the wrist
        Emote::Watch => {
            p.arm_l = 1.35 * hold;
            p.head.x = 0.3 * hold;
            p.head.y = 0.28 * hold;
        }
    }
    p
}

impl Pose {
    /// This pose plus `k` times another (for blending one emote into the next).
    pub(crate) fn add_scaled(&mut self, o: &Pose, k: f32) {
        self.head += o.head * k;
        self.torso_x += o.torso_x * k;
        self.root_y += o.root_y * k;
        self.root_z += o.root_z * k;
        self.root_x += o.root_x * k;
        self.root_spin += o.root_spin * k;
        self.stretch += o.stretch * k;
        self.arm_l += o.arm_l * k;
        self.arm_r += o.arm_r * k;
    }
}

/// Idle life: during a long quiet stretch, now and then look around, shift weight, stretch or
/// check a watch.
pub(crate) const IDLES: [Emote; 4] = [
    Emote::LookAround,
    Emote::WeightShift,
    Emote::Stretch,
    Emote::Watch,
];

pub(crate) fn idle_life(mut av: ResMut<Avatar>, lip: Res<Lip>, time: Res<Time>) {
    let now = time.elapsed_secs();
    let quiet = lip.picker.silent_for;
    if av.asleep || av.emote.is_some() || av.singing || quiet < 6.0 || av.muted || !av.idle_life {
        if quiet < 6.0 {
            av.next_idle = av.next_idle.max(now + 4.0);
        }
        return;
    }
    if now >= av.next_idle {
        let r = av.rand01();
        let e = IDLES[((r * IDLES.len() as f32) as usize).min(IDLES.len() - 1)];
        av.emote = Some((e, now));
        let r = av.rand01();
        av.next_idle = now + e.duration() + 6.0 + 8.0 * r;
    }
}

/// Runs after the animation step: sets the face parts from expression + mouth shape + blink, and
/// layers the voice nod, emotes, bop and dozing on top of the body clip. Overriding here wins over
/// the faces baked into clips.
pub(crate) fn drive_face(
    mut av: ResMut<Avatar>,
    mut fx: ResMut<fx::Fx>,
    time: Res<Time>,
    mut last_emote: Local<Option<(Emote, f32)>>,
    mut parts: Query<(&FacePart, &mut Transform), Without<Joint>>,
    mut joints: Query<(&Joint, &mut Transform), Without<FacePart>>,
    cap: Res<crate::facecap::FaceCap>,
) {
    let now = time.elapsed_secs();
    let tracked = cap.live(now).map(|(r, head)| (crate::facecap::choose(&r.shapes), head));
    let ex_i = av.face_override.map(|o| o.0).unwrap_or(av.expression);
    let ex = &EXPRESSIONS[ex_i];
    // blinks: mostly quick, sometimes a double, now and then a slow one
    if ex.blink && now >= av.next_blink {
        let r = av.rand01();
        let len = if r < 0.1 { 0.28 } else { 0.11 };
        av.blink_until = now + len;
        av.blink_again = if (0.1..0.28).contains(&r) {
            now + len + 0.14
        } else {
            -1.0
        };
        let r = av.rand01();
        av.next_blink = now + 1.8 + 4.0 * r * r + 0.6;
    }
    if av.blink_again > 0.0 && now >= av.blink_again {
        av.blink_until = now + 0.1;
        av.blink_again = -1.0;
    }
    // eyes: little glances away and back while idle (only from plain open eyes)
    if now >= av.next_saccade {
        let (r1, r2) = (av.rand01(), av.rand01());
        av.next_saccade = now + 2.5 + 5.0 * r1;
        if ex.eyes == "eyes_open" && av.glance.is_none() && !av.asleep && now - av.last_hit > 2.0 {
            let part = if r2 < 0.42 {
                "eyes_left"
            } else if r2 < 0.84 {
                "eyes_right"
            } else {
                "eyes_up"
            };
            av.glance = Some((part, now + 0.35 + 0.6 * r1));
            // a blink often comes with a look
            if r2 > 0.6 {
                av.next_blink = av.next_blink.min(now + 0.05);
            }
        }
    }
    if av.glance.is_some_and(|g| now >= g.1) {
        av.glance = None;
    }
    if ex.dart && now >= av.next_dart {
        av.dart_side = !av.dart_side;
        let r = av.rand01();
        av.next_dart = now + 0.4 + 0.7 * r;
    }
    let (mut eyes, mut brows, mut mouth) = (ex.eyes, ex.brows, ex.mouths[av.mouth.slot()]);
    if ex.blink && now < av.blink_until {
        eyes = "eyes_shut";
    } else if ex.dart {
        eyes = if av.dart_side {
            "eyes_left"
        } else {
            "eyes_right"
        };
    } else if let Some((g, _)) = av.glance {
        // a glance (toward an incoming tomato, or a saccade) from eyes that aren't doing anything special
        if matches!(ex.eyes, "eyes_open" | "eyes_wide") {
            eyes = g;
        }
    }
    // body language: brows follow your pitch and go up on questions (only from a neutral brow)
    if av.prosody_on && brows == "brows_normal" && (av.brows_up || now < av.question_until) {
        brows = "brows_raised";
    }
    if now < av.laugh_until && !av.asleep {
        // cracking up: eyes squeezed, mouth popping open with each "ha"
        eyes = "eyes_shut";
        brows = "brows_raised";
        mouth = if (now * 7.0).sin() > -0.2 {
            "mouth_open"
        } else {
            "mouth_grin"
        };
    }
    if av.asleep {
        // eyes shut, peeking now and then; mouth hangs a little open
        eyes = if (now % 7.0) < 0.6 {
            "eyes_squint"
        } else {
            "eyes_shut"
        };
        brows = "brows_normal";
        mouth = if av.mouth == Mouth::Rest {
            "mouth_wavy"
        } else {
            mouth
        };
    }
    // live face capture: your eyes, brows and mouth shape (the voice still opens the mouth)
    if let (Some((c, _)), false) = (tracked, av.asleep) {
        eyes = c.eyes;
        brows = c.brows;
        if let Some(m) = c.mouth {
            mouth = m;
        } else if av.mouth == Mouth::Rest && c.jaw > 0.35 {
            mouth = "mouth_open";
        }
    }
    for (part, mut t) in &mut parts {
        let on = part.0 == eyes || part.0 == brows || part.0 == mouth;
        t.scale = if on { Vec3::ONE } else { Vec3::ZERO };
    }

    // ---- layered motion
    let mut pose = Pose::default();
    // a new emote cutting in on one that's still playing: the old one fades out over 0.2 s
    if let Some((e, start)) = av.emote {
        if *last_emote != Some((e, start)) {
            if let Some((pe, ps)) = *last_emote {
                if (now - ps) / pe.duration() < 1.0 {
                    av.emote_prev = Some((pe, ps, now));
                }
            }
            *last_emote = Some((e, start));
        }
    } else {
        *last_emote = None;
    }
    let mut fading = Pose::default();
    if let Some((pe, ps, cut)) = av.emote_prev {
        let k = 1.0 - (now - cut) / 0.2;
        if k <= 0.0 {
            av.emote_prev = None;
        } else {
            fading.add_scaled(&emote_pose(pe, (now - ps) / pe.duration()), k);
        }
    }
    if let Some((e, start)) = av.emote {
        let t = (now - start) / e.duration();
        if t >= 1.0 {
            av.emote = None;
        } else {
            pose = emote_pose(e, t);
            pose.add_scaled(&fading, 1.0);
            // touchdown: a thump and a puff of dust
            let before = (now - time.delta_secs() - start) / e.duration();
            if e == Emote::Hop && before < HOP_LAND && t >= HOP_LAND {
                fx.play("thud", 0.45);
                fx.dust.push((Vec3::new(0.0, 0.0, 0.05), 0.8));
            }
        }
    }
    if av.emote.is_none() {
        pose.add_scaled(&fading, 1.0);
    }
    // voice nod and sway
    let lively = if av.prosody_on { av.liveliness } else { 1.0 };
    pose.head.x += av.level * 0.10
        + if av.mouth != Mouth::Rest {
            (now * 7.0).sin() * 0.025 * lively
        } else {
            0.0
        };
    if av.prosody_on {
        // stressed syllable: a quick nod, and a small hand beat on alternating arms
        let b = (now - av.talk_beat_t) / 0.3;
        if b < 1.0 {
            let k = (std::f32::consts::PI * b).sin() * av.talk_beat_amp;
            pose.head.x += 0.09 * k * lively;
            if !av.gesture_active {
                if av.talk_beat_left {
                    pose.arm_l += 0.30 * k
                } else {
                    pose.arm_r += 0.30 * k
                }
            }
        }
        // question: tilt and turn the head a little, held for a moment after the phrase
        if now < av.question_until {
            let start = av.question_until - 1.3;
            let e = ((now - start) / 0.2)
                .min((av.question_until - now) / 0.35)
                .clamp(0.0, 1.0);
            let e = e * e * (3.0 - 2.0 * e);
            pose.head.z += 0.17 * av.tilt_side * e;
            pose.head.y += 0.08 * av.tilt_side * e;
            pose.head.x -= 0.04 * e;
        }
    }
    pose.head.z += (now * 1.3).sin() * 0.02;
    // laughing: head thrown back, shoulders bouncing
    if now < av.laugh_until {
        let fade = ((av.laugh_until - now) / 0.4).clamp(0.0, 1.0);
        pose.head.x -= (0.16 + (now * 14.0).sin() * 0.05) * fade;
        pose.torso_x -= 0.05 * fade;
        pose.root_y += (now * 14.0).sin().abs() * 0.02 * fade;
    }
    // singing: a slow sway with the head
    if av.singing {
        pose.head.z += (now * 2.4).sin() * 0.09;
        pose.torso_x += (now * 1.2).sin() * 0.02;
    }
    // a viewer's question on stage: turn to the guest (on the right) and listen
    let k = 1.0 - (-time.delta_secs() * 5.0).exp();
    av.listen += k * (av.listen_to - av.listen);
    if let Some((_, head)) = tracked {
        pose.head += head;
    }
    pose.head.y += 0.42 * av.listen;
    pose.head.z -= 0.05 * av.listen;
    av.look_left += k * (av.look_left_to - av.look_left);
    pose.head.y -= 0.35 * av.look_left;
    // bop: dip and nod on each beat, swaying side to side
    let b = (now - av.last_beat) / 0.3;
    if av.bop && b < 1.0 {
        let k = (std::f32::consts::PI * b).sin();
        pose.root_y -= 0.035 * k;
        pose.head.x += 0.16 * k;
        pose.head.z += if av.beat_side { 0.06 } else { -0.06 } * k;
    }
    if av.asleep {
        pose.head.x += 0.32 + (now * 0.9).sin() * 0.04;
        pose.torso_x += 0.06;
    }
    // a hit: the head squashes flat and wobbles back
    let age = now - av.head_squash.0;
    let squash = if age < 0.6 {
        av.head_squash.1 * (-age * 7.0).exp() * (age * 32.0).cos()
    } else {
        0.0
    };
    for (j, mut t) in &mut joints {
        match j {
            Joint::Root => {
                t.translation = Vec3::new(pose.root_x, pose.root_y, pose.root_z);
                t.rotation = Quat::from_rotation_y(pose.root_spin);
                t.scale = Vec3::new(
                    1.0 - pose.stretch * 0.5,
                    1.0 + pose.stretch,
                    1.0 - pose.stretch * 0.5,
                );
            }
            Joint::Hips => t.rotation *= Quat::from_rotation_x(pose.torso_x),
            // undo the waist bend on the legs so the feet stay planted
            Joint::Leg => t.rotation = Quat::from_rotation_x(-pose.torso_x) * t.rotation,
            Joint::Head => {
                t.rotation *=
                    Quat::from_euler(EulerRot::YXZ, pose.head.y, pose.head.x, pose.head.z);
                t.scale = Vec3::new(1.0 + squash * 0.6, 1.0 - squash, 1.0 + squash * 0.6);
            }
            Joint::ArmL => t.rotation *= Quat::from_rotation_x(-pose.arm_l),
            Joint::ArmR => t.rotation *= Quat::from_rotation_x(-pose.arm_r),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Emote; 13] = [
        Emote::Nod,
        Emote::Shake,
        Emote::Tilt,
        Emote::Hop,
        Emote::Spin,
        Emote::Bow,
        Emote::Shiver,
        Emote::LeanIn,
        Emote::Jolt,
        Emote::LookAround,
        Emote::WeightShift,
        Emote::Stretch,
        Emote::Watch,
    ];

    fn size(p: &Pose) -> f32 {
        p.head.length()
            + p.torso_x.abs()
            + p.root_x.abs()
            + p.root_y.abs()
            + p.root_z.abs()
            + p.stretch.abs()
            + p.arm_l.abs()
            + p.arm_r.abs()
    }

    /// Every emote starts and ends at rest (so it blends in and out without a pop), moves in the
    /// middle, and stays within sane angles.
    #[test]
    fn emotes_start_and_end_at_rest() {
        for e in ALL {
            let (a, b) = (emote_pose(e, 0.0), emote_pose(e, 1.0));
            assert!(
                size(&a) < 0.02
                    && size(&b) < 0.02
                    && (b.root_spin.rem_euclid(std::f32::consts::TAU) < 0.02
                        || (std::f32::consts::TAU - b.root_spin.rem_euclid(std::f32::consts::TAU))
                            < 0.02),
                "{} doesn't rest at the ends",
                e.name()
            );
            let peak = (1..20)
                .map(|i| {
                    size(&emote_pose(e, i as f32 / 20.0))
                        + emote_pose(e, i as f32 / 20.0).root_spin.abs()
                })
                .fold(0.0, f32::max);
            assert!(peak > 0.03, "{} doesn't move", e.name());
            for i in 0..=40 {
                let p = emote_pose(e, i as f32 / 40.0);
                assert!(
                    p.head.abs().max_element() < 1.2
                        && p.torso_x.abs() < 1.2
                        && p.root_y.abs() < 0.6,
                    "{} goes too far at {i}/40",
                    e.name()
                );
            }
            assert!(e.duration() > 0.3 && e.duration() < 10.0);
        }
    }

    #[test]
    fn idle_emotes_are_quiet_ones() {
        for e in IDLES {
            assert!(
                !EMOTES.iter().any(|x| x.0 == e),
                "{} is also a hotkey emote",
                e.name()
            );
        }
    }
}
