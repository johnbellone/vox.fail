//! Who may do what with a structure: the rules, with no database or network in them, so the
//! directory (which keeps the records) and the world server (which enforces them) agree.
//!
//! A structure is a named blueprint with immutable versions (docs/design/world-format.md,
//! *Structures*). Three rights, each including the ones before it:
//!
//! - **Use**: place instances of it (a pointer to a version) where you may build.
//! - **Copy**: make a structure of your own from it (the same blocks, owned by you).
//! - **Edit**: save new versions of it, which every holder of the structure's pointer sees
//!   (instances placed from a version keep that version).
//!
//! The **owner** has all three and alone may grant and revoke rights, open it to everyone
//! (`public`) and delete it. The public may be given Use or Copy, never Edit. Rights are for
//! signed-in accounts; guests have none.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How many accounts may be granted rights on one structure.
pub const MAX_GRANTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Right {
    Use,
    Copy,
    Edit,
}

impl Right {
    pub fn parse(s: &str) -> Option<Right> {
        match s {
            "use" => Some(Right::Use),
            "copy" => Some(Right::Copy),
            "edit" => Some(Right::Edit),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Right::Use => "use",
            Right::Copy => "copy",
            Right::Edit => "edit",
        }
    }
}

/// Why a change to a structure's entitlements isn't allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    NotOwner,
    /// the owner has everything already
    OwnerNeedsNoGrant,
    /// open to everyone to edit
    PublicEdit,
    TooManyGrants,
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Why::NotOwner => "only the owner can change who may use a structure",
            Why::OwnerNeedsNoGrant => "the owner can do everything already",
            Why::PublicEdit => "a structure can be open to everyone to use or copy, never to edit",
            Why::TooManyGrants => "that's as many people as a structure can be shared with",
        })
    }
}

/// A structure's entitlements: its owner, what everyone may do, and what named accounts may.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Entitlements {
    pub owner: u64,
    /// what any signed-in account may do (Use or Copy), if anything
    pub public: Option<Right>,
    /// account -> right
    pub grants: BTreeMap<u64, Right>,
}

impl Entitlements {
    pub fn new(owner: u64) -> Entitlements {
        Entitlements { owner, ..Default::default() }
    }

    /// The most `account` may do: the owner everything (`Edit`), others what they were
    /// granted or the public has, the larger of the two. None for guests and strangers.
    pub fn right_of(&self, account: Option<u64>) -> Option<Right> {
        let a = account?;
        if a == self.owner {
            return Some(Right::Edit);
        }
        self.grants.get(&a).copied().max(self.public)
    }

    /// `account` may do what needs `need` (each right includes the ones before it).
    pub fn allows(&self, account: Option<u64>, need: Right) -> bool {
        self.right_of(account).is_some_and(|r| r >= need)
    }

    /// Only the owner changes entitlements and deletes.
    pub fn can_manage(&self, account: Option<u64>) -> bool {
        account == Some(self.owner)
    }

    /// `by` grants `to` the right `right`, or with `None` takes theirs away.
    pub fn grant(&mut self, by: Option<u64>, to: u64, right: Option<Right>) -> Result<(), Why> {
        if !self.can_manage(by) {
            return Err(Why::NotOwner);
        }
        if to == self.owner {
            return Err(Why::OwnerNeedsNoGrant);
        }
        match right {
            Some(r) => {
                if !self.grants.contains_key(&to) && self.grants.len() >= MAX_GRANTS {
                    return Err(Why::TooManyGrants);
                }
                self.grants.insert(to, r);
            }
            None => {
                self.grants.remove(&to);
            }
        }
        Ok(())
    }

