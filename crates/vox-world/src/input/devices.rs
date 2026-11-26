//! Which device the player is using, and for a gamepad, whose: hints show its buttons.

use bevy::{input::gamepad::GamepadButton, prelude::*};

/// Who made the controller: decides how its buttons are labelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Brand {
    /// also the default for controllers we don't recognise
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
    Steam,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad(Brand),
}

/// The device touched last.
#[derive(Resource, Debug, Default)]
pub struct ActiveDevice {
    pub device: Device,
    /// the gamepad in use, when it's a gamepad
    pub gamepad: Option<Entity>,
    /// bumped on every change, so hints know to redraw
    pub generation: u32,
}

impl ActiveDevice {
    pub fn set(&mut self, d: Device, pad: Option<Entity>) {
        if self.device != d || (pad.is_some() && self.gamepad != pad) {
            self.device = d;
            if pad.is_some() {
                self.gamepad = pad;
            }
            self.generation = self.generation.wrapping_add(1);
        }
    }
    pub fn is_gamepad(&self) -> bool {
        matches!(self.device, Device::Gamepad(_))
    }
}

/// A controller's brand from its USB vendor id, or failing that its name. Native builds get
/// the vendor id; browsers put it in the name instead ("... (Vendor: 054c Product: 0ce6)" in
/// Chrome, "054c-0ce6-..." in Firefox).
pub fn brand(vendor: Option<u16>, name: &str) -> Brand {
    let from_vendor = |v: u16| match v {
        0x045e => Some(Brand::Xbox),
        0x054c => Some(Brand::PlayStation),
        0x057e => Some(Brand::Nintendo),
        0x28de => Some(Brand::Steam),
        _ => None,
    };
    if let Some(b) = vendor.and_then(from_vendor) {
        return b;
    }
    let lower = name.to_ascii_lowercase();
    let hex = |s: &str| u16::from_str_radix(s.get(..4)?, 16).ok();
    let in_name = lower
        .split("vendor:")
        .nth(1)
        .and_then(|r| hex(r.trim_start()))
        .or_else(|| (lower.len() > 9 && lower.as_bytes()[4] == b'-').then(|| hex(&lower)).flatten());
    if let Some(b) = in_name.and_then(from_vendor) {
        return b;
    }
    if ["playstation", "dualshock", "dualsense", "sony", "wireless controller"].iter().any(|w| lower.contains(w)) {
        Brand::PlayStation
    } else if ["nintendo", "switch", "pro controller", "joy-con"].iter().any(|w| lower.contains(w)) {
        Brand::Nintendo
    } else if ["steam", "valve"].iter().any(|w| lower.contains(w)) {
        Brand::Steam
    } else {
        Brand::Xbox
    }
}

/// How a button looks in a hint: a label (or a shape) on a coloured cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cap {
    pub text: &'static str,
    pub shape: Option<Shape>,
    pub colour: Color,
    pub round: bool,
}

/// The PlayStation face buttons are shapes, drawn as small pixel pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Cross,
    Circle,
    Square,
    Triangle,
}

const GREY: Color = Color::srgb(0.27, 0.30, 0.38);

