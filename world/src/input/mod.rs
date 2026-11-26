//! Input as actions: the game asks "is Jump pressed?", never "is Space pressed?".
//!
//! Every action works from the keyboard and mouse and from a gamepad, through [`Bindings`]
//! (rebindable, saved with the player's settings). [`read_actions`] turns this frame's input
//! into [`Actions`] and keeps track of the [`ActiveDevice`]: whichever was touched last, so
//! button hints ([`hints`]) show the right buttons and menus know whether to show focus.
//!
//! - [`bindings`]: the default tables, naming inputs, loading and saving.
//! - [`devices`]: keyboard and mouse or which gamepad, and the controller's brand.
//! - [`hints`]: a UI widget that shows the button for an action on the active device.

pub mod bindings;
pub mod devices;
pub mod hints;

use std::collections::HashSet;

use bevy::{
    input::{
        InputSystems,
        gamepad::{Gamepad, GamepadButton},
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    },
    math::CompassOctant,
    prelude::*,
};
use serde::{Deserialize, Serialize};

pub use bindings::{Bindings, Input};
pub use devices::{ActiveDevice, Brand, Device};

/// Something the player can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Action {
    Forward,
    Backward,
    Left,
    Right,
    /// jump; fly up while flying
    Jump,
    /// sneak; fly down while flying
    Down,
    Sprint,
    /// break the block you aim at (hold)
    Break,
    /// place the block in your hand, or use what you aim at
    Place,
    /// put the block you aim at in your hand
    Pick,
    HotbarNext,
    HotbarPrev,
    Inventory,
    /// the pause menu; also back out of a menu
    Menu,
    Chat,
    /// change the view: first person, behind the shoulder, from above
    View,
    Fly,
    /// menus: choose what has focus
    Confirm,
    /// menus: go back
    Back,
    /// menus: next and previous tab
    TabNext,
    TabPrev,
    /// menus: a second action on what has focus (split a stack, reset a binding)
    Alternate,
}

impl Action {
    pub const ALL: [Action; 22] = [
        Action::Forward,
        Action::Backward,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Down,
        Action::Sprint,
        Action::Break,
        Action::Place,
        Action::Pick,
        Action::HotbarNext,
        Action::HotbarPrev,
        Action::Inventory,
        Action::Menu,
        Action::Chat,
        Action::View,
        Action::Fly,
        Action::Confirm,
        Action::Back,
        Action::TabNext,
        Action::TabPrev,
        Action::Alternate,
    ];

    /// What it's called on the controls screen and in hints.
    pub fn label(self) -> &'static str {
        match self {
            Action::Forward => "Forward",
            Action::Backward => "Back",
            Action::Left => "Left",
            Action::Right => "Right",
            Action::Jump => "Jump",
            Action::Down => "Sneak",
            Action::Sprint => "Sprint",
            Action::Break => "Break",
            Action::Place => "Place",
            Action::Pick => "Pick block",
            Action::HotbarNext => "Next block",
            Action::HotbarPrev => "Previous block",
            Action::Inventory => "Inventory",
            Action::Menu => "Menu",
            Action::Chat => "Chat",
            Action::View => "Change view",
            Action::Fly => "Fly",
            Action::Confirm => "Select",
            Action::Back => "Back",
            Action::TabNext => "Next tab",
            Action::TabPrev => "Previous tab",
            Action::Alternate => "More",
        }
    }

    /// Played in the world (shown on the controls screen as such), rather than in menus.
    pub fn in_game(self) -> bool {
        !matches!(self, Action::Confirm | Action::Back | Action::TabNext | Action::TabPrev | Action::Alternate)
    }
}

/// Where the player's attention is: in the world, or in a menu (which then gets the input).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UiMode {
    #[default]
    Game,
    Menu,
}

/// How the player likes the controls to feel.
#[derive(Resource, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InputSettings {
    /// radians per pixel of mouse movement
    pub mouse_look: f32,
    /// radians per second at full stick
    pub stick_look: f32,
    pub invert_y: bool,
    /// sticks: below this, nothing
    pub dead_zone: f32,
    /// Nintendo's way: confirm on the right button (East), back on the bottom (South)
    pub nintendo_confirm: bool,
}

impl Default for InputSettings {
    fn default() -> Self {
        InputSettings { mouse_look: 0.0035, stick_look: 2.6, invert_y: false, dead_zone: 0.15, nintendo_confirm: false }
    }
}

