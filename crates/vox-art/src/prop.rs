//! A prop definition: a small authored object (a street lamp, a bench, a door) as immutable data.
//!
//! The def names its size, pivot, placement, collision, named parts, sockets, states and the
//! closed set of behaviours it has. [`PropDef::validate`] lists every problem in a def, and
//! [`PropDef::version`] hashes the def with its voxels, so an instance can pin exactly the
//! version it was placed with. Reading a def from JSON rejects unknown fields and unknown
//! behaviours; nothing here runs a behaviour.

use std::collections::{BTreeMap, BTreeSet};

use blake3::Hasher;
use serde::{Deserialize, Serialize};

use crate::grid::VoxelGrid;
use crate::scale::Scale;

/// Longest id, in characters.
const MAX_ID: usize = 64;
/// Longest name, in characters.
const MAX_NAME: usize = 64;
/// Most tags a prop may carry.
const MAX_TAGS: usize = 16;
/// Longest tag, in characters.
const MAX_TAG: usize = 32;
/// Longest key-like string, in characters: a socket, part, state or variant name, a paint key
/// or value, a sound, marker, door or motion part, a light's colour or socket, a seat's socket
/// or clip. Short on purpose, so a name stays readable in a tool and in a log.
const MAX_KEY: usize = 32;
/// Most cells a prop may block on the floor.
const MAX_FOOTPRINT: usize = 64;
/// How far a footprint cell may sit from the pivot's floor cell, in either direction.
const FOOTPRINT_REACH: i16 = 8;
/// Highest placement height in half-metre blocks; the lowest is 1.
const MAX_HEIGHT: u8 = 16;
/// Most collision boxes a prop may list.
const MAX_BOXES: usize = 8;
/// Most named parts a prop may have.
const MAX_PARTS: usize = 16;
/// Most voxels a single part may have.
const MAX_PART_VOXELS: usize = 4096;
/// Most sockets a prop may have.
const MAX_SOCKETS: usize = 16;
/// Most states a prop may have.
const MAX_STATES: usize = 8;
/// Most behaviours a prop may have.
const MAX_BEHAVIOURS: usize = 8;
/// Most variants a prop may have.
const MAX_VARIANTS: usize = 8;
/// Most paint entries one variant may have.
const MAX_PAINT: usize = 16;
/// Longest text a readable may show, in characters.
const MAX_TEXT: usize = 140;
/// Strongest light a light behaviour may ask for.
const MAX_POWER: f32 = 16.0;
/// Widest a door may swing, in degrees either way.
const MAX_SWING: f32 = 180.0;

/// A prop's scale. Only the two that suit a prop: a structure is never a prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropScale {
    /// 5 cm voxels: under a metre, held items and small decoration.
    Fine,
    /// 10 cm voxels: furniture, boats and trees.
    Prop,
}

impl PropScale {
    /// The grid scale this prop is drawn at.
    pub fn scale(self) -> Scale {
        match self {
            PropScale::Fine => Scale::Character,
            PropScale::Prop => Scale::Prop,
        }
    }
}

/// The surfaces a prop may stand on or hang from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Floor,
    Wall,
    Ceiling,
    Table,
    Water,
}

/// Where a prop may be placed, and the floor cells and height it needs free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub on: Vec<Surface>,
    /// Cells (in 0.5 m blocks) relative to the pivot's floor cell.
    pub footprint: Vec<[i16; 2]>,
    /// Height in blocks that must be free above the footprint.
    pub height: u8,
}

/// What a prop blocks for walking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collide {
    /// Walk through it (a rug, a flower).
    None,
    /// Its footprint cells are blocked.
    Footprint,
    /// A few boxes, each `[min, max]` corners inclusive, in voxels.
    Boxes(Vec<[[u16; 3]; 2]>),
}

/// A named group of voxels a state can show, hide, move or spin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub voxels: Vec<[u16; 3]>,
}