/// The cap for a gamepad button (by position) on a controller of this brand.
pub fn pad_cap(brand: Brand, b: GamepadButton) -> Cap {
    use GamepadButton as G;
    let face = |text: &'static str, colour: Color| Cap { text, shape: None, colour, round: true };
    let pill = |text: &'static str| Cap { text, shape: None, colour: GREY, round: false };
    let (green, red, blue, yellow) =
        (Color::srgb(0.24, 0.62, 0.29), Color::srgb(0.78, 0.25, 0.22), Color::srgb(0.22, 0.42, 0.80), Color::srgb(0.85, 0.67, 0.13));
    match (brand, b) {
        (Brand::PlayStation, G::South) => Cap { text: "", shape: Some(Shape::Cross), colour: GREY, round: true },
        (Brand::PlayStation, G::East) => Cap { text: "", shape: Some(Shape::Circle), colour: GREY, round: true },
        (Brand::PlayStation, G::West) => Cap { text: "", shape: Some(Shape::Square), colour: GREY, round: true },
        (Brand::PlayStation, G::North) => Cap { text: "", shape: Some(Shape::Triangle), colour: GREY, round: true },
        (Brand::PlayStation, G::LeftTrigger) => pill("L1"),
        (Brand::PlayStation, G::RightTrigger) => pill("R1"),
        (Brand::PlayStation, G::LeftTrigger2) => pill("L2"),
        (Brand::PlayStation, G::RightTrigger2) => pill("R2"),
        (Brand::PlayStation, G::LeftThumb) => face("L3", GREY),
        (Brand::PlayStation, G::RightThumb) => face("R3", GREY),
        (Brand::PlayStation, G::Start) => pill("Options"),
        (Brand::PlayStation, G::Select) => pill("Create"),
        (Brand::Nintendo, G::South) => face("B", GREY),
        (Brand::Nintendo, G::East) => face("A", GREY),
        (Brand::Nintendo, G::West) => face("Y", GREY),
        (Brand::Nintendo, G::North) => face("X", GREY),
        (Brand::Nintendo, G::LeftTrigger) => pill("L"),
        (Brand::Nintendo, G::RightTrigger) => pill("R"),
        (Brand::Nintendo, G::LeftTrigger2) => pill("ZL"),
        (Brand::Nintendo, G::RightTrigger2) => pill("ZR"),
        (Brand::Nintendo, G::Start) => face("+", GREY),
        (Brand::Nintendo, G::Select) => face("-", GREY),
        (Brand::Steam, G::South) => face("A", GREY),
        (Brand::Steam, G::East) => face("B", GREY),
        (Brand::Steam, G::West) => face("X", GREY),
        (Brand::Steam, G::North) => face("Y", GREY),
        (Brand::Steam, G::LeftTrigger) => pill("L1"),
        (Brand::Steam, G::RightTrigger) => pill("R1"),
        (Brand::Steam, G::LeftTrigger2) => pill("L2"),
        (Brand::Steam, G::RightTrigger2) => pill("R2"),
        (_, G::South) => face("A", green),
        (_, G::East) => face("B", red),
        (_, G::West) => face("X", blue),
        (_, G::North) => face("Y", yellow),
        (_, G::LeftTrigger) => pill("LB"),
        (_, G::RightTrigger) => pill("RB"),
        (_, G::LeftTrigger2) => pill("LT"),
        (_, G::RightTrigger2) => pill("RT"),
        (_, G::LeftThumb) => face("LS", GREY),
        (_, G::RightThumb) => face("RS", GREY),
        (_, G::Start) => pill("Menu"),
        (_, G::Select) => pill("View"),
        (_, G::DPadUp) => pill("D-pad up"),
        (_, G::DPadDown) => pill("D-pad down"),
        (_, G::DPadLeft) => pill("D-pad left"),
        (_, G::DPadRight) => pill("D-pad right"),
        _ => pill("?"),
    }
}

/// A key or mouse button as a light keycap.
pub fn key_cap(text: &'static str) -> Cap {
    Cap { text, shape: None, colour: Color::srgb(0.93, 0.94, 0.97), round: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brands_from_vendor_ids_and_browser_names() {
        assert_eq!(brand(Some(0x054c), ""), Brand::PlayStation);
        assert_eq!(brand(Some(0x057e), "anything"), Brand::Nintendo);
        assert_eq!(brand(None, "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)"), Brand::PlayStation);
        assert_eq!(brand(None, "057e-2009-Pro Controller"), Brand::Nintendo);
        assert_eq!(brand(None, "28de-1205-Steam Deck"), Brand::Steam);
        assert_eq!(brand(None, "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)"), Brand::Xbox);
        assert_eq!(brand(None, "DualSense Wireless Controller"), Brand::PlayStation);
        assert_eq!(brand(None, "Some Generic Pad"), Brand::Xbox);
    }

    #[test]
    fn caps_follow_the_brand() {
        assert_eq!(pad_cap(Brand::Xbox, GamepadButton::South).text, "A");
        assert_eq!(pad_cap(Brand::Nintendo, GamepadButton::South).text, "B");
        assert_eq!(pad_cap(Brand::PlayStation, GamepadButton::South).shape, Some(Shape::Cross));
        assert_eq!(pad_cap(Brand::PlayStation, GamepadButton::RightTrigger2).text, "R2");
    }
}
