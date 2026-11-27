//! Checks a recipe against its palette, then builds it into a voxel grid.
//!
//! Every problem is collected before anything is built, so one call reports all the mistakes in
//! a recipe, not only the first. Building is deterministic: operations run in order, from the
//! recipe's seed, so the same recipe gives the same voxels on every target.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::{MAX_OPS, MAX_PART_NAMES, MAX_VOXEL_WRITES, Op, RECIPE_VERSION, Recipe};
use crate::grid::VoxelGrid;
use crate::palette::Palette;
use crate::rng::Rng;

/// One problem with a recipe. `op` is the index of the operation it belongs to, or `None` for
/// a problem with the recipe as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeError {
    pub op: Option<usize>,
    pub message: String,
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.op {
            Some(index) => write!(f, "op {index}: {}", self.message),
            None => write!(f, "recipe: {}", self.message),
        }
    }
}

impl std::error::Error for RecipeError {}

/// A built recipe: the voxels, and the voxels of each named part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub grid: VoxelGrid,
    pub parts: BTreeMap<String, Vec<[u16; 3]>>,
}

/// What operations write into while a recipe is being built.
pub(crate) struct Env<'a> {
    pub(crate) palette: &'a Palette,
    pub(crate) grid: VoxelGrid,
    pub(crate) parts: BTreeMap<String, Vec<[u16; 3]>>,
}

/// Checks `recipe` against `palette` and builds it.
///
/// Returns every problem found, or the built grid when there are none.
pub fn eval(recipe: &Recipe, palette: &Palette) -> Result<Built, Vec<RecipeError>> {
    let mut errors = Vec::new();
    if recipe.version != RECIPE_VERSION {
        errors.push(RecipeError {
            op: None,
            message: format!("version must be {RECIPE_VERSION}, got {}", recipe.version),
        });
    }
    if recipe.ops.len() > MAX_OPS {
        errors.push(RecipeError {
            op: None,
            message: format!(
                "a recipe has at most {MAX_OPS} ops, got {}",
                recipe.ops.len()
            ),
        });
    }
    let grid = VoxelGrid::new(recipe.scale.scale(), recipe.size);
    if let Err(error) = &grid {
        errors.push(RecipeError {
            op: None,
            message: error.to_string(),
        });
    }
    // The write budget is summed over every op, even invalid ones: an overflow counts as over.
    let mut writes: u64 = 0;
    let mut part_names = BTreeSet::new();
    for (index, op) in recipe.ops.iter().enumerate() {
        for message in op.check(recipe.size, palette) {
            errors.push(RecipeError {
                op: Some(index),
                message,
            });
        }
        writes = writes.saturating_add(op.writes().unwrap_or(u64::MAX));
        if let Op::Part(part) = op
            && part_names.insert(part.name.as_str())
            && part_names.len() > MAX_PART_NAMES
        {
            errors.push(RecipeError {
                op: Some(index),
                message: format!(
                    "a recipe has at most {MAX_PART_NAMES} part names, got {}",
                    part_names.len()
                ),
            });
        }
    }
    if writes > MAX_VOXEL_WRITES {
        errors.push(RecipeError {
            op: None,
            message: format!(
                "a recipe may write at most {MAX_VOXEL_WRITES} voxels, this one writes {writes}"
            ),
        });
    }

    match grid {
        Ok(grid) if errors.is_empty() => {
            let mut env = Env {
                palette,
                grid,
                parts: BTreeMap::new(),
            };
            let mut rng = Rng::new(recipe.seed);
            for (index, op) in recipe.ops.iter().enumerate() {
                apply_op(op, index, &mut env, &mut rng)?;
            }
            Ok(Built {
                grid: env.grid,
                parts: env.parts,
            })
        }
        _ => Err(errors),
    }
}

