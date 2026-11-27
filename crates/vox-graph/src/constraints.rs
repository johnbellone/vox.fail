//! The placement rules. [`check`] reports every break of a rule, not only the first, so an editor
//! can show them all at once, up to [`MAX_PROBLEMS`]. Nothing here changes the graph.
//!
//! Rules: each node is stored under its own id; each node's footprint sides, params and recipe
//! sizes are within their caps; a node at `structure` or `dressing` detail has a recipe; a child's
//! footprint lies inside its parent's; siblings (nodes with the same parent, roots included) do not
//! overlap; a node's `parent` and the `contains` links agree; every link names nodes that exist; no
//! parent chain loops; and the graph holds no more than [`MAX_NODES`] nodes and [`MAX_LINKS`] links.

use std::collections::{BTreeMap, BTreeSet};

use vox_art::recipe::MAX_RECIPE_BYTES;

use crate::graph::{Graph, MAX_LINKS, MAX_NODES};
use crate::link::Rel;
use crate::node::{Detail, MAX_PARAM_BYTES, MAX_PARAMS, MAX_SIDE, Node};

/// Most problems [`check`] reports. A graph from a player can break a rule at every node, so the
/// list is cut here and ends with one entry saying that more problems were not shown.
pub const MAX_PROBLEMS: usize = 256;

/// One break of a rule. `node` is the node it belongs to, or `None` for a problem with a link or
/// with the graph as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub node: Option<u32>,
    pub what: String,
}

fn problem(node: Option<u32>, what: impl Into<String>) -> Problem {
    Problem {
        node,
        what: what.into(),
    }
}

/// The problems found so far. It keeps at most [`MAX_PROBLEMS`]; the first one that does not fit
/// sets `overflow`, and the walks in [`check`] stop once it is set, so time and memory stay bounded.
struct Report {
    problems: Vec<Problem>,
    overflow: bool,
}

impl Report {
    fn push(&mut self, found: Problem) {
        if self.overflow {
            return;
        }
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(found);
        } else {
            self.overflow = true;
        }
    }

    /// True once a problem was refused for lack of room: nothing more is worth walking.
    fn full(&self) -> bool {
        self.overflow
    }

    fn finish(mut self) -> Vec<Problem> {
        if self.overflow {
            self.problems
                .push(problem(None, "and more problems not shown"));
        }
        self.problems
    }
}

/// Every break of a placement rule in `graph`, in a stable order. An empty list means the graph
/// is well placed. At most [`MAX_PROBLEMS`] problems are returned, then one entry saying more
/// were not shown.
pub fn check(graph: &Graph) -> Vec<Problem> {
    let mut report = Report {
        problems: Vec::new(),
        overflow: false,
    };
    // A graph read by deserializing skips the caps `add` and `link` enforce, so count it here.
    if graph.nodes.len() > MAX_NODES {
        report.push(problem(
            None,
            format!(
                "graph holds {} nodes, at most {MAX_NODES}",
                graph.nodes.len()
            ),
        ));
    }
    if graph.links.len() > MAX_LINKS {
        report.push(problem(
            None,
            format!(
                "graph holds {} links, at most {MAX_LINKS}",
                graph.links.len()
            ),
        ));
    }

    let contains: BTreeSet<(u32, u32)> = graph
        .links
        .iter()
        .filter(|link| link.rel == Rel::Contains)
        .map(|link| (link.from, link.to))
        .collect();

    for (&key, node) in &graph.nodes {
        if report.full() {
            break;
        }
        if key != node.id {
            report.push(problem(
                Some(key),
                format!("node key {key} has id {}", node.id),
            ));
        }
        check_node(node, &mut report);
        if let Some(parent_id) = node.parent {
            match graph.nodes.get(&parent_id) {
                None => report.push(problem(
                    Some(node.id),
                    format!("parent {parent_id} does not exist"),
                )),
                Some(parent) if !parent.footprint.contains(&node.footprint) => {
                    report.push(problem(
                        Some(node.id),
                        format!("footprint lies outside the footprint of parent {parent_id}"),
                    ))
                }
                Some(_) => {}
            }
            if !contains.contains(&(parent_id, node.id)) {
                report.push(problem(
                    Some(node.id),
                    format!("parent {parent_id} has no contains link to it"),
                ));
            }
        }
        if in_parent_cycle(graph, node.id) {
            report.push(problem(Some(node.id), "parent chain loops back to itself"));
        }
    }

    for (index, link) in graph.links.iter().enumerate() {
        if report.full() {
            break;
        }
        for end in [link.from, link.to] {
            if !graph.nodes.contains_key(&end) {
                report.push(problem(
                    None,
                    format!("link {index} names node {end}, which does not exist"),
                ));
            }
        }
        if link.rel == Rel::Contains
            && let Some(child) = graph.nodes.get(&link.to)
            && child.parent != Some(link.from)
        {
            report.push(problem(
                Some(link.to),
                format!(
                    "contains link from {} but the node's parent is {:?}",
                    link.from, child.parent
                ),
            ));
        }
    }

    check_siblings(graph, &mut report);
    report.finish()
}

