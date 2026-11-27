//! The author's undo and redo in the editor. Each change this client made that the server
//! applied is kept as a [`Step`]: the entity, its component before and after (None where it did
//! not exist), and the seq the entity has in the state the step sits at. Undo sends the step
//! back as a change of its own, expecting that seq, so a change someone else made since is
//! refused (`Changed`) rather than overwritten; redo sends it forward again. The rules are in
//! docs/design/world-editor.md (Undo). This is plain data and logic with no Bevy types, so it
//! is unit-tested on its own; the editor's systems send what it returns.

use std::collections::{BTreeMap, VecDeque};

use vox_core::entity::{Component, EntityId, EntityWhy};

use super::bridge::Command;

/// Most steps kept for undo. Redo can never hold more, since a step only moves between the two.
pub const MAX_STEPS: usize = 100;

/// Most edits sent and not yet answered that are kept. An answer that never comes (the link
/// dropped) must not grow the map without bound; the oldest is forgotten past this.
const MAX_SENT: usize = 256;

/// One change: the entity, its component before and after, and the seq the entity has in the
/// state the step sits at. On the undo stack that is the seq of the `after` state; on the redo
/// stack, the seq of the `before` state.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub entity: EntityId,
    pub before: Option<Component>,
    pub after: Option<Component>,
    pub seq: u64,
}

/// An edit this client sent and is waiting on.
#[derive(Debug, PartialEq)]
enum Sent {
    /// the author's own change: it becomes an undo step when the server applies it
    Own { before: Option<Component>, after: Option<Component> },
    /// an undo or redo, with the step it took: back on its stack if refused for a reason other
    /// than `Changed`
    Taken { step: Step, undo: bool },
}

/// The author's steps, and the edits on their way.
#[derive(Debug, Default)]
pub struct History {
    undo: VecDeque<Step>,
    redo: VecDeque<Step>,
    /// edits sent and not yet answered, by the client's number
    sent: BTreeMap<u32, Sent>,
    /// the number of the undo or redo on its way; only one at a time
    flight: Option<u32>,
}

/// Sets the seq of the newest step on `stack` that names `entity`.
fn stamp(stack: &mut VecDeque<Step>, entity: EntityId, seq: u64) {
    if let Some(step) = stack.iter_mut().rev().find(|s| s.entity == entity) {
        step.seq = seq;
    }
}

/// Brings the steps on `stack` that name `from` up to date with the entity now being `to` in the
/// state `state`. Only the newest of them, the one the next undo or redo from this stack takes,
/// is checked: it must show the entity in `state` on the side this stack reads (`after` on the
/// undo stack, `before` on the redo stack). If it does not, it is not the change the entity was
/// left in, and every step for the entity on this stack is dropped: taking any of them would
/// overwrite a change this client never saw. If it does, the steps are renamed to `to`, since a
/// re-created entity has a new id.
fn settle(stack: &mut VecDeque<Step>, from: EntityId, to: EntityId, state: &Option<Component>, undo_stack: bool) {
    let Some(newest) = stack.iter().rposition(|s| s.entity == from) else { return };
    let shown = if undo_stack { &stack[newest].after } else { &stack[newest].before };
    if shown != state {
        stack.retain(|s| s.entity != from);
    } else {
        for step in stack.iter_mut().filter(|s| s.entity == from) {
            step.entity = to;
        }
    }
}