/// This frame's input, as actions.
#[derive(Resource, Default, Debug)]
pub struct Actions {
    pressed: HashSet<Action>,
    just: HashSet<Action>,
    released: HashSet<Action>,
    /// walking: x right, y forward, at most length 1 (keys and the left stick)
    pub movement: Vec2,
    /// the right stick, past the dead zone, with a response curve (x right, y up)
    pub look_stick: Vec2,
    /// the mouse's movement this frame, in pixels
    pub mouse_delta: Vec2,
    /// the wheel this frame (lines; up is positive)
    pub scroll: f32,
    /// 1 to 9 on the keyboard: a hotbar slot (0-based)
    pub digit: Option<usize>,
    /// menus: one step in a direction (the d-pad, the left stick with repeat, arrows)
    pub nav: Option<CompassOctant>,
}

impl Actions {
    pub fn pressed(&self, a: Action) -> bool {
        self.pressed.contains(&a)
    }
    pub fn just_pressed(&self, a: Action) -> bool {
        self.just.contains(&a)
    }
    pub fn just_released(&self, a: Action) -> bool {
        self.released.contains(&a)
    }
    /// For tests and scripted input: press an action this frame.
    pub fn press(&mut self, a: Action) {
        if self.pressed.insert(a) {
            self.just.insert(a);
        }
    }
}

/// Holding a direction in a menu repeats it, after a pause (the state [`read_actions`] keeps).
#[derive(Default)]
pub struct NavRepeat {
    held: Option<CompassOctant>,
    next: f32,
}

/// The stick's value past a radial dead zone, rescaled so it starts from zero at the edge.
pub fn dead_zone(v: Vec2, zone: f32) -> Vec2 {
    let len = v.length();
    if len <= zone || zone >= 1.0 {
        return Vec2::ZERO;
    }
    let scaled = ((len - zone) / (1.0 - zone)).min(1.0);
    v / len * scaled
}

/// Look on a stick: gentle near the middle for aiming, fast at the edge for turning.
pub fn look_curve(v: Vec2) -> Vec2 {
    let len = v.length();
    if len == 0.0 {
        return v;
    }
    v / len * len.powf(1.8)
}

fn octant(v: Vec2) -> Option<CompassOctant> {
    if v.length() < 0.5 {
        return None;
    }
    // four ways are what menus need; diagonals are too easy to hit by accident
    Some(if v.x.abs() > v.y.abs() {
        if v.x > 0.0 { CompassOctant::East } else { CompassOctant::West }
    } else if v.y > 0.0 {
        CompassOctant::North
    } else {
        CompassOctant::South
    })
}

