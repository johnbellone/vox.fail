//! Menus you can drive with a controller alone: the pause menu, controls (rebinding),
//! settings, and the inventory (in a creative world, every block to put in your hotbar).
//!
//! Each screen is a set of focusable choices laid out in rows and columns; the d-pad, the left
//! stick or the arrow keys move focus to the nearest choice that way (Bevy's directional
//! navigation), Confirm chooses, Back goes back a screen and Menu (Start) closes them all. The
//! mouse works on the same screens: hovering focuses, clicking chooses. While a menu is open
//! [`UiMode::Menu`] tells the game to leave the input alone.

use std::collections::HashSet;

use bevy::{
    input::gamepad::Gamepad,
    input_focus::{FocusCause, InputFocus, InputFocusVisible, directional_navigation::DirectionalNavigationPlugin},
    picking::{events::PointerClick, hover::Hovered},
    prelude::*,
    ui::auto_directional_navigation::{AutoDirectionalNavigation, AutoDirectionalNavigator},
};

use crate::{
    blocks::Blocks,
    client::WorldClient,
    hud::{BlockIcons, pretty},
    input::{
        Action, ActiveDevice, Actions, InputSettings, UiMode,
        bindings::{self, Bindings, Input},
        hints::{self, Hint, HintStyle, hint_node, pixel_shadow},
    },
    voice::plugin::{VoiceSettings, next_channel},
    emote::EmoteSettings,
};
use vox_core::wire::VOICE_PARTY;

/// A screen of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Pause,
    Inventory,
    Controls,
    Settings,
    Voice,
    Emotes,
}

impl Screen {
    fn title(self) -> &'static str {
        match self {
            Screen::Pause => "Paused",
            Screen::Inventory => "Blocks",
            Screen::Controls => "Controls",
            Screen::Settings => "Settings",
            Screen::Voice => "Voice",
            Screen::Emotes => "Emotes",
        }
    }
}

/// The open screens (the top one shows) and what they're in the middle of.
#[derive(Resource, Default)]
pub struct Menus {
    stack: Vec<Screen>,
    dirty: bool,
    /// waiting for the button to bind to an action
    capture: Option<Capture>,
    /// the choice that had focus on each screen, to come back to it
    last: Vec<(Screen, Choice)>,
}

impl Menus {
    pub fn open(&mut self, s: Screen) {
        self.stack.push(s);
        self.dirty = true;
    }
    pub fn back(&mut self) {
        self.stack.pop();
        self.capture = None;
        self.dirty = true;
    }
    pub fn close(&mut self) {
        self.stack.clear();
        self.capture = None;
        self.dirty = true;
    }
    pub fn top(&self) -> Option<Screen> {
        self.stack.last().copied()
    }
    /// Swap the top screen for a sibling (tabs between controls and settings).
    fn switch(&mut self, s: Screen) {
        if let Some(t) = self.stack.last_mut() {
            *t = s;
            self.dirty = true;
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Capture {
    action: Action,
    gamepad: bool,
    /// everything was let go since it started (so the press that started it doesn't count)
    armed: bool,
    started: f32,
}

/// What a focusable thing in a menu does.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Resume,
    Open(Screen),
    Leave,
    Bind(Action),
    ResetAll,
    InvertY,
    Nintendo,
    Adjust(Setting, i8),
    /// flip a voice setting (enable, talk mode, channel)
    VoiceToggle(VoiceItem),
    /// step a voice setting: device lists cycle, volumes move 10 percent
    VoiceStep(VoiceItem, i8),
    /// mute or hear one player (their number), by the Voice screen's rows
    Mute(u32),
    /// put this block in your hand (the current quick slot)
    Block(u16),
    /// make this quick slot the current one
    Slot(usize),
    /// step the emote in this slot of the emote wheel through the character's clips
    Emote(usize, i8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    MouseLook,
    StickLook,
    DeadZone,
}

/// The voice settings the Voice screen changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceItem {
    Enable,
    DeviceIn,
    DeviceOut,
    InGain,
    OutGain,
    Ptt,
    Channel,
}

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct CaptureNote;

/// What the menu last drew, so it redraws only when something it shows has changed.
#[derive(Default)]
struct Seen {
    device: Option<u32>,
    /// the other players, on the Voice screen only
    roster: Vec<(u32, String)>,
}

const PANEL: Color = Color::srgba(0.06, 0.07, 0.10, 0.92);
const BUTTON: Color = Color::srgba(1.0, 1.0, 1.0, 0.07);
const BUTTON_FOCUS: Color = Color::srgba(0.45, 0.48, 0.95, 0.35);
const EDGE: Color = Color::srgba(1.0, 1.0, 1.0, 0.16);
const EDGE_FOCUS: Color = Color::srgb(0.62, 0.66, 1.0);
const INK: Color = Color::srgb(0.96, 0.96, 0.98);
const QUIET: Color = Color::srgb(0.66, 0.69, 0.78);

fn text(s: impl Into<String>, style: &HintStyle, size: f32, colour: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(size), ..default() },
        TextColor(colour),
        pixel_shadow(),
    )
}

