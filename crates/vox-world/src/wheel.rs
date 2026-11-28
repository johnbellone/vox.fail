//! The wheels that replace the hotbar (docs/design/controls-and-hud.md, "Wheels instead of a
//! hotbar"), held open by a bumper (or a key) and worked with the right stick or the mouse:
//!
//! - the **block wheel** (left bumper, Tab): the world's quick blocks round the top, and two
//!   sectors at the bottom, *All blocks* and *Props*, each opening a second wheel of every block
//!   in the world or every prop it accepts, sixteen to a page;
//! - the **emote wheel** (right bumper, G): see [`crate::emote`].
//!
//! One wheel is open at a time ([`Wheels`]); while it is, the player doesn't look round, walk or
//! build (`client::controls` reads [`Wheels`]). Each wheel's controller describes what to show
//! in a [`WheelView`], and one drawing ([`draw_wheel`]) serves both.

use bevy::prelude::*;

use crate::{
    blocks::Blocks,
    client::{HandProp, WorldClient},
    hud::{BlockIcons, EDGE, PICKED, SLOT, pretty},
    input::{Action, Actions, UiMode, hints::HintStyle},
};

/// Which wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelKind {
    Blocks,
    Emotes,
}

/// Which wheel is open, if any.
#[derive(Resource, Default, Debug)]
pub struct Wheels {
    pub open: Option<WheelKind>,
    /// held open without its button (scripts, screenshots), pointing this way (x right, y up)
    pub scripted: Option<(WheelKind, Vec2)>,
}

impl Wheels {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// What the open wheel shows this frame. Written by the open wheel's controller, drawn by
/// [`draw_wheel`].
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct WheelView {
    pub slots: Vec<SlotView>,
    /// how far from the middle the slots sit, pixels
    pub radius: f32,
    /// the name of what is pointed at
    pub centre: String,
    /// a line under the wheel (pages, how to go back)
    pub hint: String,
}

/// One slot on a wheel.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotView {
    /// from the wheel's middle, pixels, x right and y down
    pub at: Vec2,
    pub icon: Option<Handle<Image>>,
    /// shown when there's no picture (props, the two sectors, emotes)
    pub label: String,
    pub picked: bool,
}

/// The most slots a wheel shows at once (the block wheel's second wheel: one page).
pub const PER_PAGE: usize = 16;
/// The main wheel's radius and the second wheel's, pixels.
pub const MAIN_RADIUS: f32 = 118.0;
pub const MORE_RADIUS: f32 = 150.0;
/// A pointer nearer the middle than this points at nothing.
const DEAD: f32 = 28.0;
/// The quick blocks sit on the upper arc, from this far left of the top to this far right.
const ARC_DEG: f32 = 110.0;
/// Past this far from the top (either way) is the bottom half, with its two sectors.
const BOTTOM_DEG: f32 = 120.0;
/// Where the two sectors' slots sit: lower left and lower right.
const SECTOR_DEG: f32 = 155.0;
/// Pointing at a sector this long opens its second wheel.
const DWELL_SECS: f32 = 0.35;

/// Which of `n` slots evenly round a whole circle a pointer (x right, y up) points at: slot 0
/// at the top, then clockwise. Nothing until it's pushed a little way out.
pub fn wheel_choice(pointer: Vec2, n: usize) -> Option<usize> {
    if n == 0 || pointer.length() < DEAD {
        return None;
    }
    let tau = std::f32::consts::TAU;
    let a = pointer.x.atan2(pointer.y).rem_euclid(tau);
    Some(((a / (tau / n as f32)).round() as usize) % n)
}

/// Where slot `i` of `n` evenly round a circle of `radius` sits (x right, y down).
pub fn wheel_spot(i: usize, n: usize, radius: f32) -> Vec2 {
    let a = std::f32::consts::TAU * i as f32 / n.max(1) as f32;
    Vec2::new(a.sin(), -a.cos()) * radius
}

/// What the block wheel's pointer is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// a quick block, by slot
    Quick(usize),
    /// the sector that opens every block
    All,
    /// the sector that opens every prop
    Props,
}

/// The block wheel's two second wheels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum More {
    All,
    Props,
}

/// The angle of a pointer (x right, y up) from the top, clockwise positive, degrees in
/// -180..=180.
fn angle_deg(pointer: Vec2) -> f32 {
    pointer.x.atan2(pointer.y).to_degrees()
}