/// Turn this frame's keys, mouse and gamepads into [`Actions`], and note the device in use.
#[allow(clippy::too_many_arguments)]
pub fn read_actions(
    mut actions: ResMut<Actions>,
    mut active: ResMut<ActiveDevice>,
    bindings: Res<Bindings>,
    settings: Res<InputSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    pads: Query<(Entity, &Gamepad, Option<&Name>)>,
    time: Res<Time<Real>>,
    mut repeat: Local<NavRepeat>,
) {
    let was = std::mem::take(&mut actions.pressed);
    actions.just.clear();
    actions.released.clear();
    // ---- which device: the one touched last
    let kbm_touched = keys.get_just_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some()
        || motion.delta.length() > 4.0
        || scroll.delta.y != 0.0;
    let mut pad_touched = None;
    for (e, p, name) in &pads {
        let stick = dead_zone(p.left_stick(), settings.dead_zone).length() + dead_zone(p.right_stick(), settings.dead_zone).length();
        if p.get_just_pressed().next().is_some() || stick > 0.0 {
            pad_touched = Some((e, devices::brand(p.vendor_id(), name.map(|n| n.as_str()).unwrap_or(""))));
        }
    }
    if let Some((e, b)) = pad_touched {
        active.set(Device::Gamepad(b), Some(e));
    } else if kbm_touched {
        active.set(Device::KeyboardMouse, None);
    }
    // the gamepad that counts: the one in use, else the first
    let pad = active.gamepad.and_then(|e| pads.get(e).ok()).or_else(|| pads.iter().next()).map(|(_, p, _)| p);
    // ---- buttons
    let swap = settings.nintendo_confirm;
    for a in Action::ALL {
        let mut on = false;
        for i in bindings.kbm(a) {
            on |= match i {
                Input::Key(k) => keys.pressed(*k),
                Input::Mouse(m) => mouse.pressed(*m),
                Input::Pad(_) => false,
            };
        }
        if let Some(p) = pad {
            for b in bindings.pad(a) {
                let b = if swap { bindings::nintendo_swap(*b) } else { *b };
                on |= p.pressed(b);
            }
        }
        if on {
            actions.pressed.insert(a);
            if !was.contains(&a) {
                actions.just.insert(a);
            }
        } else if was.contains(&a) {
            actions.released.insert(a);
        }
    }
    // ---- axes
    let key = |a: Action| actions.pressed.contains(&a) as i32 as f32;
    let mut mv = Vec2::new(key(Action::Right) - key(Action::Left), key(Action::Forward) - key(Action::Backward));
    let mut look = Vec2::ZERO;
    if let Some(p) = pad {
        mv += dead_zone(p.left_stick(), settings.dead_zone);
        look = look_curve(dead_zone(p.right_stick(), settings.dead_zone));
    }
    actions.movement = mv.clamp_length_max(1.0);
    actions.look_stick = if settings.invert_y { Vec2::new(look.x, -look.y) } else { look };
    actions.mouse_delta = if settings.invert_y { motion.delta * Vec2::new(1.0, -1.0) } else { motion.delta };
    actions.scroll = scroll.delta.y;
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    actions.digit = DIGITS.iter().position(|d| keys.just_pressed(*d));
    // ---- menu navigation: arrows and the d-pad step once; holding (or the stick) repeats
    let mut dir = Vec2::ZERO;
    let k = |c: KeyCode| keys.pressed(c) as i32 as f32;
    dir += Vec2::new(k(KeyCode::ArrowRight) - k(KeyCode::ArrowLeft), k(KeyCode::ArrowUp) - k(KeyCode::ArrowDown));
    if let Some(p) = pad {
        let b = |x: GamepadButton| p.pressed(x) as i32 as f32;
        dir += Vec2::new(b(GamepadButton::DPadRight) - b(GamepadButton::DPadLeft), b(GamepadButton::DPadUp) - b(GamepadButton::DPadDown));
        dir += dead_zone(p.left_stick(), 0.3);
    }
    let now = time.elapsed_secs();
    let held = octant(dir);
    actions.nav = None;
    if held != repeat.held {
        repeat.held = held;
        repeat.next = now + 0.38;
        actions.nav = held;
    } else if held.is_some() && now >= repeat.next {
        repeat.next = now + 0.11;
        actions.nav = held;
    }
}

/// Actions, devices, bindings and settings, read early in every frame.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        let (b, s) = bindings::load();
        app.insert_resource(b)
            .insert_resource(s)
            .init_resource::<Actions>()
            .init_resource::<ActiveDevice>()
            .init_resource::<UiMode>()
            .add_systems(PreUpdate, read_actions.after(InputSystems))
            .add_systems(Update, hints::update_hints);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_zone_starts_from_zero_at_its_edge() {
        assert_eq!(dead_zone(Vec2::new(0.1, 0.0), 0.15), Vec2::ZERO);
        let v = dead_zone(Vec2::new(0.575, 0.0), 0.15);
        assert!((v.x - 0.5).abs() < 1e-4, "{v}");
        assert!((dead_zone(Vec2::new(1.0, 0.0), 0.15).x - 1.0).abs() < 1e-6);
        // radial: a diagonal push keeps its direction
        let d = dead_zone(Vec2::new(0.6, 0.6), 0.15);
        assert!((d.x - d.y).abs() < 1e-6);
    }

    #[test]
    fn look_curve_is_gentle_in_the_middle_and_full_at_the_edge() {
        assert!(look_curve(Vec2::new(0.3, 0.0)).x < 0.15);
        assert!((look_curve(Vec2::new(1.0, 0.0)).x - 1.0).abs() < 1e-6);
        assert_eq!(look_curve(Vec2::ZERO), Vec2::ZERO);
    }

    #[test]
    fn menus_move_in_four_directions() {
        assert_eq!(octant(Vec2::new(0.9, 0.2)), Some(CompassOctant::East));
        assert_eq!(octant(Vec2::new(-0.1, -0.8)), Some(CompassOctant::South));
        assert_eq!(octant(Vec2::new(0.2, 0.2)), None);
    }
}
