//! The emote wheel (docs/design/controls-and-hud.md, "Wheels instead of a hotbar"): held open by
//! the right bumper (or G), eight emotes round it, chosen with the right stick or the mouse.
//! Targets work as aiming does in a third-person action game: the left stick looks round (on the
//! keyboard, the view stays where it was), the player, prop or doorway nearest the middle of
//! the view within [`TARGET_M`] is aimed at, and the left trigger (right click) locks on to it;
//! pressing it again lets go. Letting go of the wheel turns you to the locked target, if any,
//! and plays the emote; walking stops it.
//!
//! The emotes are clips of the shared rig, chosen on the Emotes screen and kept on this device
//! ([`EmoteSettings`]). Others see them with no message of their own: your pose is streamed.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use vox_core::entity::{Component, PROP_UNITS_PER_BLOCK};

use crate::{
    client::WorldClient,
    entities::WorldEntities,
    input::{Action, Actions, ActiveDevice, UiMode},
    wheel::{MAIN_RADIUS, PropMenu, SlotView, WheelKind, WheelView, Wheels, wheel_choice, wheel_spot},
};

/// Emotes on the wheel.
pub const EMOTE_SLOTS: usize = 8;
/// The emotes a new player has: plain gestures, nothing that stands for a real person or an
/// event.
pub const DEFAULT_EMOTES: [&str; EMOTE_SLOTS] =
    ["Victory", "Talk", "Lawn chair", "Sleep", "Hammer", "Guard stance", "Belly slide", "Coin grab"];
/// The longest clip name kept, in characters.
const NAME_MAX: usize = 40;
/// How far a target may be, metres.
pub const TARGET_M: f32 = 16.0;
/// How far off the middle of the view a target may be to be aimed at, degrees.
const AIM_CONE_DEG: f32 = 15.0;
/// Clips that are how you move, not emotes: not offered on the Emotes screen.
const NOT_EMOTES: [&str; 6] = ["Idle", "Walk", "Run", "Sprint", "Respawn", "Enter door"];

/// The eight emotes, by clip name, as this device keeps them.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmoteSettings {
    pub slots: Vec<String>,
}

impl Default for EmoteSettings {
    fn default() -> Self {
        EmoteSettings { slots: DEFAULT_EMOTES.iter().map(|s| s.to_string()).collect() }
    }
}

impl EmoteSettings {
    /// Eight names, no control characters, none too long; a missing or empty one is the
    /// default's.
    pub fn sanitized(mut self) -> Self {
        self.slots.truncate(EMOTE_SLOTS);
        let mut slots: Vec<String> =
            self.slots.into_iter().map(|s| s.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect::<String>().trim().to_string()).collect();
        while slots.len() < EMOTE_SLOTS {
            slots.push(String::new());
        }
        for (i, s) in slots.iter_mut().enumerate() {
            if s.is_empty() {
                *s = DEFAULT_EMOTES[i].to_string();
            }
        }
        EmoteSettings { slots }
    }
}

/// Whether a clip is offered as an emote (the Emotes screen lists these).
pub fn is_emote(clip: &str) -> bool {
    !NOT_EMOTES.contains(&clip)
}

/// Settings from JSON; anything unreadable is the defaults.
pub fn from_json(text: &str) -> EmoteSettings {
    serde_json::from_str::<EmoteSettings>(text).unwrap_or_default().sanitized()
}

pub fn to_json(s: &EmoteSettings) -> String {
    serde_json::to_string_pretty(s).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
const FILE: &str = "emotes.json";

/// This device's emotes, or the defaults.
pub fn load() -> EmoteSettings {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::fs::read_to_string(crate::files::user_dir().join(FILE)) {
            Ok(t) => from_json(&t),
            Err(_) => EmoteSettings::default(),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let stored = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("vox.emotes").ok().flatten());
        stored.map(|t| from_json(&t)).unwrap_or_default()
    }
}

/// Keep this device's emotes for next time.
pub fn save(s: &EmoteSettings) {
    let text = to_json(s);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = crate::files::user_dir().join(FILE);
        if let Err(e) = std::fs::write(&path, text) {
            crate::note!("[emote] can't save {}: {e}", path.display());
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item("vox.emotes", &text);
        }
    }
}

/// Save the emotes when they change (the Emotes screen changes them).
fn save_emotes(settings: Res<EmoteSettings>, mut last: Local<Option<EmoteSettings>>) {
    if last.as_ref() == Some(&*settings) {
        return;
    }
    // the first look is what was loaded: nothing to save yet
    if last.is_some() {
        save(&settings);
    }
    *last = Some(settings.clone());
}

/// What a target is.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetKind {
    Player(u32),
    /// a prop, by its version hash
    Prop([u8; 16]),
    /// a doorway, by name
    Doorway(String),
}