/// What a pointer points at on the block wheel with `n` quick blocks: the bottom half is the two
/// sectors (All blocks lower left, Props lower right), the rest the quick blocks spread over the
/// upper arc.
pub fn block_pick(pointer: Vec2, n: usize) -> Option<Pick> {
    if pointer.length() < DEAD {
        return None;
    }
    let a = angle_deg(pointer);
    if a.abs() > BOTTOM_DEG {
        return Some(if a > 0.0 { Pick::Props } else { Pick::All });
    }
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(Pick::Quick(0));
    }
    let t = ((a + ARC_DEG) / (2.0 * ARC_DEG)).clamp(0.0, 1.0);
    Some(Pick::Quick((t * (n - 1) as f32).round() as usize))
}

/// Where quick block `i` of `n` sits on the upper arc (x right, y down).
pub fn quick_spot(i: usize, n: usize) -> Vec2 {
    let a = if n <= 1 { 0.0 } else { -ARC_DEG + 2.0 * ARC_DEG * i as f32 / (n - 1) as f32 };
    let a = a.to_radians();
    Vec2::new(a.sin(), -a.cos()) * MAIN_RADIUS
}

/// Where a sector's slot sits (x right, y down).
pub fn sector_spot(m: More) -> Vec2 {
    let a = match m {
        More::All => -SECTOR_DEG,
        More::Props => SECTOR_DEG,
    }
    .to_radians();
    Vec2::new(a.sin(), -a.cos()) * MAIN_RADIUS
}

/// Pages of [`PER_PAGE`] for `len` things (at least one).
pub fn pages(len: usize) -> usize {
    len.div_ceil(PER_PAGE).max(1)
}

/// How many things page `page` of `len` holds.
pub fn page_len(len: usize, page: usize) -> usize {
    len.saturating_sub(page * PER_PAGE).min(PER_PAGE)
}

/// The block wheel's state while it is open.
#[derive(Resource, Default, Debug)]
pub struct BlockWheel {
    pointer: Vec2,
    pick: Option<Pick>,
    /// how long the pointer has rested on a sector
    dwell: f32,
    /// the second wheel, if open: which, which page, and what is pointed at on it
    more: Option<(More, usize, Option<usize>)>,
}

/// The props this world accepts, by name, for the Props wheel: read from the catalogue (fetched
/// first in the browser).
#[derive(Resource, Default, Debug)]
pub struct PropMenu {
    pub props: Vec<HandProp>,
    asked: bool,
    read: bool,
}

impl PropMenu {
    /// Read the catalogue if it is here; in the browser, ask for it the first time.
    fn refresh(&mut self) {
        if self.read {
            return;
        }
        let path = crate::files::base().join("data/props/catalogue.json");
        match crate::files::read(&path) {
            Some(bytes) => {
                self.read = true;
                self.props = match vox_format::prop::Catalogue::decode(&bytes) {
                    Ok(cat) => {
                        let mut v: Vec<HandProp> = cat.props.values().map(|e| HandProp { hash: e.hash.0, name: crate::hud::pretty(&e.name) }).collect();
                        v.sort_by(|a, b| a.name.cmp(&b.name));
                        v
                    }
                    Err(why) => {
                        crate::note!("[wheel] the props catalogue: {why}");
                        Vec::new()
                    }
                };
            }
            None if !self.asked => {
                self.asked = true;
                crate::files::want(&path);
            }
            None => {}
        }
    }
}

/// The pointer, from the mouse (moved from the middle; screen y is down) and the right stick
/// (which points), at most 100 from the middle.
fn steer(pointer: Vec2, actions: &Actions) -> Vec2 {
    let d = actions.mouse_delta;
    let mut p = (pointer + Vec2::new(d.x, -d.y)).clamp_length_max(100.0);
    if actions.look_stick != Vec2::ZERO {
        p = actions.look_stick.normalize_or_zero() * 100.0;
    }
    p
}

/// Every block of this world you can hold (not water and the like), for the All blocks wheel.
fn all_blocks(w: &WorldClient) -> Vec<u16> {
    w.block_ids().filter(|id| !w.is_liquid(*id)).collect()
}

