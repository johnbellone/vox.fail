//! What each doorway shows about where it leads (ADR 0004 decisions 11 and 12): open, waking,
//! unlinked or restricted, decided by the world from what it already knows.
//!
//! - Where the doorway leads ([`Dest`]): somewhere in this world, a world by name, a raid
//!   party's dungeon, or a way the directory hasn't given (or gave as nowhere).
//! - Who is looking ([`Who`]): signed in or not, in a raid party or not. Restrictions are per
//!   player, so states are sent to each player apart.
//! - What the host knows ([`Ways`]): `vox-host` says whether a world by that name is running,
//!   starting, stopped, or not on this host, and can wake a stopped one. A plain `vox-server`
//!   has no host to ask, so its doorways to other worlds stay [`DoorState::Unknown`] (today's
//!   look).
//!
//! A state describes only the doorway's own destination, from what the world knows anyway, so
//! it can't be used to probe for other worlds; a restriction's reason is general.

use vox_core::travel::{DoorState, Restriction};

/// A world as its host sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Running,
    /// starting now (opening its database, loading its chunks)
    Starting,
    /// stopped, and started again when asked for (an idle world, an atlas tile not yet made)
    Stopped,
    /// not on this host: no such world, or an instance that is over
    Absent,
}

/// What the host knows about its worlds. `vox-host` gives its worlds one; tests give a fake.
pub trait Ways: Send + Sync {
    /// The world called `name` (an id or an alias such as its port).
    fn way(&self, name: &str) -> Way;
    /// Start the world called `name` if it is stopped (returns at once; the start runs on).
    fn wake(&self, name: &str);
}

/// Where a doorway leads, as far as this world knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dest {
    /// somewhere else in this world
    Here,
    /// another world, by the name in its address (`/w/<name>/`, or the port)
    World(String),
    /// the traveller's raid party's own copy of a dungeon
    Dungeon,
    /// a world doorway the directory hasn't routed yet
    Unrouted,
    /// a world doorway the directory routed to nowhere (its world is gone)
    Nowhere,
}

/// Who is looking at the doorway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Who {
    pub signed_in: bool,
    pub in_party: bool,
}

/// What tells worlds apart in an address: the `<x>` of `/w/<x>/`, else the port (the same rule
/// as the directory's `way`, so `":7781"` and `"/w/7781/ws"` name one world).
pub fn name_of(url: &str) -> Option<String> {
    let u = url.trim();
    if let Some(i) = u.find("/w/") {
        let seg = u[i + 3..].split('/').next().unwrap_or("");
        return (!seg.is_empty() && seg.len() <= 64).then(|| seg.to_string());
    }
    let authority = u.split_once("://").map_or(u, |(_, r)| r).split('/').next().unwrap_or("");
    let port = authority.rsplit_once(':').map(|(_, p)| p)?;
    (!port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())).then(|| port.to_string())
}

