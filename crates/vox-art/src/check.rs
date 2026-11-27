//! Checks a prop's def and its grid against the rules in `docs/design/props.md`, and returns
//! each problem as a [`Finding`] with a place and what to change.
//!
//! Only the checks that need no meshing or rendering are here: size, voxel budget, palette,
//! floating parts, thin walls, pivot, footprint, emitters, and the def's own validation. The
//! triangle budget, silhouette and proportion checks come later, with the mesher and renderer.
//! Every loop is bounded by the grid (at most 64 voxels a side), and the arithmetic is checked.

use std::collections::{BTreeMap, BTreeSet};

use crate::grid::VoxelGrid;
use crate::palette::Palette;
use crate::prop::{Behaviour, PropDef, Surface};

/// Most voxels a prop may have.
const MAX_VOXELS: usize = 20_000;
/// Most distinct palette colours one prop may use.
const MAX_COLOURS: usize = 24;
/// Most light behaviours (emitters) one prop may have.
const MAX_LIGHTS: usize = 4;
/// Widest, in voxels, a run of single-voxel-thick filled voxels may be before it is a warning.
const MAX_THIN_SPAN: u32 = 8;
/// Largest side a prop or its grid may have, in voxels.
const MAX_SIDE: u16 = 64;

/// Whether a finding stops acceptance or only goes to the reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Error,
    Warning,
}

/// One problem found by [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    /// Which check found it, as the props.md table names it.
    pub check: &'static str,
    /// Where in the grid, when the problem has a place.
    pub at: Option<[u16; 3]>,
    /// What is wrong and what to change.
    pub what: String,
}

impl Finding {
    fn error(check: &'static str, at: Option<[u16; 3]>, what: impl Into<String>) -> Self {
        Self {
            kind: Kind::Error,
            check,
            at,
            what: what.into(),
        }
    }

    fn warning(check: &'static str, at: Option<[u16; 3]>, what: impl Into<String>) -> Self {
        Self {
            kind: Kind::Warning,
            check,
            at,
            what: what.into(),
        }
    }
}

/// Checks `def` against its `grid` and `palette`. An empty result means no finding at all.
pub fn check(def: &PropDef, grid: &VoxelGrid, palette: &Palette) -> Vec<Finding> {
    let mut found = Vec::new();
    check_size(def, grid, &mut found);
    check_voxels(grid, &mut found);
    check_palette(grid, palette, &mut found);
    check_floating(def, grid, &mut found);
    check_thin_walls(grid, &mut found);
    check_pivot(def, grid, &mut found);
    check_footprint(def, grid, &mut found);
    check_emitters(def, &mut found);
    for reason in def.validate().err().unwrap_or_default() {
        found.push(Finding::error(
            "definition",
            None,
            format!("{}: {}", reason.field, reason.problem),
        ));
    }
    found
}

fn check_size(def: &PropDef, grid: &VoxelGrid, found: &mut Vec<Finding>) {
    if grid.size() != def.size {
        found.push(Finding::error(
            "size",
            None,
            format!(
                "grid is {:?} but the def's size is {:?}",
                grid.size(),
                def.size
            ),
        ));
    }
    for (axis, &side) in grid.size().iter().enumerate() {
        if side > MAX_SIDE {
            found.push(Finding::error(
                "size",
                None,
                format!("side {axis} is {side}; at most {MAX_SIDE} voxels a side"),
            ));
        }
    }
}

fn check_voxels(grid: &VoxelGrid, found: &mut Vec<Finding>) {
    if grid.len() > MAX_VOXELS {
        found.push(Finding::error(
            "voxels",
            None,
            format!("{} voxels; at most {MAX_VOXELS}", grid.len()),
        ));
    }
}

