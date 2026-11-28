//! The pixel HUD: what is in your hand (bottom right, with its name for a moment when it
//! changes), a row of button hints for the device in use, and the voice corner. There is no
//! hotbar: blocks and props are picked on the block wheel ([`crate::wheel`]) or with the number
//! keys.
//!
//! Add [`HudPlugin`] to draw it; without it, [`crate::client::hud`] keeps showing the quick blocks
//! as text (the streaming app does that).

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

/// Present when the pixel HUD draws what is in your hand (the status text then moves top left).
#[derive(Resource, Default)]
pub struct PixelHud;

pub const SLOT: f32 = 52.0;
pub const FRAME: Color = Color::srgba(0.05, 0.06, 0.09, 0.62);
pub const PICKED: Color = Color::srgb(0.98, 0.98, 1.0);
pub const EDGE: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct HandIcon;
#[derive(Component)]
struct HandLabel;
#[derive(Component)]
struct HandName;
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

/// What is in your hand, as the HUD shows it: a block (by id) or a prop (by name).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Hand {
    Block(u16),
    Prop(String),
}

/// What the HUD showed last, so it's only redrawn on change.
#[derive(Default)]
struct Shown {
    hand: Option<Hand>,
    name_until: f64,
    hints_until: f64,
    device: Option<u32>,
}

fn spawn(mut commands: Commands, style: Res<HintStyle>) {
    let font = |px: f32| TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(px), ..default() };
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            Pickable::IGNORE,
            HudRoot,
            Visibility::Hidden,
        ))
        .with_children(|hud| {
            // the hints, bottom centre
            hud.spawn((
                Node { position_type: PositionType::Absolute, bottom: Val::Px(14.0), left: Val::Px(0.0), right: Val::Px(0.0), justify_content: JustifyContent::Center, ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn((hint_node_row(), BackgroundColor(FRAME), HintRow, Visibility::Hidden)).with_children(|row| {
                    for a in [Action::Break, Action::Place, Action::QuickWheel, Action::EmoteWheel, Action::Inventory, Action::Menu] {
                        row.spawn((hint_node(), Hint::new(a)));
                    }
                });
            });
            // in your hand, bottom right: its name, then its picture
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(14.0),
                    right: Val::Px(14.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|corner| {
                corner.spawn((Text::new(""), font(12.0), TextColor(Color::srgb(0.98, 0.98, 1.0)), crate::input::hints::pixel_shadow(), HandName));
                corner
                    .spawn((
                        Node {
                            width: Val::Px(SLOT),
                            height: Val::Px(SLOT),
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BorderColor::all(PICKED),
                        BackgroundColor(FRAME),
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            ImageNode::default(),
                            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                            Visibility::Hidden,
                            HandIcon,
                        ));
                        slot.spawn((
                            Text::new(""),
                            font(7.0),
                            TextColor(Color::srgb(0.98, 0.98, 1.0)),
                            crate::input::hints::pixel_shadow(),
                            TextLayout::justify(Justify::Center),
                            Node { position_type: PositionType::Absolute, ..default() },
                            HandLabel,
                        ));
                    });
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
    mut root: Query<&mut Visibility, (With<HudRoot>, Without<HandIcon>, Without<HintRow>)>,
    mut rows: Query<&mut Visibility, (With<HintRow>, Without<HandIcon>, Without<HudRoot>)>,
    mut pics: Query<(&mut ImageNode, &mut Visibility), (With<HandIcon>, Without<HudRoot>, Without<HintRow>)>,
    mut labels: Query<&mut Text, (With<HandLabel>, Without<HandName>)>,
    mut names: Query<&mut Text, (With<HandName>, Without<HandLabel>)>,
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
    let hand = match &w.hand_prop {
        Some(p) => Some(Hand::Prop(p.name.clone())),
        None => w.hotbar().get(w.slot).map(|b| Hand::Block(*b)),
    };
    if hand != shown.hand {
        let icon = match &hand {
            Some(Hand::Block(id)) => icons.get(*id, &w, &blocks, &assets),
            _ => None,
        };
        for (mut img, mut v) in &mut pics {
            match &icon {
                Some(h) => {
                    img.image = h.clone();
                    *v = Visibility::Inherited;
                }
                None => *v = Visibility::Hidden,
            }
        }
        let label = match &hand {
            Some(Hand::Prop(name)) if icon.is_none() => name.clone(),
            _ => String::new(),
        };
        for mut t in &mut labels {
            t.0 = label.clone();
        }
        shown.name_until = now + 2.0;
        shown.hand = hand.clone();
    }
    // its name for a moment after it changes
    let name = match &hand {
        Some(_) if now >= shown.name_until => String::new(),
        Some(Hand::Block(b)) => pretty(w.block_name(*b)),
        Some(Hand::Prop(n)) => n.clone(),
        None => String::new(),
    };
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

/// The voice corner, bottom left (the hints are bottom centre, your hand bottom right): the mic marker, and under it
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

/// The pixel HUD: your hand, its name, hints, the voice corner. Needs
/// [`crate::input::InputPlugin`].
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PixelHud>()
            .init_resource::<BlockIcons>()
            .init_resource::<HintStyle>()
            .add_systems(Startup, (spawn, spawn_voice_mark))
            .add_systems(Update, (update, voice_mark));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
