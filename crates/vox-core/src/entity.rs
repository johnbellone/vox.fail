//! Entities: the things placed in a world beside its blocks (doorways, spawn points, an
//! objective, the world's settings). Each is a [`Component`]. They are written with postcard
//! in zones and sent on the wire, so the variant order is part of the format: never reorder
//! or remove a variant; add new ones at the end.
//!
//! This module checks the values themselves (finite numbers, ranges, caps on text and size,
//! no control characters in text). Whether a destination may be used is a rule for the edit
//! that places it, not for the value.
//!
//! [`Component::validate`] is the gate: call it on anything that came from the network.
//! [`Component::tidy`] is a convenience for authored input (level files, the editor's own
//! values); it repairs what can be repaired and never replaces validate.

use serde::{Deserialize, Serialize};

/// Longest name, in characters.
pub const MAX_NAME: usize = 32;
/// Longest settings text, in bytes.
pub const MAX_SETTINGS: usize = 16 * 1024;
/// Longest URL, in characters.
pub const MAX_URL: usize = 128;
/// Longest world id, dungeon name or level name, in characters.
pub const MAX_ID: usize = 64;
/// Largest objective radius, in blocks.
pub const MAX_RADIUS: f32 = 256.0;

/// An entity's number within its world.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct EntityId(pub u32);

/// The way a doorway is walked through: along x or along z.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum Axis {
    X,
    Z,
}

/// A doorway.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Portal {
    pub name: String,
    /// the middle of the sill, blocks
    pub at: [f32; 3],
    pub axis: Axis,
    /// width and height, blocks
    pub size: [f32; 2],
    /// which way walking through faces there, degrees (as level files)
    pub yaw: f32,
    /// the level its echo shows
    pub level: String,
    pub to: Dest,
    pub key: Option<i64>,
}

impl Portal {
    pub const VERSION: u16 = 1;

    /// Where walking through this doorway leads: the url to dial (None for somewhere in this
    /// world) and the arrival point there. None for a `World` or `Pair` destination, which the
    /// directory resolves (phase 2). Every place that reads a doorway's destination uses this,
    /// so the server and the client agree.
    pub fn travel(&self) -> Option<(Option<String>, [f32; 3])> {
        match &self.to {
            Dest::Here { arrive } => Some((None, *arrive)),
            Dest::Url { url, arrive } => Some((Some(url.clone()), *arrive)),
            Dest::Dungeon { name, arrive } => Some((Some(format!("dungeon:{name}")), *arrive)),
            Dest::World { .. } | Dest::Pair { .. } => None,
        }
    }
}

/// Where a doorway leads.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Dest {
    /// somewhere else in this world, arriving at `arrive`
    Here { arrive: [f32; 3] },
    /// another world on this host, by its id
    World { world: String },
    /// the other half of a pair of doorways with this key
    Pair { key: i64 },
    /// the traveller's raid party's copy of a dungeon, arriving at `arrive` there
    Dungeon { name: String, arrive: [f32; 3] },
    /// another host, at `url`, arriving at `arrive` there
    Url { url: String, arrive: [f32; 3] },
}

/// Where players start in a world.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Spawn {
    /// feet, blocks
    pub at: [f32; 3],
    /// degrees
    pub yaw: f32,
}

impl Spawn {
    pub const VERSION: u16 = 1;
}

/// A place to reach: a dungeon's objective.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Objective {
    pub name: String,
    /// feet, blocks
    pub at: [f32; 3],
    /// how near counts, blocks; more than zero
    pub radius: f32,
}

impl Objective {
    pub const VERSION: u16 = 1;
}

/// The level's `world` object as JSON text, without its `portals` and `objective` keys (those
/// are entities now). Kept as text because postcard cannot carry a `serde_json::Value`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings {
    pub json: String,
}

impl Settings {
    pub const VERSION: u16 = 1;
}

/// Most characters in a prop's readable text.
pub const MAX_PROP_TEXT: usize = 140;
/// Facings a prop has: yaw is in 15 degree steps, 0 to 23 (a full turn).
pub const PROP_YAWS: u8 = 24;
/// Largest variant or state number a prop may carry.
pub const MAX_PROP_CODE: u8 = 7;
/// Five-centimetre units in one 0.5 m block.
pub const PROP_UNITS_PER_BLOCK: i32 = 10;