fn check_palette(grid: &VoxelGrid, palette: &Palette, found: &mut Vec<Finding>) {
    let colours = palette
        .materials
        .iter()
        .fold(0usize, |sum, m| sum.saturating_add(m.colours.len()));
    // The first place each colour index is used, so a finding can name it.
    let mut first_use: BTreeMap<u8, [u16; 3]> = BTreeMap::new();
    for (pos, index) in grid.iter() {
        first_use.entry(index).or_insert(pos);
    }
    for (&index, &pos) in &first_use {
        if usize::from(index) > colours {
            found.push(Finding::error(
                "palette",
                Some(pos),
                format!("colour index {index} is not in the palette of {colours} colours"),
            ));
        }
    }
    if first_use.len() > MAX_COLOURS {
        found.push(Finding::error(
            "palette",
            None,
            format!(
                "uses {} distinct colours; at most {MAX_COLOURS}",
                first_use.len()
            ),
        ));
    }
}

/// The filled voxels in 6-connected components, each sorted, in order of their first voxel.
fn components(grid: &VoxelGrid) -> Vec<Vec<[u16; 3]>> {
    let filled: BTreeSet<[u16; 3]> = grid.iter().map(|(pos, _)| pos).collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for &start in &filled {
        if !seen.insert(start) {
            continue;
        }
        let mut part = Vec::new();
        let mut stack = vec![start];
        while let Some(pos) = stack.pop() {
            part.push(pos);
            for axis in 0..3 {
                for delta in [-1, 1] {
                    if let Some(next) = step(pos, axis, delta, grid.size())
                        && filled.contains(&next)
                        && seen.insert(next)
                    {
                        stack.push(next);
                    }
                }
            }
        }
        part.sort_unstable();
        out.push(part);
    }
    out
}

/// The neighbour of `pos` one voxel along `axis`, or `None` past the grid's edge.
fn step(pos: [u16; 3], axis: usize, delta: i32, size: [u16; 3]) -> Option<[u16; 3]> {
    let moved = i32::from(pos[axis]).checked_add(delta)?;
    if moved < 0 || moved >= i32::from(size[axis]) {
        return None;
    }
    let mut next = pos;
    next[axis] = u16::try_from(moved).ok()?;
    Some(next)
}

/// Most floating parts reported one by one; the rest are counted in one finding.
const MAX_FLOATING_REPORTS: usize = 8;

/// Every filled voxel must belong to the body or to a part the def names. The body is the
/// largest component. A part counts as covering a separate component only when it contains
/// every voxel of that component; a part that only overlaps it does not. That is a deliberate
/// simplification: it accepts a named part exactly matching a detached piece, and nothing more.
///
/// The work stays linear in the voxels: each voxel is mapped once to the bitmask of the named
/// parts that hold it, and a component is covered when the AND of its voxels' masks is not
/// zero. A grid over [`MAX_VOXELS`] is not scanned at all, since its `voxels` error already
/// stops acceptance. Named parts past the 16th have no bit, so they cover nothing, which errs
/// towards a finding.
fn check_floating(def: &PropDef, grid: &VoxelGrid, found: &mut Vec<Finding>) {
    if grid.len() > MAX_VOXELS {
        return;
    }
    let parts = components(grid);
    let Some(body) = largest(&parts) else {
        return;
    };
    let mut owners: BTreeMap<[u16; 3], u16> = BTreeMap::new();
    for (bit, named) in def.parts.values().take(u16::BITS as usize).enumerate() {
        for &pos in &named.voxels {
            *owners.entry(pos).or_insert(0) |= 1u16 << bit;
        }
    }
    let mut reported = 0usize;
    let mut unreported = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if i == body {
            continue;
        }
        let covering = part.iter().fold(u16::MAX, |mask, pos| {
            mask & owners.get(pos).copied().unwrap_or(0)
        });
        if covering != 0 {
            continue;
        }
        if reported < MAX_FLOATING_REPORTS {
            reported += 1;
            found.push(Finding::error(
                "floating parts",
                part.first().copied(),
                format!(
                    "{} voxel(s) are not connected to the rest and no named part covers them",
                    part.len()
                ),
            ));
        } else {
            unreported += 1;
        }
    }
    if unreported > 0 {
        found.push(Finding::error(
            "floating parts",
            None,
            format!("and {unreported} more floating part(s)"),
        ));
    }
}

