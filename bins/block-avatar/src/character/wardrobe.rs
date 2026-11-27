//! Dress-up by chat: viewers put hats, glasses and things to hold on your character.
//!
//!   !hat cowboy     !glasses shades     !hold coffee     !wear crown     !hat off
//!
//! One hat, one face piece and one held item at a time; each stays until someone replaces it, you
//! take it off, or `dressup_minutes` pass. Viewers have a cooldown, and you choose who may dress
//! you up (everyone, subs/VIPs, or mods). The control page has the same wardrobe for you.
//! Hats sit on top of each character's head (on its cap, if it has one) using data/head_fit.json,
//! measured from the models by tools/assets/build_wardrobe.py.

use std::collections::HashMap;

use bevy::{gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAssetRoot};

use crate::chat::{ChatMsg, Who};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Hat,
    Face,
    Hand,
}

pub const SLOTS: [Slot; 3] = [Slot::Hat, Slot::Face, Slot::Hand];

impl Slot {
    fn i(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Slot::Hat => "hat",
            Slot::Face => "face",
            Slot::Hand => "hand",
        }
    }
}

pub struct Item {
    pub name: &'static str,
    /// how it reads in "X put a ... on you"
    pub label: &'static str,
    pub slot: Slot,
    pub file: &'static str,
}

const fn it(name: &'static str, label: &'static str, slot: Slot, file: &'static str) -> Item {
    Item {
        name,
        label,
        slot,
        file,
    }
}

pub const ITEMS: &[Item] = &[
    it(
        "cowboy",
        "a cowboy hat",
        Slot::Hat,
        "props/wardrobe/cowboy.glb",
    ),
    it("crown", "a crown", Slot::Hat, "props/wardrobe/crown.glb"),
    it(
        "party",
        "a party hat",
        Slot::Hat,
        "props/wardrobe/party.glb",
    ),
    it(
        "tophat",
        "a top hat",
        Slot::Hat,
        "props/wardrobe/top_hat.glb",
    ),
    it(
        "propeller",
        "a propeller beanie",
        Slot::Hat,
        "props/wardrobe/propeller.glb",
    ),
    it("chef", "a chef's hat", Slot::Hat, "props/wardrobe/chef.glb"),
    it(
        "viking",
        "a viking helmet",
        Slot::Hat,
        "props/wardrobe/viking.glb",
    ),
    it(
        "pirate",
        "a pirate hat",
        Slot::Hat,
        "props/wardrobe/pirate.glb",
    ),
    it("halo", "a halo", Slot::Hat, "props/wardrobe/halo.glb"),
    it(
        "cone",
        "a traffic cone",
        Slot::Hat,
        "props/wardrobe/cone.glb",
    ),
    it(
        "wizard",
        "a wizard hat",
        Slot::Hat,
        "props/wardrobe/wizard.glb",
    ),
    it(
        "hardhat",
        "a hard hat",
        Slot::Hat,
        "props/wardrobe/hard_hat.glb",
    ),
    it(
        "santa",
        "a Santa hat",
        Slot::Hat,
        "props/wardrobe/santa.glb",
    ),
    it(
        "grad",
        "a graduation cap",
        Slot::Hat,
        "props/wardrobe/grad_cap.glb",
    ),
    it("beanie", "a beanie", Slot::Hat, "props/wardrobe/beanie.glb"),
    it("yuzu", "a yuzu", Slot::Hat, "props/wardrobe/yuzu.glb"),
    it("shades", "shades", Slot::Face, "props/wardrobe/shades.glb"),
    it(
        "nerd",
        "nerd glasses",
        Slot::Face,
        "props/wardrobe/nerd_glasses.glb",
    ),
    it(
        "moustache",
        "a moustache",
        Slot::Face,
        "props/wardrobe/moustache.glb",
    ),
    it(
        "clown",
        "a clown nose",
        Slot::Face,
        "props/wardrobe/clown_nose.glb",
    ),
    it("coffee", "a coffee", Slot::Hand, "props/cast/coffee.glb"),
    it("burger", "a burger", Slot::Hand, "props/lead/burger.glb"),
    it("soda", "a soda", Slot::Hand, "props/lead/can.glb"),
    it("donut", "a donut", Slot::Hand, "props/roasted/donut.glb"),
    it("phone", "a phone", Slot::Hand, "props/lead/phone.glb"),
    it("mic", "a microphone", Slot::Hand, "props/cast/mic.glb"),
    it("book", "a book", Slot::Hand, "props/lead/book.glb"),
    it("sign", "a sign", Slot::Hand, "props/cast/sign.glb"),
    it("bell", "a bell", Slot::Hand, "props/roasted/bell.glb"),
    it("broom", "a broom", Slot::Hand, "props/roasted/broom.glb"),
    it("watermelon", "a slice of watermelon", Slot::Hand, "props/wardrobe/watermelon.glb"),
    it("gavel", "a gavel", Slot::Hand, "props/cast/gavel.glb"),
    it(
        "flashlight",
        "a flashlight",
        Slot::Hand,
        "props/lead/flashlight.glb",
    ),
];

