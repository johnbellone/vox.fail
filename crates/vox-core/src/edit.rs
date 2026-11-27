//! Who may change a world's entities, and whether a change is allowed: the rules behind
//! `Put` and `Remove` (docs/design/world-editor.md, "Who may edit what" and "Security").
//!
//! These are pure functions over state the caller gathers (the plots, the player's feet, what
//! the zone already holds, the blocks under a spawn), so the server, the directory and the
//! tests all run the same checks. No I/O, no threads, no clocks: wasm-safe.
//!
//! [`Component::validate`] checks the values; this module checks rights, destinations, place
//! and caps. Each refusal is a distinct [`EntityWhy`] so a client can say why.

use crate::entity::{Component, Dest, EntityWhy};
use crate::world::{Bounds, Cell};

/// Most entities one zone holds.
pub const MAX_ZONE_ENTITIES: usize = 4_000;
/// Most player portals one plot holds (the directory's PORTALS_PER_PLOT).
pub const PORTALS_PER_PLOT: usize = 2;
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
    /// The player's portals already in the target plot.
    pub plot_portals: usize,
    /// Spawn points in the world now (the last one may not be removed).
    pub spawns: usize,
    /// Is that cell solid?
    pub ground: &'a dyn Fn(Cell) -> bool,
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

/// Is the position's cell inside a plot where `account` builds?
fn in_own_plot(plots: &[PlotArea], account: u64, at: [f32; 3]) -> bool {
    let c = cell_of(at);
    plots
        .iter()
        .any(|p| p.holds(c[0], c[2]) && p.builders.contains(&account))
}

/// A position must be inside the world and within the player's reach.
fn check_position(ctx: &Ctx, at: [f32; 3]) -> Result<(), EntityWhy> {
    let cell = cell_of(at);
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
/// Reach applies to every role ("anywhere in the world, within reach of where you stand"). With
/// no known position (`ctx.at` None) nothing is in reach, so the check fails closed.
pub fn check_put(ctx: &Ctx, old: Option<&Component>, new: &Component) -> Result<(), EntityWhy> {
    new.validate()?;
    let Some(account) = ctx.account else {
        return Err(EntityWhy::NotYours);
    };

    if let Component::Portal(p) = new {
        // The key a portal keeps: an old portal's, or none for a new one.
        let kept = match old {
            Some(Component::Portal(o)) => o.key,
            _ => None,
        };
        if p.key != kept {
            return Err(EntityWhy::NotYours);
        }
    }

    if ctx.role == Role::Builder {
        // Basic builders place and move portals in their own plot and nothing else.
        let Component::Portal(p) = new else {
            return Err(EntityWhy::NotYours);
        };
        if let Some(old) = old {
            let Component::Portal(o) = old else {
                return Err(EntityWhy::NotYours);
            };
            // A portal from a level file has no key and is not a player's to move. A new
            // portal has no key yet (checked above), so only an existing one must have one.
            if o.key.is_none() || !in_own_plot(ctx.plots, account, o.at) {
                return Err(EntityWhy::NotYours);
            }
        }
        if !in_own_plot(ctx.plots, account, p.at) {
            return Err(EntityWhy::NotYours);
        }
        match &p.to {
            Dest::World { world } if valid_world_id(world) => {}
            // A pair's key names another doorway; only the directory knows which exist, so it
            // checks the key.
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

    if let (Role::Editor, Component::Portal(p)) = (ctx.role, new) {
        match &p.to {
            Dest::Url { url, .. } if !valid_url(url) => return Err(EntityWhy::BadDest),
            Dest::World { world } if !valid_world_id(world) => return Err(EntityWhy::BadDest),
            Dest::Dungeon { name, .. } if !valid_dungeon(name) => return Err(EntityWhy::BadDest),
            _ => {}
        }
    }

    // Settings have no position; everything else is placed.
    let position = match new {
        Component::Portal(p) => Some(p.at),
        Component::Spawn(s) => Some(s.at),
        Component::Objective(o) => Some(o.at),
        Component::Settings(_) => None,
    };
    if let Some(at) = position {
        check_position(ctx, at)?;
    }

    if old.is_none() && ctx.zone_entities >= MAX_ZONE_ENTITIES {
        return Err(EntityWhy::TooMany);
    }

    if let Component::Spawn(s) = new {
        check_standing(ctx, s.at)?;
    }
    Ok(())
}

/// May this player remove this entity? Basic builders remove only their own plot's player
/// portals. Advanced editors remove anything except the world's settings (never removable)
/// and its last spawn point (a world needs one). Reach applies to every removal, as to a Put.
///
/// Refusal choices: removing settings is `NotYours` (it is the world's own, not an entity
/// anyone owns); removing the last spawn is `Blocked`.
pub fn check_remove(ctx: &Ctx, old: &Component) -> Result<(), EntityWhy> {
    let Some(account) = ctx.account else {
        return Err(EntityWhy::NotYours);
    };
    let position = match (ctx.role, old) {
        (Role::Builder, Component::Portal(p)) => {
            if p.key.is_none() || !in_own_plot(ctx.plots, account, p.at) {
                return Err(EntityWhy::NotYours);
            }
            Some(p.at)
        }
        (Role::Builder, _) => return Err(EntityWhy::NotYours),
        (Role::Editor, Component::Settings(_)) => return Err(EntityWhy::NotYours),
        (Role::Editor, Component::Spawn(_)) if ctx.spawns <= 1 => return Err(EntityWhy::Blocked),
        (Role::Editor, Component::Portal(p)) => Some(p.at),
        (Role::Editor, Component::Spawn(s)) => Some(s.at),
        (Role::Editor, Component::Objective(o)) => Some(o.at),
    };
    if let Some(at) = position {
        check_position(ctx, at)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Axis, Objective, Portal, Settings, Spawn};

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
            plot_portals: 0,
            spawns: 2,
            ground,
        }
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
}
