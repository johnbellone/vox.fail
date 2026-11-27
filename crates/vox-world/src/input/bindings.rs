//! Which keys, mouse buttons and gamepad buttons do what. Defaults below; the controls menu
//! changes them, and they're saved with the input settings (a file on the desktop, the
//! browser's local storage on the web).

use std::collections::BTreeMap;

use bevy::{input::gamepad::GamepadButton, prelude::*};
use serde::{Deserialize, Serialize};

use super::{Action, InputSettings};

/// One thing that can be pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
    Pad(GamepadButton),
}

/// Keyboard and mouse bindings, and gamepad bindings (by button position), per action.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Bindings {
    kbm: BTreeMap<Action, Vec<Input>>,
    pad: BTreeMap<Action, Vec<GamepadButton>>,
}

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        use GamepadButton as G;
        use Input::{Key as K, Mouse as M};
        let kbm: &[(Action, &[Input])] = &[
            (Forward, &[K(KeyCode::KeyW)]),
            (Backward, &[K(KeyCode::KeyS)]),
            (Left, &[K(KeyCode::KeyA)]),
            (Right, &[K(KeyCode::KeyD)]),
            (Jump, &[K(KeyCode::Space)]),
            (Down, &[K(KeyCode::ShiftLeft)]),
            (Sprint, &[K(KeyCode::ControlLeft)]),
            (Break, &[M(MouseButton::Left)]),
            (Place, &[M(MouseButton::Right)]),
            (Pick, &[M(MouseButton::Middle)]),
            (HotbarNext, &[]),
            (HotbarPrev, &[]),
            (Inventory, &[K(KeyCode::KeyE)]),
            (Menu, &[K(KeyCode::Escape)]),
            (Chat, &[K(KeyCode::KeyT)]),
            (View, &[K(KeyCode::F5), K(KeyCode::KeyV)]),
            (Fly, &[K(KeyCode::KeyF)]),
            (Confirm, &[K(KeyCode::Enter)]),
            (Back, &[K(KeyCode::Escape), K(KeyCode::Backspace)]),
            (TabNext, &[K(KeyCode::Tab), K(KeyCode::PageDown)]),
            (TabPrev, &[K(KeyCode::KeyQ), K(KeyCode::PageUp)]),
            (Alternate, &[K(KeyCode::KeyX)]),
            (QuickWheel, &[K(KeyCode::Tab)]),
            (PushToTalk, &[K(KeyCode::KeyB)]),
            (VoiceChannel, &[K(KeyCode::KeyN)]),
        ];
        let pad: &[(Action, &[GamepadButton])] = &[
            (Jump, &[G::South]),
            (Down, &[G::RightThumb]),
            (Sprint, &[G::LeftThumb]),
            (Break, &[G::RightTrigger2]),
            (Place, &[G::LeftTrigger2]),
            (Pick, &[G::West]),
            (HotbarNext, &[G::RightTrigger]),
            (HotbarPrev, &[G::LeftTrigger]),
            (Inventory, &[G::North]),
            (Menu, &[G::Start]),
            (Chat, &[G::Select]),
            (View, &[G::DPadUp]),
            (Fly, &[G::DPadDown]),
            (Confirm, &[G::South]),
            (Back, &[G::East]),
            (TabNext, &[G::RightTrigger]),
            (TabPrev, &[G::LeftTrigger]),
            (Alternate, &[G::West]),
            (QuickWheel, &[G::DPadLeft]),
            (PushToTalk, &[G::DPadRight]),
        ];
        Bindings {
            kbm: kbm.iter().map(|(a, i)| (*a, i.to_vec())).collect(),
            pad: pad.iter().map(|(a, b)| (*a, b.to_vec())).collect(),
        }
    }
}

