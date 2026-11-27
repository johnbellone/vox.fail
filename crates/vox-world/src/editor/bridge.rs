//! The editor bridge: the messages between the in-world editor and the page around the
//! client. [`Command`]s come in from the page as JSON text, [`Event`]s go back out. Both are
//! serde enums tagged by `type`, so the TypeScript types in `apps/site/src/game/editor.ts`
//! can mirror them exactly; `apps/site/src/game/editor.samples.json` is the written record of
//! one sample of every variant, and the test below fails when it no longer matches.
//!
//! Anything arriving as a command is untrusted: [`parse_command`] refuses it over the size
//! cap, before parsing, and rejects unknown fields, unknown types and components that fail
//! [`Component::validate`].
//!
//! A command that changes an entity (`Create`, `Set`, `Remove`) may carry a `ref`: a number the
//! page chooses. The editor echoes it on the [`Event::Done`] that says the server applied the
//! edit and on the [`Event::Refused`] that says it did not, so the page can tell its edits
//! apart. Sending the same `ref` again after an `Unavailable` refusal is a retry: the client
//! resends the edit under the same number, so it is never charged twice.

use serde::{Deserialize, Serialize};
use vox_core::entity::{Axis, Component};

/// Largest command the bridge accepts, in bytes of JSON.
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;

/// A tool the editor can hold.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Select,
    Blocks,
    Portal,
    Prop,
    Node,
}

impl Tool {
    /// Whether a click with this tool is the tool's own: it picks or places something, so the
    /// click neither breaks a block nor walks the player to the point under the pointer.
    pub fn consumes_click(self) -> bool {
        matches!(self, Tool::Select | Tool::Portal | Tool::Prop)
    }
}

/// What the page asks the editor to do.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// hold this tool
    Tool { tool: Tool },
    /// paint with this block from the world's palette
    Block { name: String },
    /// choose the catalogue prop the prop tool places: its version hash, 32 hex characters
    Prop {
        #[serde(with = "prop_hash")]
        prop: [u8; 16],
    },
    /// select an entity, or nothing
    Select { entity: Option<u32> },
    /// make a new entity from a form; it is placed where the component says
    Create {
        component: Component,
        #[serde(rename = "ref", default)]
        r#ref: Option<u32>,
    },
    /// replace an entity's component; `expect` is the seq the page last saw
    Set {
        entity: u32,
        component: Component,
        expect: Option<u64>,
        #[serde(rename = "ref", default)]
        r#ref: Option<u32>,
    },
    /// remove an entity; `expect` is the seq the page last saw
    Remove {
        entity: u32,
        expect: Option<u64>,
        #[serde(rename = "ref", default)]
        r#ref: Option<u32>,
    },
    /// move the camera to an entity
    FlyTo { entity: u32 },
    /// undo the author's last change
    Undo {},
    /// redo the change undo took back
    Redo {},
    /// build a node
    Build { node: u32 },
}

/// One entity's change: its new component, or None when it was removed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Change {
    pub entity: u32,
    pub seq: u64,
    pub component: Option<Component>,
}

/// What the editor tells the page.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// the client is in edit mode in this world, with its block palette
    Ready { world: String, palette: Vec<String> },
    /// the entities as they are (the first list), then their changes
    Entities { changes: Vec<Change> },
    /// the selection changed
    Selected { entity: Option<u32> },
    /// a command was refused: the client's number for it (0 when it never got one), the
    /// page's `ref` for it if it had one, the reason's name and the sentence for it
    Refused {
        id: u32,
        #[serde(rename = "ref", default)]
        r#ref: Option<u32>,
        why: String,
        text: String,
    },
    /// the server applied an edit the page sent with a `ref`: the entity it made, changed or
    /// removed, and the seq that entity now has (the one it had, for a removal)
    Done {
        #[serde(rename = "ref")]
        r#ref: u32,
        entity: u32,
        seq: u64,
    },
    /// a click on a prop ghost the rules allow: the prop, where it would stand (its position in
    /// 5 cm units, on the top of the block the pointer is on) and its turn. The page makes the
    /// prop (a Create) from its form, which adds the variant, state and text.
    PropSpot {
        #[serde(with = "prop_hash")]
        prop: [u8; 16],
        at: [i32; 3],
        yaw: u8,
    },
    /// the block the pointer is on, for forms that need a position
    Picked { cell: [i32; 3] },
    /// a click on a doorway ghost the rules allow: where the doorway would be (its sill's middle),
    /// which way it faces and its size. The page makes the doorway (a Create) from its form.
    Spot { at: [f32; 3], axis: Axis, size: [f32; 2] },
    /// who else is in the world
    Here { players: Vec<String> },
}