/// A direction a socket faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    #[serde(rename = "+x")]
    PosX,
    #[serde(rename = "-x")]
    NegX,
    #[serde(rename = "+y")]
    PosY,
    #[serde(rename = "-y")]
    NegY,
    #[serde(rename = "+z")]
    PosZ,
    #[serde(rename = "-z")]
    NegZ,
}

/// A named point with a direction: where light comes from, where someone sits, where another
/// prop attaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub name: String,
    pub at: [u16; 3],
    pub dir: Dir,
}

/// When a light is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Always,
    Night,
    /// On while the prop is in the named state.
    State(String),
}

/// How a motion behaviour moves its part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionKind {
    Spin,
    Swing,
    Bob,
    Sway,
    Flicker,
}

/// The closed set of things a prop can do. A def lists which it has; nothing else runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Behaviour {
    Light(Light),
    Seat(Seat),
    Door(Door),
    Switch(Switch),
    Readable(Readable),
    Sound(Sound),
    Motion(Motion),
    Marker(Marker),
}

// Each behaviour is its own struct: serde's deny_unknown_fields does not reach the fields of
// a struct variant, and an unknown field in a behaviour should fail like one anywhere else.

/// A light at a socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Light {
    pub socket: String,
    pub colour: String,
    pub power: f32,
    pub when: When,
}

/// A socket where a character sits, with the name of the pose clip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub socket: String,
    pub clip: String,
}

/// A part that swings open by `swing` degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Door {
    pub part: String,
    pub swing: f32,
}

/// Toggles its own state, or every prop on its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    pub channel: Option<u16>,
}

/// Text shown when used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readable {
    pub text: String,
}

/// One named sound from the real-recording set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sound {
    pub name: String,
    pub looped: bool,
}

/// Looks only: a deterministic motion of a part, or of the whole prop when `part` is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub kind: MotionKind,
    pub part: Option<String>,
}

/// An objective or anchor for dungeons and quests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub name: String,
}

/// A palette swap of the def, and a seed for the few props that scatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub name: String,
    /// Material name to the material it is painted as.
    #[serde(default)]
    pub paint: BTreeMap<String, String>,
}

/// An immutable prop definition, as stored in `data/props/*.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropDef {
    pub id: String,
    pub name: String,
    pub scale: PropScale,
    /// Voxels along each axis; each side is 1 to the scale's largest side.
    pub size: [u16; 3],
    /// The voxel column on the placement point; always inside `size`.
    pub pivot: [u16; 3],
    #[serde(default)]
    pub tags: Vec<String>,
    pub place: Place,
    pub collide: Collide,
    #[serde(default)]
    pub parts: BTreeMap<String, Part>,
    #[serde(default)]
    pub sockets: Vec<Socket>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub behaviours: Vec<Behaviour>,
    #[serde(default)]
    pub variants: Vec<Variant>,
    pub palette_version: u16,
}

/// One problem found in a def: the field it is in and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub field: String,
    pub problem: String,
}

impl Reason {
    fn new(field: impl Into<String>, problem: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            problem: problem.into(),
        }
    }
}

/// Checks a short string from a def: 1 to `max` characters, and no control character. Names
/// reach tools, logs and files, so a control character (a newline, an escape) is refused
/// rather than shown.
fn short(bad: &mut Vec<Reason>, field: &str, s: &str, max: usize) {
    let len = s.chars().count();
    if len == 0 || len > max {
        bad.push(Reason::new(
            field,
            format!("must be 1 to {max} characters, not {len}"),
        ));
    } else if s.chars().any(char::is_control) {
        bad.push(Reason::new(field, format!("{s:?} has a control character")));
    }
}