impl Bindings {
    pub fn kbm(&self, a: Action) -> &[Input] {
        self.kbm.get(&a).map_or(&[], |v| v.as_slice())
    }
    pub fn pad(&self, a: Action) -> &[GamepadButton] {
        self.pad.get(&a).map_or(&[], |v| v.as_slice())
    }
    /// Bind `input` to `a`, replacing that device's bindings for it. Anything else that used
    /// the same input loses it (one input, one action), except menu actions, which share
    /// buttons with the game on purpose.
    pub fn set(&mut self, a: Action, input: Input) {
        let clash = |b: Action| b != a && b.in_game() == a.in_game();
        match input {
            Input::Pad(b) => {
                for (k, v) in self.pad.iter_mut() {
                    if clash(*k) {
                        v.retain(|x| *x != b);
                    }
                }
                self.pad.insert(a, vec![b]);
            }
            other => {
                for (k, v) in self.kbm.iter_mut() {
                    if clash(*k) {
                        v.retain(|x| *x != other);
                    }
                }
                self.kbm.insert(a, vec![other]);
            }
        }
    }
    /// Back to the defaults for one action.
    pub fn reset(&mut self, a: Action) {
        let d = Bindings::default();
        self.kbm.insert(a, d.kbm(a).to_vec());
        self.pad.insert(a, d.pad(a).to_vec());
    }
}

/// Nintendo's convention: confirm on the right, back on the bottom (and X and Y swap).
pub fn nintendo_swap(b: GamepadButton) -> GamepadButton {
    use GamepadButton as G;
    match b {
        G::South => G::East,
        G::East => G::South,
        G::West => G::North,
        G::North => G::West,
        other => other,
    }
}

// ---- names, for saving and for showing