/// Something to face while you emote.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub kind: TargetKind,
    /// where it is, blocks
    pub at: Vec3,
}

/// Of `candidates`, the one to snap to from `from` (blocks) looking along `dir` (on the
/// ground): within [`TARGET_M`] and `cone_deg` of `dir`, the nearest to the line, then the
/// nearest. Nothing when `dir` is nothing.
pub fn snap_target(from: Vec3, dir: Vec3, cone_deg: f32, candidates: &[Target]) -> Option<Target> {
    let dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let reach = vox_core::in_blocks(TARGET_M);
    let cos_cone = cone_deg.to_radians().cos();
    candidates
        .iter()
        .filter_map(|t| {
            let to = Vec3::new(t.at.x - from.x, 0.0, t.at.z - from.z);
            let d = to.length();
            if !d.is_finite() || d < 1e-3 || d > reach || (t.at.y - from.y).abs() > reach {
                return None;
            }
            let cos = to.dot(dir) / d;
            (cos >= cos_cone).then_some((t, cos, d))
        })
        // most in line first (cosines rounded, so near-ties go to the nearer), then the nearest
        .max_by(|a, b| {
            let (ca, cb) = ((a.1 * 50.0).round(), (b.1 * 50.0).round());
            ca.total_cmp(&cb).then(b.2.total_cmp(&a.2))
        })
        .map(|(t, _, _)| t.clone())
}

/// Everything there is to face: other players, props, doorways.
fn candidates(w: &WorldClient, entities: Option<&WorldEntities>) -> Vec<Target> {
    let mut v: Vec<Target> = w.others_at().into_iter().map(|(id, at)| Target { kind: TargetKind::Player(id), at }).collect();
    if let Some(e) = entities {
        let u = PROP_UNITS_PER_BLOCK as f32;
        for (_, c) in e.map.values() {
            if let Component::Prop(p) = c {
                v.push(Target { kind: TargetKind::Prop(p.prop), at: Vec3::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32) / u });
            }
        }
    }
    for p in &w.portals {
        v.push(Target { kind: TargetKind::Doorway(p.name.clone()), at: p.at });
    }
    v
}

/// The emote wheel's state while it is open.
#[derive(Resource, Default, Debug)]
pub struct EmoteWheel {
    pointer: Vec2,
    choice: Option<usize>,
    /// what the view is on
    aim: Option<Target>,
    /// what lock-on holds, kept where it is as it moves
    locked: Option<Target>,
}

/// Lock-on pressed: lock on to what is aimed at, or, aimed at nothing or at what is already
/// held, let go.
pub fn toggle_lock(locked: Option<Target>, aim: Option<Target>) -> Option<Target> {
    match (locked, aim) {
        (Some(l), Some(a)) if same(&l, &a) => None,
        (_, Some(a)) => Some(a),
        (_, None) => None,
    }
}

/// How far a prop or doorway may seem to move and still be the same one, blocks. They don't
/// move, but two copies of one prop share a kind (its hash), so where it is tells them apart.
const SAME_SPOT: f32 = vox_core::in_blocks(0.5);

/// Whether two targets are the same thing: a player by number; a prop or doorway by kind and
/// place.
fn same(a: &Target, b: &Target) -> bool {
    a.kind == b.kind && (matches!(a.kind, TargetKind::Player(_)) || a.at.distance(b.at) <= SAME_SPOT)
}

/// The locked target as it is now (a player moves): the same player, or the same prop or
/// doorway; nothing once it is gone or out of reach.
fn follow(locked: &Target, from: Vec3, candidates: &[Target]) -> Option<Target> {
    let reach = vox_core::in_blocks(TARGET_M) * 1.25;
    candidates.iter().find(|t| same(t, locked) && t.at.distance(from) <= reach).cloned()
}