fn button_node() -> Node {
    Node {
        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
        border: UiRect::all(Val::Px(2.0)),
        border_radius: BorderRadius::all(Val::Px(4.0)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        ..default()
    }
}

fn choice(c: Choice, node: Node) -> impl Bundle {
    (node, Hovered::default(), c, AutoDirectionalNavigation::default(), BackgroundColor(BUTTON), BorderColor::all(EDGE))
}

// ---- opening and closing

/// Open the pause menu or the inventory from the game; back out and close from a menu.
pub fn open_close(
    actions: Res<Actions>,
    w: Res<WorldClient>,
    mut menus: ResMut<Menus>,
    mut mode: ResMut<UiMode>,
) {
    let playing = w.active() && w.ready && !w.watch;
    if !playing {
        if !menus.stack.is_empty() {
            menus.close();
        }
    } else if menus.capture.is_some() {
        // rebinding: every button is a candidate, not a command
    } else if menus.stack.is_empty() {
        if actions.just_pressed(Action::Menu) {
            menus.open(Screen::Pause);
        } else if actions.just_pressed(Action::Inventory) {
            menus.open(Screen::Inventory);
        }
    } else if actions.just_pressed(Action::Back) {
        // (Escape is both Back and Menu: one screen back)
        menus.back();
    } else if actions.just_pressed(Action::Menu) || (actions.just_pressed(Action::Inventory) && menus.top() == Some(Screen::Inventory)) {
        menus.close();
    } else if actions.just_pressed(Action::TabNext) || actions.just_pressed(Action::TabPrev) {
        match menus.top() {
            Some(Screen::Controls) => menus.switch(Screen::Settings),
            Some(Screen::Settings) => menus.switch(Screen::Controls),
            _ => {}
        }
    }
    let want = if menus.stack.is_empty() { UiMode::Game } else { UiMode::Menu };
    if *mode != want {
        *mode = want;
    }
}

// ---- drawing

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    mut menus: ResMut<Menus>,
    roots: Query<Entity, With<MenuRoot>>,
    style: Res<HintStyle>,
    active: Res<ActiveDevice>,
    bindings: Res<Bindings>,
    settings: Res<InputSettings>,
    // voice and the emotes, in one parameter to stay within Bevy's sixteen
    extra: (Option<Res<VoiceSettings>>, Option<Res<EmoteSettings>>),
    w: Res<WorldClient>,
    blocks: Res<Blocks>,
    assets: Res<AssetServer>,
    mut icons: ResMut<BlockIcons>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    choices: Query<(Entity, &Choice)>,
    mut seen: Local<Seen>,
) {
    let (voice, emotes) = extra;
    // the controls screen shows the device in use: redraw when it changes
    let device_changed = seen.device != Some(active.generation);
    seen.device = Some(active.generation);
    // the Voice screen lists the other players: redraw when they come or go
    let on_voice = menus.top() == Some(Screen::Voice);
    let roster = if on_voice { w.others_named() } else { Vec::new() };
    let roster_changed = on_voice && roster != seen.roster;
    if roster_changed {
        seen.roster = roster.clone();
    }
    let redraw = menus.dirty
        || roster_changed
        || (device_changed && menus.top() == Some(Screen::Controls) && menus.capture.is_none())
        || (bindings.is_changed() && menus.top() == Some(Screen::Controls))
        || (settings.is_changed() && menus.top() == Some(Screen::Settings))
        || (emotes.as_ref().is_some_and(|e| e.is_changed()) && menus.top() == Some(Screen::Emotes));
    if !redraw {
        return;
    }
    // remember what had focus, to come back to it
    let keep = focus.get().and_then(|e| choices.get(e).ok()).map(|(_, c)| *c);
    if let (Some(top), Some(c)) = (menus.top(), keep) {
        menus.last.retain(|(s, _)| *s != top);
        menus.last.push((top, c));
    }
    menus.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(screen) = menus.top() else {
        focus.clear();
        return;
    };
    let gamepad = active.is_gamepad();
    visible.0 = gamepad;
    let want_focus = menus.last.iter().find(|(s, _)| *s == screen).map(|(_, c)| *c);
    let mut first: Option<Entity> = None;
    let mut wanted: Option<Entity> = None;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.05, 0.45)),
            GlobalZIndex(500),
            MenuRoot,
        ))
        .id();
    let mut note = |e: Entity, c: Choice| {
        first.get_or_insert(e);
        if Some(c) == want_focus {
            wanted = Some(e);
        }
    };
    commands.entity(root).with_children(|r| {
        r.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0),
                padding: UiRect::all(Val::Px(24.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                max_width: Val::Percent(94.0),
                max_height: Val::Percent(94.0),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(EDGE),
        ))
        .with_children(|panel| {
            panel.spawn(text(screen.title(), &style, 18.0, INK));
            match screen {
                Screen::Pause => {
                    for (label, c) in [
                        ("Resume", Choice::Resume),
                        ("Blocks", Choice::Open(Screen::Inventory)),
                        ("Controls", Choice::Open(Screen::Controls)),
                        ("Emotes", Choice::Open(Screen::Emotes)),
                        ("Voice", Choice::Open(Screen::Voice)),
                        ("Settings", Choice::Open(Screen::Settings)),
                        ("Leave world", Choice::Leave),
                    ] {
                        let e = panel
                            .spawn(choice(c, Node { width: Val::Px(260.0), ..button_node() }))
                            .with_children(|b| {
                                b.spawn(text(label, &style, 12.0, INK));
                            })
                            .id();
                        note(e, c);
                    }
                }
                Screen::Controls => {
                    panel.spawn(text(
                        if gamepad { "Controller: choose an action, then press its new button" } else { "Keyboard and mouse: choose an action, then press its new key" },
                        &style,
                        9.0,
                        QUIET,
                    ));
                    let game: Vec<Action> = Action::ALL.iter().copied().filter(|a| a.in_game()).collect();
                    let half = game.len().div_ceil(2);
                    panel
                        .spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(28.0), ..default() })
                        .with_children(|cols| {
                            for column in [&game[..half], &game[half..]] {
                                cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }).with_children(|col| {
                                    for a in column {
                                        col.spawn(Node {
                                            flex_direction: FlexDirection::Row,
                                            justify_content: JustifyContent::SpaceBetween,
                                            align_items: AlignItems::Center,
                                            width: Val::Px(330.0),
                                            ..default()
                                        })
                                        .with_children(|row| {
                                            row.spawn(text(a.label(), &style, 10.0, INK));
                                            let c = Choice::Bind(*a);
                                            let e = row
                                                .spawn(choice(c, Node { min_width: Val::Px(120.0), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), ..button_node() }))
                                                .with_children(|b| {
                                                    let device = active.device;
                                                    match hints::cap_for(*a, device, &bindings, &settings) {
                                                        Some(cap) => hints::spawn_cap(b, cap, &style, 20.0),
                                                        None => {
                                                            b.spawn(text("-", &style, 10.0, QUIET));
                                                        }
                                                    }
                                                })
                                                .id();
                                            note(e, c);
                                        });
                                    }
                                });
                            }
                        });
                    panel.spawn((text("", &style, 10.0, Color::srgb(1.0, 0.85, 0.4)), CaptureNote));
                    let e = panel
                        .spawn(choice(Choice::ResetAll, button_node()))
                        .with_children(|b| {
                            b.spawn(text("Reset all to defaults", &style, 10.0, INK));
                        })
                        .id();
                    note(e, Choice::ResetAll);
                }
                Screen::Settings | Screen::Voice | Screen::Emotes => {
                    let rows: Vec<Row> = if screen == Screen::Emotes {
                        panel.spawn(text("The emote wheel: hold the right bumper, or G. Kept on this device.", &style, 9.0, QUIET));
                        emotes.as_deref().map(emote_rows).unwrap_or_default()
                    } else if screen == Screen::Voice {
                        if let Some(line) = voice_browser_line(cfg!(target_arch = "wasm32")) {
                            // the browser has no voice yet, so the line is all it shows
                            panel.spawn(text(line, &style, 10.0, INK));
                            Vec::new()
                        } else {
                            let mut rows = voice.as_deref().map(voice_rows).unwrap_or_default();
                            rows.extend(voice.as_deref().map(|v| mute_rows(&roster, &v.muted)).unwrap_or_default());
                            rows
                        }
                    } else {
                        vec![
                        ("Invert look".into(), String::new(), vec![(Choice::InvertY, if settings.invert_y { "On" } else { "Off" }.into())]),
                        (
                            "Mouse look".into(),
                            format!("{:.1}", settings.mouse_look * 1000.0),
                            vec![(Choice::Adjust(Setting::MouseLook, -1), "-".into()), (Choice::Adjust(Setting::MouseLook, 1), "+".into())],
                        ),
                        (
                            "Stick look".into(),
                            format!("{:.1}", settings.stick_look),
                            vec![(Choice::Adjust(Setting::StickLook, -1), "-".into()), (Choice::Adjust(Setting::StickLook, 1), "+".into())],
                        ),
                        (
                            "Stick dead zone".into(),
                            format!("{:.2}", settings.dead_zone),
                            vec![(Choice::Adjust(Setting::DeadZone, -1), "-".into()), (Choice::Adjust(Setting::DeadZone, 1), "+".into())],
                        ),
                        (
                            "Confirm on the right button (Nintendo)".into(),
                            String::new(),
                            vec![(Choice::Nintendo, if settings.nintendo_confirm { "On" } else { "Off" }.into())],
                        ),
                        ]
                    };
                    for (label, value, buttons) in rows {
                        panel
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                width: Val::Px(560.0),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn(text(label, &style, 10.0, INK));
                                row.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })
                                    .with_children(|right| {
                                        let n = buttons.len();
                                        for (i, (c, l)) in buttons.into_iter().enumerate() {
                                            if i == 1 && n == 2 {
                                                right.spawn((text(value.clone(), &style, 10.0, INK), Node { min_width: Val::Px(56.0), ..default() }));
                                            }
                                            let e = right
                                                .spawn(choice(c, Node { min_width: Val::Px(56.0), ..button_node() }))
                                                .with_children(|b| {
                                                    b.spawn(text(l, &style, 10.0, INK));
                                                })
                                                .id();
                                            note(e, c);
                                        }
                                    });
                            });
                    }
                    if screen == Screen::Voice && roster.is_empty() && voice_browser_line(cfg!(target_arch = "wasm32")).is_none() {
                        panel.spawn(text("Nobody else is here.", &style, 10.0, QUIET));
                    }
                }
                Screen::Inventory => {
                    panel.spawn(text("Choose a block to put in your hand. Every block is yours to build with.", &style, 9.0, QUIET));
                    let ids: Vec<u16> = w.block_ids().filter(|id| !w.is_liquid(*id)).collect();
                    let per_row = 9;
                    panel
                        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() })
                        .with_children(|grid| {
                            for row in ids.chunks(per_row) {
                                grid.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                                    for id in row {
                                        let c = Choice::Block(*id);
                                        let icon = icons.get(*id, &w, &blocks, &assets);
                                        let e = r
                                            .spawn(choice(c, Node { width: Val::Px(52.0), height: Val::Px(52.0), padding: UiRect::all(Val::Px(5.0)), ..button_node() }))
                                            .with_children(|b| {
                                                if let Some(h) = icon {
                                                    b.spawn((ImageNode::new(h), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }));
                                                }
                                            })
                                            .id();
                                        note(e, c);
                                    }
                                });
                            }
                        });
                    panel.spawn(text("Quick blocks (the top of the block wheel; the number keys)", &style, 10.0, QUIET));
                    panel.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                        for i in 0..9 {
                            let c = Choice::Slot(i);
                            let icon = w.hotbar().get(i).and_then(|id| icons.get(*id, &w, &blocks, &assets));
                            let picked = i == w.slot;
                            let e = r
                                .spawn((
                                    choice(c, Node { width: Val::Px(52.0), height: Val::Px(52.0), padding: UiRect::all(Val::Px(5.0)), ..button_node() }),
                                    Outline::new(Val::Px(2.0), Val::Px(2.0), if picked { Color::WHITE } else { Color::NONE }),
                                ))
                                .with_children(|b| {
                                    if let Some(h) = icon {
                                        b.spawn((ImageNode::new(h), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }));
                                    }
                                })
                                .id();
                            note(e, c);
                        }
                    });
                    let name = w.hotbar().get(w.slot).map(|b| pretty(w.block_name(*b))).unwrap_or_default();
                    panel.spawn(text(format!("In your hand: {name}"), &style, 10.0, INK));
                }
            }
            // what the buttons do, for the device in use
            panel
                .spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(18.0), margin: UiRect::top(Val::Px(6.0)), ..default() })
                .with_children(|bar| {
                    let confirm = match screen {
                        Screen::Controls => "Change",
                        Screen::Inventory => "Put in hand",
                        _ => "Select",
                    };
                    bar.spawn((hint_node(), Hint::with_label(Action::Confirm, confirm)));
                    if screen == Screen::Controls {
                        bar.spawn((hint_node(), Hint::with_label(Action::Alternate, "Reset")));
                    }
                    if matches!(screen, Screen::Controls | Screen::Settings) {
                        bar.spawn((hint_node(), Hint::with_label(Action::TabNext, "Next tab")));
                    }
                    bar.spawn((hint_node(), Hint::with_label(Action::Back, "Back")));
                });
        });
    });
    if let Some(e) = wanted.or(first) {
        focus.set(e, FocusCause::Navigated);
    }
}

