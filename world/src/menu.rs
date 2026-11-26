//! Menus you can drive with a controller alone: the pause menu, controls (rebinding),
//! settings, and the inventory (in a creative world, every block to put in your hotbar).
//!
//! Each screen is a set of focusable choices laid out in rows and columns; the d-pad, the left
//! stick or the arrow keys move focus to the nearest choice that way (Bevy's directional
//! navigation), Confirm chooses, Back goes back a screen and Menu (Start) closes them all. The
//! mouse works on the same screens: hovering focuses, clicking chooses. While a menu is open
//! [`UiMode::Menu`] tells the game to leave the input alone.

use bevy::{
    input::gamepad::Gamepad,
    input_focus::{FocusCause, InputFocus, InputFocusVisible, directional_navigation::DirectionalNavigationPlugin},
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
};

/// A screen of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Pause,
    Inventory,
    Controls,
    Settings,
}

impl Screen {
    fn title(self) -> &'static str {
        match self {
            Screen::Pause => "Paused",
            Screen::Inventory => "Blocks",
            Screen::Controls => "Controls",
            Screen::Settings => "Settings",
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
    /// put this block in your hand (the current hotbar slot)
    Block(u16),
    /// make this hotbar slot the current one
    Slot(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    MouseLook,
    StickLook,
    DeadZone,
}

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct CaptureNote;

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
    (node, Button, c, AutoDirectionalNavigation::default(), BackgroundColor(BUTTON), BorderColor::all(EDGE))
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
    w: Res<WorldClient>,
    blocks: Res<Blocks>,
    assets: Res<AssetServer>,
    mut icons: ResMut<BlockIcons>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    choices: Query<(Entity, &Choice)>,
    mut seen_device: Local<Option<u32>>,
) {
    // the controls screen shows the device in use: redraw when it changes
    let device_changed = *seen_device != Some(active.generation);
    *seen_device = Some(active.generation);
    let redraw = menus.dirty
        || (device_changed && menus.top() == Some(Screen::Controls) && menus.capture.is_none())
        || (bindings.is_changed() && menus.top() == Some(Screen::Controls))
        || (settings.is_changed() && menus.top() == Some(Screen::Settings));
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
                Screen::Settings => {
                    let rows: [(&str, String, Vec<(Choice, String)>); 5] = [
                        ("Invert look", String::new(), vec![(Choice::InvertY, if settings.invert_y { "On" } else { "Off" }.into())]),
                        (
                            "Mouse look",
                            format!("{:.1}", settings.mouse_look * 1000.0),
                            vec![(Choice::Adjust(Setting::MouseLook, -1), "-".into()), (Choice::Adjust(Setting::MouseLook, 1), "+".into())],
                        ),
                        (
                            "Stick look",
                            format!("{:.1}", settings.stick_look),
                            vec![(Choice::Adjust(Setting::StickLook, -1), "-".into()), (Choice::Adjust(Setting::StickLook, 1), "+".into())],
                        ),
                        (
                            "Stick dead zone",
                            format!("{:.2}", settings.dead_zone),
                            vec![(Choice::Adjust(Setting::DeadZone, -1), "-".into()), (Choice::Adjust(Setting::DeadZone, 1), "+".into())],
                        ),
                        (
                            "Confirm on the right button (Nintendo)",
                            String::new(),
                            vec![(Choice::Nintendo, if settings.nintendo_confirm { "On" } else { "Off" }.into())],
                        ),
                    ];
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
                    panel.spawn(text("Your hotbar", &style, 10.0, QUIET));
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

// ---- moving focus, choosing

fn navigate(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    menus: Res<Menus>,
    mut nav: AutoDirectionalNavigator,
    mut visible: ResMut<InputFocusVisible>,
    hovered: Query<(Entity, &Interaction), (Changed<Interaction>, With<Choice>)>,
) {
    if *mode != UiMode::Menu || menus.capture.is_some() {
        return;
    }
    if let Some(dir) = actions.nav {
        let _ = nav.navigate(dir);
        visible.0 = true;
    }
    for (e, i) in &hovered {
        if *i == Interaction::Hovered {
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
    pressed: Query<(&Interaction, &Choice), Changed<Interaction>>,
    mut bindings: ResMut<Bindings>,
    mut settings: ResMut<InputSettings>,
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
    for (i, c) in &pressed {
        if *i == Interaction::Pressed {
            chosen = Some(*c);
        }
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
        Choice::Block(id) => {
            let slot = w.slot;
            w.set_hotbar(slot, id);
            menus.dirty = true;
        }
        Choice::Slot(i) => {
            if i < w.hotbar().len() {
                w.slot = i;
            }
            menus.dirty = true;
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

/// The menus. Needs [`crate::input::InputPlugin`] and [`crate::hud::HudPlugin`].
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<DirectionalNavigationPlugin>() {
            app.add_plugins(DirectionalNavigationPlugin);
        }
        app.init_resource::<Menus>().add_systems(Update, (open_close, capture, build, navigate, choose, style_focus).chain());
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
    fn no_menus_while_watching_or_loading() {
        let mut app = app();
        app.world_mut().resource_mut::<WorldClient>().ready = false;
        press(&mut app, Action::Menu);
        assert_eq!(top(&app), None);
    }
}