/// Finds an item by name, forgiving: "top hat", "Top_Hat", "glasses" (first face piece)...
pub fn find(name: &str, slot: Option<Slot>) -> Option<usize> {
    let n: String = name
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if n.is_empty() {
        return None;
    }
    let alias = match n.as_str() {
        "sunglasses" | "glasses" | "dealwithit" => "shades",
        "mustache" | "stache" => "moustache",
        "nose" => "clown",
        "cap" | "graduation" => "grad",
        "cowboyhat" | "stetson" => "cowboy",
        "helmet" => "viking",
        "can" | "pop" => "soda",
        "microphone" => "mic",
        "torch" => "flashlight",
        "trafficcone" => "cone",
        "chefhat" | "toque" => "chef",
        "santahat" => "santa",
        other => other,
    };
    ITEMS
        .iter()
        .position(|i| i.name == alias && slot.is_none_or(|s| s == i.slot))
}

/// `!hat cowboy`, `!glasses shades`, `!hold coffee`, `!wear crown`, `!hat off`.
/// Some((slot, None)) takes that slot off.
pub fn parse_command(text: &str) -> Option<(Slot, Option<usize>)> {
    let mut w = text.split_whitespace();
    let cmd = w.next()?.to_ascii_lowercase();
    let rest: String = w.collect::<Vec<_>>().join(" ");
    let slot = match cmd.as_str() {
        "!hat" => Some(Slot::Hat),
        "!glasses" | "!face" => Some(Slot::Face),
        "!hold" => Some(Slot::Hand),
        "!wear" | "!dress" => None,
        _ => return None,
    };
    if matches!(
        rest.trim().to_ascii_lowercase().as_str(),
        "off" | "none" | "remove"
    ) {
        return slot.map(|s| (s, None));
    }
    let i = find(&rest, slot)?;
    Some((ITEMS[i].slot, Some(i)))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Fit {
    pub top: f32,
    pub width: f32,
    pub front: f32,
    pub eye_y: f32,
    pub mouth_y: f32,
}

/// data/bare_heads.json (tools/assets/build_bare_heads.py): model file -> top of the bare head.
fn load_bare() -> HashMap<String, f32> {
    std::fs::read_to_string(crate::asset_base().join("data/bare_heads.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, f32>>(&s).ok())
        .unwrap_or_default()
}

/// Marks the head meshes of every loaded character: the usual head, and the bare head (no cap)
/// that some characters carry for wearing hats.
#[derive(Component)]
pub struct HeadMesh {
    bare: bool,
}

pub fn tag_heads(mut commands: Commands, named: Query<(Entity, &Name), Added<Name>>) {
    for (e, n) in &named {
        let n = n.as_str();
        if n == "Head_bare" {
            commands
                .entity(e)
                .insert((HeadMesh { bare: true }, Visibility::Hidden));
        } else if n == "Head_mesh.Voxel" || n == "Head_mesh" {
            commands.entity(e).insert(HeadMesh { bare: false });
        }
    }
}

/// Under a hat, the main character swaps to its bare head; everyone else keeps their own.
pub fn swap_heads(
    w: Res<Wardrobe>,
    joints: Query<&crate::Joint>,
    mut heads: Query<(&HeadMesh, &ChildOf, &mut Visibility)>,
) {
    for (h, parent, mut v) in &mut heads {
        let main = joints
            .get(parent.parent())
            .is_ok_and(|j| *j == crate::Joint::Head);
        let show = if h.bare {
            main && w.bare_on
        } else {
            !(main && w.bare_on)
        };
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
}

fn load_fits() -> HashMap<String, Fit> {
    let path = crate::asset_base().join("data/head_fit.json");
    let mut m = HashMap::new();
    if let Ok(v) = std::fs::read_to_string(path)
        .map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap_or_default())
    {
        if let Some(o) = v.as_object() {
            for (k, f) in o {
                let g = |n: &str| f.get(n).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                m.insert(
                    k.clone(),
                    Fit {
                        top: g("top"),
                        width: g("width"),
                        front: g("front"),
                        eye_y: g("eye_y"),
                        mouth_y: g("mouth_y"),
                    },
                );
            }
        }
    }
    m
}

/// The host's right forearm (tagged when the character loads).
#[derive(Component)]
pub struct HostHand;

#[derive(Component)]
pub struct Worn {
    t0: f32,
    scale: f32,
    /// held items stay upright whatever the arm is doing
    upright: bool,
    /// hats drop onto the head and bounce; `at` is where they come to rest
    drop: bool,
    at: Vec3,
    landed: bool,
}

/// Bounce easing: falls, hits at u = 1/2.75, and bounces twice more, smaller.
fn bounce(u: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if u < 1.0 / d {
        n * u * u
    } else if u < 2.0 / d {
        let u = u - 1.5 / d;
        n * u * u + 0.75
    } else if u < 2.5 / d {
        let u = u - 2.25 / d;
        n * u * u + 0.9375
    } else {
        let u = u - 2.625 / d;
        n * u * u + 0.984375
    }
}

#[derive(Clone)]
pub struct On {
    pub item: usize,
    pub by: String,
    pub at: f32,
}

#[derive(Resource)]
pub struct Wardrobe {
    /// chat may dress you up
    pub chat: bool,
    pub who: Who,
    pub per_user_s: f32,
    pub global_s: f32,
    /// items come off by themselves after this long (0 = never)
    pub minutes: f32,
    pub worn: [Option<On>; 3],
    /// something the app puts in your hand for a while (the mic while you sing)
    pub override_hand: Option<usize>,
    entities: [Option<Entity>; 3],
    /// what each slot's entity was made for: (head or hand entity, item)
    made_for: [Option<(Entity, usize)>; 3],
    fits: HashMap<String, Fit>,
    /// characters whose own cap comes off under a hat: model file -> top of the bare head
    bare: HashMap<String, f32>,
    /// the main character is showing its bare head now (a hat is on and it has one)
    pub bare_on: bool,
    last_user: HashMap<String, f32>,
    last_any: f32,
}

impl Wardrobe {
    pub fn new(chat: bool, who: Who, minutes: f32) -> Self {
        Wardrobe {
            chat,
            who,
            per_user_s: 30.0,
            global_s: 1.5,
            minutes,
            worn: [None, None, None],
            override_hand: None,
            entities: [None; 3],
            made_for: [None; 3],
            fits: load_fits(),
            bare: load_bare(),
            bare_on: false,
            last_user: HashMap::new(),
            last_any: -100.0,
        }
    }

    /// A chat message; returns the credit line if it changed something.
    pub fn from_chat(&mut self, m: &ChatMsg, now: f32) -> Option<String> {
        let (slot, item) = parse_command(&m.text)?;
        if !self.chat || !self.who.allows(m) {
            return None;
        }
        let user = m.user.to_ascii_lowercase();
        if !m.broadcaster
            && (now - self.last_any < self.global_s
                || self
                    .last_user
                    .get(&user)
                    .is_some_and(|t| now - t < self.per_user_s))
        {
            return None;
        }
        self.last_any = now;
        self.last_user.insert(user, now);
        Some(self.put(slot, item, &m.user, now))
    }

    /// Puts an item on (or takes a slot off, item None). `by` empty = the streamer.
    pub fn put(&mut self, slot: Slot, item: Option<usize>, by: &str, now: f32) -> String {
        match item {
            Some(i) => {
                self.worn[slot.i()] = Some(On {
                    item: i,
                    by: by.to_string(),
                    at: now,
                });
                match (by.is_empty(), slot) {
                    (true, Slot::Hand) => format!("Holding {}", ITEMS[i].label),
                    (true, _) => format!("Wearing {}", ITEMS[i].label),
                    (false, Slot::Hand) => format!("{by} gave you {}", ITEMS[i].label),
                    (false, _) => format!("{by} put {} on you", ITEMS[i].label),
                }
            }
            None => {
                self.worn[slot.i()] = None;
                if by.is_empty() {
                    format!("{} off", slot.name())
                } else {
                    format!("{by} took your {} off", slot.name())
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.worn = [None, None, None];
    }

    /// Control page commands: wear:<item>, wear:off:<slot>, wear:clear, wear:chat, wear:who:<who>
    pub fn command(&mut self, arg: &str, now: f32) -> Option<String> {
        let (verb, rest) = arg.split_once(':').unwrap_or((arg, ""));
        Some(match verb {
            "clear" => {
                self.clear();
                "Wardrobe cleared".into()
            }
            "chat" => {
                self.chat = !self.chat;
                format!("Chat dress-up {}", if self.chat { "on" } else { "off" })
            }
            "who" => {
                self.who = Who::parse(rest)?;
                format!("Dress-up: {} can use it", self.who.name())
            }
            "off" => {
                let slot = SLOTS.into_iter().find(|s| s.name() == rest)?;
                self.put(slot, None, "", now)
            }
            name => {
                let i = find(name, None)?;
                self.put(ITEMS[i].slot, Some(i), "", now)
            }
        })
    }

    pub fn summary(&self) -> String {
        let on: Vec<String> = self
            .worn
            .iter()
            .flatten()
            .map(|o| {
                if o.by.is_empty() {
                    ITEMS[o.item].name.to_string()
                } else {
                    format!("{} (from {})", ITEMS[o.item].name, o.by)
                }
            })
            .collect();
        format!(
            "Dress-up: chat {} ({}) | wearing: {}",
            if self.chat { "on" } else { "off" },
            self.who.name(),
            if on.is_empty() {
                "nothing".into()
            } else {
                on.join(", ")
            }
        )
    }

    pub fn json(&self) -> String {
        let worn: Vec<serde_json::Value> = SLOTS
            .iter()
            .map(|s| match &self.worn[s.i()] {
                Some(o) => {
                    serde_json::json!({"slot": s.name(), "item": ITEMS[o.item].name, "by": o.by})
                }
                None => serde_json::json!({"slot": s.name(), "item": null}),
            })
            .collect();
        serde_json::json!({"chat": self.chat, "who": self.who.name(), "worn": worn,
            "items": ITEMS.iter().map(|i| serde_json::json!({"name": i.name, "slot": i.slot.name()})).collect::<Vec<_>>()})
        .to_string()
    }
}

/// Chat commands in, expired items off, and the worn items attached to the current character.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut w: ResMut<Wardrobe>,
    mut chat: ResMut<crate::chat::Chat>,
    mut av: ResMut<crate::Avatar>,
    mut fx: ResMut<crate::fx::Fx>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    heads: Query<(Entity, &crate::Joint)>,
    hands: Query<(Entity, &GlobalTransform), With<HostHand>>,
    mut worn_q: Query<(&mut Worn, &mut Transform)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let inbox = chat.inbox.clone();
    for m in &inbox {
        if let Some(line) = w.from_chat(m, now) {
            chat.credit = Some((line, now));
        }
    }
    if w.minutes > 0.0 {
        let limit = w.minutes * 60.0;
        for s in &mut w.worn {
            if s.as_ref().is_some_and(|o| now - o.at > limit) {
                *s = None;
            }
        }
    }
    let head = heads
        .iter()
        .find(|(_, j)| **j == crate::Joint::Head)
        .map(|(e, _)| e);
    let hand = hands.iter().next().map(|(e, _)| e);
    let hand_rot = hands
        .iter()
        .next()
        .map(|(_, g)| g.compute_transform().rotation)
        .unwrap_or_default();
    let model = av.models[av.model]
        .1
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();
    let mut fit = w.fits.get(&model).copied().unwrap_or(Fit {
        top: 0.62,
        width: 0.7,
        front: 0.31,
        eye_y: 0.25,
        mouth_y: 0.07,
    });
    // a character with its own cap takes it off for a hat: the hat sits on the bare head
    let bare_top = w.bare.get(&model).copied();
    let hat_on = w.worn[Slot::Hat.i()].is_some();
    w.bare_on = hat_on && bare_top.is_some();
    if let (true, Some(t)) = (w.bare_on, bare_top) {
        fit.top = t;
    }
    let head_scale = (fit.width / 0.62).clamp(0.85, 1.35);
    for slot in SLOTS {
        let i = slot.i();
        let parent = if slot == Slot::Hand { hand } else { head };
        let item = if slot == Slot::Hand {
            w.override_hand.or(w.worn[i].as_ref().map(|o| o.item))
        } else {
            w.worn[i].as_ref().map(|o| o.item)
        };
        let want = match (item, parent) {
            (Some(it), Some(p)) => Some((p, it)),
            _ => None,
        };
        if w.made_for[i] == want {
            continue;
        }
        if let Some(e) = w.entities[i].take() {
            commands.entity(e).try_despawn();
        }
        w.made_for[i] = want;
        let Some((p, item)) = want else { continue };
        let (pos, scale) = match slot {
            Slot::Hat => (Vec3::new(0.0, fit.top - 0.01, 0.0), head_scale),
            Slot::Face => {
                let y = if ITEMS[item].name == "moustache" {
                    fit.mouth_y + 0.06
                } else if ITEMS[item].name == "clown" {
                    (fit.eye_y + fit.mouth_y) / 2.0
                } else {
                    fit.eye_y
                };
                (Vec3::new(0.0, y, fit.front + 0.005), head_scale)
            }
            Slot::Hand => (Vec3::new(0.0, -0.25, 0.05), 1.0),
        };
        let e = commands
            .spawn((
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(ITEMS[item].file))),
                Transform::from_translation(pos).with_scale(Vec3::splat(0.001)),
                Worn {
                    t0: now,
                    scale,
                    upright: slot == Slot::Hand,
                    drop: slot == Slot::Hat,
                    at: pos,
                    landed: false,
                },
                ChildOf(p),
            ))
            .id();
        // hats and face pieces swing a little when the head moves
        if slot == Slot::Hat {
            commands.entity(e).insert(crate::jiggle::Jiggle::hat());
        }
        w.entities[i] = Some(e);
    }
    // hats drop on and bounce; the rest pop on with a little overshoot
    for (mut wo, mut t) in &mut worn_q {
        if wo.drop {
            let u = ((now - wo.t0) / 0.65).clamp(0.0, 1.0);
            t.translation = wo.at + Vec3::Y * 0.55 * (1.0 - bounce(u));
            t.scale = Vec3::splat(wo.scale);
            if !wo.landed && u >= 1.0 / 2.75 {
                wo.landed = true;
                fx.play("knock", 0.7);
                av.head_squash = (now, 0.06);
            }
            continue;
        }
        let u = ((now - wo.t0) / 0.35).clamp(0.0, 1.0);
        let c = 2.2;
        let k = 1.0 + (c + 1.0) * (u - 1.0).powi(3) + c * (u - 1.0).powi(2);
        t.scale = Vec3::splat((wo.scale * k).max(0.001));
        if wo.upright {
            // facing the camera, a little turned so it reads as 3D
            t.rotation = hand_rot.inverse() * Quat::from_rotation_y(-0.35);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hats_bounce_to_rest() {
        assert_eq!(super::bounce(0.0), 0.0);
        assert!(
            (super::bounce(1.0 / 2.75) - 1.0).abs() < 1e-4,
            "first touchdown"
        );
        assert!(super::bounce(0.55) < 1.0, "in the air again");
        assert!((super::bounce(1.0) - 1.0).abs() < 1e-4);
    }

    use super::*;

    #[test]
    fn chat_commands() {
        let n = |t: &str| parse_command(t).map(|(s, i)| (s, i.map(|i| ITEMS[i].name)));
        assert_eq!(n("!hat cowboy"), Some((Slot::Hat, Some("cowboy"))));
        assert_eq!(n("!hat Top Hat"), Some((Slot::Hat, Some("tophat"))));
        assert_eq!(n("!glasses"), None);
        assert_eq!(
            n("!glasses deal with it"),
            Some((Slot::Face, Some("shades")))
        );
        assert_eq!(n("!wear mustache"), Some((Slot::Face, Some("moustache"))));
        assert_eq!(n("!hold Coffee please"), None, "extra words don't match");
        assert_eq!(n("!hold coffee"), Some((Slot::Hand, Some("coffee"))));
        assert_eq!(n("!hat coffee"), None, "wrong slot");
        assert_eq!(n("!hat off"), Some((Slot::Hat, None)));
        assert_eq!(n("!wear off"), None);
        assert_eq!(n("hat cowboy"), None);
        for i in ITEMS {
            assert!(
                crate::asset_base().join("assets").join(i.file).exists(),
                "{}",
                i.file
            );
        }
    }

    #[test]
    fn who_and_cooldowns() {
        let mut w = Wardrobe::new(true, Who::Subs, 10.0);
        let mut fan = ChatMsg::new("fan", "!hat crown");
        assert!(w.from_chat(&fan, 0.0).is_none(), "not a sub");
        fan.sub = true;
        assert_eq!(
            w.from_chat(&fan, 0.0).as_deref(),
            Some("fan put a crown on you")
        );
        let mut other = ChatMsg::new("other", "!hat cone");
        other.vip = true;
        assert!(w.from_chat(&other, 1.0).is_none(), "global gap");
        assert!(w.from_chat(&other, 5.0).is_some());
        fan.text = "!glasses shades".into();
        assert!(w.from_chat(&fan, 10.0).is_none(), "per-viewer cooldown");
        assert!(w.from_chat(&fan, 31.0).is_some());
        assert_eq!(w.worn[0].as_ref().map(|o| ITEMS[o.item].name), Some("cone"));
        w.chat = false;
        fan.text = "!hat off".into();
        assert!(w.from_chat(&fan, 100.0).is_none(), "chat dress-up off");
        assert_eq!(w.command("off:hat", 0.0).as_deref(), Some("hat off"));
        assert!(w.worn[0].is_none());
        assert_eq!(
            w.command("who:mods", 0.0).as_deref(),
            Some("Dress-up: mods can use it")
        );
        assert!(w.command("bogus", 0.0).is_none());
    }

    #[test]
    fn bare_heads_are_listed_for_real_models() {
        let bare = load_bare();
        assert!(
            bare.contains_key("scene_all.glb"),
            "the lead's cap comes off under a hat"
        );
        for (m, top) in &bare {
            assert!(
                crate::asset_base().join("assets/models").join(m).exists(),
                "{m}"
            );
            assert!(*top > 0.4 && *top < 0.8, "{m} {top}");
        }
    }

    #[test]
    fn every_character_has_a_head_fit() {
        let fits = load_fits();
        for e in std::fs::read_dir(crate::asset_base().join("assets/models"))
            .unwrap()
            .flatten()
        {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".glb") {
                let f = fits.get(&n).unwrap_or_else(|| panic!("{n}"));
                assert!(
                    f.top > 0.4 && f.top < 1.2 && f.width > 0.4 && f.eye_y > 0.0,
                    "{n} {f:?}"
                );
            }
        }
    }
}