impl PropDef {
    /// Checks the def and returns every problem found, or `Ok` when there are none.
    pub fn validate(&self) -> Result<(), Vec<Reason>> {
        let mut bad = Vec::new();

        let id_len = self.id.chars().count();
        if id_len == 0 || id_len > MAX_ID {
            bad.push(Reason::new(
                "id",
                format!("must be 1 to {MAX_ID} characters, not {id_len}"),
            ));
        }
        if !self
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        {
            bad.push(Reason::new("id", "may use only a-z, 0-9, '.', '_' and '-'"));
        }

        short(&mut bad, "name", &self.name, MAX_NAME);

        let max_side = self.scale.scale().max_side();
        for (axis, &side) in self.size.iter().enumerate() {
            if side == 0 || side > max_side {
                bad.push(Reason::new(
                    "size",
                    format!("side {axis} is {side}; each side must be 1 to {max_side}"),
                ));
            }
        }

        let inside = |p: [u16; 3]| p.iter().zip(self.size).all(|(&v, s)| v < s);
        if !inside(self.pivot) {
            bad.push(Reason::new(
                "pivot",
                format!("{:?} is outside size {:?}", self.pivot, self.size),
            ));
        }

        if self.tags.len() > MAX_TAGS {
            bad.push(Reason::new("tags", format!("more than {MAX_TAGS} tags")));
        }
        for tag in &self.tags {
            short(&mut bad, "tags", tag, MAX_TAG);
        }

        if self.place.footprint.len() > MAX_FOOTPRINT {
            bad.push(Reason::new(
                "place.footprint",
                format!("more than {MAX_FOOTPRINT} cells"),
            ));
        }
        let mut cells = BTreeSet::new();
        for cell in &self.place.footprint {
            if !cells.insert(*cell) {
                bad.push(Reason::new(
                    "place.footprint",
                    format!("cell {cell:?} is listed twice"),
                ));
            }
            if !cell
                .iter()
                .all(|c| (-FOOTPRINT_REACH..=FOOTPRINT_REACH).contains(c))
            {
                bad.push(Reason::new(
                    "place.footprint",
                    format!("cell {cell:?} is outside {FOOTPRINT_REACH} cells of the pivot"),
                ));
            }
        }
        if self.place.on.is_empty() {
            bad.push(Reason::new("place.on", "must name at least one surface"));
        }
        if !(1..=MAX_HEIGHT).contains(&self.place.height) {
            bad.push(Reason::new(
                "place.height",
                format!("{} is outside 1 to {MAX_HEIGHT}", self.place.height),
            ));
        }

        if let Collide::Boxes(boxes) = &self.collide {
            if boxes.len() > MAX_BOXES {
                bad.push(Reason::new(
                    "collide",
                    format!("more than {MAX_BOXES} boxes"),
                ));
            }
            for (i, [lo, hi]) in boxes.iter().enumerate() {
                let ordered = lo.iter().zip(hi.iter()).all(|(a, b)| a <= b);
                if !ordered || !inside(*hi) {
                    bad.push(Reason::new(
                        format!("collide.boxes[{i}]"),
                        format!(
                            "{lo:?}..={hi:?} must be ordered and inside size {:?}",
                            self.size
                        ),
                    ));
                }
            }
        }

        if self.parts.len() > MAX_PARTS {
            bad.push(Reason::new("parts", format!("more than {MAX_PARTS} parts")));
        }
        for (name, part) in &self.parts {
            short(&mut bad, "parts", name, MAX_KEY);
            if part.voxels.len() > MAX_PART_VOXELS {
                bad.push(Reason::new(
                    format!("parts.{name}"),
                    format!(
                        "{} voxels is more than {MAX_PART_VOXELS}",
                        part.voxels.len()
                    ),
                ));
            }
            for voxel in &part.voxels {
                if !inside(*voxel) {
                    bad.push(Reason::new(
                        format!("parts.{name}"),
                        format!("voxel {voxel:?} is outside size {:?}", self.size),
                    ));
                }
            }
        }

        if self.sockets.len() > MAX_SOCKETS {
            bad.push(Reason::new(
                "sockets",
                format!("more than {MAX_SOCKETS} sockets"),
            ));
        }
        let mut socket_names = BTreeSet::new();
        for socket in &self.sockets {
            short(&mut bad, "sockets", &socket.name, MAX_KEY);
            if !inside(socket.at) {
                bad.push(Reason::new(
                    format!("sockets.{}", socket.name),
                    format!("at {:?} is outside size {:?}", socket.at, self.size),
                ));
            }
            if !socket_names.insert(socket.name.as_str()) {
                bad.push(Reason::new(
                    "sockets",
                    format!("socket name {:?} is used twice", socket.name),
                ));
            }
        }

        if self.states.len() > MAX_STATES {
            bad.push(Reason::new(
                "states",
                format!("more than {MAX_STATES} states"),
            ));
        }
        let mut state_names = BTreeSet::new();
        for state in &self.states {
            short(&mut bad, "states", state, MAX_KEY);
            if !state_names.insert(state.as_str()) {
                bad.push(Reason::new(
                    "states",
                    format!("state name {state:?} is used twice"),
                ));
            }
        }

        if self.variants.len() > MAX_VARIANTS {
            bad.push(Reason::new(
                "variants",
                format!("more than {MAX_VARIANTS} variants"),
            ));
        }
        let mut variant_names = BTreeSet::new();
        for variant in &self.variants {
            short(&mut bad, "variants", &variant.name, MAX_KEY);
            if !variant_names.insert(variant.name.as_str()) {
                bad.push(Reason::new(
                    "variants",
                    format!("variant name {:?} is used twice", variant.name),
                ));
            }
            if variant.paint.len() > MAX_PAINT {
                bad.push(Reason::new(
                    "variants.paint",
                    format!("more than {MAX_PAINT} paint entries"),
                ));
            }
            for (material, painted) in &variant.paint {
                short(&mut bad, "variants.paint", material, MAX_KEY);
                short(&mut bad, "variants.paint", painted, MAX_KEY);
            }
        }

        if self.behaviours.len() > MAX_BEHAVIOURS {
            bad.push(Reason::new(
                "behaviours",
                format!("more than {MAX_BEHAVIOURS} behaviours"),
            ));
        }
        let has_socket = |name: &str| self.sockets.iter().any(|s| s.name == name);
        let has_part = |name: &str| self.parts.contains_key(name);
        let has_state = |name: &str| self.states.iter().any(|s| s == name);
        for (i, behaviour) in self.behaviours.iter().enumerate() {
            let field = format!("behaviours[{i}]");
            match behaviour {
                Behaviour::Light(light) => {
                    short(&mut bad, &field, &light.colour, MAX_KEY);
                    short(&mut bad, &field, &light.socket, MAX_KEY);
                    if !has_socket(&light.socket) {
                        bad.push(Reason::new(
                            &field,
                            format!("no socket named {:?}", light.socket),
                        ));
                    }
                    let power = light.power;
                    if !power.is_finite() || !(0.0..=MAX_POWER).contains(&power) {
                        bad.push(Reason::new(
                            &field,
                            format!("power {power} must be finite and 0 to {MAX_POWER}"),
                        ));
                    }
                    if let When::State(state) = &light.when {
                        short(&mut bad, &field, state, MAX_KEY);
                        if !has_state(state) {
                            bad.push(Reason::new(&field, format!("no state named {state:?}")));
                        }
                    }
                }
                Behaviour::Seat(seat) => {
                    short(&mut bad, &field, &seat.socket, MAX_KEY);
                    short(&mut bad, &field, &seat.clip, MAX_KEY);
                    if !has_socket(&seat.socket) {
                        bad.push(Reason::new(
                            &field,
                            format!("no socket named {:?}", seat.socket),
                        ));
                    }
                }
                Behaviour::Door(door) => {
                    short(&mut bad, &field, &door.part, MAX_KEY);
                    if !has_part(&door.part) {
                        bad.push(Reason::new(
                            &field,
                            format!("no part named {:?}", door.part),
                        ));
                    }
                    // `contains` is false for NaN, so this also refuses a non-finite swing.
                    if !(-MAX_SWING..=MAX_SWING).contains(&door.swing) {
                        bad.push(Reason::new(
                            &field,
                            format!(
                                "swing {} must be finite and -{MAX_SWING} to {MAX_SWING}",
                                door.swing
                            ),
                        ));
                    }
                }
                Behaviour::Readable(readable) => {
                    if readable.text.chars().count() > MAX_TEXT {
                        bad.push(Reason::new(
                            &field,
                            format!("text is over {MAX_TEXT} characters"),
                        ));
                    }
                    if readable.text.chars().any(char::is_control) {
                        bad.push(Reason::new(&field, "text has a control character"));
                    }
                }
                Behaviour::Sound(sound) => {
                    short(&mut bad, &field, &sound.name, MAX_KEY);
                }
                Behaviour::Marker(marker) => {
                    short(&mut bad, &field, &marker.name, MAX_KEY);
                }
                Behaviour::Motion(Motion {
                    part: Some(part), ..
                }) => {
                    short(&mut bad, &field, part, MAX_KEY);
                    if !has_part(part) {
                        bad.push(Reason::new(&field, format!("no part named {part:?}")));
                    }
                }
                Behaviour::Switch(_) | Behaviour::Motion(Motion { part: None, .. }) => {}
            }
        }

        if bad.is_empty() { Ok(()) } else { Err(bad) }
    }