    /// `by` opens the structure to everyone at `right` (Use or Copy), or closes it with `None`.
    pub fn set_public(&mut self, by: Option<u64>, right: Option<Right>) -> Result<(), Why> {
        if !self.can_manage(by) {
            return Err(Why::NotOwner);
        }
        if right == Some(Right::Edit) {
            return Err(Why::PublicEdit);
        }
        self.public = right;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: u64 = 1;
    const FRIEND: u64 = 2;
    const STRANGER: u64 = 3;

    #[test]
    fn rights_include_the_ones_before() {
        assert!(Right::Use < Right::Copy && Right::Copy < Right::Edit);
        let mut e = Entitlements::new(OWNER);
        e.grant(Some(OWNER), FRIEND, Some(Right::Copy)).unwrap();
        assert!(e.allows(Some(FRIEND), Right::Use));
        assert!(e.allows(Some(FRIEND), Right::Copy));
        assert!(!e.allows(Some(FRIEND), Right::Edit));
    }

    #[test]
    fn the_owner_can_do_everything_and_strangers_and_guests_nothing() {
        let e = Entitlements::new(OWNER);
        assert!(e.allows(Some(OWNER), Right::Edit));
        assert!(!e.allows(Some(STRANGER), Right::Use));
        assert!(!e.allows(None, Right::Use), "a guest has no rights, not even on an owner-less structure");
        assert_eq!(e.right_of(None), None);
    }

    #[test]
    fn open_to_everyone_is_a_floor_not_a_ceiling() {
        let mut e = Entitlements::new(OWNER);
        e.set_public(Some(OWNER), Some(Right::Use)).unwrap();
        assert!(e.allows(Some(STRANGER), Right::Use));
        assert!(!e.allows(Some(STRANGER), Right::Copy));
        assert!(!e.allows(None, Right::Use), "guests still can't");
        // a friend granted more than the public keeps it; granted less still gets the public's
        e.grant(Some(OWNER), FRIEND, Some(Right::Edit)).unwrap();
        assert_eq!(e.right_of(Some(FRIEND)), Some(Right::Edit));
        e.set_public(Some(OWNER), Some(Right::Copy)).unwrap();
        e.grant(Some(OWNER), STRANGER, Some(Right::Use)).unwrap();
        assert_eq!(e.right_of(Some(STRANGER)), Some(Right::Copy));
    }

    #[test]
    fn only_the_owner_changes_things_and_never_to_make_the_public_editors() {
        let mut e = Entitlements::new(OWNER);
        e.grant(Some(OWNER), FRIEND, Some(Right::Edit)).unwrap();
        // even an editor can't pass it on
        assert_eq!(e.grant(Some(FRIEND), STRANGER, Some(Right::Use)), Err(Why::NotOwner));
        assert_eq!(e.set_public(Some(FRIEND), Some(Right::Use)), Err(Why::NotOwner));
        assert_eq!(e.grant(None, STRANGER, Some(Right::Use)), Err(Why::NotOwner));
        assert!(!e.can_manage(Some(FRIEND)) && !e.can_manage(None) && e.can_manage(Some(OWNER)));
        assert_eq!(e.set_public(Some(OWNER), Some(Right::Edit)), Err(Why::PublicEdit));
        assert_eq!(e.grant(Some(OWNER), OWNER, Some(Right::Use)), Err(Why::OwnerNeedsNoGrant));
    }

    #[test]
    fn revoking_takes_it_away() {
        let mut e = Entitlements::new(OWNER);
        e.grant(Some(OWNER), FRIEND, Some(Right::Use)).unwrap();
        e.grant(Some(OWNER), FRIEND, None).unwrap();
        assert!(!e.allows(Some(FRIEND), Right::Use));
        assert!(e.grants.is_empty());
        // taking away what was never given is fine
        e.grant(Some(OWNER), STRANGER, None).unwrap();
    }

    #[test]
    fn grants_are_capped() {
        let mut e = Entitlements::new(0);
        for a in 1..=MAX_GRANTS as u64 {
            e.grant(Some(0), a, Some(Right::Use)).unwrap();
        }
        assert_eq!(e.grant(Some(0), 9999, Some(Right::Use)), Err(Why::TooManyGrants));
        // changing someone who's already in is not a new one
        e.grant(Some(0), 1, Some(Right::Edit)).unwrap();
    }

    #[test]
    fn rights_have_names() {
        for r in [Right::Use, Right::Copy, Right::Edit] {
            assert_eq!(Right::parse(r.name()), Some(r));
        }
        assert_eq!(Right::parse("own"), None);
        assert_eq!(serde_json::to_string(&Right::Copy).unwrap(), "\"copy\"");
    }
}
