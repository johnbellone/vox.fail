//! Who may change a world's entities, and whether a change is allowed: the rules behind
//! `Put` and `Remove` (docs/design/world-editor.md, "Who may edit what" and "Security").
//!
//! These are pure functions over state the caller gathers (the plots, the player's feet, what
//! the zone already holds, the blocks under a spawn), so the server, the directory and the
//! tests all run the same checks. No I/O, no threads, no clocks: wasm-safe.
//!
//! [`Component::validate`] checks the values; this module checks rights, destinations, place
//! and caps. Each refusal is a distinct [`EntityWhy`] so a client can say why.

use crate::entity::{Component, Dest, EntityWhy, Prop, prop_cell};
use crate::world::{Bounds, Cell};

/// Most entities one zone holds.
pub const MAX_ZONE_ENTITIES: usize = 4_000;
/// Most player portals one plot holds (the directory's PORTALS_PER_PLOT).
pub const PORTALS_PER_PLOT: usize = 2;
/// Most props one plot holds (docs/design/props.md, "Placing").
pub const PROPS_PER_PLOT: usize = 64;
/// How far, in blocks from the eyes, a player may edit. The same value as vox-server's REACH
/// until the server switches to this module; its own copy is then removed.
pub const REACH: f32 = 16.0;
/// Eye height above the feet, in blocks. The same value as vox-server's EYES.
pub const EYES: f32 = 3.2;
/// A character's height, in blocks (vox-core::body's player box).
const CHARACTER_HEIGHT: f32 = 3.6;

/// What a player may do in a world: a plot's builder (basic) or the world's owner or one of
/// its editors (advanced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Builder,
    Editor,
}

/// A plot's footprint and the accounts that may build in it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotArea {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
    pub builders: Vec<u64>,
}

impl PlotArea {
    /// Is the column at (x, z) inside the plot? Lo inclusive, hi exclusive, as the server's
    /// Plot::holds.
    pub fn holds(&self, x: i32, z: i32) -> bool {
        (self.x0..self.x1).contains(&x) && (self.z0..self.z1).contains(&z)
    }
}

/// Everything a check needs about the edit and the player making it.
pub struct Ctx<'a> {
    pub role: Role,
    /// None for a guest, who may edit nothing.
    pub account: Option<u64>,
    /// The player's feet, if known. Reach is measured from here.
    pub at: Option<[f32; 3]>,
    pub bounds: Bounds,
    pub plots: &'a [PlotArea],
    /// Entities already in the target zone.
    pub zone_entities: usize,
    /// An existing entity is moving into a zone other than its own. The server works the zones
    /// out from the two positions (vox-core keeps no zone grid), and the target zone's cap then
    /// applies as it does to a new entity.
    pub moves_zone: bool,
    /// The player's portals already in the target plot.
    pub plot_portals: usize,
    /// Spawn points in the world now (the last one may not be removed).
    pub spawns: usize,
    /// Is that cell solid?
    pub ground: &'a dyn Fn(Cell) -> bool,
    /// Props already in the plot a prop lands in (the caller counts for the plot the new
    /// position falls in).
    pub props_in_plot: usize,
    /// Is the prop's footprint free: no solid block and no other prop's footprint under it?
    /// The caller answers from its occupancy and must leave out the entity being changed, or
    /// a prop could never be turned in place.
    pub prop_footprint_free: &'a dyn Fn(&Prop) -> bool,
    /// Does the world accept this prop (its version hash)?
    pub known_prop: &'a dyn Fn(&[u8; 16]) -> bool,
}

/// The cell a position stands in: the floor of each coordinate.
fn cell_of(at: [f32; 3]) -> Cell {
    [at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32]
}

/// Is the cell within reach of the player's eyes? Measured from the middle of the cell.
/// Without a known position nothing is in reach.
fn within_reach(at: Option<[f32; 3]>, cell: Cell) -> bool {
    let Some(at) = at else { return false };
    let eye = [at[0], at[1] + EYES, at[2]];
    let d2: f32 = (0..3).map(|i| (cell[i] as f32 + 0.5 - eye[i]).powi(2)).sum();
    d2 <= REACH * REACH
}

/// Is the cell inside a plot where `account` builds?
fn in_own_plot(plots: &[PlotArea], account: u64, cell: Cell) -> bool {
    plots
        .iter()
        .any(|p| p.holds(cell[0], cell[2]) && p.builders.contains(&account))
}

/// The index of the plot holding a cell, if one does.
fn plot_index(plots: &[PlotArea], cell: Cell) -> Option<usize> {
    plots.iter().position(|p| p.holds(cell[0], cell[2]))
}

