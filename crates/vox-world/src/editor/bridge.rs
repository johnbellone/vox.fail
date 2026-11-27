//! The editor bridge: the messages between the in-world editor and the page around the
//! client. [`Command`]s come in from the page as JSON text, [`Event`]s go back out. Both are
//! serde enums tagged by `type`, so the TypeScript types in `apps/site/src/game/editor.ts`
//! can mirror them exactly; `apps/site/src/game/editor.samples.json` is the written record of
//! one sample of every variant, and the test below fails when it no longer matches.
//!
//! Anything arriving as a command is untrusted: [`parse_command`] refuses it over the size
//! cap, before parsing, and rejects unknown fields, unknown types and components that fail
//! [`Component::validate`].

use serde::{Deserialize, Serialize};
use vox_core::entity::Component;

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

/// What the page asks the editor to do.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// hold this tool
    Tool { tool: Tool },
    /// paint with this block from the world's palette
    Block { name: String },
    /// select an entity, or nothing
    Select { entity: Option<u32> },
    /// make a new entity from a form; it is placed where the component says
    Create { component: Component },
    /// replace an entity's component; `expect` is the seq the page last saw
    Set {
        entity: u32,
        component: Component,
        expect: Option<u64>,
    },
    /// remove an entity; `expect` is the seq the page last saw
    Remove { entity: u32, expect: Option<u64> },
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
    /// a command was refused: its id, the reason's name and the sentence for it
    Refused { id: u32, why: String, text: String },
    /// the block the pointer is on, for forms that need a position
    Picked { cell: [i32; 3] },
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
        Command::Create { component } | Command::Set { component, .. } => {
            component.validate().map_err(|why| why.to_string())?;
        }
        _ => {}
    }
    Ok(command)
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
    use vox_core::entity::{Axis, Dest, Portal};

    const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/site/src/game/editor.samples.json");

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
            Command::Select { entity: Some(3) },
            Command::Select { entity: None },
            Command::Create { component: yard_portal() },
            Command::Set { entity: 3, component: yard_portal(), expect: Some(12) },
            Command::Set { entity: 3, component: yard_portal(), expect: None },
            Command::Remove { entity: 3, expect: Some(12) },
            Command::Remove { entity: 3, expect: None },
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
            Event::Refused { id: 7, why: "TooFar".into(), text: "that's out of reach".into() },
            Event::Picked { cell: [1, -2, 30] },
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
    fn unknown_types_are_refused() {
        assert!(parse_command(r#"{"type":"explode"}"#).is_err());
        assert!(parse_command(r#"{"type":"tool","tool":"hammer"}"#).is_err());
    }

    #[test]
    fn invalid_components_are_refused() {
        let bad = r#"{"type":"create","component":{"Spawn":{"at":[0,0,0],"yaw":1e999}}}"#;
        assert!(parse_command(bad).is_err());

        let objective = r#"{"type":"set","entity":1,"component":{"Objective":{"name":"altar","at":[0,0,0],"radius":0}},"expect":null}"#;
        assert_eq!(parse_command(objective), Err("that isn't a valid value".to_string()));
    }
}