fn apply_op(
    op: &Op,
    index: usize,
    env: &mut Env<'_>,
    rng: &mut Rng,
) -> Result<(), Vec<RecipeError>> {
    op.apply(env, rng).map_err(|message| {
        vec![RecipeError {
            op: Some(index),
            message,
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::MAX_RECIPE_BYTES;

    /// Wood has three tones (palette indices 1 to 3) and iron two (4 and 5).
    const PALETTE: &str = r#"{
        "version": 1,
        "materials": [
            { "name": "wood", "colours": [[90, 50, 20, 255], [140, 90, 40, 255], [190, 140, 80, 255]] },
            { "name": "iron", "colours": [[70, 70, 78, 255], [120, 120, 130, 255]] }
        ]
    }"#;

    const BARREL: &str = r#"{
        "version": 1, "scale": "prop", "seed": 1, "size": [8, 10, 8],
        "ops": [
            { "lathe": { "axis_at": [4, 4], "from_y": 0, "profile": [3, 4, 4, 4, 4, 4, 3], "mat": "wood", "tone": 1 } },
            { "lathe": { "axis_at": [4, 4], "from_y": 0, "profile": [4, 0, 0, 0, 0, 0, 4], "mat": "iron", "tone": 0 } }
        ]
    }"#;

    const STOOL: &str = r#"{
        "version": 1, "scale": "prop", "seed": 2, "size": [8, 8, 8],
        "ops": [
            { "lathe": { "axis_at": [4, 4], "from_y": 6, "profile": [3], "mat": "wood", "tone": 2 } },
            { "legs": { "at": [[1, 1], [5, 1], [1, 5], [5, 5]], "from_y": 0, "height": 6, "thick": 1, "mat": "wood", "tone": 0 } }
        ]
    }"#;

    const CRATE: &str = r#"{
        "version": 1, "scale": "prop", "seed": 3, "size": [8, 6, 8],
        "ops": [
            { "part": { "name": "body", "from": [1, 0, 1], "to": [6, 3, 6], "mat": "wood", "tone": 1 } },
            { "part": { "name": "lid", "from": [0, 4, 0], "to": [7, 4, 7], "mat": "wood", "tone": 2 } }
        ]
    }"#;

    fn palette() -> Palette {
        Palette::from_json(PALETTE).unwrap()
    }

    fn recipe(json: &str) -> Recipe {
        serde_json::from_str(json).unwrap()
    }

    /// BLAKE3 over every filled voxel in sorted order: x, y, z (u16 little-endian) then the
    /// palette index. The first eight bytes in hex are pinned by the golden tests.
    fn voxel_hash(grid: &VoxelGrid) -> String {
        let mut hasher = blake3::Hasher::new();
        for ([x, y, z], index) in grid.iter() {
            hasher.update(&x.to_le_bytes());
            hasher.update(&y.to_le_bytes());
            hasher.update(&z.to_le_bytes());
            hasher.update(&[index]);
        }
        let hash = hasher.finalize();
        hash.as_bytes()[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn golden_barrel() {
        let built = eval(&recipe(BARREL), &palette()).unwrap();
        // Layers of radius 4 hold 52 voxels and radius 3 hold 32; the iron hoops (layers 0 and 6)
        // overwrite the wood there, leaving 5 wood layers of 52 and 2 iron layers of 52.
        assert_eq!(built.grid.len(), 364);
        assert_eq!(voxel_hash(&built.grid), "5b0f3b4920b2cfe8");
    }

    #[test]
    fn golden_stool() {
        let built = eval(&recipe(STOOL), &palette()).unwrap();
        // Seat: 32 voxels in one layer. Four legs of 6 voxels each.
        assert_eq!(built.grid.len(), 56);
        assert_eq!(voxel_hash(&built.grid), "ed66c3a36fe05594");
    }

    #[test]
    fn golden_crate_with_lid_records_parts() {
        let built = eval(&recipe(CRATE), &palette()).unwrap();
        assert_eq!(built.grid.len(), 208);
        assert_eq!(built.parts["body"].len(), 144);
        assert_eq!(built.parts["lid"].len(), 64);
        assert_eq!(voxel_hash(&built.grid), "cfc35dc25b093924");
    }

    #[test]
    fn same_recipe_gives_same_voxels() {
        let a = eval(&recipe(BARREL), &palette()).unwrap();
        let b = eval(&recipe(BARREL), &palette()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn unknown_op_fails_to_parse() {
        let json =
            r#"{"version":1,"scale":"prop","seed":1,"size":[4,4,4],"ops":[{"sphere":{"r":2}}]}"#;
        assert!(serde_json::from_str::<Recipe>(json).is_err());
    }

    #[test]
    fn two_hundred_and_one_ops_are_refused() {
        let op = r#"{"part":{"name":"box","from":[0,0,0],"to":[0,0,0],"mat":"wood","tone":0}}"#;
        let ops = vec![op; MAX_OPS + 1].join(",");
        let json =
            format!(r#"{{"version":1,"scale":"prop","seed":1,"size":[4,4,4],"ops":[{ops}]}}"#);
        let errors = eval(&recipe(&json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, None);
        assert!(errors[0].message.contains("200"));
    }

    #[test]
    fn unknown_material_is_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{"part":{"name":"box","from":[0,0,0],"to":[1,1,1],"mat":"gold","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(0));
        assert!(errors[0].message.contains("gold"));
    }

    #[test]
    fn missing_tone_is_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{"part":{"name":"box","from":[0,0,0],"to":[1,1,1],"mat":"iron","tone":2}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert!(errors[0].message.contains("tone 2"));
    }

    #[test]
    fn coordinate_outside_the_size_is_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[8,8,8],
            "ops":[{"part":{"name":"box","from":[0,0,0],"to":[8,0,0],"mat":"wood","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(0));
    }

    #[test]
    fn lathe_radius_past_the_grid_edge_is_refused() {
        // Axis at x=2 with radius 3 would reach x=-1.
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[8,4,8],
            "ops":[{"lathe":{"axis_at":[2,4],"from_y":0,"profile":[3],"mat":"wood","tone":0}}]}"#;
        assert!(eval(&recipe(json), &palette()).is_err());
    }

    #[test]
    fn every_problem_is_reported_together() {
        let json = r#"{"version":2,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{"part":{"name":"box","from":[0,0,0],"to":[9,0,0],"mat":"gold","tone":0}},
                   {"legs":{"at":[[0,0]],"from_y":0,"height":1,"thick":9,"mat":"wood","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert!(errors.iter().any(|e| e.op.is_none()));
        assert!(errors.iter().any(|e| e.op == Some(0)));
        assert!(errors.iter().any(|e| e.op == Some(1)));
        assert!(errors.len() >= 4);
    }

    /// A part of `n` voxels per side, all of one palette index.
    fn cube_part(name: &str, side: u16) -> String {
        let last = side - 1;
        format!(
            r#"{{"part":{{"name":"{name}","from":[0,0,0],"to":[{last},{last},{last}],"mat":"wood","tone":0}}}}"#
        )
    }

    #[test]
    fn over_the_write_budget_is_refused() {
        // Five 64-cubes write 1,310,720 voxels, over the 1,048,576 budget; each op is valid alone.
        let ops: Vec<String> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| cube_part(name, 64))
            .collect();
        let json = format!(
            r#"{{"version":1,"scale":"prop","seed":1,"size":[64,64,64],"ops":[{}]}}"#,
            ops.join(",")
        );
        let errors = eval(&recipe(&json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, None);
        assert!(errors[0].message.contains("1048576"));
        assert!(errors[0].message.contains("1310720"));
    }

    #[test]
    fn repeated_part_name_replaces_the_earlier_voxels() {
        // The second "lid" is 2 voxels; it replaces the 4 of the first, but both are built.
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{"part":{"name":"lid","from":[0,0,0],"to":[3,0,0],"mat":"wood","tone":0}},
                   {"part":{"name":"lid","from":[0,1,0],"to":[1,1,0],"mat":"wood","tone":1}}]}"#;
        let built = eval(&recipe(json), &palette()).unwrap();
        assert_eq!(built.parts["lid"].len(), 2);
        assert_eq!(built.grid.len(), 6);
    }

    #[test]
    fn seventeen_part_names_are_refused() {
        let ops: Vec<String> = (0..=MAX_PART_NAMES)
            .map(|i| cube_part(&format!("p{i}"), 1))
            .collect();
        let json = format!(
            r#"{{"version":1,"scale":"prop","seed":1,"size":[4,4,4],"ops":[{}]}}"#,
            ops.join(",")
        );
        let errors = eval(&recipe(&json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(MAX_PART_NAMES));
        assert!(errors[0].message.contains("16 part names"));
    }

    #[test]
    fn sixteen_part_names_are_allowed() {
        let ops: Vec<String> = (0..MAX_PART_NAMES)
            .map(|i| cube_part(&format!("p{i}"), 1))
            .collect();
        let json = format!(
            r#"{{"version":1,"scale":"prop","seed":1,"size":[4,4,4],"ops":[{}]}}"#,
            ops.join(",")
        );
        let built = eval(&recipe(&json), &palette()).unwrap();
        assert_eq!(built.parts.len(), MAX_PART_NAMES);
    }

    #[test]
    fn recipe_text_over_the_cap_is_refused_before_parsing() {
        let text = " ".repeat(MAX_RECIPE_BYTES + 1);
        let error = Recipe::from_json(&text).unwrap_err();
        assert!(error.contains("16384"));
    }

    #[test]
    fn recipe_text_within_the_cap_parses() {
        assert!(Recipe::from_json(CRATE).is_ok());
    }

    #[test]
    fn profile_of_65_layers_is_refused() {
        let layers = vec!["1"; 65].join(",");
        let json = format!(
            r#"{{"version":1,"scale":"prop","seed":1,"size":[8,64,8],
            "ops":[{{"lathe":{{"axis_at":[4,4],"from_y":0,"profile":[{layers}],"mat":"wood","tone":0}}}}]}}"#
        );
        let errors = eval(&recipe(&json), &palette()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.op == Some(0) && e.message.contains("got 65"))
        );
    }

    #[test]
    fn legs_thicker_than_four_are_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[8,8,8],
            "ops":[{"legs":{"at":[[0,0]],"from_y":0,"height":1,"thick":5,"mat":"wood","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(0));
        assert!(errors[0].message.contains("got 5"));
    }

    #[test]
    fn nine_legs_are_refused() {
        let posts = ["[0,0]"; 9].join(",");
        let json = format!(
            r#"{{"version":1,"scale":"prop","seed":1,"size":[8,8,8],
            "ops":[{{"legs":{{"at":[{posts}],"from_y":0,"height":1,"thick":1,"mat":"wood","tone":0}}}}]}}"#
        );
        let errors = eval(&recipe(&json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("got 9"));
    }

    #[test]
    fn empty_legs_are_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[8,8,8],
            "ops":[{"legs":{"at":[],"from_y":0,"height":1,"thick":1,"mat":"wood","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("at least one post"));
    }

    #[test]
    fn part_name_with_a_control_character_is_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{"part":{"name":"a\u0007b","from":[0,0,0],"to":[0,0,0],"mat":"wood","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("control character"));
    }

    #[test]
    fn part_name_of_33_characters_is_refused() {
        let name = "x".repeat(33);
        let json = format!(
            r#"{{"version":1,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{{"part":{{"name":"{name}","from":[0,0,0],"to":[0,0,0],"mat":"wood","tone":0}}}}]}}"#
        );
        let errors = eval(&recipe(&json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("1 to 32"));
    }

    #[test]
    fn part_from_past_to_is_refused() {
        let json = r#"{"version":1,"scale":"prop","seed":1,"size":[4,4,4],
            "ops":[{"part":{"name":"box","from":[2,0,0],"to":[1,0,0],"mat":"wood","tone":0}}]}"#;
        let errors = eval(&recipe(json), &palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(0));
        assert!(errors[0].message.contains("past to"));
    }

    /// Brick and wood for the world-scale buildings: brick has one tone (palette index 1), wood
    /// two (indices 2 and 3).
    const BUILDING_PALETTE: &str = r#"{
        "version": 1,
        "materials": [
            { "name": "brick", "colours": [[150, 70, 50, 255]] },
            { "name": "wood", "colours": [[90, 50, 20, 255], [140, 90, 40, 255]] }
        ]
    }"#;

    /// A house at world scale: a hollow brick shell 10 by 6 by 8, a gable roof whose ridge runs
    /// along x, and a door 2 wide and 3 high cut into the front wall (the z = 0 face).
    const HOUSE: &str = r#"{
        "version": 1, "scale": "world", "seed": 1, "size": [10, 12, 8],
        "ops": [
            { "box": { "from": [0, 0, 0], "to": [9, 5, 7], "mat": "brick", "tone": 0, "hollow": true } },
            { "roof": { "over": [0, 0, 9, 7], "at_y": 6, "style": "gable", "along_x": true, "mat": "wood", "tone": 0 } },
            { "cut": { "from": [4, 0, 0], "to": [5, 2, 0] } }
        ]
    }"#;

    fn building_palette() -> Palette {
        Palette::from_json(BUILDING_PALETTE).unwrap()
    }

    #[test]
    fn golden_house_with_gable_roof_and_door() {
        let built = eval(&recipe(HOUSE), &building_palette()).unwrap();
        // The hollow shell of a 10 by 6 by 8 box has 480 - 8 * 4 * 6 = 288 voxels. The gable has
        // four layers over the 10-wide rectangle, 8 by 6 by 4 by 2 deep: 80 + 60 + 40 + 20 = 200.
        // The door takes 2 * 3 = 6 voxels from the front wall, leaving 482.
        assert_eq!(built.grid.len(), 482);
        assert_eq!(voxel_hash(&built.grid), "6e7a3d742f4d85ea");
    }

    #[test]
    fn golden_flat_roof() {
        let json = r#"{"version":1,"scale":"world","seed":1,"size":[4,1,3],
            "ops":[{"roof":{"over":[0,0,3,2],"at_y":0,"style":"flat","mat":"wood","tone":0}}]}"#;
        let built = eval(&recipe(json), &building_palette()).unwrap();
        // One layer of 4 by 3.
        assert_eq!(built.grid.len(), 12);
        assert_eq!(voxel_hash(&built.grid), "8f23b02c993d5fdc");
    }

    #[test]
    fn golden_stepped_roof() {
        let json = r#"{"version":1,"scale":"world","seed":1,"size":[12,3,4],
            "ops":[{"roof":{"over":[0,0,11,3],"at_y":0,"style":"stepped","mat":"wood","tone":0}}]}"#;
        let built = eval(&recipe(json), &building_palette()).unwrap();
        // Layers shrink by 2 a side per voxel up: 12 by 4, 8 by 4, 4 by 4, then empty.
        // 48 + 32 + 16 = 96.
        assert_eq!(built.grid.len(), 96);
        assert_eq!(voxel_hash(&built.grid), "a77a62251b1ca900");
    }

    #[test]
    fn roof_past_the_grid_top_is_refused() {
        // The house's gable tops out at height 9, so a grid 9 high is one too short.
        let json = HOUSE.replace(r#""size": [10, 12, 8]"#, r#""size": [10, 9, 8]"#);
        let errors = eval(&recipe(&json), &building_palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(1));
        assert!(errors[0].message.contains("above the grid"));
    }

    #[test]
    fn box_from_past_to_is_refused() {
        let json = r#"{"version":1,"scale":"world","seed":1,"size":[4,4,4],
            "ops":[{"box":{"from":[3,0,0],"to":[1,0,0],"mat":"brick","tone":0}}]}"#;
        let errors = eval(&recipe(json), &building_palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(0));
        assert!(errors[0].message.contains("past to"));
    }

    #[test]
    fn cut_outside_the_size_is_refused() {
        let json = r#"{"version":1,"scale":"world","seed":1,"size":[8,8,8],
            "ops":[{"cut":{"from":[0,0,0],"to":[8,0,0]}}]}"#;
        let errors = eval(&recipe(json), &building_palette()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].op, Some(0));
        assert!(errors[0].message.contains("outside a grid"));
    }
}