/// A cell must be inside the world and within the player's reach.
fn check_cell(ctx: &Ctx, cell: Cell) -> Result<(), EntityWhy> {
    if !ctx.bounds.contains(cell) {
        return Err(EntityWhy::Outside);
    }
    if !within_reach(ctx.at, cell) {
        return Err(EntityWhy::TooFar);
    }
    Ok(())
}

/// A spawn needs room to stand: the cell under the feet solid, and the character's height
/// above the feet clear. Only the centre column is checked.
fn check_standing(ctx: &Ctx, at: [f32; 3]) -> Result<(), EntityWhy> {
    let [x, y, z] = cell_of(at);
    if !(ctx.ground)([x, y - 1, z]) {
        return Err(EntityWhy::Blocked);
    }
    let cells = CHARACTER_HEIGHT.ceil() as i32;
    if (0..cells).any(|dy| (ctx.ground)([x, y + dy, z])) {
        return Err(EntityWhy::Blocked);
    }
    Ok(())
}

/// A world's id: 1 to 64 of `a-z`, `0-9`, `-` and `_`. The same rule as bins/vox-host's
/// `valid_id` (vox-core cannot depend on the host, so the two copies must agree).
fn valid_world_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// A `Url` destination the editor may use: `":<port>"` for this host (digits only, no leading
/// zero, 1 to 65535), or `"/w/<id>/ws"` for a world on it. A whole `ws://` or `wss://` address
/// is refused: the client would dial it with a ticket.
fn valid_url(url: &str) -> bool {
    if let Some(port) = url.strip_prefix(':') {
        return (1..=5).contains(&port.len())
            && port.bytes().all(|b| b.is_ascii_digit())
            && !port.starts_with('0')
            && port.parse::<u32>().is_ok_and(|p| (1..=65535).contains(&p));
    }
    if let Some(id) = url.strip_prefix("/w/").and_then(|r| r.strip_suffix("/ws")) {
        return valid_world_id(id);
    }
    false
}