/// The index of the largest component, the first one of the largest size.
fn largest(parts: &[Vec<[u16; 3]>]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, part) in parts.iter().enumerate() {
        if best.is_none_or(|b| part.len() > parts[b].len()) {
            best = Some(i);
        }
    }
    best
}

/// A filled voxel is thin along an axis when both its neighbours on that axis are empty (or
/// outside the grid). The check groups the thin voxels that touch within the plane across that
/// axis and warns when a group is more than [`MAX_THIN_SPAN`] voxels across. This is an
/// approximation of "a slab one voxel thick": it sees a thin wall that bends, and a thin plate
/// at a surface edge, but not a thin wall that is thick in another place.
fn check_thin_walls(grid: &VoxelGrid, found: &mut Vec<Finding>) {
    let size = grid.size();
    let filled: BTreeSet<[u16; 3]> = grid.iter().map(|(pos, _)| pos).collect();
    for axis in 0..3 {
        let is_empty = |pos: Option<[u16; 3]>| pos.is_none_or(|p| !filled.contains(&p));
        let thin: BTreeSet<[u16; 3]> = filled
            .iter()
            .copied()
            .filter(|&pos| {
                is_empty(step(pos, axis, -1, size)) && is_empty(step(pos, axis, 1, size))
            })
            .collect();
        let mut seen = BTreeSet::new();
        for &start in &thin {
            if !seen.insert(start) {
                continue;
            }
            let mut group = Vec::new();
            let mut stack = vec![start];
            while let Some(pos) = stack.pop() {
                group.push(pos);
                // Connect only within the plane: the axis stays fixed.
                for other in (0..3).filter(|&a| a != axis) {
                    for delta in [-1, 1] {
                        if let Some(next) = step(pos, other, delta, size)
                            && thin.contains(&next)
                            && seen.insert(next)
                        {
                            stack.push(next);
                        }
                    }
                }
            }
            let span = (0..3)
                .filter(|&a| a != axis)
                .map(|a| {
                    let lo = group.iter().map(|p| p[a]).min().unwrap_or(0);
                    let hi = group.iter().map(|p| p[a]).max().unwrap_or(0);
                    u32::from(hi - lo) + 1
                })
                .max()
                .unwrap_or(0);
            if span > MAX_THIN_SPAN {
                group.sort_unstable();
                found.push(Finding::warning(
                    "thin walls",
                    group.first().copied(),
                    format!(
                        "a single voxel thick across {span} voxels (axis {axis}); it may shimmer"
                    ),
                ));
            }
        }
    }
}

fn check_pivot(def: &PropDef, grid: &VoxelGrid, found: &mut Vec<Finding>) {
    let size = grid.size();
    let inside = def.pivot.iter().zip(size).all(|(&p, side)| p < side);
    if !inside {
        found.push(Finding::error(
            "pivot",
            Some(def.pivot),
            format!("pivot {:?} is outside the grid {:?}", def.pivot, size),
        ));
    }
    let floor = def.place.on.contains(&Surface::Floor);
    if floor && def.pivot[1] != 0 {
        found.push(Finding::error(
            "pivot",
            Some(def.pivot),
            format!(
                "a floor prop's pivot must be on the ground plane (y 0), not y {}",
                def.pivot[1]
            ),
        ));
    }
}