/// Parses a command from the page. Refuses it over [`MAX_COMMAND_BYTES`] before reading it,
/// then parses it strictly, then checks any component it carries.
pub fn parse_command(text: &str) -> Result<Command, String> {
    if text.len() > MAX_COMMAND_BYTES {
        return Err(format!("command is over {MAX_COMMAND_BYTES} bytes"));
    }
    let command: Command = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match &command {
        Command::Create { component, .. } | Command::Set { component, .. } => {
            component.validate().map_err(|why| why.to_string())?;
        }
        _ => {}
    }
    Ok(command)
}

impl Command {
    /// The page's `ref` on this command, if it has one.
    pub fn page_ref(&self) -> Option<u32> {
        match self {
            Command::Create { r#ref, .. } | Command::Set { r#ref, .. } | Command::Remove { r#ref, .. } => *r#ref,
            _ => None,
        }
    }
}

/// The `ref` in command text that could not be used (over the cap or invalid), when it can
/// still be read, so the page hears which of its commands was refused. Best effort: None when
/// the text is not JSON, is over the cap, or has no number there.
pub fn ref_of(text: &str) -> Option<u32> {
    if text.len() > MAX_COMMAND_BYTES {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    u32::try_from(v.get("ref")?.as_u64()?).ok()
}

/// A prop's version hash on the bridge: 32 hex characters. Anything else is refused by
/// `Hash::from_hex`, so a bad hash never reaches the rules as a prop that might exist.
mod prop_hash {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use vox_format::Hash;

    pub fn serialize<S: Serializer>(prop: &[u8; 16], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&Hash(*prop).hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 16], D::Error> {
        let text = String::deserialize(d)?;
        Hash::from_hex(&text).map(|h| h.0).map_err(|e| D::Error::custom(e.to_string()))
    }
}

/// An event as the JSON text the page receives.
pub fn event_json(e: &Event) -> String {
    // Serialising these types cannot fail: every field is plain data, and non-finite floats
    // become null rather than an error.
    serde_json::to_string(e).expect("editor events always serialise")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::{Dest, Portal};

    const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/site/src/game/editor.samples.json");

    /// A catalogue prop's hash for the samples: 0102...10 in hex.
    const SAMPLE_PROP: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

    /// The yard's `void` doorway, as its level file has it.
    fn yard_portal() -> Component {
        Component::Portal(Portal {
            name: "void".into(),
            at: [0.0, 0.0, 11.5],
            axis: Axis::Z,
            size: [8.0, 16.0],
            yaw: 0.0,
            level: "void".into(),
            to: Dest::Url { url: ":7780".into(), arrive: [0.0, 0.0, -39.5] },
            key: None,
        })
    }

    fn every_command() -> Vec<Command> {
        vec![
            Command::Tool { tool: Tool::Select },
            Command::Tool { tool: Tool::Blocks },
            Command::Tool { tool: Tool::Portal },
            Command::Tool { tool: Tool::Prop },
            Command::Tool { tool: Tool::Node },
            Command::Block { name: "stone".into() },
            Command::Prop { prop: SAMPLE_PROP },
            Command::Select { entity: Some(3) },
            Command::Select { entity: None },
            Command::Create { component: yard_portal(), r#ref: None },
            Command::Create { component: yard_portal(), r#ref: Some(1) },
            Command::Set { entity: 3, component: yard_portal(), expect: Some(12), r#ref: Some(2) },
            Command::Set { entity: 3, component: yard_portal(), expect: None, r#ref: None },
            Command::Remove { entity: 3, expect: Some(12), r#ref: Some(3) },
            Command::Remove { entity: 3, expect: None, r#ref: None },
            Command::FlyTo { entity: 3 },
            Command::Undo {},
            Command::Redo {},
            Command::Build { node: 4 },
        ]
    }

    fn every_event() -> Vec<Event> {
        vec![
            Event::Ready { world: "yard".into(), palette: vec!["stone".into(), "grass".into()] },
            Event::Entities {
                changes: vec![
                    Change { entity: 3, seq: 12, component: Some(yard_portal()) },
                    Change { entity: 4, seq: 13, component: None },
                ],
            },
            Event::Selected { entity: Some(3) },
            Event::Selected { entity: None },
            Event::Refused { id: 7, r#ref: Some(2), why: "TooFar".into(), text: "that's out of reach".into() },
            Event::Refused { id: 0, r#ref: None, why: "NotYet".into(), text: "build is not in the editor yet".into() },
            Event::Done { r#ref: 2, entity: 3, seq: 13 },
            Event::PropSpot { prop: SAMPLE_PROP, at: [25, 30, -12], yaw: 3 },
            Event::Picked { cell: [1, -2, 30] },
            Event::Spot { at: [4.5, 3.0, -2.5], axis: Axis::X, size: [4.0, 6.0] },
            Event::Here { players: vec!["Sam".into()] },
        ]
    }

    fn samples_text() -> String {
        let commands: Vec<serde_json::Value> =
            every_command().iter().map(|c| serde_json::to_value(c).expect("serialises")).collect();
        let events: Vec<serde_json::Value> =
            every_event().iter().map(|e| serde_json::to_value(e).expect("serialises")).collect();
        let all = serde_json::json!({ "commands": commands, "events": events });
        let mut text = serde_json::to_string_pretty(&all).expect("serialises");
        text.push('\n');
        text
    }

    #[test]
    fn bridge_samples() {
        let text = samples_text();
        if std::env::var("UPDATE_SAMPLES").as_deref() == Ok("1") {
            std::fs::write(SAMPLES, &text).expect("writes the samples");
            return;
        }
        let on_disk = std::fs::read_to_string(SAMPLES).expect("reads the samples (run with UPDATE_SAMPLES=1 to write them)");
        assert!(on_disk == text, "editor.samples.json is out of date; run with UPDATE_SAMPLES=1");
    }

    #[test]
    fn every_command_round_trips_through_parse() {
        for c in every_command() {
            let text = serde_json::to_string(&c).expect("serialises");
            assert_eq!(parse_command(&text), Ok(c));
        }
    }

    #[test]
    fn every_event_round_trips() {
        for e in every_event() {
            let back: Event = serde_json::from_str(&event_json(&e)).expect("parses");
            assert_eq!(back, e);
        }
    }

    #[test]
    fn a_command_without_a_ref_parses_with_none() {
        let c = parse_command(r#"{"type":"remove","entity":3,"expect":null}"#).expect("parses");
        assert_eq!(c, Command::Remove { entity: 3, expect: None, r#ref: None });
        assert_eq!(c.page_ref(), None);
        let c = parse_command(r#"{"type":"remove","entity":3,"expect":null,"ref":9}"#).expect("parses");
        assert_eq!(c.page_ref(), Some(9));
    }

    #[test]
    fn ref_of_reads_a_refused_commands_ref() {
        assert_eq!(ref_of(r#"{"type":"explode","ref":5}"#), Some(5));
        assert_eq!(ref_of(r#"{"type":"explode"}"#), None);
        assert_eq!(ref_of(r#"{"ref":-1}"#), None);
        assert_eq!(ref_of("not json"), None);
    }

    #[test]
    fn over_the_cap_is_refused_before_parsing() {
        let padded = format!("{{\"type\":\"undo\"{}}}", " ".repeat(MAX_COMMAND_BYTES));
        assert!(parse_command(&padded).unwrap_err().contains("bytes"));
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(parse_command(r#"{"type":"select","entity":3,"extra":1}"#).is_err());
        assert!(parse_command(r#"{"type":"undo","extra":1}"#).is_err());
        assert!(parse_command(r#"{"type":"block","name":"stone","extra":1}"#).is_err());
    }

    #[test]
    fn a_prop_command_carries_its_hash_as_hex() {
        let text = r#"{"type":"prop","prop":"0102030405060708090a0b0c0d0e0f10"}"#;
        assert_eq!(parse_command(text), Ok(Command::Prop { prop: SAMPLE_PROP }));
    }

    #[test]
    fn a_bad_prop_hash_is_refused() {
        // too short, too long, not hex, and empty
        for bad in ["0102", "0102030405060708090a0b0c0d0e0f1011", "zz02030405060708090a0b0c0d0e0f10", ""] {
            let text = format!(r#"{{"type":"prop","prop":"{bad}"}}"#);
            assert!(parse_command(&text).is_err(), "{bad:?} should be refused");
        }
        // a number is not a hash either
        assert!(parse_command(r#"{"type":"prop","prop":16}"#).is_err());
    }

    #[test]
    fn a_prop_command_refuses_unknown_fields() {
        let text = r#"{"type":"prop","prop":"0102030405060708090a0b0c0d0e0f10","variant":1}"#;
        assert!(parse_command(text).is_err());
    }

    #[test]
    fn unknown_types_are_refused() {
        assert!(parse_command(r#"{"type":"explode"}"#).is_err());
        assert!(parse_command(r#"{"type":"tool","tool":"hammer"}"#).is_err());
    }

    #[test]
    fn the_select_and_portal_tools_take_the_click() {
        assert!(Tool::Select.consumes_click());
        assert!(Tool::Portal.consumes_click());
        // placing a prop takes its click too, so it does not break the block under the pointer
        assert!(Tool::Prop.consumes_click());
        assert!(!Tool::Blocks.consumes_click());
        assert!(!Tool::Node.consumes_click());
    }

    #[test]
    fn invalid_components_are_refused() {
        let bad = r#"{"type":"create","component":{"Spawn":{"at":[0,0,0],"yaw":1e999}}}"#;
        assert!(parse_command(bad).is_err());

        let objective = r#"{"type":"set","entity":1,"component":{"Objective":{"name":"altar","at":[0,0,0],"radius":0}},"expect":null}"#;
        assert_eq!(parse_command(objective), Err("that isn't a valid value".to_string()));
    }
}
