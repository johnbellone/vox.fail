//! Button hints: "[A] Jump", with the button drawn as on the controller in your hands (or the
//! key on your keyboard), redrawn the moment you switch device or rebind.

use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    input::gamepad::GamepadButton,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{
    Action, InputSettings,
    bindings::{self, Bindings, Input},
    devices::{self, ActiveDevice, Cap, Device, Shape},
};

/// Show the button for `action` (and, with `label`, the action's name after it).
#[derive(Component, Clone, Debug)]
pub struct Hint {
    pub action: Action,
    /// text after the button; `None` for the button alone
    pub label: Option<String>,
}

impl Hint {
    pub fn new(action: Action) -> Hint {
        Hint { action, label: Some(action.label().to_string()) }
    }
    pub fn button(action: Action) -> Hint {
        Hint { action, label: None }
    }
    pub fn with_label(action: Action, label: impl Into<String>) -> Hint {
        Hint { action, label: Some(label.into()) }
    }
}

/// What hints draw with: the pixel font and the PlayStation shapes.
#[derive(Resource, Clone)]
pub struct HintStyle {
    pub font: Handle<Font>,
    shapes: [Handle<Image>; 4],
}

impl FromWorld for HintStyle {
    fn from_world(world: &mut World) -> Self {
        let font = world.resource::<AssetServer>().load("fonts/PressStart2P-Regular.ttf");
        let mut images = world.resource_mut::<Assets<Image>>();
        let shapes = [CROSS, CIRCLE, SQUARE, TRIANGLE].map(|art| images.add(pixel_image(art)));
        HintStyle { font, shapes }
    }
}

const CROSS: [&str; 9] = ["#.......#", ".#.....#.", "..#...#..", "...#.#...", "....#....", "...#.#...", "..#...#..", ".#.....#.", "#.......#"];
const CIRCLE: [&str; 9] = ["..#####..", ".#.....#.", "#.......#", "#.......#", "#.......#", "#.......#", "#.......#", ".#.....#.", "..#####.."];
const SQUARE: [&str; 9] = ["#########", "#.......#", "#.......#", "#.......#", "#.......#", "#.......#", "#.......#", "#.......#", "#########"];
const TRIANGLE: [&str; 9] = ["....#....", "...#.#...", "...#.#...", "..#...#..", "..#...#..", ".#.....#.", ".#.....#.", "#.......#", "#########"];

/// A small white-on-clear picture from rows of `#` and `.`, kept crisp when scaled.
pub fn pixel_image(rows: [&str; 9]) -> Image {
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for row in rows {
        for ch in row.bytes() {
            data.extend_from_slice(if ch == b'#' { &[255, 255, 255, 255] } else { &[0, 0, 0, 0] });
        }
    }
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::nearest();
    img
}

/// The cap to show for `action` on `device`.
pub fn cap_for(action: Action, device: Device, b: &Bindings, s: &InputSettings) -> Option<Cap> {
    match device {
        Device::KeyboardMouse => {
            if matches!(action, Action::HotbarNext | Action::HotbarPrev) && b.kbm(action).is_empty() {
                return Some(devices::key_cap("Wheel"));
            }
            b.kbm(action).iter().find_map(|i| match i {
                Input::Key(k) => bindings::key_name(*k).map(devices::key_cap),
                Input::Mouse(m) => bindings::mouse_name(*m).map(|n| {
                    devices::key_cap(match n {
                        "Left click" => "LMB",
                        "Right click" => "RMB",
                        "Middle click" => "MMB",
                        other => other,
                    })
                }),
                Input::Pad(_) => None,
            })
        }
        Device::Gamepad(brand) => {
            let first: Option<GamepadButton> = b.pad(action).first().copied();
            let first = if s.nintendo_confirm { first.map(bindings::nintendo_swap) } else { first };
            first.map(|btn| devices::pad_cap(brand, btn)).or_else(|| match action {
                Action::Forward | Action::Backward | Action::Left | Action::Right => Some(devices::pad_cap(brand, GamepadButton::LeftThumb)),
                _ => None,
            })
        }
    }
}

/// Draw a cap as UI: a coloured round button or pill with its letters or shape.
pub fn spawn_cap(parent: &mut ChildSpawnerCommands, cap: Cap, style: &HintStyle, size: f32) {
    let light = cap.colour.to_srgba().red > 0.8;
    let ink = if light { Color::srgb(0.15, 0.17, 0.23) } else { Color::WHITE };
    let mut node = parent.spawn((
        Node {
            min_width: Val::Px(size),
            height: Val::Px(size),
            padding: UiRect::axes(Val::Px(if cap.round { 0.0 } else { size * 0.3 }), Val::Px(0.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(Val::Px(if cap.round { size / 2.0 } else { size * 0.25 })),
            border: UiRect::bottom(Val::Px((size * 0.1).max(1.0))),
            ..default()
        },
        BackgroundColor(cap.colour),
        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.35)),
    ));
    node.with_children(|p| {
        if let Some(shape) = cap.shape {
            let (i, tint) = match shape {
                Shape::Cross => (0, Color::srgb(0.55, 0.70, 0.95)),
                Shape::Circle => (1, Color::srgb(0.95, 0.45, 0.45)),
                Shape::Square => (2, Color::srgb(0.90, 0.55, 0.85)),
                Shape::Triangle => (3, Color::srgb(0.40, 0.85, 0.70)),
            };
            p.spawn((
                ImageNode { image: style.shapes[i].clone(), color: tint, ..default() },
                Node { width: Val::Px(size * 0.56), height: Val::Px(size * 0.56), ..default() },
            ));
        } else {
            p.spawn((
                Text::new(cap.text),
                TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px((size * 0.42).round()), ..default() },
                TextColor(ink),
            ));
        }
    });
}

/// Redraw hints when they appear, when the device changes and when bindings change.
pub fn update_hints(
    mut commands: Commands,
    hints: Query<(Entity, &Hint, Ref<Hint>)>,
    active: Res<ActiveDevice>,
    bindings: Res<Bindings>,
    settings: Res<InputSettings>,
    style: Option<Res<HintStyle>>,
    mut seen: Local<Option<u32>>,
) {
    let Some(style) = style else { return };
    let all = *seen != Some(active.generation) || bindings.is_changed() || settings.is_changed();
    *seen = Some(active.generation);
    for (e, hint, r) in &hints {
        if !all && !r.is_added() && !r.is_changed() {
            continue;
        }
        let cap = cap_for(hint.action, active.device, &bindings, &settings);
        commands.entity(e).despawn_related::<Children>();
        commands.entity(e).with_children(|p| {
            if let Some(cap) = cap {
                spawn_cap(p, cap, &style, 22.0);
            }
            if let Some(label) = &hint.label {
                p.spawn((
                    Text::new(label.clone()),
                    TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(10.0), ..default() },
                    TextColor(Color::srgb(0.96, 0.96, 0.98)),
                    crate::input::hints::pixel_shadow(),
                ));
            }
        });
    }
}

/// A crisp one-pixel shadow for pixel text (the default is too soft and too far).
pub fn pixel_shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.7) }
}

/// A hint's own node: a row, centred, with a little gap.
pub fn hint_node() -> Node {
    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }
}
