//! The graph: nodes by id and the links between them, with caps on both. Adding and removing keep
//! the links consistent with the nodes; the placement rules live in [`crate::constraints`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::link::Link;
use crate::node::{Footprint, Node};

/// Most nodes one graph holds. Graphs come from players, so the count is capped.
pub const MAX_NODES: usize = 4_096;

/// Most links one graph holds.
pub const MAX_LINKS: usize = 16_384;

/// Most bytes of graph JSON [`Graph::from_json`] reads. Text past this is refused before it is
/// parsed, so a large document cannot take memory or time to read.
pub const MAX_GRAPH_BYTES: usize = 1 << 20;

/// A set of nodes and the links between them. Nodes are kept in id order so that every walk over
/// the graph, and so every expansion, is the same on every run and on every target.
///
/// Network input must be read with [`Graph::from_json`], never by deserializing directly: it is
/// the one place that applies [`MAX_GRAPH_BYTES`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: BTreeMap<u32, Node>,
    pub links: Vec<Link>,
}

impl Graph {
    /// Parses a graph from JSON text. Text over [`MAX_GRAPH_BYTES`] is refused before parsing. The
    /// result is not checked against the placement rules: run [`crate::check`] on it.
    pub fn from_json(text: &str) -> Result<Graph, String> {
        if text.len() > MAX_GRAPH_BYTES {
            return Err(format!(
                "a graph is at most {MAX_GRAPH_BYTES} bytes, got {}",
                text.len()
            ));
        }
        serde_json::from_str(text).map_err(|error| error.to_string())
    }

    /// Adds a node. Refused when its id is taken or the graph is at [`MAX_NODES`].
    pub fn add(&mut self, node: Node) -> Result<(), String> {
        if self.nodes.contains_key(&node.id) {
            return Err(format!("node {} already exists", node.id));
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(format!("a graph holds at most {MAX_NODES} nodes"));
        }
        self.nodes.insert(node.id, node);
        Ok(())
    }

    /// Adds a link. Refused at [`MAX_LINKS`]. The ends are not checked here: [`crate::check`]
    /// reports a link that names a node which is not there, so a graph being edited may hold one
    /// briefly.
    pub fn link(&mut self, link: Link) -> Result<(), String> {
        if self.links.len() >= MAX_LINKS {
            return Err(format!("a graph holds at most {MAX_LINKS} links"));
        }
        self.links.push(link);
        Ok(())
    }