/// The change that takes `entity` from the state `from` to the state `to` (None: it does not
/// exist), expecting `seq` where the entity exists already. Undoing a Create removes it;
/// undoing a Remove makes it again, under a new id the server gives.
pub fn transition(entity: EntityId, from: Option<Component>, to: Option<Component>, seq: u64) -> Option<Command> {
    match (from, to) {
        (Some(_), Some(component)) => Some(Command::Set { entity: entity.0, component, expect: Some(seq), r#ref: None }),
        (None, Some(component)) => Some(Command::Create { component, r#ref: None }),
        (Some(_), None) => Some(Command::Remove { entity: entity.0, expect: Some(seq), r#ref: None }),
        (None, None) => None,
    }
}

impl History {
    /// Notes an own change sent as `id`. It becomes a step when the server applies it, and is
    /// forgotten when the server refuses it.
    pub fn sent_own(&mut self, id: u32, before: Option<Component>, after: Option<Component>) {
        self.track(id, Sent::Own { before, after });
    }

    /// Whether an undo or redo is on its way.
    pub fn in_flight(&self) -> bool {
        self.flight.is_some()
    }

    /// Takes the newest step to undo (or to redo), numbered `id`, and returns the change that
    /// does it. None when there is nothing to take, or one is already on its way. The step
    /// waits with the edit until the server answers.
    pub fn take(&mut self, id: u32, undo: bool) -> Option<Command> {
        if self.flight.is_some() {
            return None;
        }
        loop {
            let newest = if undo { self.undo.back() } else { self.redo.back() }?;
            let (from, to) = if undo {
                (newest.after.clone(), newest.before.clone())
            } else {
                (newest.before.clone(), newest.after.clone())
            };
            let command = transition(newest.entity, from, to, newest.seq);
            let step = if undo { self.undo.pop_back() } else { self.redo.pop_back() }?;
            // a step with nothing to send is dropped, so it cannot block the older steps behind it
            let Some(command) = command else { continue };
            self.track(id, Sent::Taken { step, undo });
            self.flight = Some(id);
            return Some(command);
        }
    }

    /// The server applied edit `id`, now at `seq`. An own change becomes the newest undo step
    /// and clears redo, since a new change ends what could be redone. An undo or redo moves its
    /// step to the other stack. A Create that gave the entity a new id renames the steps that
    /// named the old one.
    pub fn done(&mut self, id: u32, entity: EntityId, seq: u64) {
        match self.sent.remove(&id) {
            Some(Sent::Own { before, after }) => {
                self.redo.clear();
                self.push_undo(Step { entity, before, after, seq });
            }
            Some(Sent::Taken { step, undo }) => {
                self.flight = None;
                // the entity is at `seq` now, in the state the step took it to. On each stack, the
                // next step naming it must chain from that state (see `settle`). On the stack the
                // step came from, the newest step that names it is the one whose state this is,
                // so its seq is updated too, or its own undo or redo would be refused as Changed.
                if undo {
                    // undone: the entity is at `before`, and the step's redo leaves it at `after`
                    settle(&mut self.undo, step.entity, entity, &step.before, true);
                    stamp(&mut self.undo, entity, seq);
                    settle(&mut self.redo, step.entity, entity, &step.after, false);
                    self.redo.push_back(Step { entity, seq, ..step });
                } else {
                    // redone: the entity is at `after`, and the step's undo leaves it at `before`
                    settle(&mut self.redo, step.entity, entity, &step.after, false);
                    stamp(&mut self.redo, entity, seq);
                    settle(&mut self.undo, step.entity, entity, &step.before, true);
                    self.push_undo(Step { entity, seq, ..step });
                }
            }
            None => {}
        }
    }

    /// The server refused edit `id`. An own change is simply not a step. A refused undo or redo
    /// goes back on its stack, unless the entity changed since (`Changed`): then the step is
    /// dropped, as the spec says, since undoing it would overwrite someone else's change.
    pub fn refused(&mut self, id: u32, why: EntityWhy) {
        if let Some(Sent::Taken { step, undo }) = self.sent.remove(&id) {
            self.flight = None;
            if why != EntityWhy::Changed {
                if undo {
                    self.push_undo(step);
                } else {
                    self.redo.push_back(step);
                }
            }
        }
    }

    /// A new world: none of the author's steps or edits apply to it.
    pub fn clear(&mut self) {
        *self = History::default();
    }

    /// The link dropped while an undo or redo was on its way: its answer will not come, so the
    /// step goes back on its stack and the undo or redo can be tried again. If the server did
    /// apply it after all, the step's seq is stale and its next try is refused as Changed.
    pub fn link_lost(&mut self) {
        let Some(id) = self.flight.take() else { return };
        if let Some(Sent::Taken { step, undo }) = self.sent.remove(&id) {
            if undo {
                self.push_undo(step);
            } else {
                self.redo.push_back(step);
            }
        }
    }

    fn push_undo(&mut self, step: Step) {
        self.undo.push_back(step);
        while self.undo.len() > MAX_STEPS {
            self.undo.pop_front();
        }
    }

    /// Records an edit as sent and waiting. Past `MAX_SENT` the oldest is forgotten, so an answer
    /// lost with a dropped link cannot grow the map; if that edit was the undo or redo on its way,
    /// the flight is freed too, so the author is not left with Busy.
    fn track(&mut self, id: u32, what: Sent) {
        self.sent.insert(id, what);
        while self.sent.len() > MAX_SENT {
            let Some((&oldest, _)) = self.sent.first_key_value() else { break };
            self.sent.remove(&oldest);
            if self.flight == Some(oldest) {
                self.flight = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::Spawn;

    fn at(x: f32) -> Component {
        Component::Spawn(Spawn { at: [x, 0.0, 0.0], yaw: 0.0 })
    }

    fn set(entity: u32, component: Component, seq: u64) -> Command {
        Command::Set { entity, component, expect: Some(seq), r#ref: None }
    }

    #[test]
    fn editor_undo_set_restores_the_before_value_and_redo_replays_it() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert_eq!(h.take(2, true), Some(set(4, at(1.0), 5)));
        assert!(h.in_flight());
        h.done(2, EntityId(4), 6);
        assert!(!h.in_flight());
        // the undone step waits on redo, at the seq the entity now has
        assert_eq!(h.take(3, false), Some(set(4, at(2.0), 6)));
        h.done(3, EntityId(4), 7);
        assert_eq!(h.take(4, true), Some(set(4, at(1.0), 7)));
    }

    #[test]
    fn editor_undo_create_is_undone_by_a_remove() {
        let mut h = History::default();
        h.sent_own(1, None, Some(at(3.0)));
        h.done(1, EntityId(9), 2);
        assert_eq!(h.take(2, true), Some(Command::Remove { entity: 9, expect: Some(2), r#ref: None }));
        h.done(2, EntityId(9), 3);
        // redo makes it again, under the id the server gives
        assert_eq!(h.take(3, false), Some(Command::Create { component: at(3.0), r#ref: None }));
    }

    #[test]
    fn editor_undo_remove_is_undone_by_a_create_of_the_component() {
        let mut h = History::default();
        h.sent_own(1, Some(at(4.0)), None);
        h.done(1, EntityId(7), 8);
        assert_eq!(h.take(2, true), Some(Command::Create { component: at(4.0), r#ref: None }));
        h.done(2, EntityId(11), 1);
        assert_eq!(h.take(3, false), Some(Command::Remove { entity: 11, expect: Some(1), r#ref: None }));
    }

    #[test]
    fn editor_undo_changed_drops_the_step() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert!(h.take(2, true).is_some());
        h.refused(2, EntityWhy::Changed);
        assert!(!h.in_flight());
        assert!(h.take(3, true).is_none());
        assert!(h.take(4, false).is_none());
    }

    #[test]
    fn editor_undo_other_refusal_keeps_the_step() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert!(h.take(2, true).is_some());
        h.refused(2, EntityWhy::Unavailable);
        assert_eq!(h.take(3, true), Some(set(4, at(1.0), 5)));
    }

    #[test]
    fn editor_undo_refused_own_change_is_no_step() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.refused(1, EntityWhy::TooFar);
        assert!(h.take(2, true).is_none());
    }

    #[test]
    fn editor_undo_one_at_a_time() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert!(h.take(2, true).is_some());
        // a second undo while the first is on its way takes nothing
        assert!(h.take(3, true).is_none());
    }

    #[test]
    fn editor_undo_new_change_clears_redo() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert!(h.take(2, true).is_some());
        h.done(2, EntityId(4), 6);
        h.sent_own(3, Some(at(2.0)), Some(at(5.0)));
        h.done(3, EntityId(4), 7);
        assert!(h.take(4, false).is_none());
        assert_eq!(h.take(5, true), Some(set(4, at(2.0), 7)));
    }

    #[test]
    fn editor_undo_keeps_at_most_100_steps() {
        let mut h = History::default();
        for n in 0..(MAX_STEPS as u32 + 5) {
            h.sent_own(n + 1, Some(at(0.0)), Some(at(n as f32)));
            h.done(n + 1, EntityId(1), n as u64 + 1);
        }
        assert_eq!(h.undo.len(), MAX_STEPS);
        // the oldest five were dropped: the first one left is the sixth change
        assert_eq!(h.undo.front().map(|s| s.after.clone()), Some(Some(at(5.0))));
    }

    #[test]
    fn editor_undo_reset_clears_both_stacks_and_the_flight() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert!(h.take(2, true).is_some());
        h.clear();
        assert!(!h.in_flight());
        assert!(h.take(3, true).is_none());
        // an answer for an edit from before the reset is nothing
        h.done(2, EntityId(4), 6);
        assert!(h.undo.is_empty() && h.redo.is_empty());
    }

    #[test]
    fn editor_undo_stepping_back_keeps_the_seq_current() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        h.sent_own(2, Some(at(2.0)), Some(at(3.0)));
        h.done(2, EntityId(4), 6);
        // undo the newer change at seq 6; the older one is then at seq 7, so its undo expects 7
        assert_eq!(h.take(3, true), Some(set(4, at(2.0), 6)));
        h.done(3, EntityId(4), 7);
        assert_eq!(h.take(4, true), Some(set(4, at(1.0), 7)));
        h.done(4, EntityId(4), 8);
        // redo the older change at seq 8; the newer one is then at seq 9
        assert_eq!(h.take(5, false), Some(set(4, at(2.0), 8)));
        h.done(5, EntityId(4), 9);
        assert_eq!(h.take(6, false), Some(set(4, at(3.0), 9)));
    }

    #[test]
    fn editor_undo_recreated_id_is_remapped() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        h.sent_own(2, Some(at(2.0)), None);
        h.done(2, EntityId(4), 6);
        // undoing the removal makes the entity again as 9: the earlier Set now names 9
        assert_eq!(h.take(3, true), Some(Command::Create { component: at(2.0), r#ref: None }));
        h.done(3, EntityId(9), 1);
        assert_eq!(h.take(4, true), Some(set(9, at(1.0), 1)));
    }

    #[test]
    fn editor_undo_a_step_whose_neighbour_moved_on_is_not_undoable() {
        // this client sets entity 4 from X to Y; another client sets it to Z (not seen here);
        // this client sets it from Z to W and undoes that. The older step would set X over Z.
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        h.sent_own(2, Some(at(3.0)), Some(at(4.0)));
        h.done(2, EntityId(4), 7);
        assert_eq!(h.take(3, true), Some(set(4, at(3.0), 7)));
        h.done(3, EntityId(4), 8);
        assert!(h.take(4, true).is_none());
        // the undone change is still redoable, at the seq the entity now has
        assert_eq!(h.take(5, false), Some(set(4, at(4.0), 8)));
    }

    #[test]
    fn editor_undo_redo_drops_a_neighbour_that_does_not_chain() {
        let mut h = History::default();
        // two undone steps on entity 4. The redo taken first leaves the entity at 2, but the
        // step behind it expects it to start from 9, so that one cannot be redone safely
        h.redo.push_back(Step { entity: EntityId(4), before: Some(at(9.0)), after: Some(at(10.0)), seq: 3 });
        h.redo.push_back(Step { entity: EntityId(4), before: Some(at(1.0)), after: Some(at(2.0)), seq: 5 });
        assert_eq!(h.take(1, false), Some(set(4, at(2.0), 5)));
        h.done(1, EntityId(4), 6);
        assert!(h.redo.is_empty());
        assert_eq!(h.take(2, true), Some(set(4, at(1.0), 6)));
    }

    #[test]
    fn editor_undo_recreated_id_drops_steps_that_do_not_chain_from_it() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        // the removal was of 3: another client changed the entity from the 2 that step left
        h.sent_own(2, Some(at(3.0)), None);
        h.done(2, EntityId(4), 6);
        assert_eq!(h.take(3, true), Some(Command::Create { component: at(3.0), r#ref: None }));
        h.done(3, EntityId(9), 1);
        assert!(h.take(4, true).is_none());
        assert_eq!(h.take(5, false), Some(Command::Remove { entity: 9, expect: Some(1), r#ref: None }));
    }

    #[test]
    fn editor_undo_a_step_with_nothing_to_send_does_not_block_older_ones() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        h.sent_own(2, None, None);
        h.done(2, EntityId(7), 3);
        assert_eq!(h.take(3, true), Some(set(4, at(1.0), 5)));
        h.done(3, EntityId(4), 6);
        assert!(h.take(4, true).is_none());
    }

    #[test]
    fn editor_undo_link_lost_puts_the_step_back() {
        let mut h = History::default();
        h.sent_own(1, Some(at(1.0)), Some(at(2.0)));
        h.done(1, EntityId(4), 5);
        assert!(h.take(2, true).is_some());
        h.link_lost();
        assert!(!h.in_flight());
        assert_eq!(h.take(3, true), Some(set(4, at(1.0), 5)));
    }

    #[test]
    fn editor_undo_unanswered_sends_are_capped() {
        let mut h = History::default();
        for n in 0..(MAX_SENT as u32 + 10) {
            h.sent_own(n + 1, None, Some(at(n as f32)));
        }
        assert_eq!(h.sent.len(), MAX_SENT);
        assert!(!h.sent.contains_key(&1));
        assert!(h.sent.contains_key(&(MAX_SENT as u32 + 10)));
    }
}