/// Each voxel's column (x and z) falls in a 0.5 m cell. Cells are aligned to the grid origin, and
/// each is named relative to the pivot's cell, so `place.footprint` lists offsets from the
/// pivot. The cells the voxels cover must all be in the footprint. The height is measured from
/// the pivot's y (the ground for a floor prop) up to the top of the highest filled voxel, in
/// cells, which is what a placement has to leave free; a gap below the lowest voxel counts.
fn check_footprint(def: &PropDef, grid: &VoxelGrid, found: &mut Vec<Finding>) {
    if grid.is_empty() {
        return;
    }
    // One 0.5 m cell holds this many voxels along an axis: 5 at 10 cm, 10 at 5 cm.
    let per_cell = (0.5 / def.scale.scale().voxel_size()).round().max(1.0) as u16;
    let cell = |v: u16| i32::from(v / per_cell);
    let pivot_cell = (cell(def.pivot[0]), cell(def.pivot[2]));

    let allowed: BTreeSet<(i32, i32)> = def
        .place
        .footprint
        .iter()
        .map(|&[x, z]| (i32::from(x), i32::from(z)))
        .collect();
    let mut covered: BTreeMap<(i32, i32), [u16; 3]> = BTreeMap::new();
    for (pos, _) in grid.iter() {
        let rel = (cell(pos[0]) - pivot_cell.0, cell(pos[2]) - pivot_cell.1);
        covered.entry(rel).or_insert(pos);
    }
    for (&rel, &pos) in &covered {
        if !allowed.contains(&rel) {
            found.push(Finding::error(
                "footprint",
                Some(pos),
                format!(
                    "covers the 0.5 m cell {rel:?} from the pivot, which place.footprint does not list"
                ),
            ));
        }
    }

    let high = grid.iter().map(|(p, _)| p[1]).max().unwrap_or(0);
    let rise = (u32::from(high) + 1).saturating_sub(u32::from(def.pivot[1]));
    let cells_tall = rise.div_ceil(u32::from(per_cell));
    if cells_tall > u32::from(def.place.height) {
        found.push(Finding::error(
            "footprint",
            None,
            format!(
                "{} cells tall; place.height is {}",
                cells_tall, def.place.height
            ),
        ));
    }
}