/// A prop placed in a world: a catalogue prop (or a player's own) at a position. Its shape,
/// footprint and behaviour come from its definition, which the server looks up by `prop`.
/// vox-core cannot depend on vox-format, so the definition's name is carried as its 16 bytes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prop {
    /// the prop's version hash (`vox_format::Hash`)
    pub prop: [u8; 16],
    /// the position, in 5 cm units (see [`prop_cell`])
    pub at: [i32; 3],
    /// facing, in 15 degree steps: 0 to 23
    pub yaw: u8,
    /// which look the prop has: 0 to 7
    pub variant: u8,
    /// which state it is in (open, lit, ...): 0 to 7
    pub state: u8,
    /// the channel a switch or a light listens to
    pub channel: Option<u16>,
    /// a readable's text: at most [`MAX_PROP_TEXT`] characters, no control characters
    pub text: Option<String>,
    /// the account that placed it; None for one that came from a level file
    pub owner: Option<u64>,
}

impl Prop {
    pub const VERSION: u16 = 1;
}

/// The block a prop's position falls in. It is floored, so a negative position lands in the
/// block below it: -1 is block -1, -10 is block -1, -11 is block -2. Dividing by the constant
/// ten cannot overflow.
pub fn prop_cell(at: [i32; 3]) -> [i32; 3] {
    at.map(|c| c.div_euclid(PROP_UNITS_PER_BLOCK))
}

/// Longest kind name, in characters.
pub const MAX_KIND: usize = 32;
/// Longest recipe, in bytes.
pub const MAX_RECIPE: usize = 16 * 1024;
/// Most detail levels a node may name: 0 (intent) to 4 (dressing).
pub const MAX_DETAIL: u8 = 4;
/// Longest side of a node's footprint, in blocks.
pub const MAX_NODE_SIDE: u16 = 64;

/// Largest node seed: the largest whole number a JavaScript number holds exactly (2^53 - 1), so
/// the editor's page can read a seed and send it back unchanged.
pub const MAX_NODE_SEED: u64 = (1 << 53) - 1;

/// A node of the world graph placed in a world: what it is, how far it has been refined, where
/// its footprint starts and how big it is, and the recipe that builds it. The server expands the
/// recipe with vox-graph when an editor builds the node (`ToServer::Build`). vox-core cannot
/// depend on vox-graph, so the kind is carried as its snake_case name and checked here only for
/// its shape; vox-graph's own vocabulary decides whether it is a kind at all.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Node {
    /// the kind's snake_case name (`house`, `tower`, ...), at most [`MAX_KIND`] characters
    pub kind: String,
    /// how far it has been refined, 0 (intent) to [`MAX_DETAIL`] (dressing)
    pub detail: u8,
    /// the footprint's minimum corner, in blocks
    pub at: [i32; 3],
    /// the footprint's size, in blocks: each side 1 to [`MAX_NODE_SIDE`]
    pub size: [u16; 3],
    /// the node this one sits inside, if any
    pub parent: Option<EntityId>,
    /// the recipe as JSON text, at most [`MAX_RECIPE`] bytes; None until it has one
    pub recipe: Option<String>,
    /// seeds the recipe, so a build is the same every time
    pub seed: u64,
}

impl Node {
    pub const VERSION: u16 = 1;
}

/// One entity's data. Never reorder or remove a variant; new ones go at the end.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Component {
    Portal(Portal),
    Spawn(Spawn),
    Objective(Objective),
    Settings(Settings),
    Prop(Prop),
    Node(Node),
}

