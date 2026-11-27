//! The in-world editor: the edit mode a player turns on in a world they may change. It is
//! made of the [`bridge`] (the messages between the page and the client), the [`camera`] (the
//! edit flight and fly-to), and, in later steps, picking, ghosts, gizmos and undo.
//!
//! [`EditorPlugin`] is added only when the client starts in edit mode. Its systems take the
//! page's commands from [`EditorInbox`], send the entity changes to the server, and put what
//! the page must hear (the world, the entities as they change, refusals, edits done) in
//! [`EditorOutbox`]. The page side (the window callbacks) is vox-client's, which moves the two
//! queues. The editor is a page feature: only the browser client adds it.
//!
//! # Telling the page an edit is done
//!
//! The server answers every Put or Remove it applies with [`ToClient::EntityDone`] (the edit's
//! number, the entity and the seq of the change), to the sender only, and every one it refuses
//! with [`ToClient::EntityRefused`]. An edit sent with a `ref` is tracked by its client number:
//! the echo becomes [`Event::Done`] or [`Event::Refused`] with that `ref`, and the edit is
//! forgotten (except after `Unavailable`, which keeps it so the page's retry reuses the number).

pub mod bridge;
pub mod camera;

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;
use vox_core::entity::{Component, EntityId, EntityWhy};
use vox_core::wire::ToServer;

use bridge::{Change, Command, Event, Tool};

use crate::client::{WorldClient, receive};
use crate::entities::WorldEntities;

/// Commands from the page, not yet acted on this frame.
#[derive(Resource, Default)]
pub struct EditorInbox(pub Vec<Command>);

/// Events for the page, to be sent this frame.
#[derive(Resource, Default)]
pub struct EditorOutbox(pub Vec<Event>);

/// Most edits tracked (by `ref`) at once; the oldest is forgotten past this.
const MAX_TRACKED: usize = 256;

/// An edit the page numbered, from sending to done or refused.
#[derive(Clone, Debug, PartialEq)]
struct Tracked {
    /// the page's `ref`
    page: u32,
    /// the client's number for it, kept across a retry so the server sees one edit
    id: u32,
    /// refused as Unavailable: the next command with this `ref` is a retry
    unavailable: bool,
}

/// What a command's `ref` says about how to send it.
#[derive(Debug, PartialEq)]
enum Slot {
    /// a new edit, with this client number
    Fresh(u32),
    /// a retry after `Unavailable`: the same number as before
    Again(u32),
    /// the same `ref` is still in flight: nothing to send, its answer is coming
    InFlight,
}

/// What the editor holds: the tool, the block to paint with, the selection, a camera request
/// for the next step to take, the number the next server message carries, what the page has
/// been told of the entities, and the edits it is waiting on.
#[derive(Resource, Default)]
pub struct EditorState {
    pub tool: Option<Tool>,
    pub block: Option<String>,
    pub selected: Option<u32>,
    /// an entity the camera should fly to; taken by the edit camera
    pub fly_to: Option<u32>,
    /// the id of the next Put or Remove sent (0 is never used: it stands for "no command")
    pub next_id: u32,
    /// the commands not built yet were logged (once, not for each)
    noted: bool,
    /// the seq of each entity as the page last heard it
    sent: BTreeMap<EntityId, u64>,
    /// the world has said Ready: nothing is reported to the page before it, so the first report
    /// after it is the whole list (every entity arrives before Ready)
    live: bool,
    /// Ready just came: the next change report is a full snapshot, even when empty
    snapshot_due: bool,
    /// edits sent with a `ref`, oldest first
    tracked: VecDeque<Tracked>,
}

impl EditorState {
    /// A new world (after a Welcome): what the page heard and what it waits on is about the
    /// last world, so forget it. The entities arrive again and are all reported as new.
    pub(crate) fn reset(&mut self) {
        self.sent.clear();
        self.tracked.clear();
        self.live = false;
        self.snapshot_due = false;
    }

    /// The world says it has sent everything it has: tell the page the whole list next.
    pub(crate) fn ready(&mut self) {
        self.live = true;
        self.snapshot_due = true;
    }