/// The emote wheel: open while its button is held; letting go plays what is pointed at.
#[allow(clippy::too_many_arguments)]
pub fn emote_wheel(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    settings: Res<EmoteSettings>,
    entities: Option<Res<WorldEntities>>,
    mut wheels: ResMut<Wheels>,
    mut st: ResMut<EmoteWheel>,
    mut w: ResMut<WorldClient>,
    mut av: ResMut<crate::LocalAvatar>,
    edit: Option<Res<crate::editor::camera::EditFly>>,
) {
    let playing = w.active() && w.ready && !w.watch && *mode == UiMode::Game && edit.is_none();
    let scripted = wheels.scripted.filter(|(k, _)| *k == WheelKind::Emotes).map(|(_, p)| p);
    let held = playing && (actions.pressed(Action::EmoteWheel) || scripted.is_some());
    if held && wheels.open.is_none() {
        wheels.open = Some(WheelKind::Emotes);
        *st = EmoteWheel::default();
    }
    if wheels.open != Some(WheelKind::Emotes) {
        return;
    }
    if !playing {
        wheels.open = None;
        return;
    }
    if held {
        st.pointer = scripted.unwrap_or_else(|| {
            let d = actions.mouse_delta;
            let mut p = (st.pointer + Vec2::new(d.x, -d.y)).clamp_length_max(100.0);
            if actions.look_stick != Vec2::ZERO {
                p = actions.look_stick.normalize_or_zero() * 100.0;
            }
            p
        });
        if let Some(c) = wheel_choice(st.pointer, EMOTE_SLOTS) {
            st.choice = Some(c);
        }
        // aim at what is in the middle of the view (the left stick turns it: client::controls),
        // and lock on with the lock-on button
        let from = Vec3::from(w.body.pos);
        let all = candidates(&w, entities.as_deref());
        st.aim = snap_target(from, w.ground_dir(Vec2::Y), AIM_CONE_DEG, &all);
        st.locked = st.locked.as_ref().and_then(|l| follow(l, from, &all));
        if actions.just_pressed(Action::LockOn) {
            st.locked = toggle_lock(st.locked.take(), st.aim.clone());
        }
        return;
    }
    // let go: face what is locked on, play the emote
    wheels.open = None;
    let Some(clip) = st.choice.and_then(|c| settings.slots.get(c)).cloned() else { return };
    if let Some(t) = st.locked.take() {
        w.face(t.at);
    }
    av.emote = Some(clip);
}

/// What a target is called on the wheel.
fn target_name(t: &Target, w: &WorldClient, props: Option<&PropMenu>) -> String {
    match &t.kind {
        TargetKind::Player(id) => w.others_named().into_iter().find(|(i, _)| i == id).map(|(_, n)| n).unwrap_or_else(|| "someone".into()),
        TargetKind::Prop(hash) => props
            .and_then(|m| m.props.iter().find(|p| p.hash == *hash))
            .map(|p| p.name.to_lowercase())
            .map(|n| format!("the {n}"))
            .unwrap_or_else(|| "a prop".into()),
        TargetKind::Doorway(name) if !name.is_empty() => format!("the way to {}", crate::hud::pretty(name)),
        TargetKind::Doorway(_) => "a doorway".into(),
    }
}

/// What the emote wheel shows, while it is open.
pub fn emote_wheel_view(
    wheels: Res<Wheels>,
    st: Res<EmoteWheel>,
    settings: Res<EmoteSettings>,
    w: Res<WorldClient>,
    props: Option<Res<PropMenu>>,
    active: Res<ActiveDevice>,
    mut view: ResMut<WheelView>,
) {
    if wheels.open != Some(WheelKind::Emotes) {
        return;
    }
    let mut v = WheelView { radius: MAIN_RADIUS, ..default() };
    for (i, name) in settings.slots.iter().enumerate().take(EMOTE_SLOTS) {
        v.slots.push(SlotView { at: wheel_spot(i, EMOTE_SLOTS, MAIN_RADIUS), icon: None, label: name.clone(), picked: st.choice == Some(i) });
    }
    let name = st.choice.and_then(|c| settings.slots.get(c)).cloned().unwrap_or_default();
    let target = match (&st.locked, &st.aim) {
        (Some(l), _) => format!("\nlocked on {}", target_name(l, &w, props.as_deref())),
        (None, Some(a)) => format!("\n({})", target_name(a, &w, props.as_deref())),
        (None, None) => String::new(),
    };
    v.centre = format!("{name}{target}");
    v.hint = match (active.is_gamepad(), st.locked.is_some()) {
        (true, false) => "Left stick to look, left trigger to lock on".into(),
        (true, true) => "Left trigger again to let go".into(),
        (false, false) => "Right click locks on to what is in the middle of the view".into(),
        (false, true) => "Right click again to let go".into(),
    };
    if *view != v {
        *view = v;
    }
}

/// The emotes: settings, the wheel's state, saving. The wheel itself is driven by
/// [`crate::wheel::WheelPlugin`].
pub struct EmotePlugin;