    /// The def's version: BLAKE3 over a domain tag, the length and bytes of its canonical JSON,
    /// then its voxels in grid order, cut to 16 bytes. Deterministic on every target, so an
    /// instance can pin it. The grid must be the def's own size; any other is an error, since
    /// its voxels would not be the def's.
    pub fn version(&self, grid: &VoxelGrid) -> Result<[u8; 16], Reason> {
        if grid.size() != self.size {
            return Err(Reason::new(
                "size",
                format!("grid is {:?} but the def is {:?}", grid.size(), self.size),
            ));
        }
        let canonical =
            serde_json::to_vec(self).map_err(|e| Reason::new("json", e.to_string()))?;
        let len = u32::try_from(canonical.len())
            .map_err(|_| Reason::new("json", "canonical form is over 4 GiB"))?;
        let mut hasher = Hasher::new();
        hasher.update(b"VOXP-def-v1\0");
        hasher.update(&len.to_le_bytes());
        hasher.update(&canonical);
        for ([x, y, z], index) in grid.iter() {
            hasher.update(&x.to_le_bytes());
            hasher.update(&y.to_le_bytes());
            hasher.update(&z.to_le_bytes());
            hasher.update(&[index]);
        }
        let mut out = [0u8; 16];
        out.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAMP: &str = r#"{
        "id": "lamp.street",
        "name": "Street lamp",
        "scale": "fine",
        "size": [7, 44, 7],
        "pivot": [3, 0, 3],
        "tags": ["street", "light", "village", "outdoor"],
        "place": { "on": ["floor"], "footprint": [[0, 0]], "height": 2 },
        "collide": "footprint",
        "parts": { "lantern": { "voxels": [[3, 38, 3], [3, 39, 3]] } },
        "sockets": [{ "name": "light", "at": [3, 38, 3], "dir": "+y" }],
        "states": ["off", "lit"],
        "behaviours": [
            { "light": { "socket": "light", "colour": "warm", "power": 3.0, "when": "night" } }
        ],
        "variants": [{ "name": "iron" }, { "name": "copper", "paint": { "iron": "copper" } }],
        "palette_version": 1
    }"#;