    fn fresh_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id.wrapping_add(1);
        id
    }

    /// Decides the client number for an entity command and starts tracking it when the page
    /// gave it a `ref`. A `ref` seen before and refused as Unavailable is a retry: same number.
    fn slot(&mut self, page: Option<u32>) -> Slot {
        let Some(page) = page else { return Slot::Fresh(self.fresh_id()) };
        if let Some(t) = self.tracked.iter_mut().find(|t| t.page == page) {
            if !t.unavailable {
                return Slot::InFlight;
            }
            t.unavailable = false;
            return Slot::Again(t.id);
        }
        let id = self.fresh_id();
        if self.tracked.len() >= MAX_TRACKED {
            self.tracked.pop_front();
        }
        self.tracked.push_back(Tracked { page, id, unavailable: false });
        Slot::Fresh(id)
    }

    /// The event for a refusal of the edit numbered `id`. Unavailable keeps the edit tracked, so
    /// the page's retry reuses its number; any other reason ends it.
    pub(crate) fn refused(&mut self, id: u32, why: EntityWhy) -> Event {
        let mut page = None;
        if let Some(i) = self.tracked.iter().position(|t| t.id == id) {
            page = Some(self.tracked[i].page);
            if why == EntityWhy::Unavailable {
                self.tracked[i].unavailable = true;
            } else {
                self.tracked.remove(i);
            }
        }
        Event::Refused { id, r#ref: page, why: format!("{why:?}"), text: why.to_string() }
    }

    /// The server applied the edit numbered `id`: the page's `Done` for it, if the page gave it
    /// a `ref` (an edit without one, or one forgotten, has nobody waiting). The edit is
    /// forgotten.
    pub(crate) fn done(&mut self, id: u32, entity: EntityId, seq: u64) -> Option<Event> {
        let i = self.tracked.iter().position(|t| t.id == id)?;
        let t = self.tracked.remove(i)?;
        Some(Event::Done { r#ref: t.page, entity: entity.0, seq })
    }
}

/// The editor's systems, so the page's callbacks can run before and after them.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EditorSystems;

/// Adds the editor. Add it only in edit mode: without it there are no queues and nothing is
/// sent to the page.
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorInbox>()
            .init_resource::<EditorOutbox>()
            .init_resource::<EditorState>()
            .init_resource::<camera::EditFly>()
            .add_systems(Update, (apply_commands, send_changes).chain().in_set(EditorSystems).after(receive))
            .add_systems(Update, camera::fly_to.after(EditorSystems).before(crate::client::controls));
    }
}

/// The server message a command makes, if it changes an entity: `Create`, `Set` or `Remove`.
/// `id` is the client's number for the message, echoed back on a refusal.
pub fn to_server(command: &Command, id: u32) -> Option<ToServer> {
    match command {
        Command::Create { component, .. } => {
            Some(ToServer::Put { id, entity: None, expect: None, component: component.clone() })
        }
        Command::Set { entity, component, expect, .. } => {
            Some(ToServer::Put { id, entity: Some(EntityId(*entity)), expect: *expect, component: component.clone() })
        }
        Command::Remove { entity, expect, .. } => {
            Some(ToServer::Remove { id, entity: EntityId(*entity), expect: *expect })
        }
        _ => None,
    }
}

/// What changed since the last call, as the page hears it: each entity whose seq differs from
/// the one in `sent` (and `sent` is updated), then a removal (`component: None`, with the last
/// seq seen) for each entity that is no longer in `now`. The removal's seq is the one the page
/// had, since the server's own seq for a removal is not kept.
pub fn diff_entities(sent: &mut BTreeMap<EntityId, u64>, now: &BTreeMap<EntityId, (u64, Component)>) -> Vec<Change> {
    let mut changes = Vec::new();
    for (id, (seq, component)) in now {
        if sent.get(id) != Some(seq) {
            sent.insert(*id, *seq);
            changes.push(Change { entity: id.0, seq: *seq, component: Some(component.clone()) });
        }
    }
    let gone: Vec<EntityId> = sent.keys().filter(|id| !now.contains_key(id)).copied().collect();
    for id in gone {
        if let Some(seq) = sent.remove(&id) {
            changes.push(Change { entity: id.0, seq, component: None });
        }
    }
    changes
}