/// A dungeon name: 1 to 64 of `a-z`, `0-9`, `_` and `-`.
fn valid_dungeon(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// May this player place or change this entity? `old` is what is there now (None for a new
/// entity). Checked in order: the values, the rights (a portal's key is the directory's: it is
/// stamped after the directory charges, so a client never sets one and a changed one is
/// refused), the destination, the size, the caps, then the place.
///
/// A prop's owner is the account that placed it: a new prop must name the placer, and an edit
/// keeps the owner it had, so nobody hands a prop over or takes one. Editors place props
/// anywhere in reach; the plot cap then applies to a prop only when it lies in a plot.
///
/// Reach applies to every role ("anywhere in the world, within reach of where you stand"). With
/// no known position (`ctx.at` None) nothing is in reach, so the check fails closed.
pub fn check_put(ctx: &Ctx, old: Option<&Component>, new: &Component) -> Result<(), EntityWhy> {
    new.validate()?;
    let Some(account) = ctx.account else {
        return Err(EntityWhy::NotYours);
    };
    // An entity keeps its kind: a settings entity is never made a spawn, nor a portal a spawn
    // (which would drop the portal's key and dodge its charge).
    if old.is_some_and(|o| std::mem::discriminant(o) != std::mem::discriminant(new)) {
        return Err(EntityWhy::NotYours);
    }

    if let Component::Portal(p) = new {
        // The key a portal keeps: an old portal's, or none for a new one.
        let kept = match old {
            Some(Component::Portal(o)) => o.key,
            _ => None,
        };
        if p.key != kept {
            return Err(EntityWhy::NotYours);
        }
        // A keyed portal's name and level come from the directory, which filled them in when
        // it charged the portal; nobody may change them by a Put, whatever their role.
        if let Some(Component::Portal(o)) = old
            && o.key.is_some()
            && (p.name != o.name || p.level != o.level)
        {
            return Err(EntityWhy::NotYours);
        }
    }

    if let Component::Prop(p) = new {
        if !(ctx.known_prop)(&p.prop) {
            return Err(EntityWhy::NoSuchProp);
        }
        let owner_kept = match old {
            Some(Component::Prop(o)) => p.owner == o.owner,
            _ => p.owner == Some(account),
        };
        if !owner_kept {
            return Err(EntityWhy::NotYours);
        }
    }

    if ctx.role == Role::Builder {
        match new {
            Component::Portal(p) => {
                // Basic builders place and move portals in their own plot and nothing else.
                if let Some(old) = old {
                    let Component::Portal(o) = old else {
                        return Err(EntityWhy::NotYours);
                    };
                    // A portal from a level file has no key and is not a player's to move. A
                    // new portal has no key yet (checked above), so only an existing one must
                    // have one.
                    if o.key.is_none() || !in_own_plot(ctx.plots, account, cell_of(o.at)) {
                        return Err(EntityWhy::NotYours);
                    }
                }
                if !in_own_plot(ctx.plots, account, cell_of(p.at)) {
                    return Err(EntityWhy::NotYours);
                }
                match &p.to {
                    Dest::World { world } if valid_world_id(world) => {}
                    // A pair's key names another doorway; only the directory knows which exist,
                    // so it checks the key.
                    Dest::Pair { .. } => {}
                    _ => return Err(EntityWhy::BadDest),
                }
                let [w, h] = p.size;
                if !(4.0..=8.0).contains(&w) || !(6.0..=16.0).contains(&h) {
                    return Err(EntityWhy::TooBig);
                }
                if old.is_none() && ctx.plot_portals >= PORTALS_PER_PLOT {
                    return Err(EntityWhy::TooMany);
                }
            }
            Component::Prop(p) => {
                // A builder places props in their own plot, and moves or changes only the
                // props they placed (the owner was checked above to be unchanged).
                if let Some(Component::Prop(o)) = old
                    && (o.owner != Some(account) || !in_own_plot(ctx.plots, account, prop_cell(o.at)))
                {
                    return Err(EntityWhy::NotYours);
                }
                if !in_own_plot(ctx.plots, account, prop_cell(p.at)) {
                    return Err(EntityWhy::NotYours);
                }
            }
            _ => return Err(EntityWhy::NotYours),
        }
    }

    if let (Role::Editor, Component::Portal(p)) = (ctx.role, new) {
        match &p.to {
            Dest::Url { url, .. } if !valid_url(url) => return Err(EntityWhy::BadDest),
            Dest::World { world } if !valid_world_id(world) => return Err(EntityWhy::BadDest),
            Dest::Dungeon { name, .. } if !valid_dungeon(name) => return Err(EntityWhy::BadDest),
            _ => {}
        }
    }

    // Settings have no position; everything else is placed, in the cell it stands in.
    let cell = match new {
        Component::Portal(p) => Some(cell_of(p.at)),
        Component::Spawn(s) => Some(cell_of(s.at)),
        Component::Objective(o) => Some(cell_of(o.at)),
        Component::Prop(p) => Some(prop_cell(p.at)),
        Component::Node(n) => Some(n.at),
        Component::Settings(_) => None,
    };
    if let Some(cell) = cell {
        check_cell(ctx, cell)?;
    }

    // A new entity, or one moving into another zone, needs room in the zone it lands in.
    if (old.is_none() || ctx.moves_zone) && ctx.zone_entities >= MAX_ZONE_ENTITIES {
        return Err(EntityWhy::TooMany);
    }

    if let Component::Prop(p) = new {
        let here = prop_cell(p.at);
        // A prop that is new here, or moves into another plot, needs room in that plot.
        let entering = match old {
            Some(Component::Prop(o)) => plot_index(ctx.plots, prop_cell(o.at)) != plot_index(ctx.plots, here),
            _ => true,
        };
        if entering && plot_index(ctx.plots, here).is_some() && ctx.props_in_plot >= PROPS_PER_PLOT {
            return Err(EntityWhy::TooMany);
        }
        // Turning or moving a prop needs its footprint free; a change of text alone does not.
        // a variant may change what the prop covers, so it counts as part of the placement
        let placement_kept = matches!(
            old,
            Some(Component::Prop(o)) if o.prop == p.prop && o.at == p.at && o.yaw == p.yaw && o.variant == p.variant
        );
        if !placement_kept && !(ctx.prop_footprint_free)(p) {
            return Err(EntityWhy::Blocked);
        }
    }

    if let Component::Spawn(s) = new {
        check_standing(ctx, s.at)?;
    }
    Ok(())
}

/// May this player remove this entity? Basic builders remove only their own plot's player
/// portals and the props they placed there. Advanced editors remove anything except the
/// world's settings (never removable) and its last spawn point (a world needs one). Reach
/// applies to every removal, as to a Put.
///
/// Refusal choices: removing settings is `NotYours` (it is the world's own, not an entity
/// anyone owns); removing the last spawn is `Blocked`.
pub fn check_remove(ctx: &Ctx, old: &Component) -> Result<(), EntityWhy> {
    let Some(account) = ctx.account else {
        return Err(EntityWhy::NotYours);
    };
    let cell = match (ctx.role, old) {
        (Role::Builder, Component::Portal(p)) => {
            if p.key.is_none() || !in_own_plot(ctx.plots, account, cell_of(p.at)) {
                return Err(EntityWhy::NotYours);
            }
            Some(cell_of(p.at))
        }
        (Role::Builder, Component::Prop(p)) => {
            if p.owner != Some(account) || !in_own_plot(ctx.plots, account, prop_cell(p.at)) {
                return Err(EntityWhy::NotYours);
            }
            Some(prop_cell(p.at))
        }
        (Role::Builder, _) => return Err(EntityWhy::NotYours),
        (Role::Editor, Component::Settings(_)) => return Err(EntityWhy::NotYours),
        (Role::Editor, Component::Spawn(_)) if ctx.spawns <= 1 => return Err(EntityWhy::Blocked),
        (Role::Editor, Component::Portal(p)) => Some(cell_of(p.at)),
        (Role::Editor, Component::Spawn(s)) => Some(cell_of(s.at)),
        (Role::Editor, Component::Objective(o)) => Some(cell_of(o.at)),
        (Role::Editor, Component::Prop(p)) => Some(prop_cell(p.at)),
        (Role::Editor, Component::Node(n)) => Some(n.at),
    };
    if let Some(cell) = cell {
        check_cell(ctx, cell)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Axis, Objective, Portal, Prop, Settings, Spawn};

    /// The prop every test places: a catalogue name nobody's world refuses by default.
    const HASH: [u8; 16] = [9; 16];

    fn any_prop(_: &[u8; 16]) -> bool {
        true
    }

    fn free_footprint(_: &Prop) -> bool {
        true
    }

    fn floor(c: Cell) -> bool {
        c[1] < 0
    }

    fn no_ground(_: Cell) -> bool {
        false
    }

    /// A floor at y = -1, and a ceiling at y = 2 above the spawn column.
    fn ceiling(c: Cell) -> bool {
        c[1] < 0 || c[1] == 2
    }

    fn plot(builders: &[u64]) -> PlotArea {
        PlotArea { x0: 0, z0: 0, x1: 10, z1: 10, builders: builders.to_vec() }
    }

    fn ctx<'a>(
        role: Role,
        account: Option<u64>,
        at: Option<[f32; 3]>,
        plots: &'a [PlotArea],
        ground: &'a dyn Fn(Cell) -> bool,
    ) -> Ctx<'a> {
        Ctx {
            role,
            account,
            at,
            bounds: Bounds::default(),
            plots,
            zone_entities: 0,
            moves_zone: false,
            plot_portals: 0,
            spawns: 2,
            ground,
            props_in_plot: 0,
            prop_footprint_free: &free_footprint,
            known_prop: &any_prop,
        }
    }

    /// A prop at `at` (5 cm units) owned by `owner`.
    fn prop(at: [i32; 3], owner: Option<u64>) -> Prop {
        Prop { prop: HASH, at, yaw: 0, variant: 0, state: 0, channel: None, text: None, owner }
    }

    fn portal(at: [f32; 3], to: Dest) -> Portal {
        Portal {
            name: "door".into(),
            at,
            axis: Axis::X,
            size: [4.0, 6.0],
            yaw: 0.0,
            level: "yard".into(),
            to,
            key: None,
        }
    }

    fn world_dest() -> Dest {
        Dest::World { world: "yard".into() }
    }

    fn url_dest(url: &str) -> Dest {
        Dest::Url { url: url.into(), arrive: [0.0; 3] }
    }

    #[test]
    fn builder_outside_plot_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([50.5, 0.0, 5.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_in_own_plot_is_ok() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([5.5, 0.0, 5.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Ok(()));
    }

    #[test]
    fn builder_non_portal_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let s = Component::Spawn(Spawn { at: [5.5, 0.0, 5.5], yaw: 0.0 });
        assert_eq!(check_put(&c, None, &s), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_url_dest_is_bad_dest() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(":7781")));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::BadDest));
    }

    #[test]
    fn builder_too_big_is_refused() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let mut p = portal([5.5, 0.0, 5.5], world_dest());
        p.size = [9.0, 6.0];
        assert_eq!(check_put(&c, None, &Component::Portal(p)), Err(EntityWhy::TooBig));
    }

    #[test]
    fn builder_plot_portal_cap_is_too_many() {
        let plots = [plot(&[7])];
        let mut c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        c.plot_portals = PORTALS_PER_PLOT;
        let p = Component::Portal(portal([5.5, 0.0, 5.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::TooMany));
    }

    #[test]
    fn guest_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Editor, None, Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(":7781")));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_wss_address_is_bad_dest() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        for url in ["wss://vox.example:443/ws", "ws://vox.example:7781/ws"] {
            let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(url)));
            assert_eq!(check_put(&c, None, &p), Err(EntityWhy::BadDest), "{url}");
        }
    }

    #[test]
    fn editor_port_destination_is_ok() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(":7781")));
        assert_eq!(check_put(&c, None, &p), Ok(()));
    }

    #[test]
    fn editor_world_path_destination_is_ok() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest("/w/yard/ws")));
        assert_eq!(check_put(&c, None, &p), Ok(()));
    }

    #[test]
    fn editor_bad_dungeon_name_is_bad_dest() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let dest = Dest::Dungeon { name: "Crypt!".into(), arrive: [0.0; 3] };
        let p = Component::Portal(portal([5.5, 0.0, 5.5], dest));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::BadDest));
    }

    #[test]
    fn position_outside_bounds_is_outside() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([299.0, 0.0, 0.0]), &plots, &floor);
        let p = Component::Portal(portal([300.5, 0.0, 0.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::Outside));
    }

    #[test]
    fn position_out_of_reach_is_too_far() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([0.0, 0.0, 0.0]), &plots, &floor);
        let p = Component::Portal(portal([100.5, 0.0, 0.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::TooFar));
    }

    #[test]
    fn zone_cap_refuses_a_new_entity() {
        let plots: [PlotArea; 0] = [];
        let mut c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        c.zone_entities = MAX_ZONE_ENTITIES;
        let p = Component::Portal(portal([5.5, 0.0, 5.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::TooMany));
        // Moving an existing one is not a new entity (it keeps its key).
        let mut old = portal([5.5, 0.0, 5.5], world_dest());
        old.key = Some(3);
        let old = Component::Portal(old);
        let mut moved = portal([5.5, 0.0, 5.5], world_dest());
        moved.key = Some(3);
        assert_eq!(check_put(&c, Some(&old), &Component::Portal(moved)), Ok(()));
    }

    #[test]
    fn spawn_without_floor_is_blocked() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([0.5, 0.0, 0.5]), &plots, &no_ground);
        let s = Component::Spawn(Spawn { at: [0.5, 0.0, 0.5], yaw: 0.0 });
        assert_eq!(check_put(&c, None, &s), Err(EntityWhy::Blocked));
    }

    #[test]
    fn spawn_on_floor_is_ok() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([0.5, 0.0, 0.5]), &plots, &floor);
        let s = Component::Spawn(Spawn { at: [0.5, 0.0, 0.5], yaw: 0.0 });
        assert_eq!(check_put(&c, None, &s), Ok(()));
    }

    #[test]
    fn spawn_under_ceiling_is_blocked() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([0.5, 0.0, 0.5]), &plots, &ceiling);
        let s = Component::Spawn(Spawn { at: [0.5, 0.0, 0.5], yaw: 0.0 });
        assert_eq!(check_put(&c, None, &s), Err(EntityWhy::Blocked));
    }

    fn node_at(at: Cell) -> Component {
        Component::Node(crate::entity::Node {
            kind: "house".into(),
            detail: 0,
            at,
            size: [4, 4, 4],
            parent: None,
            recipe: None,
            seed: 0,
        })
    }

    #[test]
    fn builder_node_is_not_yours_and_editor_node_is_placed_in_reach() {
        let plots = [plot(&[1])];
        let builder = ctx(Role::Builder, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        assert_eq!(check_put(&builder, None, &node_at([5, 0, 5])), Err(EntityWhy::NotYours));
        let editor = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        assert_eq!(check_put(&editor, None, &node_at([5, 0, 5])), Ok(()));
        assert_eq!(check_put(&editor, None, &node_at([100, 0, 5])), Err(EntityWhy::TooFar));
    }

    #[test]
    fn editor_may_remove_a_node_in_reach_and_builder_may_not() {
        let plots = [plot(&[1])];
        let builder = ctx(Role::Builder, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        assert_eq!(check_remove(&builder, &node_at([5, 0, 5])), Err(EntityWhy::NotYours));
        let editor = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        assert_eq!(check_remove(&editor, &node_at([5, 0, 5])), Ok(()));
    }

    #[test]
    fn objective_outside_bounds_is_outside() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([255.0, 0.0, 0.0]), &plots, &floor);
        let o = Component::Objective(Objective { name: "altar".into(), at: [-300.5, 0.0, 0.5], radius: 3.0 });
        assert_eq!(check_put(&c, None, &o), Err(EntityWhy::Outside));
    }

    #[test]
    fn settings_need_no_position() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), None, &plots, &floor);
        let s = Component::Settings(Settings { json: "{}".into() });
        assert_eq!(check_put(&c, None, &s), Ok(()));
    }

    #[test]
    fn builder_cannot_remove_settings() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let s = Component::Settings(Settings { json: "{}".into() });
        assert_eq!(check_remove(&c, &s), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_cannot_remove_settings() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let s = Component::Settings(Settings { json: "{}".into() });
        assert_eq!(check_remove(&c, &s), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_cannot_remove_last_spawn() {
        let plots: [PlotArea; 0] = [];
        let mut c = ctx(Role::Editor, Some(1), Some([0.5, 0.0, 0.5]), &plots, &floor);
        c.spawns = 1;
        let s = Component::Spawn(Spawn { at: [0.5, 0.0, 0.5], yaw: 0.0 });
        assert_eq!(check_remove(&c, &s), Err(EntityWhy::Blocked));
        c.spawns = 2;
        assert_eq!(check_remove(&c, &s), Ok(()));
    }

    #[test]
    fn builder_cannot_remove_another_builders_portal() {
        let plots = [plot(&[8])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let mut p = portal([5.5, 0.0, 5.5], world_dest());
        p.key = Some(3);
        assert_eq!(check_remove(&c, &Component::Portal(p)), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_can_remove_own_keyed_portal() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let mut p = portal([5.5, 0.0, 5.5], world_dest());
        p.key = Some(3);
        assert_eq!(check_remove(&c, &Component::Portal(p)), Ok(()));
    }

    fn keyed(at: [f32; 3], to: Dest, key: i64) -> Portal {
        let mut p = portal(at, to);
        p.key = Some(key);
        p
    }

    #[test]
    fn builder_changing_a_portal_key_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let new = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 4));
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_clearing_a_portal_key_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let new = Component::Portal(portal([5.5, 0.0, 5.5], world_dest()));
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_renaming_a_keyed_portal_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let mut p = keyed([5.5, 0.0, 5.5], world_dest(), 3);
        p.name = "renamed".into();
        assert_eq!(check_put(&c, Some(&old), &Component::Portal(p)), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_relevelling_a_keyed_portal_is_not_yours() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let mut p = keyed([5.5, 0.0, 5.5], world_dest(), 3);
        p.level = "orbit".into();
        assert_eq!(check_put(&c, Some(&old), &Component::Portal(p)), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_renaming_an_unkeyed_portal_is_ok() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(portal([5.5, 0.0, 5.5], world_dest()));
        let mut p = portal([5.5, 0.0, 5.5], world_dest());
        p.name = "renamed".into();
        assert_eq!(check_put(&c, Some(&old), &Component::Portal(p)), Ok(()));
    }

    #[test]
    fn builder_new_portal_with_a_key_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let new = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        assert_eq!(check_put(&c, None, &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_new_portal_with_a_key_is_not_yours() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let new = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        assert_eq!(check_put(&c, None, &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_changing_a_keyed_portal_key_is_not_yours() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let new = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 4));
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_cannot_move_a_portal_from_a_foreign_plot_into_its_own() {
        let plots = [
            plot(&[7]),
            PlotArea { x0: 20, z0: 0, x1: 30, z1: 10, builders: vec![8] },
        ];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([25.5, 0.0, 5.5], world_dest(), 3));
        let new = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_world_dest_with_a_bad_id_is_bad_dest() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        for id in ["Yard", "a:b", "", "a/b"] {
            let p = Component::Portal(portal([5.5, 0.0, 5.5], Dest::World { world: id.into() }));
            assert_eq!(check_put(&c, None, &p), Err(EntityWhy::BadDest), "{id}");
        }
    }

    #[test]
    fn url_destinations_follow_the_host_id_rule() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let ok = [":7781", ":1", ":65535", "/w/yard/ws", "/w/atlas-ea-1-2/ws"];
        let bad = [
            ":0",
            ":65536",
            ":+80",
            ":-1",
            ": 80",
            ":00080",
            ":\u{0661}\u{0662}",
            "/w//ws",
            "/w/a/ws/x",
            "/w/../ws",
            "/w/a:b/ws",
            "/w/A/ws",
            "/w/a~b/ws",
            "ws://x/ws",
            "wss://x/ws",
        ];
        for url in ok {
            let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(url)));
            assert_eq!(check_put(&c, None, &p), Ok(()), "{url}");
        }
        for url in bad {
            let p = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(url)));
            assert_eq!(check_put(&c, None, &p), Err(EntityWhy::BadDest), "{url}");
        }
    }

    #[test]
    fn nan_position_is_invalid() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Portal(portal([f32::NAN, 0.0, 5.5], world_dest()));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::Invalid));
    }

    #[test]
    fn settings_cannot_become_a_spawn() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([0.5, 0.0, 0.5]), &plots, &floor);
        let old = Component::Settings(Settings { json: "{}".into() });
        let new = Component::Spawn(Spawn { at: [0.5, 0.0, 0.5], yaw: 0.0 });
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn last_spawn_cannot_become_a_portal() {
        let plots: [PlotArea; 0] = [];
        let mut c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        c.spawns = 1;
        let old = Component::Spawn(Spawn { at: [5.5, 0.0, 5.5], yaw: 0.0 });
        let new = Component::Portal(portal([5.5, 0.0, 5.5], url_dest(":7781")));
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn keyed_portal_cannot_become_a_spawn() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let new = Component::Spawn(Spawn { at: [5.5, 0.0, 5.5], yaw: 0.0 });
        assert_eq!(check_put(&c, Some(&old), &new), Err(EntityWhy::NotYours));
    }

    #[test]
    fn moving_into_a_full_zone_is_too_many() {
        let plots: [PlotArea; 0] = [];
        let mut c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        c.zone_entities = MAX_ZONE_ENTITIES;
        let old = Component::Portal(keyed([5.5, 0.0, 5.5], world_dest(), 3));
        let moved = Component::Portal(keyed([6.5, 0.0, 5.5], world_dest(), 3));
        c.moves_zone = true;
        assert_eq!(check_put(&c, Some(&old), &moved), Err(EntityWhy::TooMany));
        // within its own zone, a full zone is no obstacle (the entity is already counted there)
        c.moves_zone = false;
        assert_eq!(check_put(&c, Some(&old), &moved), Ok(()));
    }

    // Props: a builder's plot is x and z in 0..10 (blocks), and the reach point is (5, 0, 5).
    // A prop at 5 cm units [55, 0, 55] is in block [5, 0, 5].

    #[test]
    fn unknown_prop_is_no_such_prop() {
        let plots: [PlotArea; 0] = [];
        let unknown = |_: &[u8; 16]| false;
        let c = Ctx { known_prop: &unknown, ..ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor) };
        let p = Component::Prop(prop([55, 0, 55], Some(1)));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::NoSuchProp));
    }

    #[test]
    fn builder_places_own_prop_in_own_plot() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Prop(prop([55, 0, 55], Some(7)));
        assert_eq!(check_put(&c, None, &p), Ok(()));
    }

    #[test]
    fn new_prop_owned_by_someone_else_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Prop(prop([55, 0, 55], Some(8)));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::NotYours));
        let unowned = Component::Prop(prop([55, 0, 55], None));
        assert_eq!(check_put(&c, None, &unowned), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_prop_outside_own_plot_is_not_yours() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Prop(prop([555, 0, 55], Some(7)));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_cannot_move_someone_elses_prop() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Prop(prop([55, 0, 55], Some(8)));
        let moved = Component::Prop(Prop { yaw: 1, ..prop([55, 0, 55], Some(8)) });
        assert_eq!(check_put(&c, Some(&old), &moved), Err(EntityWhy::NotYours));
        // a level file's prop (no owner) is not a builder's to move either
        let level = Component::Prop(prop([55, 0, 55], None));
        assert_eq!(check_put(&c, Some(&level), &moved), Err(EntityWhy::NotYours));
    }

    #[test]
    fn builder_moves_own_prop_within_plot() {
        let plots = [plot(&[7])];
        let c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Prop(prop([55, 0, 55], Some(7)));
        let moved = Component::Prop(prop([65, 0, 55], Some(7)));
        assert_eq!(check_put(&c, Some(&old), &moved), Ok(()));
    }

    #[test]
    fn prop_owner_cannot_be_handed_over() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let old = Component::Prop(prop([55, 0, 55], Some(1)));
        let handed = Component::Prop(prop([55, 0, 55], Some(2)));
        assert_eq!(check_put(&c, Some(&old), &handed), Err(EntityWhy::NotYours));
    }

    #[test]
    fn editor_places_prop_in_reach_outside_plots() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Prop(prop([55, 0, 55], Some(1)));
        assert_eq!(check_put(&c, None, &p), Ok(()));
        let far = Component::Prop(prop([1000, 0, 55], Some(1)));
        assert_eq!(check_put(&c, None, &far), Err(EntityWhy::TooFar));
    }

    #[test]
    fn sixty_fifth_prop_in_a_plot_is_too_many() {
        let plots = [plot(&[7])];
        let mut c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Prop(prop([55, 0, 55], Some(7)));
        c.props_in_plot = PROPS_PER_PLOT - 1;
        assert_eq!(check_put(&c, None, &p), Ok(()));
        c.props_in_plot = PROPS_PER_PLOT;
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::TooMany));
    }

    #[test]
    fn moving_a_prop_within_its_plot_is_not_counted_again() {
        let plots = [plot(&[7])];
        let mut c = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        c.props_in_plot = PROPS_PER_PLOT;
        let old = Component::Prop(prop([55, 0, 55], Some(7)));
        let moved = Component::Prop(prop([65, 0, 55], Some(7)));
        assert_eq!(check_put(&c, Some(&old), &moved), Ok(()));
    }

    #[test]
    fn prop_on_a_taken_footprint_is_blocked() {
        let plots: [PlotArea; 0] = [];
        let taken = |_: &Prop| false;
        let c = Ctx { prop_footprint_free: &taken, ..ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor) };
        let p = Component::Prop(prop([55, 0, 55], Some(1)));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::Blocked));
    }

    #[test]
    fn changing_only_a_prop_text_skips_the_footprint_check() {
        let plots: [PlotArea; 0] = [];
        let taken = |_: &Prop| false;
        let c = Ctx { prop_footprint_free: &taken, ..ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor) };
        let old = Component::Prop(prop([55, 0, 55], Some(1)));
        let mut text = prop([55, 0, 55], Some(1));
        text.text = Some("open".into());
        assert_eq!(check_put(&c, Some(&old), &Component::Prop(text)), Ok(()));
        // turning it is a placement change, so the footprint is checked again
        let turned = Component::Prop(Prop { yaw: 3, ..prop([55, 0, 55], Some(1)) });
        assert_eq!(check_put(&c, Some(&old), &turned), Err(EntityWhy::Blocked));
    }

    #[test]
    fn prop_yaw_24_is_invalid() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let p = Component::Prop(Prop { yaw: 24, ..prop([55, 0, 55], Some(1)) });
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::Invalid));
    }

    #[test]
    fn prop_text_over_140_is_too_big_and_control_characters_are_invalid() {
        let plots: [PlotArea; 0] = [];
        let c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let long = Component::Prop(Prop { text: Some("a".repeat(141)), ..prop([55, 0, 55], Some(1)) });
        assert_eq!(check_put(&c, None, &long), Err(EntityWhy::TooBig));
        let ctl = Component::Prop(Prop { text: Some("hi\u{7}".into()), ..prop([55, 0, 55], Some(1)) });
        assert_eq!(check_put(&c, None, &ctl), Err(EntityWhy::Invalid));
    }

    #[test]
    fn prop_into_a_full_zone_is_too_many() {
        let plots: [PlotArea; 0] = [];
        let mut c = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        c.zone_entities = MAX_ZONE_ENTITIES;
        let p = Component::Prop(prop([55, 0, 55], Some(1)));
        assert_eq!(check_put(&c, None, &p), Err(EntityWhy::TooMany));
    }

    #[test]
    fn remove_prop_by_role() {
        let plots = [plot(&[7])];
        let builder = ctx(Role::Builder, Some(7), Some([5.0, 0.0, 5.0]), &plots, &floor);
        let own = Component::Prop(prop([55, 0, 55], Some(7)));
        assert_eq!(check_remove(&builder, &own), Ok(()));
        let theirs = Component::Prop(prop([55, 0, 55], Some(8)));
        assert_eq!(check_remove(&builder, &theirs), Err(EntityWhy::NotYours));
        let level = Component::Prop(prop([55, 0, 55], None));
        assert_eq!(check_remove(&builder, &level), Err(EntityWhy::NotYours));

        let editor = ctx(Role::Editor, Some(1), Some([5.0, 0.0, 5.0]), &plots, &floor);
        assert_eq!(check_remove(&editor, &theirs), Ok(()));
        assert_eq!(check_remove(&editor, &level), Ok(()));
        let far = Component::Prop(prop([1000, 0, 55], Some(1)));
        assert_eq!(check_remove(&editor, &far), Err(EntityWhy::TooFar));
    }

    #[test]
    fn prop_cell_is_the_block_for_a_negative_position() {
        assert_eq!(prop_cell([-1, -10, -11]), [-1, -1, -2]);
    }
}