fn check_node(node: &Node, report: &mut Report) {
    for (axis, side) in ["x", "y", "z"].iter().zip(node.footprint.size) {
        if side == 0 || side > MAX_SIDE {
            report.push(problem(
                Some(node.id),
                format!("footprint {axis} side must be 1 to {MAX_SIDE}, got {side}"),
            ));
        }
    }
    if node.params.len() > MAX_PARAMS {
        report.push(problem(
            Some(node.id),
            format!(
                "params have at most {MAX_PARAMS} keys, got {}",
                node.params.len()
            ),
        ));
    }
    let param_bytes = serde_json::to_vec(&node.params).map_or(usize::MAX, |bytes| bytes.len());
    if param_bytes > MAX_PARAM_BYTES {
        report.push(problem(
            Some(node.id),
            format!("params are at most {MAX_PARAM_BYTES} bytes of JSON, got {param_bytes}"),
        ));
    }
    if let Some(recipe) = &node.recipe
        && recipe.len() > MAX_RECIPE_BYTES
    {
        report.push(problem(
            Some(node.id),
            format!(
                "recipe is at most {MAX_RECIPE_BYTES} bytes, got {}",
                recipe.len()
            ),
        ));
    }
    if node.detail >= Detail::Structure && node.recipe.is_none() {
        report.push(problem(
            Some(node.id),
            format!("detail {:?} needs a recipe", node.detail),
        ));
    }
}

/// True when walking up from `start` returns to `start`. The walk is bounded by the node count,
/// so a loop that does not pass through `start` ends the walk too, and is reported on its own
/// members instead.
fn in_parent_cycle(graph: &Graph, start: u32) -> bool {
    let mut current = start;
    for _ in 0..=graph.nodes.len() {
        match graph.nodes.get(&current).and_then(|node| node.parent) {
            Some(parent) if parent == start => return true,
            Some(parent) => current = parent,
            None => return false,
        }
    }
    false
}