/// Pages to turn this frame: the left stick (or the d-pad, or the arrows) right or left, and the
/// mouse wheel (down is on). Nothing when neither moves.
pub fn page_turn(nav: Option<bevy::math::CompassOctant>, scroll: f32) -> i32 {
    let by_nav = match nav {
        Some(bevy::math::CompassOctant::East) => 1,
        Some(bevy::math::CompassOctant::West) => -1,
        _ => 0,
    };
    // (f32::signum of 0 is 1, so a still wheel must be told apart first)
    let by_wheel = if scroll.is_finite() && scroll != 0.0 { -(scroll.signum() as i32) } else { 0 };
    by_nav + by_wheel
}

/// The block wheel: open while its button is held; letting go takes what is pointed at.
#[allow(clippy::too_many_arguments)]
pub fn block_wheel(
    actions: Res<Actions>,
    mode: Res<UiMode>,
    time: Res<Time<Real>>,
    mut wheels: ResMut<Wheels>,
    mut st: ResMut<BlockWheel>,
    mut w: ResMut<WorldClient>,
    mut props: ResMut<PropMenu>,
    edit: Option<Res<crate::editor::camera::EditFly>>,
) {
    // in edit mode the wheels stay shut: the mouse and stick fly the edit camera
    let playing = w.active() && w.ready && !w.watch && *mode == UiMode::Game && edit.is_none();
    let scripted = wheels.scripted.filter(|(k, _)| *k == WheelKind::Blocks).map(|(_, p)| p);
    let held = playing && (actions.pressed(Action::QuickWheel) || scripted.is_some());
    if held && wheels.open.is_none() {
        wheels.open = Some(WheelKind::Blocks);
        *st = BlockWheel::default();
    }
    if wheels.open != Some(WheelKind::Blocks) {
        return;
    }
    if !playing {
        wheels.open = None;
        return;
    }
    let n = w.hotbar().len().min(9);
    if held {
        st.pointer = scripted.unwrap_or_else(|| steer(st.pointer, &actions));
        match st.more {
            None => {
                st.pick = block_pick(st.pointer, n);
                let sector = match st.pick {
                    Some(Pick::All) => Some(More::All),
                    Some(Pick::Props) => Some(More::Props),
                    _ => None,
                };
                match sector {
                    Some(m) => {
                        st.dwell += time.delta_secs();
                        let pressed = actions.just_pressed(Action::Confirm) || actions.just_pressed(Action::Break);
                        if st.dwell >= DWELL_SECS || pressed {
                            if m == More::Props {
                                props.refresh();
                            }
                            st.more = Some((m, 0, None));
                            st.pointer = Vec2::ZERO;
                            st.dwell = 0.0;
                        }
                    }
                    None => st.dwell = 0.0,
                }
            }
            Some((m, mut page, _)) => {
                if m == More::Props {
                    props.refresh();
                }
                let len = match m {
                    More::All => all_blocks(&w).len(),
                    More::Props => props.props.len(),
                };
                let turn = page_turn(actions.nav, actions.scroll);
                if turn != 0 {
                    page = (page as i32 + turn).rem_euclid(pages(len) as i32) as usize;
                }
                if actions.just_pressed(Action::Back) {
                    st.more = None;
                    st.pointer = Vec2::ZERO;
                } else {
                    let choice = wheel_choice(st.pointer, page_len(len, page));
                    st.more = Some((m, page, choice));
                }
            }
        }
        return;
    }
    // let go: take what is pointed at
    wheels.open = None;
    match st.more {
        Some((More::All, page, Some(c))) => {
            if let Some(id) = all_blocks(&w).get(page * PER_PAGE + c).copied() {
                // as the inventory screen does: into the current quick slot
                let slot = w.slot;
                w.set_hotbar(slot, id);
                w.hand_prop = None;
            }
        }
        Some((More::Props, page, Some(c))) => {
            if let Some(p) = props.props.get(page * PER_PAGE + c) {
                w.hand_prop = Some(p.clone());
            }
        }
        Some(_) => {}
        None => {
            if let Some(Pick::Quick(i)) = st.pick.filter(|_| n > 0)
                && i < n
            {
                w.slot = i;
                w.hand_prop = None;
            }
        }
    }
}