    /// Removes a node and every link that names it. Refused while it still has children: they
    /// would be left without a parent, so remove them first.
    pub fn remove(&mut self, id: u32) -> Result<Node, String> {
        if !self.nodes.contains_key(&id) {
            return Err(format!("no node {id}"));
        }
        if !self.children(id).is_empty() {
            return Err(format!("node {id} still has children; remove them first"));
        }
        self.links.retain(|link| link.from != id && link.to != id);
        Ok(self.nodes.remove(&id).expect("checked above"))
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// The nodes whose parent is `id`, in id order.
    pub fn children(&self, id: u32) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|node| node.parent == Some(id))
            .collect()
    }

    /// The nodes whose footprint shares a block with `area`, in id order.
    pub fn nodes_in(&self, area: &Footprint) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|node| node.footprint.overlaps(area))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Rel;
    use crate::node::Kind;

    fn at(x: i32, y: i32, z: i32, size: [u16; 3]) -> Footprint {
        Footprint::new([x, y, z], size)
    }

    fn house(id: u32, parent: Option<u32>, footprint: Footprint) -> Node {
        let mut node = Node::new(id, Kind::House, footprint);
        node.parent = parent;
        node
    }

    #[test]
    fn add_and_get_a_node() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [4, 4, 4]))).unwrap();
        assert_eq!(graph.node(1).unwrap().kind, Kind::House);
        assert!(graph.node(2).is_none());
    }

    #[test]
    fn a_duplicate_id_is_refused() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [4, 4, 4]))).unwrap();
        assert!(graph.add(house(1, None, at(9, 0, 0, [4, 4, 4]))).is_err());
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn children_are_the_nodes_with_that_parent() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [20, 4, 20]))).unwrap();
        graph
            .add(house(2, Some(1), at(0, 0, 0, [4, 4, 4])))
            .unwrap();
        graph
            .add(house(3, Some(1), at(8, 0, 0, [4, 4, 4])))
            .unwrap();
        graph.add(house(4, None, at(40, 0, 0, [4, 4, 4]))).unwrap();
        let ids: Vec<u32> = graph.children(1).iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![2, 3]);
        assert!(graph.children(2).is_empty());
    }

    #[test]
    fn remove_drops_the_node_and_its_links() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [4, 4, 4]))).unwrap();
        graph.add(house(2, None, at(10, 0, 0, [4, 4, 4]))).unwrap();
        graph
            .link(Link {
                from: 1,
                to: 2,
                rel: Rel::NextTo,
            })
            .unwrap();
        graph
            .link(Link {
                from: 2,
                to: 1,
                rel: Rel::Faces,
            })
            .unwrap();
        let removed = graph.remove(2).unwrap();
        assert_eq!(removed.id, 2);
        assert!(graph.node(2).is_none());
        assert!(graph.links.is_empty());
    }

    #[test]
    fn remove_refuses_while_children_remain() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [20, 4, 20]))).unwrap();
        graph
            .add(house(2, Some(1), at(0, 0, 0, [4, 4, 4])))
            .unwrap();
        assert!(graph.remove(1).is_err());
        assert!(graph.node(1).is_some());
        graph.remove(2).unwrap();
        graph.remove(1).unwrap();
        assert!(graph.nodes.is_empty());
    }

    #[test]
    fn remove_of_a_missing_node_is_refused() {
        let mut graph = Graph::default();
        assert!(graph.remove(7).is_err());
    }

    #[test]
    fn nodes_in_returns_footprints_that_touch_the_area() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [4, 4, 4]))).unwrap();
        graph.add(house(2, None, at(10, 0, 0, [4, 4, 4]))).unwrap();
        graph.add(house(3, None, at(3, 0, 0, [2, 4, 4]))).unwrap();
        // Touches node 1 at x = 3 and node 3 at x = 3..4, but not node 2 at x = 10.
        let ids: Vec<u32> = graph
            .nodes_in(&at(3, 0, 0, [1, 1, 1]))
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn the_node_cap_is_enforced() {
        let mut graph = Graph::default();
        for id in 0..MAX_NODES as u32 {
            graph.add(house(id, None, at(0, 0, 0, [1, 1, 1]))).unwrap();
        }
        let extra = house(MAX_NODES as u32, None, at(0, 0, 0, [1, 1, 1]));
        assert!(graph.add(extra).is_err());
        assert_eq!(graph.nodes.len(), MAX_NODES);
    }

    #[test]
    fn the_link_cap_is_enforced() {
        let mut graph = Graph::default();
        let link = Link {
            from: 1,
            to: 2,
            rel: Rel::NextTo,
        };
        for _ in 0..MAX_LINKS {
            graph.link(link).unwrap();
        }
        assert!(graph.link(link).is_err());
        assert_eq!(graph.links.len(), MAX_LINKS);
    }

    #[test]
    fn a_graph_round_trips_through_json() {
        let mut graph = Graph::default();
        graph.add(house(1, None, at(0, 0, 0, [4, 4, 4]))).unwrap();
        let text = serde_json::to_string(&graph).unwrap();
        assert_eq!(Graph::from_json(&text).unwrap(), graph);
    }

    #[test]
    fn json_over_the_byte_cap_is_refused_before_parsing() {
        let text = " ".repeat(MAX_GRAPH_BYTES + 1);
        let error = Graph::from_json(&text).unwrap_err();
        assert!(error.contains("at most"));
    }

    #[test]
    fn json_that_is_not_a_graph_is_refused() {
        assert!(Graph::from_json("{not json").is_err());
    }
}