/// Acts on the page's commands in the order it sent them. Tool, block, selection and the
/// camera request are kept for the steps that use them; entity changes go to the server, or
/// are refused to the page when there is no link to send them on (`Offline`). Build is not in
/// the editor until phase 5 and is refused (`NotYet`).
fn apply_commands(
    mut inbox: ResMut<EditorInbox>,
    mut state: ResMut<EditorState>,
    mut outbox: ResMut<EditorOutbox>,
    w: ResMut<WorldClient>,
) {
    for command in inbox.0.drain(..) {
        let page = command.page_ref();
        match &command {
            Command::Tool { tool } => {
                state.tool = Some(*tool);
                continue;
            }
            Command::Block { name } => {
                state.block = Some(name.clone());
                continue;
            }
            Command::Select { entity } => {
                state.selected = *entity;
                continue;
            }
            Command::FlyTo { entity } => {
                state.fly_to = Some(*entity);
                continue;
            }
            Command::Undo {} | Command::Redo {} => {
                if !state.noted {
                    state.noted = true;
                    crate::note!("[editor] undo and redo are not in the editor yet");
                }
                continue;
            }
            Command::Build { .. } => {
                outbox.0.push(Event::Refused {
                    id: 0,
                    r#ref: None,
                    why: "NotYet".into(),
                    text: "building nodes is not in the editor yet".into(),
                });
                continue;
            }
            Command::Create { .. } | Command::Set { .. } | Command::Remove { .. } => {}
        }
        if !w.linked() {
            outbox.0.push(Event::Refused {
                id: 0,
                r#ref: page,
                why: "Offline".into(),
                text: "the world isn't connected right now".into(),
            });
            continue;
        }
        let id = match state.slot(page) {
            Slot::Fresh(id) | Slot::Again(id) => id,
            Slot::InFlight => continue,
        };
        if let Some(msg) = to_server(&command, id) {
            w.send_entity(&msg);
        }
    }
}