/// What the block wheel shows, while it is open.
#[allow(clippy::too_many_arguments)]
pub fn block_wheel_view(
    wheels: Res<Wheels>,
    st: Res<BlockWheel>,
    w: Res<WorldClient>,
    props: Res<PropMenu>,
    blocks: Res<Blocks>,
    assets: Res<AssetServer>,
    mut icons: ResMut<BlockIcons>,
    mut view: ResMut<WheelView>,
) {
    if wheels.open != Some(WheelKind::Blocks) {
        return;
    }
    let mut v = WheelView::default();
    match st.more {
        None => {
            v.radius = MAIN_RADIUS;
            let bar = w.hotbar();
            let n = bar.len().min(9);
            let picked = st.pick.or(if w.hand_prop.is_none() { Some(Pick::Quick(w.slot)) } else { None });
            for (i, id) in bar.iter().take(n).enumerate() {
                v.slots.push(SlotView {
                    at: quick_spot(i, n),
                    icon: icons.get(*id, &w, &blocks, &assets),
                    label: String::new(),
                    picked: picked == Some(Pick::Quick(i)),
                });
            }
            for (m, label, p) in [(More::All, "All", Pick::All), (More::Props, "Props", Pick::Props)] {
                v.slots.push(SlotView { at: sector_spot(m), icon: None, label: label.into(), picked: picked == Some(p) });
            }
            v.centre = match picked {
                Some(Pick::Quick(i)) => bar.get(i).map(|b| pretty(w.block_name(*b))).unwrap_or_default(),
                Some(Pick::All) => "All blocks".into(),
                Some(Pick::Props) => "Props".into(),
                None => String::new(),
            };
        }
        Some((m, page, choice)) => {
            v.radius = MORE_RADIUS;
            let ids = if m == More::All { all_blocks(&w) } else { Vec::new() };
            let (len, title) = match m {
                More::All => (ids.len(), "All blocks"),
                More::Props => (props.props.len(), "Props"),
            };
            let on_page = page_len(len, page);
            for i in 0..on_page {
                let k = page * PER_PAGE + i;
                let (icon, label) = match m {
                    More::All => (ids.get(k).and_then(|id| icons.get(*id, &w, &blocks, &assets)), String::new()),
                    More::Props => (None, props.props.get(k).map(|p| p.name.clone()).unwrap_or_default()),
                };
                v.slots.push(SlotView { at: wheel_spot(i, on_page, MORE_RADIUS), icon, label, picked: choice == Some(i) });
            }
            v.centre = match (m, choice) {
                (More::All, Some(c)) => ids.get(page * PER_PAGE + c).map(|b| pretty(w.block_name(*b))).unwrap_or_default(),
                (More::Props, Some(c)) => props.props.get(page * PER_PAGE + c).map(|p| p.name.clone()).unwrap_or_default(),
                _ if len == 0 => format!("No {}", title.to_lowercase()),
                _ => title.to_string(),
            };
            v.hint = if pages(len) > 1 { format!("Page {} of {}: turn with the left stick or the mouse wheel", page + 1, pages(len)) } else { String::new() };
        }
    }
    if *view != v {
        *view = v;
    }
}

#[derive(Component)]
struct WheelRoot;
#[derive(Component)]
struct WheelDisc;
#[derive(Component)]
struct WheelSlot(usize);
#[derive(Component)]
struct WheelIcon(usize);
#[derive(Component)]
struct WheelLabel(usize);
#[derive(Component)]
struct WheelCentre;
#[derive(Component)]
struct WheelHint;

/// The wheel's nodes: a disc, a pool of slots, the name in the middle, a line under it.
fn spawn_wheel(mut commands: Commands, style: Res<HintStyle>) {
    let font = |px: f32| TextFont { font: style.font.clone().into(), font_size: bevy::text::FontSize::Px(px), ..default() };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(400),
            Visibility::Hidden,
            WheelRoot,
        ))
        .with_children(|r| {
            r.spawn((
                Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.55)),
                WheelDisc,
            ))
            .with_children(|ring| {
                ring.spawn((
                    Text::new(""),
                    font(11.0),
                    TextColor(Color::srgb(0.98, 0.98, 1.0)),
                    crate::input::hints::pixel_shadow(),
                    TextLayout::justify(Justify::Center),
                    Node { max_width: Val::Px(150.0), ..default() },
                    WheelCentre,
                ));
                for i in 0..PER_PAGE.max(11) {
                    ring.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(SLOT),
                            height: Val::Px(SLOT),
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(SLOT / 2.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BorderColor::all(EDGE),
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                        Visibility::Hidden,
                        WheelSlot(i),
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            ImageNode::default(),
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                border_radius: BorderRadius::all(Val::Px(SLOT / 2.0)),
                                ..default()
                            },
                            Visibility::Hidden,
                            WheelIcon(i),
                        ));
                        slot.spawn((
                            Text::new(""),
                            font(7.0),
                            TextColor(Color::srgb(0.98, 0.98, 1.0)),
                            crate::input::hints::pixel_shadow(),
                            TextLayout::justify(Justify::Center),
                            WheelLabel(i),
                        ));
                    });
                }
            });
            r.spawn((Text::new(""), font(8.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)), crate::input::hints::pixel_shadow(), WheelHint));
        });
}