fn check_emitters(def: &PropDef, found: &mut Vec<Finding>) {
    let lights = def
        .behaviours
        .iter()
        .filter(|b| matches!(b, Behaviour::Light(_)))
        .count();
    if lights > MAX_LIGHTS {
        found.push(Finding::error(
            "emitters",
            None,
            format!("{lights} light behaviours; at most {MAX_LIGHTS}"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Material;
    use crate::prop::Part;
    use crate::scale::Scale;

    /// A small valid prop: a solid 4 by 4 by 4 block on the floor, one colour.
    const BLOCK: &str = r#"{
        "id": "test.block",
        "name": "Test block",
        "scale": "prop",
        "size": [4, 4, 4],
        "pivot": [0, 0, 0],
        "place": { "on": ["floor"], "footprint": [[0, 0]], "height": 1 },
        "collide": "footprint",
        "palette_version": 1
    }"#;

    fn def(json: &str) -> PropDef {
        serde_json::from_str(json).unwrap()
    }

    fn palette(colours: usize) -> Palette {
        Palette {
            version: 1,
            materials: vec![Material {
                name: "test".to_string(),
                colours: vec![[90, 90, 90, 255]; colours],
            }],
        }
    }

    fn kinds(found: &[Finding], check_name: &str) -> Vec<Kind> {
        found
            .iter()
            .filter(|f| f.check == check_name)
            .map(|f| f.kind)
            .collect()
    }

    fn solid(size: [u16; 3], index: u8) -> VoxelGrid {
        let mut grid = VoxelGrid::new(Scale::Prop, size).unwrap();
        for x in 0..size[0] {
            for y in 0..size[1] {
                for z in 0..size[2] {
                    grid.set([x, y, z], index).unwrap();
                }
            }
        }
        grid
    }

    #[test]
    fn a_valid_small_prop_has_no_errors() {
        let found = check(&def(BLOCK), &solid([4, 4, 4], 1), &palette(1));
        assert!(
            found.iter().all(|f| f.kind != Kind::Error),
            "unexpected: {found:?}"
        );
    }

    #[test]
    fn two_separated_voxels_are_a_floating_part_error() {
        let mut grid = VoxelGrid::new(Scale::Prop, [3, 1, 1]).unwrap();
        grid.set([0, 0, 0], 1).unwrap();
        grid.set([2, 0, 0], 1).unwrap();
        let json = BLOCK.replace("[4, 4, 4]", "[3, 1, 1]");
        let found = check(&def(&json), &grid, &palette(1));
        assert_eq!(kinds(&found, "floating parts"), vec![Kind::Error]);
    }

    #[test]
    fn a_named_part_covering_a_detached_piece_is_accepted() {
        let mut grid = VoxelGrid::new(Scale::Prop, [3, 1, 1]).unwrap();
        grid.set([0, 0, 0], 1).unwrap();
        grid.set([2, 0, 0], 1).unwrap();
        let json = BLOCK.replace("[4, 4, 4]", "[3, 1, 1]").replace(
            "\"collide\"",
            "\"parts\": { \"knob\": { \"voxels\": [[2, 0, 0]] } }, \"collide\"",
        );
        let found = check(&def(&json), &grid, &palette(1));
        assert!(kinds(&found, "floating parts").is_empty(), "{found:?}");
    }

    #[test]
    fn a_one_thick_ten_by_ten_plate_is_a_thin_walls_warning() {
        let grid = solid([10, 1, 10], 1);
        let json = BLOCK.replace("[4, 4, 4]", "[10, 1, 10]");
        let found = check(&def(&json), &grid, &palette(1));
        assert_eq!(kinds(&found, "thin walls"), vec![Kind::Warning]);
    }

    #[test]
    fn thirty_colours_is_a_palette_error() {
        let mut grid = VoxelGrid::new(Scale::Prop, [30, 1, 1]).unwrap();
        for x in 0..30u16 {
            grid.set([x, 0, 0], u8::try_from(x + 1).unwrap()).unwrap();
        }
        let json = BLOCK.replace("[4, 4, 4]", "[30, 1, 1]");
        let found = check(&def(&json), &grid, &palette(30));
        assert_eq!(kinds(&found, "palette"), vec![Kind::Error]);
    }

    #[test]
    fn a_colour_index_outside_the_palette_is_a_palette_error() {
        let found = check(&def(BLOCK), &solid([4, 4, 4], 9), &palette(2));
        assert_eq!(kinds(&found, "palette"), vec![Kind::Error]);
    }

    #[test]
    fn a_floor_prop_with_its_pivot_raised_is_a_pivot_error() {
        let json = BLOCK.replace("\"pivot\": [0, 0, 0]", "\"pivot\": [0, 1, 0]");
        let found = check(&def(&json), &solid([4, 4, 4], 1), &palette(1));
        assert_eq!(kinds(&found, "pivot"), vec![Kind::Error]);
    }

    #[test]
    fn a_def_of_another_size_is_a_size_error() {
        let found = check(&def(BLOCK), &solid([2, 2, 2], 1), &palette(1));
        assert_eq!(kinds(&found, "size"), vec![Kind::Error]);
    }

    #[test]
    fn too_many_light_behaviours_is_an_emitters_error() {
        let light =
            r#"{ "light": { "socket": "s", "colour": "warm", "power": 1.0, "when": "always" } }"#;
        let lights = [light; 5].join(",");
        let json = BLOCK.replace(
            "\"collide\"",
            &format!(
                "\"sockets\": [{{ \"name\": \"s\", \"at\": [0, 0, 0], \"dir\": \"+y\" }}], \"behaviours\": [{lights}], \"collide\""
            ),
        );
        let found = check(&def(&json), &solid([4, 4, 4], 1), &palette(1));
        assert_eq!(kinds(&found, "emitters"), vec![Kind::Error]);
    }

    #[test]
    fn a_checkerboard_of_sixty_four_cubed_returns_with_the_voxel_error_only() {
        // 131072 isolated voxels: the component scan is skipped above MAX_VOXELS, so the
        // check must return quickly and report the budget, not one finding per voxel.
        let mut grid = VoxelGrid::new(Scale::Prop, [64, 64, 64]).unwrap();
        for x in 0..64u16 {
            for y in 0..64u16 {
                for z in 0..64u16 {
                    if (x + y + z) % 2 == 0 {
                        grid.set([x, y, z], 1).unwrap();
                    }
                }
            }
        }
        let mut prop = def(&BLOCK.replace("[4, 4, 4]", "[64, 64, 64]"));
        for bit in 0..16 {
            let voxels = (0..4096u16).map(|i| [i % 64, bit, i / 64]).collect();
            prop.parts.insert(format!("p{bit}"), Part { voxels });
        }
        let found = check(&prop, &grid, &palette(1));
        assert_eq!(kinds(&found, "voxels"), vec![Kind::Error]);
        assert!(kinds(&found, "floating parts").len() <= 9, "{found:?}");
    }

    #[test]
    fn floating_parts_are_capped_at_eight_then_counted() {
        // Twenty isolated voxels in a row: the body is the first, the other nineteen float.
        let mut grid = VoxelGrid::new(Scale::Prop, [40, 1, 1]).unwrap();
        for x in (0..40u16).step_by(2) {
            grid.set([x, 0, 0], 1).unwrap();
        }
        let json = BLOCK.replace("[4, 4, 4]", "[40, 1, 1]");
        let found = check(&def(&json), &grid, &palette(1));
        let floating: Vec<&Finding> = found
            .iter()
            .filter(|f| f.check == "floating parts")
            .collect();
        assert_eq!(floating.len(), MAX_FLOATING_REPORTS + 1, "{found:?}");
        assert!(floating.last().unwrap().what.starts_with("and 11 more"));
    }

    #[test]
    fn height_is_measured_from_the_pivot_so_a_gap_above_it_counts() {
        // One voxel at y 5 above a pivot at y 0: 6 voxels of rise, which is 2 cells of 5.
        let mut grid = VoxelGrid::new(Scale::Prop, [4, 8, 4]).unwrap();
        grid.set([0, 5, 0], 1).unwrap();
        let json = BLOCK.replace("[4, 4, 4]", "[4, 8, 4]");
        let found = check(&def(&json), &grid, &palette(1));
        assert!(
            found
                .iter()
                .any(|f| f.check == "footprint" && f.what == "2 cells tall; place.height is 1"),
            "{found:?}"
        );
    }

    #[test]
    fn footprint_cells_are_relative_to_the_pivots_cell() {
        // Pivot [5, 0, 5] is in cell (1, 1) of the grid's 5-voxel cells. Voxels at x and z 0..10
        // cover cells 0 and 1 on each axis, so relative to the pivot: (-1, -1), (-1, 0),
        // (0, -1) and (0, 0).
        let mut grid = VoxelGrid::new(Scale::Prop, [10, 1, 10]).unwrap();
        for x in 0..10u16 {
            for z in 0..10u16 {
                grid.set([x, 0, z], 1).unwrap();
            }
        }
        let base = BLOCK
            .replace("[4, 4, 4]", "[10, 1, 10]")
            .replace("\"pivot\": [0, 0, 0]", "\"pivot\": [5, 0, 5]");

        let all_four = base.replace(
            "\"footprint\": [[0, 0]]",
            "\"footprint\": [[-1, -1], [-1, 0], [0, -1], [0, 0]]",
        );
        let found = check(&def(&all_four), &grid, &palette(1));
        assert!(kinds(&found, "footprint").is_empty(), "{found:?}");

        let only_origin = check(&def(&base), &grid, &palette(1));
        let reported: Vec<String> = only_origin
            .iter()
            .filter(|f| f.check == "footprint")
            .map(|f| f.what.clone())
            .collect();
        assert_eq!(reported.len(), 3, "{reported:?}");
        for cell in ["(-1, -1)", "(-1, 0)", "(0, -1)"] {
            assert!(
                reported.iter().any(|w| w.contains(cell)),
                "{cell}: {reported:?}"
            );
        }
    }
}