const KEYS: &[(KeyCode, &str)] = &[
    (KeyCode::KeyA, "A"),
    (KeyCode::KeyB, "B"),
    (KeyCode::KeyC, "C"),
    (KeyCode::KeyD, "D"),
    (KeyCode::KeyE, "E"),
    (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"),
    (KeyCode::KeyH, "H"),
    (KeyCode::KeyI, "I"),
    (KeyCode::KeyJ, "J"),
    (KeyCode::KeyK, "K"),
    (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"),
    (KeyCode::KeyN, "N"),
    (KeyCode::KeyO, "O"),
    (KeyCode::KeyP, "P"),
    (KeyCode::KeyQ, "Q"),
    (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"),
    (KeyCode::KeyT, "T"),
    (KeyCode::KeyU, "U"),
    (KeyCode::KeyV, "V"),
    (KeyCode::KeyW, "W"),
    (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"),
    (KeyCode::KeyZ, "Z"),
    (KeyCode::Digit0, "0"),
    (KeyCode::Space, "Space"),
    (KeyCode::Enter, "Enter"),
    (KeyCode::Escape, "Esc"),
    (KeyCode::Backspace, "Backspace"),
    (KeyCode::Tab, "Tab"),
    (KeyCode::ShiftLeft, "Shift"),
    (KeyCode::ShiftRight, "Right Shift"),
    (KeyCode::ControlLeft, "Ctrl"),
    (KeyCode::ControlRight, "Right Ctrl"),
    (KeyCode::AltLeft, "Alt"),
    (KeyCode::CapsLock, "Caps Lock"),
    (KeyCode::PageUp, "Page Up"),
    (KeyCode::PageDown, "Page Down"),
    (KeyCode::Home, "Home"),
    (KeyCode::End, "End"),
    (KeyCode::Insert, "Insert"),
    (KeyCode::Delete, "Delete"),
    (KeyCode::ArrowUp, "Up"),
    (KeyCode::ArrowDown, "Down"),
    (KeyCode::ArrowLeft, "Left"),
    (KeyCode::ArrowRight, "Right"),
    (KeyCode::Backquote, "`"),
    (KeyCode::Minus, "-"),
    (KeyCode::Equal, "="),
    (KeyCode::BracketLeft, "["),
    (KeyCode::BracketRight, "]"),
    (KeyCode::Semicolon, ";"),
    (KeyCode::Quote, "'"),
    (KeyCode::Comma, ","),
    (KeyCode::Period, "."),
    (KeyCode::Slash, "/"),
    (KeyCode::Backslash, "\\"),
    (KeyCode::F1, "F1"),
    (KeyCode::F2, "F2"),
    (KeyCode::F3, "F3"),
    (KeyCode::F4, "F4"),
    (KeyCode::F5, "F5"),
    (KeyCode::F6, "F6"),
    (KeyCode::F7, "F7"),
    (KeyCode::F8, "F8"),
    (KeyCode::F9, "F9"),
    (KeyCode::F10, "F10"),
];

const MICE: &[(MouseButton, &str)] = &[
    (MouseButton::Left, "Left click"),
    (MouseButton::Right, "Right click"),
    (MouseButton::Middle, "Middle click"),
    (MouseButton::Back, "Mouse back"),
    (MouseButton::Forward, "Mouse forward"),
];

const PAD: &[(GamepadButton, &str)] = &[
    (GamepadButton::South, "South"),
    (GamepadButton::East, "East"),
    (GamepadButton::West, "West"),
    (GamepadButton::North, "North"),
    (GamepadButton::LeftTrigger, "LeftBumper"),
    (GamepadButton::RightTrigger, "RightBumper"),
    (GamepadButton::LeftTrigger2, "LeftTrigger"),
    (GamepadButton::RightTrigger2, "RightTrigger"),
    (GamepadButton::LeftThumb, "LeftStick"),
    (GamepadButton::RightThumb, "RightStick"),
    (GamepadButton::Select, "Select"),
    (GamepadButton::Start, "Start"),
    (GamepadButton::DPadUp, "DPadUp"),
    (GamepadButton::DPadDown, "DPadDown"),
    (GamepadButton::DPadLeft, "DPadLeft"),
    (GamepadButton::DPadRight, "DPadRight"),
];

/// A key's name on a keycap ("Space", "E"); `None` for keys that can't be bound.
pub fn key_name(k: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|(c, _)| *c == k).map(|(_, n)| *n)
}

pub fn mouse_name(m: MouseButton) -> Option<&'static str> {
    MICE.iter().find(|(c, _)| *c == m).map(|(_, n)| *n)
}

/// Can this key be bound (it has a name)?
pub fn bindable(k: KeyCode) -> bool {
    key_name(k).is_some()
}

fn input_to_str(i: &Input) -> Option<String> {
    match i {
        Input::Key(k) => key_name(*k).map(|n| format!("key:{n}")),
        Input::Mouse(m) => mouse_name(*m).map(|n| format!("mouse:{n}")),
        Input::Pad(b) => PAD.iter().find(|(c, _)| c == b).map(|(_, n)| format!("pad:{n}")),
    }
}

fn input_from_str(s: &str) -> Option<Input> {
    let (kind, name) = s.split_once(':')?;
    match kind {
        "key" => KEYS.iter().find(|(_, n)| *n == name).map(|(c, _)| Input::Key(*c)),
        "mouse" => MICE.iter().find(|(_, n)| *n == name).map(|(c, _)| Input::Mouse(*c)),
        "pad" => PAD.iter().find(|(_, n)| *n == name).map(|(c, _)| Input::Pad(*c)),
        _ => None,
    }
}

/// What's saved: bindings by name, and the settings.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    keyboard: BTreeMap<Action, Vec<String>>,
    gamepad: BTreeMap<Action, Vec<String>>,
    settings: InputSettings,
}

impl Bindings {
    fn to_saved(&self, settings: &InputSettings) -> Saved {
        let names = |v: Vec<Input>| v.iter().filter_map(input_to_str).collect::<Vec<_>>();
        Saved {
            keyboard: self.kbm.iter().map(|(a, v)| (*a, names(v.clone()))).collect(),
            gamepad: self.pad.iter().map(|(a, v)| (*a, names(v.iter().map(|b| Input::Pad(*b)).collect()))).collect(),
            settings: settings.clone(),
        }
    }

    fn from_saved(s: &Saved) -> Bindings {
        let mut b = Bindings::default();
        for (a, v) in &s.keyboard {
            b.kbm.insert(*a, v.iter().filter_map(|n| input_from_str(n)).filter(|i| !matches!(i, Input::Pad(_))).collect());
        }
        for (a, v) in &s.gamepad {
            let pads = v
                .iter()
                .filter_map(|n| match input_from_str(n) {
                    Some(Input::Pad(p)) => Some(p),
                    _ => None,
                })
                .collect();
            b.pad.insert(*a, pads);
        }
        b
    }
}

/// The saved bindings and settings as JSON.
pub fn to_json(b: &Bindings, s: &InputSettings) -> String {
    serde_json::to_string_pretty(&b.to_saved(s)).unwrap_or_default()
}

/// Bindings and settings from JSON; anything missing or unreadable keeps its default.
pub fn from_json(text: &str) -> (Bindings, InputSettings) {
    let saved: Saved = serde_json::from_str(text).unwrap_or_default();
    let b = if saved.keyboard.is_empty() && saved.gamepad.is_empty() { Bindings::default() } else { Bindings::from_saved(&saved) };
    (b, saved.settings)
}

#[cfg(not(target_arch = "wasm32"))]
const FILE: &str = "controls.json";

/// The player's saved controls, or the defaults.
pub fn load() -> (Bindings, InputSettings) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::fs::read_to_string(crate::files::user_dir().join(FILE)) {
            Ok(t) => from_json(&t),
            Err(_) => (Bindings::default(), InputSettings::default()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let stored = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("vox.controls").ok().flatten());
        match stored {
            Some(t) => from_json(&t),
            None => (Bindings::default(), InputSettings::default()),
        }
    }
}