impl Component {
    /// The component's stable name, as the format and the directory know it.
    pub fn name(&self) -> &'static str {
        match self {
            Component::Portal(_) => "vox::Portal",
            Component::Spawn(_) => "vox::Spawn",
            Component::Objective(_) => "vox::Objective",
            Component::Settings(_) => "vox::Settings",
            Component::Prop(_) => "vox::Prop",
            Component::Node(_) => "vox::Node",
        }
    }

    /// The version of this component's data.
    pub fn version(&self) -> u16 {
        match self {
            Component::Portal(_) => Portal::VERSION,
            Component::Spawn(_) => Spawn::VERSION,
            Component::Objective(_) => Objective::VERSION,
            Component::Settings(_) => Settings::VERSION,
            Component::Prop(_) => Prop::VERSION,
            Component::Node(_) => Node::VERSION,
        }
    }

    /// Tidies what can be fixed without refusing it: strips control characters from every
    /// text field, and makes a doorway's size finite and in range (as `portal.rs` does). This
    /// is for authored input only; it does not make a value safe, so call `validate` on
    /// anything from the network.
    pub fn tidy(&mut self) {
        match self {
            Component::Portal(p) => {
                p.name = strip_control(&p.name);
                p.level = strip_control(&p.level);
                match &mut p.to {
                    Dest::Here { .. } | Dest::Pair { .. } => {}
                    Dest::World { world } => *world = strip_control(world),
                    Dest::Dungeon { name, .. } => *name = strip_control(name),
                    Dest::Url { url, .. } => *url = strip_control(url),
                }
                p.size = [tidy_size(p.size[0], 4.0), tidy_size(p.size[1], 6.0)];
            }
            Component::Objective(o) => o.name = strip_control(&o.name),
            Component::Prop(p) => {
                p.yaw %= PROP_YAWS;
                p.variant = p.variant.min(MAX_PROP_CODE);
                p.state = p.state.min(MAX_PROP_CODE);
                if let Some(t) = &mut p.text {
                    *t = strip_control(t.as_str());
                }
            }
            Component::Node(n) => {
                n.kind = strip_control(&n.kind);
                n.detail = n.detail.min(MAX_DETAIL);
                n.size = n.size.map(|s| s.clamp(1, MAX_NODE_SIDE));
                if let Some(r) = &mut n.recipe {
                    *r = strip_layout_control(r);
                }
            }
            Component::Spawn(_) | Component::Settings(_) => {}
        }
    }

    /// Checks the values: numbers finite, sizes and radius in range, text within its caps and
    /// free of control characters, settings within their cap. Destination rules are not
    /// checked here. Call this on anything from the network.
    pub fn validate(&self) -> Result<(), EntityWhy> {
        match self {
            Component::Portal(p) => {
                finite(&p.at)?;
                if !p.size.iter().all(|c| (1.0..=64.0).contains(c)) {
                    return Err(EntityWhy::Invalid);
                }
                finite(&[p.yaw])?;
                text(&p.name, MAX_NAME)?;
                text(&p.level, MAX_ID)?;
                match &p.to {
                    Dest::Here { arrive } => finite(arrive),
                    Dest::World { world } => text(world, MAX_ID),
                    Dest::Pair { .. } => Ok(()),
                    Dest::Dungeon { name, arrive } => {
                        text(name, MAX_ID)?;
                        finite(arrive)
                    }
                    Dest::Url { url, arrive } => {
                        text(url, MAX_URL)?;
                        finite(arrive)
                    }
                }
            }
            Component::Spawn(s) => {
                finite(&s.at)?;
                finite(&[s.yaw])
            }
            Component::Objective(o) => {
                text(&o.name, MAX_NAME)?;
                finite(&o.at)?;
                finite(&[o.radius])?;
                if o.radius > 0.0 && o.radius <= MAX_RADIUS { Ok(()) } else { Err(EntityWhy::Invalid) }
            }
            Component::Settings(s) => {
                if s.json.len() > MAX_SETTINGS {
                    return Err(EntityWhy::TooBig);
                }
                match serde_json::from_str::<serde_json::Value>(&s.json) {
                    Ok(serde_json::Value::Object(map)) => {
                        if map.contains_key("portals") || map.contains_key("objective") {
                            Err(EntityWhy::Invalid)
                        } else {
                            Ok(())
                        }
                    }
                    _ => Err(EntityWhy::Invalid),
                }
            }
            Component::Prop(p) => {
                if p.yaw >= PROP_YAWS || p.variant > MAX_PROP_CODE || p.state > MAX_PROP_CODE {
                    return Err(EntityWhy::Invalid);
                }
                match &p.text {
                    Some(t) => text(t, MAX_PROP_TEXT),
                    None => Ok(()),
                }
            }
            Component::Node(n) => {
                // the kind is a snake_case word; vox-graph decides which words are kinds
                if n.kind.chars().count() > MAX_KIND {
                    return Err(EntityWhy::TooBig);
                }
                let word = !n.kind.is_empty() && n.kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                if !word || n.detail > MAX_DETAIL || !n.size.iter().all(|s| (1..=MAX_NODE_SIDE).contains(s)) || n.seed > MAX_NODE_SEED {
                    return Err(EntityWhy::Invalid);
                }
                match &n.recipe {
                    Some(r) if r.len() > MAX_RECIPE => Err(EntityWhy::TooBig),
                    // a recipe is JSON: a line break or a tab is fine, nothing else that is a control
                    Some(r) if r.chars().any(|c| c.is_control() && c != '\n' && c != '\t') => Err(EntityWhy::Invalid),
                    _ => Ok(()),
                }
            }
        }
    }
}