impl Plugin for EmotePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load()).init_resource::<EmoteWheel>().add_systems(Update, save_emotes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(kind: TargetKind, x: f32, z: f32) -> Target {
        Target { kind, at: Vec3::new(x, 0.0, z) }
    }

    #[test]
    fn aiming_takes_what_is_in_view_and_near() {
        let m = vox_core::in_blocks(1.0);
        let ahead = at(TargetKind::Player(1), 0.0, 4.0 * m);
        let right = at(TargetKind::Doorway("yard".into()), 4.0 * m, 0.0);
        let far = at(TargetKind::Player(2), 0.0, (TARGET_M + 1.0) * m);
        let all = vec![ahead.clone(), right.clone(), far];
        assert_eq!(snap_target(Vec3::ZERO, Vec3::Z, AIM_CONE_DEG, &all), Some(ahead.clone()));
        assert_eq!(snap_target(Vec3::ZERO, Vec3::X, AIM_CONE_DEG, &all), Some(right));
        assert_eq!(snap_target(Vec3::ZERO, Vec3::NEG_Z, AIM_CONE_DEG, &all), None, "nothing behind");
        assert_eq!(snap_target(Vec3::ZERO, Vec3::ZERO, AIM_CONE_DEG, &all), None, "no direction");
        // two in line: the nearer
        let nearer = at(TargetKind::Prop([1; 16]), 0.0, 2.0 * m);
        assert_eq!(snap_target(Vec3::ZERO, Vec3::Z, AIM_CONE_DEG, &[ahead, nearer.clone()]), Some(nearer));
        // 30 degrees off the middle of the view is not aimed at
        let off = at(TargetKind::Player(3), 30f32.to_radians().sin() * 5.0 * m, 30f32.to_radians().cos() * 5.0 * m);
        assert!(snap_target(Vec3::ZERO, Vec3::Z, AIM_CONE_DEG, &[off]).is_none());
    }

    #[test]
    fn lock_on_takes_the_aim_and_lets_go_the_second_time() {
        let sam = at(TargetKind::Player(1), 0.0, 4.0);
        let door = at(TargetKind::Doorway("yard".into()), 4.0, 0.0);
        assert_eq!(toggle_lock(None, Some(sam.clone())), Some(sam.clone()));
        assert_eq!(toggle_lock(Some(sam.clone()), Some(sam.clone())), None, "the same again lets go");
        assert_eq!(toggle_lock(Some(sam.clone()), Some(door.clone())), Some(door), "aimed at another: switch to it");
        assert_eq!(toggle_lock(Some(sam), None), None, "aimed at nothing lets go");
        assert_eq!(toggle_lock(None, None), None);
    }

    #[test]
    fn a_lock_follows_its_target_until_it_is_gone() {
        let m = vox_core::in_blocks(1.0);
        let sam = at(TargetKind::Player(1), 0.0, 4.0 * m);
        let moved = at(TargetKind::Player(1), 2.0 * m, 6.0 * m);
        assert_eq!(follow(&sam, Vec3::ZERO, std::slice::from_ref(&moved)), Some(moved));
        assert_eq!(follow(&sam, Vec3::ZERO, &[]), None, "gone");
        let away = at(TargetKind::Player(1), 0.0, (TARGET_M * 2.0) * m);
        assert_eq!(follow(&sam, Vec3::ZERO, &[away]), None, "out of reach");
    }

    #[test]
    fn two_copies_of_one_prop_are_told_apart_by_where_they_are() {
        let m = vox_core::in_blocks(1.0);
        let lamp = at(TargetKind::Prop([3; 16]), 0.0, 4.0 * m);
        let other = at(TargetKind::Prop([3; 16]), 3.0 * m, 4.0 * m);
        assert_eq!(follow(&lamp, Vec3::ZERO, &[other.clone(), lamp.clone()]), Some(lamp.clone()), "not the first copy found");
        assert_eq!(follow(&lamp, Vec3::ZERO, std::slice::from_ref(&other)), None);
        assert_eq!(toggle_lock(Some(lamp.clone()), Some(other.clone())), Some(other), "aimed at the other copy: switch to it");
        assert_eq!(toggle_lock(Some(lamp.clone()), Some(lamp)), None);
    }

    #[test]
    fn saved_emotes_are_eight_clean_names() {
        let s = from_json(r#"{"slots": ["Sleep", "", "Bad\u0007name", "x"]}"#);
        assert_eq!(s.slots.len(), EMOTE_SLOTS);
        assert_eq!(s.slots[0], "Sleep");
        assert_eq!(s.slots[1], DEFAULT_EMOTES[1], "an empty one is the default's");
        assert_eq!(s.slots[2], "Badname", "no control characters");
        assert_eq!(s.slots[7], DEFAULT_EMOTES[7]);
        assert_eq!(from_json("not json"), EmoteSettings::default());
        let long = format!(r#"{{"slots": ["{}"]}}"#, "a".repeat(200));
        assert_eq!(from_json(&long).slots[0].chars().count(), NAME_MAX);
        assert_eq!(from_json(&to_json(&s)), s, "round trip");
    }

    #[test]
    fn moving_is_not_an_emote() {
        assert!(!is_emote("Walk") && !is_emote("Idle"));
        assert!(is_emote("Victory"));
        for e in DEFAULT_EMOTES {
            assert!(is_emote(e));
        }
    }
}