/// The doorway's state for `who`, in a world that needs sign-in when `sign_in` is set. Wakes a
/// stopped destination as it is looked at (its start then hides behind the approach), so a
/// stopped world shows as waking.
pub fn state(dest: &Dest, who: Who, sign_in: bool, ways: Option<&dyn Ways>) -> DoorState {
    match dest {
        Dest::Here => DoorState::Open,
        Dest::Unrouted => DoorState::Unknown,
        Dest::Nowhere => DoorState::Unlinked,
        // a dungeon's way is given only to a party (the directory refuses the ticket otherwise);
        // a world without sign-in has no directory and no parties to check
        Dest::Dungeon if !sign_in => DoorState::Unknown,
        Dest::Dungeon if !who.signed_in => DoorState::Restricted(Restriction::SignIn),
        Dest::Dungeon if !who.in_party => DoorState::Restricted(Restriction::Party),
        Dest::Dungeon => DoorState::Open,
        Dest::World(name) => {
            // another world of a signed-in directory lets in only signed-in players (a guest
            // gets no ticket)
            if sign_in && !who.signed_in {
                return DoorState::Restricted(Restriction::SignIn);
            }
            let Some(ways) = ways else { return DoorState::Unknown };
            match ways.way(name) {
                Way::Running => DoorState::Open,
                Way::Starting => DoorState::Waking,
                Way::Stopped => {
                    ways.wake(name);
                    DoorState::Waking
                }
                Way::Absent => DoorState::Unlinked,
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A host for tests: worlds by name, and the names woken.
    #[derive(Default)]
    pub(crate) struct FakeHost {
        pub worlds: Mutex<HashMap<String, Way>>,
        pub woken: Mutex<Vec<String>>,
    }

    impl Ways for FakeHost {
        fn way(&self, name: &str) -> Way {
            *self.worlds.lock().unwrap_or_else(|e| e.into_inner()).get(name).unwrap_or(&Way::Absent)
        }
        fn wake(&self, name: &str) {
            self.woken.lock().unwrap_or_else(|e| e.into_inner()).push(name.to_string());
            self.worlds.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), Way::Starting);
        }
    }

    const ANN: Who = Who { signed_in: true, in_party: false };

    #[test]
    fn names_come_from_the_path_or_the_port() {
        assert_eq!(name_of("/w/yard/ws").as_deref(), Some("yard"));
        assert_eq!(name_of("wss://vox.fail/w/atlas-big-1-2/ws").as_deref(), Some("atlas-big-1-2"));
        assert_eq!(name_of(":7781").as_deref(), Some("7781"));
        assert_eq!(name_of("ws://127.0.0.1:7782/ws").as_deref(), Some("7782"));
        assert_eq!(name_of("nowhere"), None);
        assert_eq!(name_of("/w//ws"), None);
        assert_eq!(name_of(&format!("/w/{}/ws", "x".repeat(65))), None, "too long a name");
    }

    #[test]
    fn a_world_doorway_shows_what_its_host_knows() {
        let host = FakeHost::default();
        host.worlds.lock().unwrap().extend([("yard".to_string(), Way::Running), ("sea".to_string(), Way::Starting), ("orbit".to_string(), Way::Stopped)]);
        let at = |n: &str| state(&Dest::World(n.into()), ANN, true, Some(&host));
        assert_eq!(at("yard"), DoorState::Open);
        assert_eq!(at("sea"), DoorState::Waking);
        assert_eq!(at("gone"), DoorState::Unlinked);
        // a stopped one is woken by being looked at, and shows as waking
        assert_eq!(at("orbit"), DoorState::Waking);
        assert_eq!(host.woken.lock().unwrap().as_slice(), ["orbit".to_string()]);
        // with no host to ask, today's look
        assert_eq!(state(&Dest::World("yard".into()), ANN, true, None), DoorState::Unknown);
    }

    #[test]
    fn restrictions_are_per_player_and_general() {
        let host = FakeHost::default();
        host.worlds.lock().unwrap().insert("yard".into(), Way::Running);
        let guest = Who { signed_in: false, in_party: false };
        assert_eq!(state(&Dest::World("yard".into()), guest, true, Some(&host)), DoorState::Restricted(Restriction::SignIn));
        assert_eq!(state(&Dest::World("yard".into()), guest, false, Some(&host)), DoorState::Open, "a world without sign-in takes guests");
        assert_eq!(state(&Dest::Dungeon, ANN, true, None), DoorState::Restricted(Restriction::Party));
        assert_eq!(state(&Dest::Dungeon, Who { signed_in: true, in_party: true }, true, None), DoorState::Open);
        assert_eq!(state(&Dest::Dungeon, guest, true, None), DoorState::Restricted(Restriction::SignIn));
        assert_eq!(state(&Dest::Dungeon, guest, false, None), DoorState::Unknown, "no sign-in, no parties to check");
    }

    #[test]
    fn doorways_within_the_world_and_unrouted_ones() {
        assert_eq!(state(&Dest::Here, ANN, true, None), DoorState::Open);
        assert_eq!(state(&Dest::Unrouted, ANN, true, None), DoorState::Unknown);
        assert_eq!(state(&Dest::Nowhere, ANN, true, None), DoorState::Unlinked);
    }
}