/// Draw the open wheel's [`WheelView`].
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn draw_wheel(
    wheels: Res<Wheels>,
    view: Res<WheelView>,
    mut root: Query<&mut Visibility, (With<WheelRoot>, Without<WheelSlot>, Without<WheelIcon>)>,
    mut disc: Query<&mut Node, (With<WheelDisc>, Without<WheelSlot>)>,
    mut slots: Query<(&WheelSlot, &mut Node, &mut BorderColor, &mut BackgroundColor, &mut Visibility), (Without<WheelRoot>, Without<WheelDisc>, Without<WheelIcon>)>,
    mut pics: Query<(&WheelIcon, &mut ImageNode, &mut Visibility), (Without<WheelRoot>, Without<WheelSlot>)>,
    mut labels: Query<(&WheelLabel, &mut Text), (Without<WheelCentre>, Without<WheelHint>)>,
    mut centre: Query<&mut Text, (With<WheelCentre>, Without<WheelLabel>, Without<WheelHint>)>,
    mut hint: Query<&mut Text, (With<WheelHint>, Without<WheelLabel>, Without<WheelCentre>)>,
) {
    let show = if wheels.is_open() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut root {
        if *v != show {
            *v = show;
        }
    }
    if !wheels.is_open() || !(view.is_changed() || wheels.is_changed()) {
        return;
    }
    let size = view.radius * 2.0 + SLOT + 16.0;
    for mut n in &mut disc {
        n.width = Val::Px(size);
        n.height = Val::Px(size);
        n.border_radius = BorderRadius::all(Val::Px(size / 2.0));
    }
    for (s, mut node, mut b, mut bg, mut vis) in &mut slots {
        let Some(sv) = view.slots.get(s.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        let at = sv.at + Vec2::splat(size / 2.0) - Vec2::splat(SLOT / 2.0);
        node.left = Val::Px(at.x);
        node.top = Val::Px(at.y);
        *b = BorderColor::all(if sv.picked { PICKED } else { EDGE });
        bg.0 = if sv.picked { Color::srgba(1.0, 1.0, 1.0, 0.28) } else { Color::srgba(1.0, 1.0, 1.0, 0.08) };
    }
    for (i, mut img, mut vis) in &mut pics {
        match view.slots.get(i.0).and_then(|s| s.icon.clone()) {
            Some(h) => {
                img.image = h;
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (l, mut t) in &mut labels {
        let want = view.slots.get(l.0).filter(|s| s.icon.is_none()).map(|s| s.label.clone()).unwrap_or_default();
        if t.0 != want {
            t.0 = want;
        }
    }
    for mut t in &mut centre {
        if t.0 != view.centre {
            t.0 = view.centre.clone();
        }
    }
    for mut t in &mut hint {
        if t.0 != view.hint {
            t.0 = view.hint.clone();
        }
    }
}

/// The wheels. Needs [`crate::input::InputPlugin`] and [`crate::hud::HudPlugin`] (for the
/// block pictures and the font).
pub struct WheelPlugin;

impl Plugin for WheelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::emote::EmotePlugin)
            .init_resource::<Wheels>()
            .init_resource::<WheelView>()
            .init_resource::<BlockWheel>()
            .init_resource::<PropMenu>()
            .add_systems(Startup, spawn_wheel)
            .add_systems(
                Update,
                (
                    (block_wheel, crate::emote::emote_wheel).chain().before(crate::client::controls),
                    (block_wheel_view, crate::emote::emote_wheel_view, draw_wheel).chain().after(crate::client::controls),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_circle_of_slots_points_clockwise_from_the_top() {
        assert_eq!(wheel_choice(Vec2::new(0.0, 100.0), 8), Some(0));
        assert_eq!(wheel_choice(Vec2::new(100.0, 0.0), 8), Some(2));
        assert_eq!(wheel_choice(Vec2::new(-100.0, 0.0), 8), Some(6));
        assert_eq!(wheel_choice(Vec2::new(5.0, 5.0), 8), None, "too near the middle");
        assert_eq!(wheel_choice(Vec2::new(0.0, 100.0), 0), None);
        let top = wheel_spot(0, 8, 100.0);
        assert!(top.x.abs() < 1e-3 && (top.y + 100.0).abs() < 1e-3, "slot 0 at the top: {top}");
    }

    #[test]
    fn the_block_wheel_has_quick_blocks_on_top_and_two_sectors_below() {
        // straight up is the middle quick block of nine; far left and right of the arc the ends
        assert_eq!(block_pick(Vec2::new(0.0, 100.0), 9), Some(Pick::Quick(4)));
        let left_end = Vec2::new(-(ARC_DEG.to_radians().sin()), ARC_DEG.to_radians().cos()) * 100.0;
        assert_eq!(block_pick(left_end, 9), Some(Pick::Quick(0)));
        assert_eq!(block_pick(Vec2::new(100.0, 0.0), 9), Some(Pick::Quick(7)), "a quarter turn right is one short of the arc's end (110 degrees)");
        // the bottom half: lower left is All blocks, lower right Props
        assert_eq!(block_pick(Vec2::new(-40.0, -90.0), 9), Some(Pick::All));
        assert_eq!(block_pick(Vec2::new(40.0, -90.0), 9), Some(Pick::Props));
        assert_eq!(block_pick(Vec2::new(0.0, -100.0), 0), Some(Pick::Props), "straight down is a sector even with no quick blocks");
        assert_eq!(block_pick(Vec2::new(0.0, 100.0), 0), None);
        assert_eq!(block_pick(Vec2::new(0.0, 100.0), 1), Some(Pick::Quick(0)));
        assert_eq!(block_pick(Vec2::new(3.0, 3.0), 9), None, "too near the middle");
    }

    #[test]
    fn every_slot_is_picked_by_pointing_at_it() {
        for n in 1..=9 {
            for i in 0..n {
                // a spot is in screen space (y down); a pointer has y up
                let s = quick_spot(i, n);
                assert_eq!(block_pick(Vec2::new(s.x, -s.y), n), Some(Pick::Quick(i)), "{i} of {n}");
            }
        }
        for (m, p) in [(More::All, Pick::All), (More::Props, Pick::Props)] {
            let s = sector_spot(m);
            assert_eq!(block_pick(Vec2::new(s.x, -s.y), 9), Some(p));
        }
        for n in 1..=PER_PAGE {
            for i in 0..n {
                let s = wheel_spot(i, n, MORE_RADIUS);
                assert_eq!(wheel_choice(Vec2::new(s.x, -s.y), n), Some(i), "{i} of {n}");
            }
        }
    }

    #[test]
    fn pages_turn_only_when_something_moves() {
        use bevy::math::CompassOctant as O;
        assert_eq!(page_turn(None, 0.0), 0, "a still mouse wheel turns nothing");
        assert_eq!(page_turn(None, -0.0), 0);
        assert_eq!(page_turn(Some(O::East), 0.0), 1);
        assert_eq!(page_turn(Some(O::West), 0.0), -1);
        assert_eq!(page_turn(Some(O::North), 0.0), 0);
        assert_eq!(page_turn(None, -3.0), 1, "the wheel down turns on");
        assert_eq!(page_turn(None, 1.0), -1);
        assert_eq!(page_turn(None, f32::NAN), 0);
    }

    #[test]
    fn pages_hold_sixteen() {
        assert_eq!(pages(0), 1);
        assert_eq!(pages(16), 1);
        assert_eq!(pages(17), 2);
        assert_eq!(page_len(25, 0), 16);
        assert_eq!(page_len(25, 1), 9);
        assert_eq!(page_len(25, 2), 0);
        assert_eq!(page_len(0, 0), 0);
    }
}