/// Siblings share a parent; roots (no parent) are siblings of each other.
fn check_siblings(graph: &Graph, report: &mut Report) {
    let mut groups: BTreeMap<Option<u32>, Vec<&Node>> = BTreeMap::new();
    for node in graph.nodes.values() {
        groups.entry(node.parent).or_default().push(node);
    }
    for siblings in groups.values() {
        for (i, a) in siblings.iter().enumerate() {
            for b in &siblings[i + 1..] {
                if report.full() {
                    return;
                }
                if a.footprint.overlaps(&b.footprint) {
                    report.push(problem(
                        Some(a.id),
                        format!("footprint overlaps sibling {}", b.id),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{MAX_LINKS, MAX_NODES};
    use crate::link::{Link, Rel};
    use crate::node::{Detail, Footprint, Kind, Node};

    fn fp(x: i32, y: i32, z: i32, size: [u16; 3]) -> Footprint {
        Footprint::new([x, y, z], size)
    }

    fn node(id: u32, parent: Option<u32>, footprint: Footprint) -> Node {
        let mut node = Node::new(id, Kind::House, footprint);
        node.parent = parent;
        node
    }

    /// A graph with the parent-child pair 1 and 2, linked by `contains` as the rules want.
    fn pair() -> Graph {
        let mut graph = Graph::default();
        graph.add(node(1, None, fp(0, 0, 0, [20, 4, 20]))).unwrap();
        graph.add(node(2, Some(1), fp(2, 0, 2, [4, 4, 4]))).unwrap();
        graph
            .link(Link {
                from: 1,
                to: 2,
                rel: Rel::Contains,
            })
            .unwrap();
        graph
    }

    #[test]
    fn a_well_placed_graph_has_no_problems() {
        assert!(check(&pair()).is_empty());
    }

    #[test]
    fn a_child_outside_its_parent_is_a_problem() {
        let mut graph = pair();
        graph.nodes.get_mut(&2).unwrap().footprint = fp(18, 0, 2, [4, 4, 4]);
        let problems = check(&graph);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].node, Some(2));
        assert!(problems[0].what.contains("outside"));
    }

    #[test]
    fn overlapping_siblings_are_a_problem() {
        let mut graph = pair();
        graph.add(node(3, Some(1), fp(4, 0, 4, [4, 4, 4]))).unwrap();
        graph
            .link(Link {
                from: 1,
                to: 3,
                rel: Rel::Contains,
            })
            .unwrap();
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.node == Some(2) && p.what.contains("sibling 3"))
        );
    }

    #[test]
    fn touching_siblings_that_share_no_block_are_fine() {
        let mut graph = pair();
        graph.add(node(3, Some(1), fp(6, 0, 2, [4, 4, 4]))).unwrap();
        graph
            .link(Link {
                from: 1,
                to: 3,
                rel: Rel::Contains,
            })
            .unwrap();
        assert!(check(&graph).is_empty());
    }

    #[test]
    fn overlapping_roots_are_siblings_too() {
        let mut graph = Graph::default();
        graph.add(node(1, None, fp(0, 0, 0, [4, 4, 4]))).unwrap();
        graph.add(node(2, None, fp(2, 0, 2, [4, 4, 4]))).unwrap();
        let problems = check(&graph);
        assert!(problems.iter().any(|p| p.what.contains("sibling")));
    }

    #[test]
    fn a_parent_without_a_contains_link_is_a_problem() {
        let mut graph = pair();
        graph.links.clear();
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.node == Some(2) && p.what.contains("no contains link"))
        );
    }

    #[test]
    fn a_contains_link_without_a_parent_is_a_problem() {
        let mut graph = pair();
        graph.nodes.get_mut(&2).unwrap().parent = None;
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.node == Some(2) && p.what.contains("contains link from 1"))
        );
    }

    #[test]
    fn a_link_to_a_missing_node_is_a_problem() {
        let mut graph = pair();
        graph
            .link(Link {
                from: 1,
                to: 99,
                rel: Rel::NextTo,
            })
            .unwrap();
        let problems = check(&graph);
        assert!(problems.iter().any(|p| p.what.contains("node 99")));
    }

    #[test]
    fn a_parent_that_does_not_exist_is_a_problem() {
        let mut graph = Graph::default();
        graph.add(node(2, Some(7), fp(0, 0, 0, [4, 4, 4]))).unwrap();
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.what.contains("parent 7 does not exist"))
        );
    }

    #[test]
    fn a_parent_cycle_is_a_problem() {
        let mut graph = Graph::default();
        graph.add(node(1, Some(2), fp(0, 0, 0, [4, 4, 4]))).unwrap();
        graph.add(node(2, Some(1), fp(0, 0, 0, [4, 4, 4]))).unwrap();
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.node == Some(1) && p.what.contains("loops"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.node == Some(2) && p.what.contains("loops"))
        );
    }

    #[test]
    fn a_structure_without_a_recipe_is_a_problem() {
        let mut graph = pair();
        graph.nodes.get_mut(&1).unwrap().detail = Detail::Structure;
        let problems = check(&graph);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].node, Some(1));
        assert!(problems[0].what.contains("needs a recipe"));
    }

    #[test]
    fn a_structure_with_a_recipe_is_fine() {
        let mut graph = pair();
        let parent = graph.nodes.get_mut(&1).unwrap();
        parent.detail = Detail::Structure;
        parent.recipe = Some("{}".to_string());
        assert!(check(&graph).is_empty());
    }

    #[test]
    fn a_side_past_the_cap_or_zero_is_a_problem() {
        let mut graph = Graph::default();
        graph.add(node(1, None, fp(0, 0, 0, [65, 0, 4]))).unwrap();
        let problems = check(&graph);
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().all(|p| p.node == Some(1)));
    }

    #[test]
    fn too_many_params_is_a_problem() {
        let mut graph = Graph::default();
        let mut n = node(1, None, fp(0, 0, 0, [4, 4, 4]));
        for i in 0..=MAX_PARAMS {
            n.params.insert(format!("k{i}"), serde_json::Value::from(i));
        }
        graph.add(n).unwrap();
        let problems = check(&graph);
        assert!(problems.iter().any(|p| p.what.contains("keys")));
    }

    #[test]
    fn params_over_the_byte_cap_are_a_problem() {
        let mut graph = Graph::default();
        let mut n = node(1, None, fp(0, 0, 0, [4, 4, 4]));
        n.params.insert(
            "text".to_string(),
            serde_json::Value::from("x".repeat(MAX_PARAM_BYTES)),
        );
        graph.add(n).unwrap();
        let problems = check(&graph);
        assert!(problems.iter().any(|p| p.what.contains("bytes of JSON")));
    }

    #[test]
    fn a_recipe_over_the_byte_cap_is_a_problem() {
        let mut graph = Graph::default();
        let mut n = node(1, None, fp(0, 0, 0, [4, 4, 4]));
        n.recipe = Some(" ".repeat(MAX_RECIPE_BYTES + 1));
        graph.add(n).unwrap();
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.what.contains("recipe is at most"))
        );
    }

    #[test]
    fn the_problems_are_capped_with_a_final_entry() {
        // 4,096 roots on one footprint: every pair overlaps, so the walk must stop at the cap.
        let mut graph = Graph::default();
        for id in 0..MAX_NODES as u32 {
            graph.add(node(id, None, fp(0, 0, 0, [4, 4, 4]))).unwrap();
        }
        let problems = check(&graph);
        assert_eq!(problems.len(), MAX_PROBLEMS + 1);
        assert_eq!(problems.last().unwrap().what, "and more problems not shown");
    }

    #[test]
    fn a_node_stored_under_another_key_is_a_problem() {
        let mut graph = Graph::default();
        graph.nodes.insert(5, node(7, None, fp(0, 0, 0, [4, 4, 4])));
        let problems = check(&graph);
        assert!(problems.iter().any(|p| p.what == "node key 5 has id 7"));
    }

    #[test]
    fn a_graph_over_the_node_cap_is_a_problem() {
        // Deserialized graphs skip the caps `add` enforces, so `check` counts them.
        let mut graph = Graph::default();
        for id in 0..=MAX_NODES as u32 {
            graph
                .nodes
                .insert(id, node(id, None, fp(id as i32 * 2, 0, 0, [1, 1, 1])));
        }
        let problems = check(&graph);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0]
                .what
                .contains(&format!("{} nodes", MAX_NODES + 1))
        );
    }

    #[test]
    fn a_graph_over_the_link_cap_is_a_problem() {
        let mut graph = pair();
        let link = Link {
            from: 1,
            to: 2,
            rel: Rel::NextTo,
        };
        graph.links = vec![link; MAX_LINKS + 1];
        let problems = check(&graph);
        assert!(
            problems
                .iter()
                .any(|p| p.what.contains(&format!("{} links", MAX_LINKS + 1)))
        );
    }
}