/// Keep the player's controls for next time.
pub fn save(b: &Bindings, s: &InputSettings) {
    let text = to_json(b, s);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = crate::files::user_dir().join(FILE);
        if let Err(e) = std::fs::write(&path, text) {
            crate::note!("[input] can't save {}: {e}", path.display());
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item("vox.controls", &text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_action_works_on_both_devices() {
        let b = Bindings::default();
        for a in Action::ALL {
            let moving = matches!(a, Action::Forward | Action::Backward | Action::Left | Action::Right);
            let wheel = matches!(a, Action::HotbarNext | Action::HotbarPrev);
            // every nameable pad button already has a game action, so VoiceChannel is
            // keyboard-only by default; rebind it from the controls screen if you want a pad key
            let keyboard_only = matches!(a, Action::VoiceChannel);
            assert!(moving || keyboard_only || !b.pad(a).is_empty(), "{a:?} has no gamepad button");
            assert!(wheel || !b.kbm(a).is_empty(), "{a:?} has no key");
        }
    }

    #[test]
    fn saving_and_loading_keeps_bindings_and_settings() {
        let mut b = Bindings::default();
        b.set(Action::Jump, Input::Key(KeyCode::KeyJ));
        b.set(Action::Break, Input::Pad(GamepadButton::RightTrigger));
        let s = InputSettings { invert_y: true, ..default() };
        let (b2, s2) = from_json(&to_json(&b, &s));
        assert_eq!(b2, b);
        assert_eq!(s2, s);
        assert_eq!(from_json("not json").0, Bindings::default());
    }

    #[test]
    fn binding_takes_the_input_from_another_game_action_but_not_from_menus() {
        let mut b = Bindings::default();
        b.set(Action::Fly, Input::Key(KeyCode::Space));
        assert_eq!(b.kbm(Action::Fly), &[Input::Key(KeyCode::Space)]);
        assert!(b.kbm(Action::Jump).is_empty(), "Jump lost Space");
        // South stays Confirm in menus while it becomes Pick in the game
        b.set(Action::Pick, Input::Pad(GamepadButton::South));
        assert_eq!(b.pad(Action::Confirm), &[GamepadButton::South]);
        b.reset(Action::Jump);
        assert_eq!(b.kbm(Action::Jump), &[Input::Key(KeyCode::Space)]);
    }

    #[test]
    fn nintendo_swaps_the_face_buttons_only() {
        assert_eq!(nintendo_swap(GamepadButton::South), GamepadButton::East);
        assert_eq!(nintendo_swap(GamepadButton::North), GamepadButton::West);
        assert_eq!(nintendo_swap(GamepadButton::Start), GamepadButton::Start);
    }
}
