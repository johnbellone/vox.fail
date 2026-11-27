//! Links: a relation between two nodes. `contains` is enforced against the parent of each node;
//! the other relations are stored and drawn, but nothing checks them yet.

use serde::{Deserialize, Serialize};

/// A directed relation from one node to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub from: u32,
    pub to: u32,
    pub rel: Rel,
}

/// The relation a link stands for. Closed, like [`crate::node::Kind`]: new variants are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rel {
    /// `from` is the parent of `to`. Must agree with `to`'s `parent`.
    Contains,
    NextTo,
    Faces,
    /// A road or path joins the two.
    Connects,
    PortalTo,
}