// ---- voice

type Row = (String, String, Vec<(Choice, String)>);

/// The Emotes screen's rows: each slot of the wheel, its emote, and buttons to step it.
fn emote_rows(e: &EmoteSettings) -> Vec<Row> {
    e.slots
        .iter()
        .enumerate()
        .map(|(i, name)| (format!("Emote {}", i + 1), name.clone(), vec![(Choice::Emote(i, -1), "<".into()), (Choice::Emote(i, 1), ">".into())]))
        .collect()
}

/// The emote `step` places along `clips` from `current` (round the end); with no clips known
/// (the character isn't loaded), the defaults.
pub fn step_emote(current: &str, step: i8, clips: &[String]) -> String {
    let fallback: Vec<String> = crate::emote::DEFAULT_EMOTES.iter().map(|s| s.to_string()).collect();
    let list = if clips.is_empty() { &fallback } else { clips };
    let n = list.len() as i32;
    let at = list.iter().position(|c| c == current).map(|i| i as i32).unwrap_or(if step > 0 { -1 } else { 0 });
    list[(at + i32::from(step)).rem_euclid(n) as usize].clone()
}

/// What the Voice screen says in place of its rows on the browser, which has no voice yet.
fn voice_browser_line(browser: bool) -> Option<&'static str> {
    browser.then_some("Voice is not available in the browser yet.")
}

