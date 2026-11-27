//! The bridge between the world's entities and the graph: a `vox_core` [`Node`](vox_core::entity::Node)
//! entity becomes a graph [`Node`], and a world's block names become the palette `vox-art` builds
//! with and the ids the cells are placed as.
//!
//! The server (to build) and the editor client (to preview) both call this, so a preview and a
//! build read a node the same way and cannot drift apart. `vox-core` cannot depend on this crate,
//! so the entity carries its kind as text and the checks that need the vocabulary happen here.

use std::collections::HashMap;

use vox_art::Palette;

use crate::node::{Detail, Footprint, Kind, Node};

/// The detail levels in the order of the entity's `detail` number (0 is `Intent`).
const DETAILS: [Detail; 5] = [Detail::Intent, Detail::Layout, Detail::Massing, Detail::Structure, Detail::Dressing];

/// A node entity that is not a valid graph node: its kind is not one of the vocabulary, or its
/// detail number is past the last level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadNode;

/// The graph node a node entity stands for. `id` is the entity's number.
pub fn from_entity(id: u32, n: &vox_core::entity::Node) -> Result<Node, BadNode> {
    let kind = serde_json::from_value::<Kind>(serde_json::Value::String(n.kind.clone())).map_err(|_| BadNode)?;
    let detail = DETAILS.get(usize::from(n.detail)).copied().ok_or(BadNode)?;
    let mut graph = Node::new(id, kind, Footprint::new(n.at, n.size));
    graph.detail = detail;
    graph.parent = n.parent.map(|p| p.0);
    graph.recipe = n.recipe.clone();
    graph.seed = n.seed;
    Ok(graph)
}

/// The palette `vox-art` builds a world's nodes with, and each block name's id in that world
/// (1 up, in the world's order; 0 is air). Every block is given a placeholder colour: only the
/// names matter to a build. None when `vox-art` does not take the names.
pub fn build_palette(names: &[String]) -> Option<(Palette, HashMap<String, u16>)> {
    let materials: Vec<serde_json::Value> =
        names.iter().map(|n| serde_json::json!({ "name": n, "colours": [[128, 128, 128, 255]] })).collect();
    let json = serde_json::json!({ "version": 1, "materials": materials }).to_string();
    let palette = Palette::from_json(&json).ok()?;
    let ids = names.iter().enumerate().filter_map(|(i, n)| u16::try_from(i + 1).ok().map(|id| (n.clone(), id))).collect();
    Some((palette, ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::entity::EntityId;

    fn entity() -> vox_core::entity::Node {
        vox_core::entity::Node {
            kind: "house".into(),
            detail: 3,
            at: [4, 0, -2],
            size: [10, 12, 8],
            parent: Some(EntityId(9)),
            recipe: Some("{}".into()),
            seed: 77,
        }
    }

    #[test]
    fn a_node_entity_becomes_a_graph_node() {
        let node = from_entity(5, &entity()).expect("valid");
        assert_eq!(node.id, 5);
        assert_eq!(node.kind, Kind::House);
        assert_eq!(node.detail, Detail::Structure);
        assert_eq!(node.footprint, Footprint::new([4, 0, -2], [10, 12, 8]));
        assert_eq!((node.parent, node.recipe.as_deref(), node.seed), (Some(9), Some("{}"), 77));
    }

    #[test]
    fn an_unknown_kind_or_detail_is_refused() {
        let mut n = entity();
        n.kind = "castle".into();
        assert_eq!(from_entity(1, &n), Err(BadNode));
        let mut n = entity();
        n.detail = 5;
        assert_eq!(from_entity(1, &n), Err(BadNode));
        n.detail = 4;
        assert!(from_entity(1, &n).is_ok());
    }

    #[test]
    fn the_palette_numbers_blocks_from_one() {
        let names = vec!["stone".to_string(), "brick".to_string()];
        let (_, ids) = build_palette(&names).expect("palette");
        assert_eq!((ids["stone"], ids["brick"]), (1, 2));
        assert_eq!(ids.len(), 2);
    }
}