/// Why an entity wasn't placed or changed.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityWhy {
    /// it's someone else's
    NotYours,
    /// outside the world
    Outside,
    /// further than a player can reach
    TooFar,
    /// too many of these here
    TooMany,
    /// too long or too much
    TooBig,
    /// something is in the way
    Blocked,
    /// that doorway can't lead there
    BadDest,
    /// no such prop
    NoSuchProp,
    /// not enough to pay for it
    CantAfford,
    /// too many changes too fast
    TooFast,
    /// it changed since you last looked
    Changed,
    /// not available right now
    Unavailable,
    /// a malformed value: a number that isn't finite, bad JSON
    Invalid,
}

impl std::fmt::Display for EntityWhy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            EntityWhy::NotYours => "that's not yours",
            EntityWhy::Outside => "that's outside the world",
            EntityWhy::TooFar => "that's out of reach",
            EntityWhy::TooMany => "there are too many of those here",
            EntityWhy::TooBig => "that's too big",
            EntityWhy::Blocked => "something is in the way",
            EntityWhy::BadDest => "that doorway can't lead there",
            EntityWhy::NoSuchProp => "there's no such prop",
            EntityWhy::CantAfford => "that costs more than you have",
            EntityWhy::TooFast => "slow down",
            EntityWhy::Changed => "that changed since you looked",
            EntityWhy::Unavailable => "that's not available right now",
            EntityWhy::Invalid => "that isn't a valid value",
        })
    }
}

fn finite<const N: usize>(v: &[f32; N]) -> Result<(), EntityWhy> {
    if v.iter().all(|c| c.is_finite()) { Ok(()) } else { Err(EntityWhy::Invalid) }
}

fn text(s: &str, max: usize) -> Result<(), EntityWhy> {
    if s.chars().count() > max {
        return Err(EntityWhy::TooBig);
    }
    if s.chars().any(char::is_control) { Err(EntityWhy::Invalid) } else { Ok(()) }
}

/// A doorway's size component: a number that isn't finite becomes the default `parse()` uses,
/// then it is clamped. Clamping first would leave NaN as NaN.
fn tidy_size(c: f32, default: f32) -> f32 {
    let c = if c.is_finite() { c } else { default };
    c.clamp(1.0, 64.0)
}