/// The Voice screen's rows: label, value, buttons (the same shape as the settings rows).
fn voice_rows(v: &VoiceSettings) -> Vec<Row> {
    let pair = |item: VoiceItem| -> Vec<(Choice, String)> {
        vec![(Choice::VoiceStep(item, -1), "-".into()), (Choice::VoiceStep(item, 1), "+".into())]
    };
    vec![
        ("Enable voice".into(), String::new(), vec![(Choice::VoiceToggle(VoiceItem::Enable), if v.enabled { "On" } else { "Off" }.into())]),
        ("Input device".into(), device_label(v.device_in.as_deref()), pair(VoiceItem::DeviceIn)),
        ("Output device".into(), device_label(v.device_out.as_deref()), pair(VoiceItem::DeviceOut)),
        ("Input volume".into(), percent(v.in_gain), pair(VoiceItem::InGain)),
        ("Output volume".into(), percent(v.out_gain), pair(VoiceItem::OutGain)),
        ("Talk mode".into(), String::new(), vec![(Choice::VoiceToggle(VoiceItem::Ptt), if v.ptt { "Push-to-talk" } else { "Open mic" }.into())]),
        ("Channel".into(), String::new(), vec![(Choice::VoiceToggle(VoiceItem::Channel), if v.channel == VOICE_PARTY { "Party" } else { "Nearby" }.into())]),
    ]
}

