//! The pixel HUD: the hotbar at the bottom with block pictures, the name of the block in your
//! hand when it changes, and a row of button hints for the device in use.
//!
//! Add [`HudPlugin`] to draw it; without it, [`crate::client::hud`] keeps showing the hotbar as
//! text (the streaming app does that).

use std::collections::HashMap;

use bevy::prelude::*;
use vox_core::wire::VOICE_PARTY;

use crate::{
    blocks::Blocks,
    client::{CamMode, WorldClient},
    voice::plugin::{VoiceSettings, VoiceState},
    input::{
        Action, ActiveDevice, UiMode,
        hints::{Hint, HintStyle, hint_node},
    },
};

/// Present when the pixel HUD draws the hotbar (the status text then moves top left).
#[derive(Resource, Default)]
pub struct PixelHud;

pub const SLOT: f32 = 52.0;
pub const FRAME: Color = Color::srgba(0.05, 0.06, 0.09, 0.62);
pub const PICKED: Color = Color::srgb(0.98, 0.98, 1.0);
pub const EDGE: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);

#[derive(Component)]
struct HotbarRoot;
#[derive(Component)]
struct HotbarSlot(usize);
#[derive(Component)]
struct HotbarIcon(usize);
#[derive(Component)]
struct HotbarName;
#[derive(Component)]
struct HintRow;

/// Block pictures by world block id: the block's side tile.
#[derive(Resource, Default)]
pub struct BlockIcons(HashMap<u16, Handle<Image>>);

impl BlockIcons {
    /// The picture for world block `id`, loading it the first time.
    pub fn get(&mut self, id: u16, w: &WorldClient, blocks: &Blocks, assets: &AssetServer) -> Option<Handle<Image>> {
        if let Some(h) = self.0.get(&id) {
            return Some(h.clone());
        }
        let pal = blocks.palette.as_ref()?;
        let def = pal.id(w.block_name(id)).and_then(|local| pal.def(local))?;
        let h: Handle<Image> = assets.load(format!("blocks/tiles/{}.png", def.side()));
        self.0.insert(id, h.clone());
        Some(h)
    }
}

/// What the hotbar showed last, so it's only redrawn on change.
#[derive(Default)]
struct Shown {
    bar: Vec<u16>,
    slot: usize,
    name_until: f64,
    hints_until: f64,
    device: Option<u32>,
}

fn spawn(mut commands: Commands, style: Res<HintStyle>) {
    // the hotbar, its block name above it, and the hints above that, bottom centre
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(8.0),
                ..default()
            },
            Pickable::IGNORE,
            HotbarRoot,
            Visibility::Hidden,
        ))
        .with_children(|col| {
            col.spawn((hint_node_row(), BackgroundColor(FRAME), HintRow, Visibility::Hidden)).with_children(|row| {
                for a in [Action::Break, Action::Place, Action::QuickWheel, Action::Inventory, Action::Menu] {
                    row.spawn((hint_node(), Hint::new(a)));
                }
            });
            col.spawn((
                Text::new(""),
                TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(12.0), ..default() },
                TextColor(Color::srgb(0.98, 0.98, 1.0)),
                crate::input::hints::pixel_shadow(),
                HotbarName,
            ));
            col.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(FRAME),
            ))
            .with_children(|bar| {
                for i in 0..9 {
                    bar.spawn((
                        Node {
                            width: Val::Px(SLOT),
                            height: Val::Px(SLOT),
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BorderColor::all(EDGE),
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)),
                        HotbarSlot(i),
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            ImageNode::default(),
                            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                            Visibility::Hidden,
                            HotbarIcon(i),
                        ));
                        slot.spawn((
                            Text::new(format!("{}", i + 1)),
                            TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(8.0), ..default() },
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
                            crate::input::hints::pixel_shadow(),
                            Node { position_type: PositionType::Absolute, top: Val::Px(2.0), left: Val::Px(3.0), ..default() },
                        ));
                    });
                }
            });
        });
}