fn strip_control(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// As [`strip_control`], but keeps the line breaks and tabs a recipe's JSON may have.
fn strip_layout_control(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{decode, encode};

    fn portal(to: Dest) -> Portal {
        Portal {
            name: "void".into(),
            at: [0.0, 0.0, 11.5],
            axis: Axis::Z,
            size: [8.0, 16.0],
            yaw: 0.0,
            level: "void".into(),
            to,
            key: None,
        }
    }

    fn node(kind: &str) -> Node {
        Node { kind: kind.into(), detail: 3, at: [-40, 0, 12], size: [10, 12, 8], parent: None, recipe: None, seed: 9 }
    }

    fn settings(json: &str) -> Component {
        Component::Settings(Settings { json: json.into() })
    }

    fn prop(at: [i32; 3]) -> Prop {
        Prop {
            prop: [3; 16],
            at,
            yaw: 0,
            variant: 0,
            state: 0,
            channel: None,
            text: None,
            owner: None,
        }
    }

    #[test]
    fn postcard_round_trip_of_every_variant() {
        let all = vec![
            Component::Portal(portal(Dest::Here { arrive: [0.0, 0.0, -39.5] })),
            Component::Portal(portal(Dest::World { world: "yard".into() })),
            Component::Portal(Portal { key: Some(7), ..portal(Dest::Pair { key: 7 }) }),
            Component::Portal(portal(Dest::Dungeon { name: "crypt".into(), arrive: [0.0, 0.0, 1.0] })),
            Component::Portal(portal(Dest::Url { url: ":7781".into(), arrive: [1.0, 0.0, 2.0] })),
            Component::Spawn(Spawn { at: [1.0, 2.0, 3.0], yaw: 90.0 }),
            Component::Objective(Objective { name: "the altar".into(), at: [0.0, 0.0, -9.0], radius: 3.0 }),
            settings(r#"{"sky":"earth","time":16.5}"#),
            Component::Prop(prop([-11, 0, 25])),
            Component::Prop(Prop { channel: Some(9), text: Some("ahoy".into()), owner: Some(7), ..prop([0, 0, 0]) }),
            Component::Node(node("house")),
            Component::Node(Node { parent: Some(EntityId(4)), recipe: Some("{\n\t\"ops\": []\n}".into()), ..node("tower") }),
        ];
        for c in all {
            let back: Component = decode(&encode(&c)).expect("decodes");
            assert_eq!(back, c);
        }
    }

    #[test]
    fn node_shape_and_caps_are_checked() {
        assert_eq!(Component::Node(node("house")).validate(), Ok(()));
        assert_eq!(Component::Node(node("")).validate(), Err(EntityWhy::Invalid));
        assert_eq!(Component::Node(node("House")).validate(), Err(EntityWhy::Invalid));
        assert_eq!(Component::Node(node("a b")).validate(), Err(EntityWhy::Invalid));
        assert_eq!(Component::Node(node(&"a".repeat(MAX_KIND + 1))).validate(), Err(EntityWhy::TooBig));
        let mut n = node("house");
        n.detail = MAX_DETAIL + 1;
        assert_eq!(Component::Node(n.clone()).validate(), Err(EntityWhy::Invalid));
        n.detail = 0;
        n.size = [0, 12, 8];
        assert_eq!(Component::Node(n.clone()).validate(), Err(EntityWhy::Invalid));
        n.size = [MAX_NODE_SIDE, 12, 8];
        assert_eq!(Component::Node(n.clone()).validate(), Ok(()));
        n.size = [MAX_NODE_SIDE + 1, 12, 8];
        assert_eq!(Component::Node(n.clone()).validate(), Err(EntityWhy::Invalid));
        n.size = [8, 12, 8];
        n.seed = MAX_NODE_SEED;
        assert_eq!(Component::Node(n.clone()).validate(), Ok(()));
        n.seed = MAX_NODE_SEED + 1;
        assert_eq!(Component::Node(n).validate(), Err(EntityWhy::Invalid));
    }

    #[test]
    fn node_recipe_is_capped_and_free_of_control_characters() {
        let mut n = node("house");
        n.recipe = Some("{\n\t}".into());
        assert_eq!(Component::Node(n.clone()).validate(), Ok(()));
        n.recipe = Some("{\u{7}}".into());
        assert_eq!(Component::Node(n.clone()).validate(), Err(EntityWhy::Invalid));
        n.recipe = Some("x".repeat(MAX_RECIPE + 1));
        assert_eq!(Component::Node(n).validate(), Err(EntityWhy::TooBig));
    }

    #[test]
    fn tidy_clamps_a_node_and_strips_its_recipe() {
        let mut n = node("ho\u{1b}use");
        n.detail = 9;
        n.size = [0, 200, 8];
        n.recipe = Some("{\n\u{7}}".into());
        let mut c = Component::Node(n);
        c.tidy();
        let Component::Node(n) = c else { panic!("still a node") };
        assert_eq!(n.kind, "house");
        assert_eq!(n.detail, MAX_DETAIL);
        assert_eq!(n.size, [1, MAX_NODE_SIDE, 8]);
        assert_eq!(n.recipe.as_deref(), Some("{\n}"));
        assert_eq!(Component::Node(n).validate(), Ok(()));
    }

    #[test]
    fn names_and_versions() {
        let p = Component::Portal(portal(Dest::Here { arrive: [0.0; 3] }));
        assert_eq!(p.name(), "vox::Portal");
        assert_eq!(p.version(), 1);
        assert_eq!(settings("{}").name(), "vox::Settings");
        let pr = Component::Prop(prop([0; 3]));
        assert_eq!(pr.name(), "vox::Prop");
        assert_eq!(pr.version(), 1);
        let nd = Component::Node(node("house"));
        assert_eq!(nd.name(), "vox::Node");
        assert_eq!(nd.version(), 1);
    }

    #[test]
    fn prop_cell_floors_negative_positions() {
        assert_eq!(prop_cell([0, 9, 10]), [0, 0, 1]);
        assert_eq!(prop_cell([-1, -10, -11]), [-1, -1, -2]);
        assert_eq!(prop_cell([19, 20, -20]), [1, 2, -2]);
        assert_eq!(prop_cell([i32::MIN, i32::MAX, 0]), [i32::MIN / 10 - 1, i32::MAX / 10, 0]);
    }

    #[test]
    fn prop_yaw_and_codes_out_of_range_are_refused() {
        let mut p = prop([0; 3]);
        p.yaw = 23;
        assert_eq!(Component::Prop(p.clone()).validate(), Ok(()));
        p.yaw = PROP_YAWS;
        assert_eq!(Component::Prop(p.clone()).validate(), Err(EntityWhy::Invalid));
        p.yaw = 0;
        p.variant = 8;
        assert_eq!(Component::Prop(p.clone()).validate(), Err(EntityWhy::Invalid));
        p.variant = 0;
        p.state = 8;
        assert_eq!(Component::Prop(p.clone()).validate(), Err(EntityWhy::Invalid));
        p.state = 7;
        assert_eq!(Component::Prop(p).validate(), Ok(()));
    }

    #[test]
    fn prop_text_is_capped_and_free_of_control_characters() {
        let mut p = prop([0; 3]);
        p.text = Some("a".repeat(MAX_PROP_TEXT));
        assert_eq!(Component::Prop(p.clone()).validate(), Ok(()));
        p.text = Some("a".repeat(MAX_PROP_TEXT + 1));
        assert_eq!(Component::Prop(p.clone()).validate(), Err(EntityWhy::TooBig));
        p.text = Some("sign\u{7}".into());
        assert_eq!(Component::Prop(p.clone()).validate(), Err(EntityWhy::Invalid));
    }

    #[test]
    fn tidy_strips_prop_text_and_clamps_codes() {
        let mut p = prop([0; 3]);
        p.yaw = 25;
        p.variant = 9;
        p.state = 200;
        p.text = Some("sig\nn".into());
        let mut c = Component::Prop(p);
        c.tidy();
        let Component::Prop(p) = c else { panic!("still a prop") };
        assert_eq!(p.yaw, 1);
        assert_eq!(p.variant, 7);
        assert_eq!(p.state, 7);
        assert_eq!(p.text.as_deref(), Some("sign"));
        assert_eq!(Component::Prop(p).validate(), Ok(()));
    }

    #[test]
    fn travel_names_the_url_and_the_arrival() {
        let arrive = [1.0, 2.0, 3.0];
        assert_eq!(portal(Dest::Here { arrive }).travel(), Some((None, arrive)));
        assert_eq!(
            portal(Dest::Url { url: ":7781".into(), arrive }).travel(),
            Some((Some(":7781".into()), arrive))
        );
        assert_eq!(
            portal(Dest::Dungeon { name: "crypt".into(), arrive }).travel(),
            Some((Some("dungeon:crypt".into()), arrive))
        );
        assert_eq!(portal(Dest::World { world: "yard".into() }).travel(), None);
        assert_eq!(portal(Dest::Pair { key: 7 }).travel(), None);

        let mut bad = portal(Dest::Dungeon { name: "crypt".into(), arrive: [f32::NAN, 0.0, 0.0] });
        assert_eq!(Component::Portal(bad.clone()).validate(), Err(EntityWhy::Invalid));
        bad.to = Dest::Dungeon { name: "crypt".into(), arrive: [0.0; 3] };
        assert_eq!(Component::Portal(bad).validate(), Ok(()));
    }

    #[test]
    fn non_finite_numbers_are_refused() {
        let mut p = portal(Dest::Here { arrive: [0.0; 3] });
        p.at[1] = f32::NAN;
        assert_eq!(Component::Portal(p).validate(), Err(EntityWhy::Invalid));

        let s = Component::Spawn(Spawn { at: [0.0; 3], yaw: f32::INFINITY });
        assert_eq!(s.validate(), Err(EntityWhy::Invalid));

        let o = Component::Objective(Objective { name: "x".into(), at: [0.0; 3], radius: 0.0 });
        assert_eq!(o.validate(), Err(EntityWhy::Invalid));
    }

    #[test]
    fn over_long_text_is_refused() {
        let long = "a".repeat(MAX_NAME + 1);
        let mut p = portal(Dest::Here { arrive: [0.0; 3] });
        p.name = long.clone();
        assert_eq!(Component::Portal(p).validate(), Err(EntityWhy::TooBig));

        let url = "u".repeat(MAX_URL + 1);
        let u = portal(Dest::Url { url, arrive: [0.0; 3] });
        assert_eq!(Component::Portal(u).validate(), Err(EntityWhy::TooBig));

        let big = format!("{{\"sky\":\"{}\"}}", "x".repeat(MAX_SETTINGS));
        assert_eq!(settings(&big).validate(), Err(EntityWhy::TooBig));
    }

    #[test]
    fn tidy_strips_control_characters_and_clamps_size() {
        let mut p = portal(Dest::Here { arrive: [0.0; 3] });
        p.name = "ya\u{7}rd\n".into();
        p.size = [0.0, 200.0];
        let mut c = Component::Portal(p);
        c.tidy();
        let Component::Portal(p) = c else { panic!("still a portal") };
        assert_eq!(p.name, "yard");
        assert_eq!(p.size, [1.0, 64.0]);

        let mut o = Component::Objective(Objective { name: "\u{1b}altar".into(), at: [0.0; 3], radius: 3.0 });
        o.tidy();
        let Component::Objective(o) = o else { panic!("still an objective") };
        assert_eq!(o.name, "altar");
    }

    #[test]
    fn settings_with_portals_or_objective_are_refused() {
        assert_eq!(settings(r#"{"sky":"earth"}"#).validate(), Ok(()));
        assert_eq!(settings(r#"{"portals":[]}"#).validate(), Err(EntityWhy::Invalid));
        assert_eq!(settings(r#"{"objective":{}}"#).validate(), Err(EntityWhy::Invalid));
        assert_eq!(settings("[1,2]").validate(), Err(EntityWhy::Invalid));
        assert_eq!(settings("{not json").validate(), Err(EntityWhy::Invalid));
    }

    #[test]
    fn control_characters_are_refused_in_text() {
        let url = portal(Dest::Url { url: "\u{7}:7781".into(), arrive: [0.0; 3] });
        assert_eq!(Component::Portal(url).validate(), Err(EntityWhy::Invalid));

        let mut level = portal(Dest::Here { arrive: [0.0; 3] });
        level.level = "vo\nid".into();
        assert_eq!(Component::Portal(level).validate(), Err(EntityWhy::Invalid));

        let dungeon = portal(Dest::Dungeon { name: "cr\u{1b}ypt".into(), arrive: [0.0; 3] });
        assert_eq!(Component::Portal(dungeon).validate(), Err(EntityWhy::Invalid));
    }

    #[test]
    fn doorway_size_outside_one_to_sixty_four_is_refused() {
        let mut small = portal(Dest::Here { arrive: [0.0; 3] });
        small.size = [0.5, 6.0];
        assert_eq!(Component::Portal(small).validate(), Err(EntityWhy::Invalid));

        let mut big = portal(Dest::Here { arrive: [0.0; 3] });
        big.size = [4.0, 65.0];
        assert_eq!(Component::Portal(big).validate(), Err(EntityWhy::Invalid));
    }

    #[test]
    fn objective_radius_above_the_cap_is_refused() {
        let far = Component::Objective(Objective { name: "altar".into(), at: [0.0; 3], radius: 300.0 });
        assert_eq!(far.validate(), Err(EntityWhy::Invalid));

        let edge = Component::Objective(Objective { name: "altar".into(), at: [0.0; 3], radius: MAX_RADIUS });
        assert_eq!(edge.validate(), Ok(()));
    }

    #[test]
    fn tidy_replaces_a_non_finite_size_with_the_default() {
        let mut p = portal(Dest::Dungeon { name: "cr\u{1b}ypt".into(), arrive: [0.0; 3] });
        p.size = [f32::NAN, f32::INFINITY];
        let mut c = Component::Portal(p);
        c.tidy();
        let Component::Portal(p) = c else { panic!("still a portal") };
        assert_eq!(p.size, [4.0, 6.0]);
        assert_eq!(p.to, Dest::Dungeon { name: "crypt".into(), arrive: [0.0; 3] });
        assert_eq!(Component::Portal(p).validate(), Ok(()));
    }
}