/// Most players the Voice screen lists a mute row for; a crowd past this is not listed.
const MUTE_ROWS_MAX: usize = 12;

/// One "Mute <name>" row per other player (the roster is by name). The button says whether that
/// player is muted or heard, as the Channel row does.
fn mute_rows(roster: &[(u32, String)], muted: &HashSet<u32>) -> Vec<Row> {
    roster
        .iter()
        .take(MUTE_ROWS_MAX)
        .map(|(id, name)| {
            let state = if muted.contains(id) { "Muted" } else { "Heard" };
            (format!("Mute {name}"), String::new(), vec![(Choice::Mute(*id), state.into())])
        })
        .collect()
}

fn percent(gain: f32) -> String {
    format!("{:.0}%", gain * 100.0)
}

fn device_label(name: Option<&str>) -> String {
    match name {
        None => "Default".into(),
        Some(n) if n.chars().count() > 22 => format!("{}...", n.chars().take(22).collect::<String>()),
        Some(n) => n.to_string(),
    }
}

/// The microphones and speakers on this machine, "Default" not included. This only lists them;
/// it opens nothing. The browser has none.
fn device_names() -> (Vec<String>, Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        (crate::voice::capture::input_devices(), crate::voice::playback::output_devices())
    }
    #[cfg(target_arch = "wasm32")]
    {
        (Vec::new(), Vec::new())
    }
}

/// The next device in the list, cycling through "Default" (`None`) first. A device that is no
/// longer listed starts the cycle from the default.
fn cycle_device(current: Option<&str>, names: &[String], d: i8) -> Option<String> {
    let n = names.len() as i64 + 1;
    let at = current.and_then(|c| names.iter().position(|x| x == c)).map_or(0, |i| i as i64 + 1);
    let next = (at + d as i64).rem_euclid(n);
    if next == 0 { None } else { Some(names[next as usize - 1].clone()) }
}

/// A volume moved by 10 percent, kept in 0..=`max` (a tenth at a time, so no drift).
fn volume_step(v: f32, d: i8, max: f32) -> f32 {
    (((v + d as f32 * 0.1) * 10.0).round() / 10.0).clamp(0.0, max)
}

/// Applies one voice choice to the settings. Input volume goes up to 2; output volume up to 1,
/// the range the plugin keeps when it loads the file.
fn apply_voice(c: Choice, v: &mut VoiceSettings, inputs: &[String], outputs: &[String]) {
    match c {
        Choice::VoiceToggle(item) => match item {
            VoiceItem::Enable => v.enabled = !v.enabled,
            VoiceItem::Ptt => v.ptt = !v.ptt,
            VoiceItem::Channel => v.channel = next_channel(v.channel),
            _ => {}
        },
        Choice::VoiceStep(item, d) => match item {
            VoiceItem::DeviceIn => v.device_in = cycle_device(v.device_in.as_deref(), inputs, d),
            VoiceItem::DeviceOut => v.device_out = cycle_device(v.device_out.as_deref(), outputs, d),
            VoiceItem::InGain => v.in_gain = volume_step(v.in_gain, d, 2.0),
            VoiceItem::OutGain => v.out_gain = volume_step(v.out_gain, d, 1.0),
            _ => {}
        },
        Choice::Mute(id) => {
            // insert says whether it was new: an old one is heard again
            if !v.muted.insert(id) {
                v.muted.remove(&id);
            }
        }
        _ => {}
    }
}

// ---- moving focus, choosing