fn hint_node_row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(16.0),
        padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
        border_radius: BorderRadius::all(Val::Px(6.0)),
        ..default()
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update(
    w: Res<WorldClient>,
    mode: Res<UiMode>,
    active: Res<ActiveDevice>,
    blocks: Res<Blocks>,
    assets: Res<AssetServer>,
    mut icons: ResMut<BlockIcons>,
    mut shown: Local<Shown>,
    mut root: Query<&mut Visibility, (With<HotbarRoot>, Without<HotbarIcon>, Without<HintRow>)>,
    mut rows: Query<&mut Visibility, (With<HintRow>, Without<HotbarIcon>, Without<HotbarRoot>)>,
    mut slots: Query<(&HotbarSlot, &mut BorderColor, &mut BackgroundColor)>,
    mut pics: Query<(&HotbarIcon, &mut ImageNode, &mut Visibility), (Without<HotbarRoot>, Without<HintRow>)>,
    mut names: Query<&mut Text, With<HotbarName>>,
) {
    let playing = w.active() && w.ready && !w.watch;
    let want = if playing && *mode == UiMode::Game { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut root {
        if *v != want {
            *v = want;
        }
    }
    if !playing {
        return;
    }
    let now = w.now();
    let bar = w.hotbar().to_vec();
    let changed = bar != shown.bar || w.slot != shown.slot;
    if changed {
        for (s, mut b, mut bg) in &mut slots {
            let picked = s.0 == w.slot;
            *b = BorderColor::all(if picked { PICKED } else { EDGE });
            bg.0 = if picked { Color::srgba(1.0, 1.0, 1.0, 0.22) } else { Color::srgba(1.0, 1.0, 1.0, 0.06) };
        }
        for (i, mut img, mut v) in &mut pics {
            match bar.get(i.0).and_then(|id| icons.get(*id, &w, &blocks, &assets)) {
                Some(h) => {
                    img.image = h;
                    *v = Visibility::Inherited;
                }
                None => *v = Visibility::Hidden,
            }
        }
        if w.slot != shown.slot || shown.bar.is_empty() {
            shown.name_until = now + 2.0;
        }
        shown.bar = bar.clone();
        shown.slot = w.slot;
    }
    // the block's name for a moment after it changes
    let name = if now < shown.name_until { bar.get(w.slot).map(|b| pretty(w.block_name(*b))).unwrap_or_default() } else { String::new() };
    for mut t in &mut names {
        if t.0 != name {
            t.0 = name.clone();
        }
    }
    // hints for the first 20 seconds, and for 10 after switching device; not from above
    if shown.device != Some(active.generation) {
        let first = shown.device.is_none();
        shown.device = Some(active.generation);
        shown.hints_until = now + if first { 20.0 } else { 10.0 };
    }
    let hints = if now < shown.hints_until && w.cam_mode != CamMode::Diablo { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut rows {
        if *v != hints {
            *v = hints;
        }
    }
}

// ---- the quick wheel

/// The quick wheel: the hotbar's nine blocks round a circle, open while its button is held
/// (d-pad left, or Tab). Point with the right stick or the mouse; let go to take the block.
/// It picks hotbar slots, so the keyboard's number keys and the wheel stay in step.
#[derive(Resource, Default, Debug)]
pub struct QuickWheel {
    pub open: bool,
    /// held open without its button (scripts, screenshots), pointing this way
    pub scripted: Option<Vec2>,
    pointer: Vec2,
    choice: Option<usize>,
}

#[derive(Component)]
struct WheelRoot;
#[derive(Component)]
struct WheelSlot(usize);
#[derive(Component)]
struct WheelIcon(usize);
#[derive(Component)]
struct WheelName;

const WHEEL_RADIUS: f32 = 118.0;

/// Which of `n` slots a pointer (x right, y up) points at: slot 0 at the top, then clockwise.
/// Nothing until it's pushed a little way out.
pub fn wheel_choice(pointer: Vec2, n: usize) -> Option<usize> {
    if n == 0 || pointer.length() < 28.0 {
        return None;
    }
    let tau = std::f32::consts::TAU;
    let a = pointer.x.atan2(pointer.y).rem_euclid(tau);
    Some(((a / (tau / n as f32)).round() as usize) % n)
}

/// Where slot `i` of `n` sits round the wheel, from its centre (x right, y down, pixels).
fn wheel_spot(i: usize, n: usize) -> Vec2 {
    let a = std::f32::consts::TAU * i as f32 / n as f32;
    Vec2::new(a.sin(), -a.cos()) * WHEEL_RADIUS
}

fn quick_wheel(actions: Res<crate::input::Actions>, mode: Res<UiMode>, mut wheel: ResMut<QuickWheel>, mut w: ResMut<WorldClient>) {
    let playing = w.active() && w.ready && !w.watch && *mode == UiMode::Game;
    if !playing {
        wheel.open = false;
        return;
    }
    let n = w.hotbar().len().min(9);
    if actions.pressed(Action::QuickWheel) || wheel.scripted.is_some() {
        if !wheel.open {
            let scripted = wheel.scripted;
            *wheel = QuickWheel { open: true, scripted, ..default() };
        }
        if let Some(p) = wheel.scripted {
            wheel.pointer = p;
        }
        // the mouse moves a pointer from the middle (screen y is down); the stick points
        let d = actions.mouse_delta;
        wheel.pointer = (wheel.pointer + Vec2::new(d.x, -d.y)).clamp_length_max(100.0);
        if actions.look_stick != Vec2::ZERO {
            wheel.pointer = actions.look_stick.normalize_or_zero() * 100.0;
        }
        if let Some(c) = wheel_choice(wheel.pointer, n) {
            wheel.choice = Some(c);
        }
    } else if wheel.open {
        wheel.open = false;
        if let Some(c) = wheel.choice.filter(|c| *c < n) {
            w.slot = c;
        }
    }
}

fn spawn_wheel(mut commands: Commands, style: Res<HintStyle>) {
    let size = WHEEL_RADIUS * 2.0 + SLOT + 16.0;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(400),
            Visibility::Hidden,
            WheelRoot,
        ))
        .with_children(|r| {
            r.spawn((
                Node {
                    width: Val::Px(size),
                    height: Val::Px(size),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(size / 2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.55)),
            ))
            .with_children(|ring| {
                ring.spawn((
                    Text::new(""),
                    TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(11.0), ..default() },
                    TextColor(Color::srgb(0.98, 0.98, 1.0)),
                    crate::input::hints::pixel_shadow(),
                    TextLayout::justify(Justify::Center),
                    Node { max_width: Val::Px(150.0), ..default() },
                    WheelName,
                ));
                for i in 0..9 {
                    let at = wheel_spot(i, 9) + Vec2::splat(size / 2.0) - Vec2::splat(SLOT / 2.0);
                    ring.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(at.x),
                            top: Val::Px(at.y),
                            width: Val::Px(SLOT),
                            height: Val::Px(SLOT),
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(SLOT / 2.0)),
                            ..default()
                        },
                        BorderColor::all(EDGE),
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                        WheelSlot(i),
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            ImageNode::default(),
                            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), border_radius: BorderRadius::all(Val::Px(SLOT / 2.0)), ..default() },
                            Visibility::Hidden,
                            WheelIcon(i),
                        ));
                    });
                }
            });
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_wheel(
    wheel: Res<QuickWheel>,
    w: Res<WorldClient>,
    blocks: Res<Blocks>,
    assets: Res<AssetServer>,
    mut icons: ResMut<BlockIcons>,
    mut root: Query<&mut Visibility, (With<WheelRoot>, Without<WheelIcon>)>,
    mut slots: Query<(&WheelSlot, &mut BorderColor, &mut BackgroundColor, &mut Node)>,
    mut pics: Query<(&WheelIcon, &mut ImageNode, &mut Visibility), Without<WheelRoot>>,
    mut names: Query<&mut Text, With<WheelName>>,
    mut was_open: Local<bool>,
) {
    let show = if wheel.open { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut root {
        if *v != show {
            *v = show;
        }
    }
    if !wheel.open {
        *was_open = false;
        return;
    }
    let bar = w.hotbar();
    let picked = wheel.choice.unwrap_or(w.slot);
    if !*was_open || wheel.is_changed() {
        for (s, mut b, mut bg, mut node) in &mut slots {
            let on = s.0 == picked;
            *b = BorderColor::all(if on { PICKED } else { EDGE });
            bg.0 = if on { Color::srgba(1.0, 1.0, 1.0, 0.28) } else { Color::srgba(1.0, 1.0, 1.0, 0.08) };
            let grow = if on { 6.0 } else { 0.0 };
            node.padding = UiRect::all(Val::Px(6.0 - grow / 2.0));
        }
        for (i, mut img, mut v) in &mut pics {
            match bar.get(i.0).and_then(|id| icons.get(*id, &w, &blocks, &assets)) {
                Some(h) => {
                    img.image = h;
                    *v = Visibility::Inherited;
                }
                None => *v = Visibility::Hidden,
            }
        }
        let name = bar.get(picked).map(|b| pretty(w.block_name(*b))).unwrap_or_default();
        for mut t in &mut names {
            if t.0 != name {
                t.0 = name.clone();
            }
        }
    }
    *was_open = true;
}

/// "oak_planks" as "Oak planks".
pub fn pretty(name: &str) -> String {
    let s = name.replace('_', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

#[derive(Component)]
struct VoiceMark;

/// The text of the mic marker: what you are sending on, or nothing (voice off, or not sending).
pub fn mic_text(enabled: bool, transmitting: bool, channel: u8) -> &'static str {
    match (enabled && transmitting, channel == VOICE_PARTY) {
        (false, _) => "",
        (true, true) => "MIC Party",
        (true, false) => "MIC Nearby",
    }
}

/// How long after its last packet a player still counts as speaking, in seconds.
const SPEAKING_FOR: f64 = 0.4;
/// Most names the HUD lists under the mic marker.
const SPEAKERS_MAX: usize = 4;

/// The names to list under the mic marker: players heard within [`SPEAKING_FOR`] seconds, in the
/// roster's order (by name), at most [`SPEAKERS_MAX`].
pub fn speaker_names(roster: &[(u32, String)], speaking: &HashMap<u32, f64>, now: f64) -> Vec<String> {
    roster
        .iter()
        .filter(|(id, _)| speaking.get(id).is_some_and(|&heard| now - heard < SPEAKING_FOR))
        .take(SPEAKERS_MAX)
        .map(|(_, name)| name.clone())
        .collect()
}

/// The speaker list text: one "> Name" line each, or nothing.
pub fn speaker_text(names: &[String]) -> String {
    names.iter().map(|n| format!("> {n}")).collect::<Vec<_>>().join("\n")
}

#[derive(Component)]
struct SpeakerMark;

/// Puts `want` in a HUD text and shows the text only while it is not empty.
fn show_text(text: &mut Text, vis: &mut Visibility, want: &str) {
    let show = if want.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    if *vis != show {
        *vis = show;
    }
    if text.0 != want {
        text.0 = want.into();
    }
}

/// The voice corner, bottom left (the hotbar is bottom centre): the mic marker, and under it
/// the names of players who are speaking. Each text hides itself while it is empty.
fn spawn_voice_mark(mut commands: Commands, style: Res<HintStyle>) {
    let label = || {
        (
            Text::new(""),
            TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(12.0), ..default() },
            TextColor(Color::srgb(0.98, 0.98, 1.0)),
            crate::input::hints::pixel_shadow(),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(FRAME),
            Pickable::IGNORE,
            Visibility::Hidden,
        )
    };
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, bottom: Val::Px(14.0), left: Val::Px(14.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|corner| {
            corner.spawn((label(), VoiceMark));
            corner.spawn((label(), SpeakerMark));
        });
}

// the two texts are told apart by their markers, which the filters spell out
#[allow(clippy::type_complexity)]
fn voice_mark(
    settings: Option<Res<VoiceSettings>>,
    state: Option<Res<VoiceState>>,
    w: Res<WorldClient>,
    mut mic: Query<(&mut Text, &mut Visibility), (With<VoiceMark>, Without<SpeakerMark>)>,
    mut speakers: Query<(&mut Text, &mut Visibility), (With<SpeakerMark>, Without<VoiceMark>)>,
) {
    // without the voice plugin there is nothing to show
    let (mic_want, speakers_want) = match (settings, state) {
        (Some(s), Some(st)) => {
            let mic_want = mic_text(s.enabled, st.transmitting, s.channel);
            // the roster is only read while voice is on: it is a copy of the player list
            let speakers_want = if s.enabled { speaker_text(&speaker_names(&w.others_named(), &st.speaking, w.now())) } else { String::new() };
            (mic_want.to_string(), speakers_want)
        }
        _ => (String::new(), String::new()),
    };
    for (mut text, mut vis) in &mut mic {
        show_text(&mut text, &mut vis, &mic_want);
    }
    for (mut text, mut vis) in &mut speakers {
        show_text(&mut text, &mut vis, &speakers_want);
    }
}

/// The pixel HUD: hotbar, block names, hints. Needs [`crate::input::InputPlugin`].
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PixelHud>()
            .init_resource::<BlockIcons>()
            .init_resource::<HintStyle>()
            .init_resource::<QuickWheel>()
            .add_systems(Startup, (spawn, spawn_wheel, spawn_voice_mark))
            .add_systems(Update, (quick_wheel.before(crate::client::controls), update, draw_wheel, voice_mark));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_points_at_slots_clockwise_from_the_top() {
        assert_eq!(wheel_choice(Vec2::new(0.0, 100.0), 9), Some(0));
        assert_eq!(wheel_choice(Vec2::new(100.0, 0.0), 9), Some(2), "a quarter turn of nine is about two slots");
        assert_eq!(wheel_choice(Vec2::new(-100.0, 10.0), 9), Some(7));
        assert_eq!(wheel_choice(Vec2::new(5.0, 5.0), 9), None, "too near the middle");
        assert_eq!(wheel_choice(Vec2::new(0.0, 100.0), 0), None);
    }

    #[test]
    fn slots_sit_round_the_circle() {
        let top = wheel_spot(0, 9);
        assert!(top.x.abs() < 1e-3 && top.y < 0.0, "slot 0 at the top: {top}");
        assert!((wheel_spot(3, 9).length() - WHEEL_RADIUS).abs() < 1e-3);
    }

    #[test]
    fn mic_marker_shows_only_while_sending_with_voice_on() {
        assert_eq!(mic_text(true, true, vox_core::wire::VOICE_NEARBY), "MIC Nearby");
        assert_eq!(mic_text(true, true, VOICE_PARTY), "MIC Party");
        assert_eq!(mic_text(false, true, VOICE_PARTY), "", "voice off shows nothing");
        assert_eq!(mic_text(true, false, VOICE_PARTY), "", "silent shows nothing");
    }

    #[test]
    fn speakers_are_heard_inside_the_window_in_roster_order_capped_at_four() {
        let roster: Vec<(u32, String)> = (1..=6).map(|i| (i, format!("P{i}"))).collect();
        let mut speaking = HashMap::new();
        speaking.insert(1, 9.7); // 0.3 s ago: speaking
        speaking.insert(2, 9.9);
        speaking.insert(3, 9.6); // 0.4 s ago: quiet (the window is strict)
        speaking.insert(4, 9.9);
        speaking.insert(5, 9.8);
        speaking.insert(6, 9.95);
        assert_eq!(speaker_names(&roster, &speaking, 10.0), vec!["P1", "P2", "P4", "P5"], "four at most, roster order");
        assert_eq!(speaker_names(&roster[2..3], &speaking, 10.0), Vec::<String>::new(), "a quiet player is not listed");
        assert!(speaker_names(&roster, &HashMap::new(), 10.0).is_empty(), "nobody speaks");
    }

    #[test]
    fn speaker_text_is_one_line_per_name_or_nothing() {
        assert_eq!(speaker_text(&[]), "");
        assert_eq!(speaker_text(&["Ann".into(), "Bo".into()]), "> Ann\n> Bo");
    }

    #[test]
    fn block_names_read_as_words() {
        assert_eq!(super::pretty("oak_planks"), "Oak planks");
        assert_eq!(super::pretty(""), "");
    }
}
