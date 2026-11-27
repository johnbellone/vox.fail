//! World entities on the client: the things placed in a world beside its blocks (doorways,
//! spawn points, an objective, the settings), as the server sends them. Kept by id with the
//! edit number of each one's latest change. The portal entities become the doorways
//! [`crate::portals`] draws and walks through.

use std::collections::BTreeMap;

use bevy::prelude::*;
use vox_core::entity::{Axis, Component, Dest, EntityId};
use vox_core::wire::EntityChange;

use crate::portals::PortalDef;

/// The entities of the world this client is in.
#[derive(Resource, Default)]
pub struct WorldEntities {
    pub map: BTreeMap<EntityId, (u64, Component)>,
    /// a portal entity was added, changed or removed since the doorways were last built
    pub portals_changed: bool,
    /// a Settings entity was added, changed or removed since the receive step last looked; it
    /// clears this there and applies the new settings (`WorldClient::apply_live_settings`)
    pub settings_changed: bool,
}

/// Whether a component is a portal, and whether it is the settings (the two change flags).
fn kinds(c: Option<&Component>) -> (bool, bool) {
    (matches!(c, Some(Component::Portal(_))), matches!(c, Some(Component::Settings(_))))
}

impl WorldEntities {
    /// Takes in the server's changes in the order it sent them. A component that fails its
    /// checks is ignored, since it came from the network. A change older than the entity's
    /// current one is ignored too: the server sends everything from one thread, in order, so
    /// this only guards against a stale change for an entity that is still here. There are no
    /// tombstones, so a removed entity is not protected: a late change for it would bring it
    /// back, and the server's order is what keeps that from happening.
    pub fn apply(&mut self, changes: &[EntityChange]) {
        for c in changes {
            if self.map.get(&c.entity).is_some_and(|(seq, _)| *seq > c.seq) {
                continue;
            }
            let new = match &c.component {
                Some(comp) => {
                    if comp.validate().is_err() {
                        continue;
                    }
                    Some(comp)
                }
                None => None,
            };
            // a change touches a flag when the component it replaces or removes, or the one
            // it adds, is of that kind
            let (was_portal, was_settings) = kinds(self.map.get(&c.entity).map(|(_, old)| old));
            let (is_portal, is_settings) = kinds(new);
            self.portals_changed |= was_portal || is_portal;
            self.settings_changed |= was_settings || is_settings;
            match new {
                Some(comp) => {
                    self.map.insert(c.entity, (c.seq, comp.clone()));
                }
                None => {
                    self.map.remove(&c.entity);
                }
            }
        }
    }

    /// Forgets every entity: a new world (or the same one again after travel) sends its own.
    pub fn clear(&mut self) {
        self.map.clear();
        self.portals_changed = true;
        self.settings_changed = true;
    }

    /// The Settings entity's json, parsed. A world has one Settings entity; if it ever had
    /// more, the first by id is used. `None` when the world has no settings entity, or its
    /// text does not parse (the checks in `apply` already refuse that).
    pub fn settings_json(&self) -> Option<serde_json::Value> {
        self.map.values().find_map(|(_, c)| match c {
            Component::Settings(s) => serde_json::from_str::<serde_json::Value>(&s.json).ok(),
            _ => None,
        })
    }