    fn lamp() -> PropDef {
        serde_json::from_str(LAMP).unwrap()
    }

    fn lamp_grid() -> VoxelGrid {
        let mut grid = VoxelGrid::new(PropScale::Fine.scale(), [7, 44, 7]).unwrap();
        grid.set([3, 0, 3], 1).unwrap();
        grid.set([3, 38, 3], 2).unwrap();
        grid.set([3, 39, 3], 2).unwrap();
        grid
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn street_lamp_parses_and_validates() {
        assert_eq!(lamp().validate(), Ok(()));
    }

    #[test]
    fn unknown_behaviour_fails_to_parse() {
        let json = LAMP.replace("\"light\": {", "\"teleport\": {");
        assert!(serde_json::from_str::<PropDef>(&json).is_err());
    }

    #[test]
    fn unknown_field_inside_a_behaviour_fails_to_parse() {
        let json = LAMP.replace("\"when\": \"night\"", "\"when\": \"night\", \"speed\": 2");
        assert!(serde_json::from_str::<PropDef>(&json).is_err());
    }

    #[test]
    fn behaviour_naming_a_missing_socket_gives_a_reason() {
        let mut def = lamp();
        if let Behaviour::Light(light) = &mut def.behaviours[0] {
            light.socket = "nope".to_string();
        }
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].field, "behaviours[0]");
        assert!(reasons[0].problem.contains("nope"));
    }

    #[test]
    fn validate_collects_every_problem() {
        let mut def = lamp();
        def.id = "Bad Id".to_string();
        def.pivot = [9, 0, 3];
        def.behaviours.push(Behaviour::Light(Light {
            socket: "light".to_string(),
            colour: "warm".to_string(),
            power: f32::NAN,
            when: When::State("broken".to_string()),
        }));
        let reasons = def.validate().unwrap_err();
        let fields: Vec<&str> = reasons.iter().map(|r| r.field.as_str()).collect();
        assert!(fields.contains(&"id"));
        assert!(fields.contains(&"pivot"));
        assert!(fields.contains(&"behaviours[1]"));
        assert_eq!(reasons.len(), 4);
    }

    #[test]
    fn readable_text_is_capped_and_has_no_control_characters() {
        let mut def = lamp();
        def.behaviours = vec![Behaviour::Readable(Readable {
            text: "x".repeat(MAX_TEXT + 1) + "\u{7}",
        })];
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 2);
    }

    #[test]
    fn socket_name_with_a_control_character_gives_a_reason() {
        let mut def = lamp();
        def.sockets[0].name = "li\u{7}ght".to_string();
        let reasons = def.validate().unwrap_err();
        assert!(
            reasons
                .iter()
                .any(|r| r.field == "sockets" && r.problem.contains("control"))
        );
    }

    #[test]
    fn part_name_over_the_cap_gives_a_reason() {
        let mut def = lamp();
        let part = def.parts.remove("lantern").unwrap();
        def.parts.insert("p".repeat(MAX_KEY + 1), part);
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].field, "parts");
    }

    #[test]
    fn duplicate_state_names_give_a_reason() {
        let mut def = lamp();
        def.states = vec!["off".to_string(), "off".to_string()];
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].field, "states");
    }

    #[test]
    fn part_with_too_many_voxels_gives_a_reason() {
        let mut def = lamp();
        def.parts.get_mut("lantern").unwrap().voxels = vec![[3, 38, 3]; MAX_PART_VOXELS + 1];
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].field, "parts.lantern");
    }

    #[test]
    fn empty_place_on_gives_a_reason() {
        let mut def = lamp();
        def.place.on.clear();
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].field, "place.on");
    }

    #[test]
    fn door_swing_past_180_degrees_gives_a_reason() {
        let mut def = lamp();
        def.behaviours.push(Behaviour::Door(Door {
            part: "lantern".to_string(),
            swing: 200.0,
        }));
        let reasons = def.validate().unwrap_err();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].field, "behaviours[1]");
    }

    #[test]
    fn version_is_stable_and_pinned() {
        let def = lamp();
        let grid = lamp_grid();
        let first = def.version(&grid).unwrap();
        assert_eq!(first, def.version(&grid).unwrap());
        assert_eq!(hex(&first), "823c58e73ca3608ca805375d5253762a");
    }

    #[test]
    fn version_changes_when_one_voxel_changes() {
        let def = lamp();
        let mut grid = lamp_grid();
        let before = def.version(&grid).unwrap();
        grid.set([3, 39, 3], 3).unwrap();
        assert_ne!(before, def.version(&grid).unwrap());
    }

    #[test]
    fn version_of_a_grid_of_another_size_is_an_error() {
        let def = lamp();
        let grid = VoxelGrid::new(PropScale::Fine.scale(), [7, 44, 6]).unwrap();
        let reason = def.version(&grid).unwrap_err();
        assert_eq!(reason.field, "size");
    }
}