fn navigate(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    menus: Res<Menus>,
    mut nav: AutoDirectionalNavigator,
    mut visible: ResMut<InputFocusVisible>,
    hovered: Query<(Entity, &Hovered), (Changed<Hovered>, With<Choice>)>,
) {
    if *mode != UiMode::Menu || menus.capture.is_some() {
        return;
    }
    if let Some(dir) = actions.nav {
        let _ = nav.navigate(dir);
        visible.0 = true;
    }
    for (e, h) in &hovered {
        if h.get() {
            // (the navigator holds the focus)
            nav.manual_directional_navigation.focus.set(e, FocusCause::Pressed);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn choose(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    mut menus: ResMut<Menus>,
    focus: Res<InputFocus>,
    choices: Query<&Choice>,
    mut clicked: ResMut<Clicked>,
    mut bindings: ResMut<Bindings>,
    mut settings: ResMut<InputSettings>,
    mut voice: Option<ResMut<VoiceSettings>>,
    emotes: (Option<ResMut<EmoteSettings>>, Option<Res<crate::LocalAvatar>>),
    mut w: ResMut<WorldClient>,
    active: Res<ActiveDevice>,
    time: Res<Time<Real>>,
    mut exit: MessageWriter<AppExit>,
) {
    if *mode != UiMode::Menu || menus.capture.is_some() {
        return;
    }
    let mut chosen: Option<Choice> = None;
    if actions.just_pressed(Action::Confirm) {
        chosen = focus.get().and_then(|e| choices.get(e).ok()).copied();
    }
    if let Some(c) = clicked.0.take() {
        chosen = Some(c);
    }
    if actions.just_pressed(Action::Alternate) {
        if let Some(Choice::Bind(a)) = focus.get().and_then(|e| choices.get(e).ok()).copied() {
            bindings.reset(a);
            bindings::save(&bindings, &settings);
        }
    }
    let Some(c) = chosen else { return };
    match c {
        Choice::Resume => menus.close(),
        Choice::Open(s) => menus.open(s),
        Choice::Leave => leave(&mut exit),
        Choice::Bind(a) => {
            menus.capture = Some(Capture { action: a, gamepad: active.is_gamepad(), armed: false, started: time.elapsed_secs() });
        }
        Choice::ResetAll => {
            *bindings = Bindings::default();
            bindings::save(&bindings, &settings);
        }
        Choice::InvertY => {
            settings.invert_y = !settings.invert_y;
            bindings::save(&bindings, &settings);
        }
        Choice::Nintendo => {
            settings.nintendo_confirm = !settings.nintendo_confirm;
            bindings::save(&bindings, &settings);
        }
        Choice::Adjust(s, d) => {
            let d = d as f32;
            match s {
                Setting::MouseLook => settings.mouse_look = (settings.mouse_look + d * 0.0005).clamp(0.0005, 0.02),
                Setting::StickLook => settings.stick_look = (settings.stick_look + d * 0.2).clamp(0.4, 8.0),
                Setting::DeadZone => settings.dead_zone = (settings.dead_zone + d * 0.02).clamp(0.02, 0.5),
            }
            bindings::save(&bindings, &settings);
        }
        Choice::VoiceToggle(_) | Choice::VoiceStep(..) | Choice::Mute(_) => {
            if let Some(v) = voice.as_deref_mut() {
                // (the plugin saves the change and opens or closes the devices)
                let (inputs, outputs) = device_names();
                apply_voice(c, v, &inputs, &outputs);
                menus.dirty = true;
            }
        }
        Choice::Block(id) => {
            let slot = w.slot;
            w.set_hotbar(slot, id);
            w.hand_prop = None;
            menus.dirty = true;
        }
        Choice::Slot(i) => {
            if i < w.hotbar().len() {
                w.slot = i;
                w.hand_prop = None;
            }
            menus.dirty = true;
        }
        Choice::Emote(i, step) => {
            // (EmotePlugin saves the change)
            let (mut e, avatar) = emotes;
            if let Some(e) = e.as_deref_mut()
                && let Some(current) = e.slots.get(i).cloned()
            {
                let clips = avatar.as_ref().map(|a| a.clips.as_slice()).unwrap_or_default();
                e.slots[i] = step_emote(&current, step, clips);
            }
        }
    }
}

/// Out of the world: the app closes on the desktop; the browser goes back to the start page.
fn leave(exit: &mut MessageWriter<AppExit>) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = exit;
        if let Some(win) = web_sys::window() {
            let _ = win.location().set_href("/");
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    exit.write(AppExit::Success);
}

/// Waiting for a new binding: the next key or mouse button (keyboard), or gamepad button.
/// Escape, or six seconds, cancels.
#[allow(clippy::too_many_arguments)]
fn capture(
    mut menus: ResMut<Menus>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    mut bindings: ResMut<Bindings>,
    settings: Res<InputSettings>,
    time: Res<Time<Real>>,
    mut notes: Query<&mut Text, With<CaptureNote>>,
) {
    let Some(mut c) = menus.capture else {
        for mut t in &mut notes {
            if !t.0.is_empty() {
                t.0.clear();
            }
        }
        return;
    };
    let say = format!("Press the new {} for {} (Esc to cancel)", if c.gamepad { "button" } else { "key" }, c.action.label());
    for mut t in &mut notes {
        if t.0 != say {
            t.0 = say.clone();
        }
    }
    let anything_held = keys.get_pressed().next().is_some()
        || mouse.get_pressed().next().is_some()
        || pads.iter().any(|p| p.get_pressed().next().is_some());
    if !c.armed {
        if !anything_held {
            c.armed = true;
        }
        menus.capture = Some(c);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || time.elapsed_secs() - c.started > 6.0 {
        menus.capture = None;
        menus.dirty = true;
        return;
    }
    let got = if c.gamepad {
        pads.iter().find_map(|p| p.get_just_pressed().next().copied()).map(Input::Pad)
    } else {
        keys.get_just_pressed()
            .find(|k| bindings::bindable(**k))
            .map(|k| Input::Key(*k))
            .or_else(|| mouse.get_just_pressed().find(|m| bindings::mouse_name(**m).is_some()).map(|m| Input::Mouse(*m)))
    };
    if let Some(i) = got {
        let i = match i {
            Input::Pad(b) if settings.nintendo_confirm => Input::Pad(bindings::nintendo_swap(b)),
            other => other,
        };
        bindings.set(c.action, i);
        bindings::save(&bindings, &settings);
        menus.capture = None;
        menus.dirty = true;
    }
}

/// Focused choices stand out: a bright frame (always with a controller, and with the mouse
/// over them).
fn style_focus(focus: Res<InputFocus>, mut q: Query<(Entity, &mut BackgroundColor, &mut BorderColor), With<Choice>>) {
    let f = focus.get();
    for (e, mut bg, mut border) in &mut q {
        let on = Some(e) == f;
        let (want_bg, want_edge) = if on { (BUTTON_FOCUS, EDGE_FOCUS) } else { (BUTTON, EDGE) };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = BorderColor::all(want_edge);
        if *border != want {
            *border = want;
        }
    }
}

/// The choice the mouse last clicked, until `choose` acts on it.
#[derive(Resource, Default)]
pub struct Clicked(Option<Choice>);

/// A click lands on the deepest thing under the pointer (often a choice's text) and climbs up
/// through its parents: the first one that is a choice is what was chosen.
fn clicked(ev: On<PointerClick>, choices: Query<&Choice>, mut clicked: ResMut<Clicked>) {
    if let Ok(c) = choices.get(ev.entity) {
        clicked.0 = Some(*c);
    }
}

/// The menus. Needs [`crate::input::InputPlugin`] and [`crate::hud::HudPlugin`].
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<DirectionalNavigationPlugin>() {
            app.add_plugins(DirectionalNavigationPlugin);
        }
        app.init_resource::<Menus>().init_resource::<Clicked>().add_observer(clicked).add_systems(Update, (open_close, capture, build, navigate, choose, style_focus).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        let mut w = WorldClient::new(Some("ws://127.0.0.1:7780/ws".into()), "Tester".into());
        w.ready = true;
        app.insert_resource(w)
            .init_resource::<Actions>()
            .init_resource::<Menus>()
            .init_resource::<UiMode>()
            .add_systems(Update, open_close);
        app
    }

    fn press(app: &mut App, a: Action) {
        let mut acts = Actions::default();
        acts.press(a);
        app.insert_resource(acts);
        app.update();
        app.insert_resource(Actions::default());
        app.update();
    }

    fn top(app: &App) -> Option<Screen> {
        app.world().resource::<Menus>().top()
    }

    #[test]
    fn menu_opens_backs_out_and_closes() {
        let mut app = app();
        press(&mut app, Action::Menu);
        assert_eq!(top(&app), Some(Screen::Pause));
        assert_eq!(*app.world().resource::<UiMode>(), UiMode::Menu);
        app.world_mut().resource_mut::<Menus>().open(Screen::Controls);
        press(&mut app, Action::Back);
        assert_eq!(top(&app), Some(Screen::Pause), "back one screen");
        press(&mut app, Action::Back);
        assert_eq!(top(&app), None);
        assert_eq!(*app.world().resource::<UiMode>(), UiMode::Game);
    }

    #[test]
    fn inventory_toggles_and_start_closes_everything() {
        let mut app = app();
        press(&mut app, Action::Inventory);
        assert_eq!(top(&app), Some(Screen::Inventory));
        press(&mut app, Action::Inventory);
        assert_eq!(top(&app), None);
        press(&mut app, Action::Menu);
        app.world_mut().resource_mut::<Menus>().open(Screen::Settings);
        press(&mut app, Action::Menu);
        assert_eq!(top(&app), None, "Start closes every screen");
    }

    #[test]
    fn tabs_switch_between_controls_and_settings() {
        let mut app = app();
        press(&mut app, Action::Menu);
        app.world_mut().resource_mut::<Menus>().open(Screen::Controls);
        press(&mut app, Action::TabNext);
        assert_eq!(top(&app), Some(Screen::Settings));
        press(&mut app, Action::TabPrev);
        assert_eq!(top(&app), Some(Screen::Controls));
    }

    #[test]
    fn voice_enable_toggle_changes_the_setting() {
        let mut v = VoiceSettings::default();
        assert!(!v.enabled, "voice is off by default");
        apply_voice(Choice::VoiceToggle(VoiceItem::Enable), &mut v, &[], &[]);
        assert!(v.enabled);
        apply_voice(Choice::VoiceToggle(VoiceItem::Enable), &mut v, &[], &[]);
        assert!(!v.enabled);
    }

    #[test]
    fn voice_device_lists_cycle_from_default() {
        let names = vec!["A mic".to_string(), "B mic".to_string()];
        assert_eq!(cycle_device(None, &names, 1), Some("A mic".into()));
        assert_eq!(cycle_device(Some("A mic"), &names, 1), Some("B mic".into()));
        assert_eq!(cycle_device(Some("B mic"), &names, 1), None, "wraps to Default");
        assert_eq!(cycle_device(None, &names, -1), Some("B mic".into()));
        assert_eq!(cycle_device(Some("gone"), &names, 1), Some("A mic".into()), "unplugged starts over");
        assert_eq!(cycle_device(None, &[], 1), None, "no devices: stays on Default");
    }

    #[test]
    fn voice_volumes_step_by_ten_percent_and_clamp() {
        assert!((volume_step(0.5, 1, 2.0) - 0.6).abs() < 1e-6);
        assert!((volume_step(0.0, -1, 2.0)).abs() < 1e-6, "not below zero");
        assert!((volume_step(2.0, 1, 2.0) - 2.0).abs() < 1e-6, "input stops at 2");
        assert!((volume_step(1.0, 1, 1.0) - 1.0).abs() < 1e-6, "output stops at 1");
    }

    #[test]
    fn mute_choice_toggles_one_player_and_leaves_the_rest() {
        let mut v = VoiceSettings::default();
        apply_voice(Choice::Mute(7), &mut v, &[], &[]);
        assert!(v.muted.contains(&7));
        apply_voice(Choice::Mute(9), &mut v, &[], &[]);
        apply_voice(Choice::Mute(7), &mut v, &[], &[]);
        assert!(!v.muted.contains(&7), "a second choice hears them again");
        assert!(v.muted.contains(&9), "other players keep their state");
    }

    #[test]
    fn mute_rows_follow_the_roster_with_state_and_a_cap() {
        let roster: Vec<(u32, String)> = (1..=13).map(|i| (i, format!("P{i}"))).collect();
        let muted: HashSet<u32> = [2].into_iter().collect();
        let rows = mute_rows(&roster, &muted);
        assert_eq!(rows.len(), MUTE_ROWS_MAX, "capped at twelve");
        assert_eq!(rows[0].0, "Mute P1");
        assert_eq!(rows[0].2, vec![(Choice::Mute(1), "Heard".to_string())]);
        assert_eq!(rows[1].2, vec![(Choice::Mute(2), "Muted".to_string())]);
        assert!(mute_rows(&[], &muted).is_empty(), "nobody here: no rows");
    }

    #[test]
    fn voice_browser_line_only_in_the_browser() {
        assert_eq!(voice_browser_line(true), Some("Voice is not available in the browser yet."));
        assert_eq!(voice_browser_line(false), None);
    }

    #[test]
    fn voice_rows_reach_every_setting() {
        let rows = voice_rows(&VoiceSettings::default());
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0].0, "Enable voice");
        let choices: Vec<Choice> = rows.iter().flat_map(|r| r.2.iter().map(|b| b.0)).collect();
        assert!(choices.contains(&Choice::VoiceToggle(VoiceItem::Enable)));
        assert!(choices.contains(&Choice::VoiceStep(VoiceItem::DeviceIn, 1)));
        assert!(choices.contains(&Choice::VoiceStep(VoiceItem::DeviceOut, -1)));
        assert!(choices.contains(&Choice::VoiceStep(VoiceItem::InGain, 1)));
        assert!(choices.contains(&Choice::VoiceStep(VoiceItem::OutGain, -1)));
        assert!(choices.contains(&Choice::VoiceToggle(VoiceItem::Ptt)));
        assert!(choices.contains(&Choice::VoiceToggle(VoiceItem::Channel)));
    }

    #[test]
    fn no_menus_while_watching_or_loading() {
        let mut app = app();
        app.world_mut().resource_mut::<WorldClient>().ready = false;
        press(&mut app, Action::Menu);
        assert_eq!(top(&app), None);
    }

    #[test]
    fn emotes_step_round_the_clips() {
        let clips: Vec<String> = ["Sleep", "Talk", "Victory"].iter().map(|s| s.to_string()).collect();
        assert_eq!(step_emote("Talk", 1, &clips), "Victory");
        assert_eq!(step_emote("Victory", 1, &clips), "Sleep", "round the end");
        assert_eq!(step_emote("Sleep", -1, &clips), "Victory");
        assert_eq!(step_emote("Gone", 1, &clips), "Sleep", "one it doesn't have starts at the beginning");
        assert_eq!(step_emote("Gone", -1, &clips), "Victory");
        assert_eq!(step_emote("Victory", 1, &[]), crate::emote::DEFAULT_EMOTES[1], "no clips known: the defaults");
    }
}