    /// The portal entities as doorways. The server decides where a traveller really arrives,
    /// and what a pair or a dungeon leads to. The client only needs the doorway's place and
    /// its name (it sends `Through` with the name), so the `url` of a destination the server
    /// resolves is a label here, and `arrive` is the doorway's own sill for those. The
    /// `pair:<key>` and `dungeon:<name>` urls are labels, not addresses: normal play sends
    /// `Through` by name and the server resolves the travel, so nothing here dials them.
    pub fn portal_defs(&self) -> Vec<PortalDef> {
        self.map
            .values()
            .filter_map(|(_, c)| {
                let Component::Portal(p) = c else { return None };
                let (url, arrive) = match &p.to {
                    Dest::Here { arrive } => (None, *arrive),
                    Dest::Url { url, arrive } => (Some(url.clone()), *arrive),
                    Dest::World { world } => (Some(format!("/w/{world}/ws")), p.at),
                    Dest::Dungeon { name } => (Some(format!("dungeon:{name}")), p.at),
                    Dest::Pair { key } => (Some(format!("pair:{key}")), p.at),
                };
                Some(PortalDef {
                    name: p.name.clone(),
                    at: Vec3::from_array(p.at),
                    along_x: p.axis == Axis::X,
                    size: Vec2::from_array(p.size),
                    level: p.level.clone(),
                    url,
                    arrive: Vec3::from_array(arrive),
                    yaw: p.yaw.to_radians(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::{Objective, Portal, Settings, Spawn};

    fn portal(to: Dest) -> Component {
        Component::Portal(Portal {
            name: "yard".into(),
            at: [0.0, 0.0, -39.5],
            axis: Axis::Z,
            size: [8.0, 16.0],
            yaw: 0.0,
            level: "yard".into(),
            to,
            key: None,
        })
    }

    fn change(seq: u64, entity: u32, component: Option<Component>) -> EntityChange {
        EntityChange { seq, entity: EntityId(entity), component }
    }

    #[test]
    fn apply_adds_replaces_and_removes() {
        let mut e = WorldEntities::default();
        let spawn = Component::Spawn(Spawn { at: [1.0, 2.0, 3.0], yaw: 0.0 });
        e.apply(&[change(10, 1, Some(spawn.clone()))]);
        assert_eq!(e.map.get(&EntityId(1)), Some(&(10, spawn)));

        // a newer change replaces; an older one does not
        let moved = Component::Spawn(Spawn { at: [4.0, 5.0, 6.0], yaw: 90.0 });
        e.apply(&[change(12, 1, Some(moved.clone())), change(11, 1, None)]);
        assert_eq!(e.map.get(&EntityId(1)), Some(&(12, moved)));

        e.apply(&[change(13, 1, None)]);
        assert!(e.map.is_empty());
        e.clear();
        assert!(e.portals_changed && e.settings_changed);
    }

    #[test]
    fn invalid_component_is_ignored() {
        let mut e = WorldEntities::default();
        let bad = Component::Spawn(Spawn { at: [f32::NAN, 0.0, 0.0], yaw: 0.0 });
        e.apply(&[change(1, 1, Some(bad))]);
        assert!(e.map.is_empty());
        assert!(!e.portals_changed && !e.settings_changed);
    }

    #[test]
    fn spawn_change_leaves_portals_alone() {
        let mut e = WorldEntities::default();
        let spawn = Component::Spawn(Spawn { at: [1.0, 2.0, 3.0], yaw: 0.0 });
        e.apply(&[change(1, 1, Some(spawn.clone()))]);
        assert!(!e.portals_changed);
        assert!(!e.settings_changed);
        e.apply(&[change(2, 1, None)]);
        assert!(!e.portals_changed);
    }

    #[test]
    fn portal_change_sets_portals_changed() {
        let mut e = WorldEntities::default();
        e.apply(&[change(1, 1, Some(portal(Dest::Here { arrive: [0.0, 0.0, 11.5] })))]);
        assert!(e.portals_changed);
        assert!(!e.settings_changed);

        // removing a portal counts too
        e.portals_changed = false;
        e.apply(&[change(2, 1, None)]);
        assert!(e.portals_changed);
    }

    #[test]
    fn settings_change_sets_settings_changed_only() {
        let mut e = WorldEntities::default();
        e.apply(&[change(1, 1, Some(Component::Settings(Settings { json: "{}".into() })))]);
        assert!(e.settings_changed);
        assert!(!e.portals_changed);
    }

    #[test]
    fn settings_live() {
        let mut e = WorldEntities::default();
        assert!(e.settings_json().is_none());

        e.apply(&[change(1, 1, Some(Component::Settings(Settings { json: r#"{"sky":"earth"}"#.into() })))]);
        assert!(e.settings_changed);
        assert_eq!(e.settings_json().unwrap()["sky"], "earth");

        // a later change replaces the settings, and sets the flag again
        e.settings_changed = false;
        e.apply(&[change(2, 1, Some(Component::Settings(Settings { json: r#"{"sky":"dusk"}"#.into() })))]);
        assert!(e.settings_changed);
        assert_eq!(e.settings_json().unwrap()["sky"], "dusk");

        // removed: nothing to apply
        e.apply(&[change(3, 1, None)]);
        assert!(e.settings_json().is_none());
    }

    #[test]
    fn portal_defs_map_each_destination() {
        let mut e = WorldEntities::default();
        let arrive = [0.0, 0.0, 11.5];
        e.apply(&[
            change(1, 1, Some(portal(Dest::Here { arrive }))),
            change(2, 2, Some(portal(Dest::Url { url: ":7781".into(), arrive }))),
            change(3, 3, Some(portal(Dest::World { world: "yard".into() }))),
            change(4, 4, Some(portal(Dest::Dungeon { name: "crypt".into() }))),
            change(5, 5, Some(portal(Dest::Pair { key: 7 }))),
            change(6, 6, Some(Component::Objective(Objective { name: "altar".into(), at: [0.0; 3], radius: 3.0 }))),
        ]);
        let defs = e.portal_defs();
        assert_eq!(defs.len(), 5);

        assert_eq!(defs[0].url, None);
        assert_eq!(defs[0].arrive, Vec3::from_array(arrive));
        assert_eq!(defs[1].url.as_deref(), Some(":7781"));
        assert_eq!(defs[1].arrive, Vec3::from_array(arrive));
        assert_eq!(defs[2].url.as_deref(), Some("/w/yard/ws"));
        assert_eq!(defs[2].arrive, Vec3::from_array([0.0, 0.0, -39.5]));
        assert_eq!(defs[3].url.as_deref(), Some("dungeon:crypt"));
        assert_eq!(defs[4].url.as_deref(), Some("pair:7"));

        assert_eq!(defs[0].name, "yard");
        assert!(!defs[0].along_x);
        assert_eq!(defs[0].size, Vec2::new(8.0, 16.0));
        assert_eq!(defs[0].level, "yard");
    }
}
