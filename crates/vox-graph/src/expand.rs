//! Expansion: a node's recipe built through `vox-art` into block cells at the node's footprint.
//!
//! Deterministic: the recipe takes the node's seed, and `vox-art` builds it the same way on every
//! target, so a preview in the browser and a build on the server place the same blocks.

use vox_art::recipe::{Recipe, RecipeScale, eval};
use vox_art::{Palette, export::level::{LevelError, level_cells}};

use crate::node::{MAX_SIDE, Node};

/// Why a node cannot be expanded: `TooBig` when the build is over the cap (the caller refuses it
/// as too big, and can say how many blocks it would build), `Invalid` with a message for each
/// problem otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    TooBig { cells: usize },
    Invalid(Vec<String>),
}

/// Builds a node's recipe into `(block position, block name)` cells, sorted by position.
///
/// A footprint side must be 1 to [`MAX_SIDE`] blocks, checked first. A node without a recipe
/// expands to nothing. Otherwise the recipe must be world scale and its size must equal the
/// footprint's, since the footprint is where the blocks go; both are refused with a message rather
/// than built into the wrong place.
pub fn expand(node: &Node, palette: &Palette) -> Result<Vec<([i32; 3], String)>, ExpandError> {
    let bad_sides: Vec<String> = ["x", "y", "z"]
        .iter()
        .zip(node.footprint.size)
        .filter(|(_, side)| *side == 0 || *side > MAX_SIDE)
        .map(|(axis, side)| format!("footprint {axis} side must be 1 to {MAX_SIDE}, got {side}"))
        .collect();
    if !bad_sides.is_empty() {
        return Err(ExpandError::Invalid(bad_sides));
    }
    let Some(text) = &node.recipe else {
        return Ok(Vec::new());
    };
    let invalid = |message: String| ExpandError::Invalid(vec![message]);
    let mut recipe = Recipe::from_json(text).map_err(invalid)?;
    if recipe.scale != RecipeScale::World {
        return Err(invalid(format!(
            "a node builds at world scale, its recipe says {:?}",
            recipe.scale
        )));
    }
    if recipe.size != node.footprint.size {
        return Err(invalid(format!(
            "recipe size {:?} does not match the footprint size {:?}",
            recipe.size, node.footprint.size
        )));
    }
    // The node's seed wins: the recipe's own seed is only a default for a recipe read on its own.
    recipe.seed = node.seed;
    let built = eval(&recipe, palette).map_err(|errors| {
        ExpandError::Invalid(errors.iter().map(ToString::to_string).collect())
    })?;
    level_cells(&built, palette, node.footprint.at).map_err(|error| match error {
        LevelError::TooBig { cells } => ExpandError::TooBig { cells },
        other => invalid(other.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Footprint, Kind};

    /// Brick (index 1) and wood (indices 2 and 3), as the world-scale recipes name them.
    const PALETTE: &str = r#"{
        "version": 1,
        "materials": [
            { "name": "brick", "colours": [[150, 70, 50, 255]] },
            { "name": "wood", "colours": [[90, 50, 20, 255], [140, 90, 40, 255]] }
        ]
    }"#;

    /// A hollow brick box 10 by 6 by 8 with a gable roof and a door: 482 voxels, as the
    /// vox-art house test pins.
    const HOUSE: &str = r#"{
        "version": 1, "scale": "world", "seed": 1, "size": [10, 12, 8],
        "ops": [
            { "box": { "from": [0, 0, 0], "to": [9, 5, 7], "mat": "brick", "tone": 0, "hollow": true } },
            { "roof": { "over": [0, 0, 9, 7], "at_y": 6, "style": "gable", "along_x": true, "mat": "wood", "tone": 0 } },
            { "cut": { "from": [4, 0, 0], "to": [5, 2, 0] } }
        ]
    }"#;

    fn palette() -> Palette {
        Palette::from_json(PALETTE).unwrap()
    }

    fn house_node(recipe: &str) -> Node {
        let mut node = Node::new(1, Kind::House, Footprint::new([100, 0, 50], [10, 12, 8]));
        node.recipe = Some(recipe.to_string());
        node.seed = 9;
        node
    }

    /// The messages of a refused expansion; panics on any other outcome.
    fn invalid(result: Result<Vec<([i32; 3], String)>, ExpandError>) -> Vec<String> {
        match result {
            Err(ExpandError::Invalid(errors)) => errors,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn a_node_without_a_recipe_expands_to_nothing() {
        let node = Node::new(1, Kind::Well, Footprint::new([0, 0, 0], [2, 2, 2]));
        assert_eq!(expand(&node, &palette()), Ok(Vec::new()));
    }

    #[test]
    fn a_house_recipe_expands_at_the_footprint() {
        let cells = expand(&house_node(HOUSE), &palette()).unwrap();
        assert_eq!(cells.len(), 482);
        // The first cell in position order is the box's corner voxel, at the footprint's origin.
        assert_eq!(cells[0], ([100, 0, 50], "brick".to_string()));
        // Every cell lies inside the footprint.
        let footprint = Footprint::new([100, 0, 50], [10, 12, 8]);
        for (at, _) in &cells {
            assert!(footprint.contains(&Footprint::new(*at, [1, 1, 1])));
        }
    }

    #[test]
    fn expansion_is_the_same_twice() {
        let node = house_node(HOUSE);
        assert_eq!(
            expand(&node, &palette()).unwrap(),
            expand(&node, &palette()).unwrap()
        );
    }

    #[test]
    fn the_node_seed_is_used_for_the_recipe() {
        // The house is seeded from the recipe's seed 1; a node with seed 9 still expands, and the
        // same recipe with another node seed gives the same cells, because no op uses the seed.
        let mut a = house_node(HOUSE);
        let mut b = house_node(HOUSE);
        a.seed = 9;
        b.seed = 10;
        assert_eq!(expand(&a, &palette()), expand(&b, &palette()));
    }

    #[test]
    fn a_recipe_of_the_wrong_size_is_refused() {
        let mut node = house_node(HOUSE);
        node.footprint.size = [10, 12, 9];
        let errors = invalid(expand(&node, &palette()));
        assert!(errors[0].contains("does not match"));
    }

    #[test]
    fn a_recipe_of_the_wrong_scale_is_refused() {
        let recipe = HOUSE.replace(r#""scale": "world""#, r#""scale": "prop""#);
        let errors = invalid(expand(&house_node(&recipe), &palette()));
        assert!(errors[0].contains("world scale"));
    }

    #[test]
    fn a_recipe_that_does_not_parse_is_refused() {
        let errors = invalid(expand(&house_node("{not json"), &palette()));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn a_footprint_side_of_zero_or_past_the_cap_is_refused() {
        let mut node = house_node(HOUSE);
        node.footprint.size = [0, 65, 4];
        let errors = invalid(expand(&node, &palette()));
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("x side must be 1 to 64, got 0"));
        assert!(errors[1].contains("y side must be 1 to 64, got 65"));
    }

    #[test]
    fn a_recipe_that_fails_its_checks_reports_each_problem() {
        let recipe = HOUSE.replace(r#""mat": "brick""#, r#""mat": "gold""#);
        let errors = invalid(expand(&house_node(&recipe), &palette()));
        assert!(errors.iter().any(|e| e.contains("gold")));
    }

    #[test]
    fn a_build_over_the_cap_is_too_big() {
        // 40 cubed is 64,000 voxels, past the 32,768 cap.
        let recipe = r#"{"version":1,"scale":"world","seed":1,"size":[40,40,40],
            "ops":[{"box":{"from":[0,0,0],"to":[39,39,39],"mat":"brick","tone":0,"hollow":false}}]}"#;
        let mut node = house_node(recipe);
        node.footprint.size = [40, 40, 40];
        assert_eq!(expand(&node, &palette()), Err(ExpandError::TooBig { cells: 64_000 }));
    }
}