/// Sends the page the entities that changed since it last heard of them. Nothing goes before
/// the world's Ready (every entity arrives before it), so the first report after it is the
/// whole list, and it always goes, even when the world has no entities.
fn send_changes(entities: Res<WorldEntities>, mut state: ResMut<EditorState>, mut outbox: ResMut<EditorOutbox>) {
    if !state.live || (!entities.is_changed() && !state.snapshot_due) {
        return;
    }
    let changes = diff_entities(&mut state.sent, &entities.map);
    let snapshot = std::mem::take(&mut state.snapshot_due);
    if changes.is_empty() && !snapshot {
        return;
    }
    outbox.0.push(Event::Entities { changes });
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::Spawn;

    fn spawn(x: f32) -> Component {
        Component::Spawn(Spawn { at: [x, 0.0, 0.0], yaw: 0.0 })
    }

    #[test]
    fn editor_diff_sends_only_what_changed() {
        let mut sent = BTreeMap::new();
        let mut now = BTreeMap::new();
        now.insert(EntityId(1), (5, spawn(1.0)));
        now.insert(EntityId(2), (6, spawn(2.0)));
        let first = diff_entities(&mut sent, &now);
        assert_eq!(first.len(), 2);

        // nothing changed: nothing sent
        assert!(diff_entities(&mut sent, &now).is_empty());

        // one moved (new seq), one removed
        now.insert(EntityId(1), (9, spawn(3.0)));
        now.remove(&EntityId(2));
        let next = diff_entities(&mut sent, &now);
        assert_eq!(
            next,
            vec![
                Change { entity: 1, seq: 9, component: Some(spawn(3.0)) },
                Change { entity: 2, seq: 6, component: None },
            ]
        );
        assert_eq!(sent.len(), 1);
    }

    #[test]
    fn editor_set_becomes_put_with_its_fields() {
        let cmd = Command::Set { entity: 4, component: spawn(7.0), expect: Some(12), r#ref: None };
        match to_server(&cmd, 3) {
            Some(ToServer::Put { id, entity, expect, component }) => {
                assert_eq!(id, 3);
                assert_eq!(entity, Some(EntityId(4)));
                assert_eq!(expect, Some(12));
                assert_eq!(component, spawn(7.0));
            }
            other => panic!("expected a Put, got {other:?}"),
        }
    }

    #[test]
    fn editor_create_and_remove_map_to_server_messages() {
        match to_server(&Command::Create { component: spawn(0.0), r#ref: None }, 1) {
            Some(ToServer::Put { id, entity, expect, .. }) => {
                assert_eq!((id, entity, expect), (1, None, None));
            }
            other => panic!("expected a Put, got {other:?}"),
        }
        match to_server(&Command::Remove { entity: 5, expect: None, r#ref: None }, 2) {
            Some(ToServer::Remove { id, entity, expect }) => {
                assert_eq!((id, entity, expect), (2, EntityId(5), None));
            }
            other => panic!("expected a Remove, got {other:?}"),
        }
        assert!(to_server(&Command::Tool { tool: Tool::Select }, 1).is_none());
        assert!(to_server(&Command::Undo {}, 1).is_none());
    }

    #[test]
    fn editor_retry_after_unavailable_reuses_the_client_id() {
        let mut st = EditorState::default();
        let Slot::Fresh(id) = st.slot(Some(7)) else { panic!("first send is fresh") };
        // still in flight: a duplicate sends nothing
        assert_eq!(st.slot(Some(7)), Slot::InFlight);
        let ev = st.refused(id, EntityWhy::Unavailable);
        assert_eq!(ev, Event::Refused { id, r#ref: Some(7), why: "Unavailable".into(), text: EntityWhy::Unavailable.to_string() });
        assert_eq!(st.slot(Some(7)), Slot::Again(id));
        // any other refusal ends it: the next use of the ref is a new edit
        st.refused(id, EntityWhy::TooFar);
        assert!(matches!(st.slot(Some(7)), Slot::Fresh(n) if n != id));
        // without a ref every send is fresh and a refusal carries none
        let Slot::Fresh(a) = st.slot(None) else { panic!() };
        assert_eq!(st.refused(a, EntityWhy::Blocked), Event::Refused { id: a, r#ref: None, why: "Blocked".into(), text: EntityWhy::Blocked.to_string() });
    }

    #[test]
    fn editor_tracking_is_bounded() {
        let mut st = EditorState::default();
        for r in 0..(MAX_TRACKED as u32 + 10) {
            st.slot(Some(r));
        }
        assert_eq!(st.tracked.len(), MAX_TRACKED);
        assert_eq!(st.tracked.front().map(|t| t.page), Some(10));
    }

    #[test]
    fn editor_done_is_the_servers_echo_for_the_page_ref() {
        let mut st = EditorState::default();
        let Slot::Fresh(a) = st.slot(Some(1)) else { panic!() };
        let Slot::Fresh(b) = st.slot(Some(2)) else { panic!() };
        let Slot::Fresh(c) = st.slot(None) else { panic!() };
        assert_eq!(st.done(b, EntityId(9), 22), Some(Event::Done { r#ref: 2, entity: 9, seq: 22 }));
        // forgotten once heard; an edit with no ref has nobody to tell
        assert_eq!(st.done(b, EntityId(9), 22), None);
        assert_eq!(st.done(c, EntityId(1), 5), None);
        // a retry after Unavailable keeps the number, and its echo still finds the ref
        st.refused(a, EntityWhy::Unavailable);
        assert_eq!(st.slot(Some(1)), Slot::Again(a));
        assert_eq!(st.done(a, EntityId(4), 21), Some(Event::Done { r#ref: 1, entity: 4, seq: 21 }));
        assert!(st.tracked.is_empty());
    }

    #[test]
    fn editor_a_refusal_with_no_tracked_edit_has_no_ref() {
        let mut st = EditorState::default();
        assert_eq!(st.refused(99, EntityWhy::Changed), Event::Refused { id: 99, r#ref: None, why: "Changed".into(), text: EntityWhy::Changed.to_string() });
    }

    #[test]
    fn editor_reset_forgets_the_last_world() {
        let mut st = EditorState::default();
        st.slot(Some(1));
        st.sent.insert(EntityId(4), 3);
        st.reset();
        assert!(st.sent.is_empty() && st.tracked.is_empty() && !st.snapshot_due && !st.live);
        st.ready();
        assert!(st.snapshot_due && st.live);
    }
}
