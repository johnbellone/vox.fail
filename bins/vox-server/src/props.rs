//! The props a world accepts (docs/design/props.md, "Placing" and "Standing in the way").
//!
//! The props folder (`data/props/` unless `--props` says otherwise) holds `catalogue.json`, the
//! accepted props by name (`vox_format::prop::Catalogue`), and one `<hash>.voxp` blob per prop.
//! A world reads them once, when it opens. A blob that does not decode to the name its catalogue
//! entry gives is left out and logged; a missing folder or catalogue means no props at all.
//!
//! What the world keeps of a prop is what its rules need, taken from the decoded definition: the
//! footprint, the height and whether it collides. The blob's bytes are not kept, so nothing that
//! arrives later is served or checked against them.
//!
//! A prop already in a world whose hash has since left the catalogue (an orphan) keeps its
//! entity. It takes no cells, so others may build where it stands; it cannot be moved or turned
//! (`NoSuchProp`), but whoever may remove a prop may remove it. Putting its hash back in the
//! catalogue restores it the next time the world opens.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    ops::Range,
    path::Path,
};

use vox_art::prop::Collide;
use vox_core::{
    ChunkKey,
    body::prop_footprint_cells,
    entity::{Component, EntityId, Prop, prop_cell},
};
use vox_format::prop::{Catalogue, PropBlob};

/// Largest catalogue read, in bytes.
const MAX_CATALOGUE: u64 = 1 << 20;
/// Largest prop blob read, in bytes.
const MAX_BLOB: u64 = 4 << 20;

/// What a world's rules need of an accepted prop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footprint {
    /// the floor cells, in 0.5 m blocks, relative to the pivot's floor cell
    floor: Vec<[i16; 2]>,
    /// blocks above each floor cell that the prop takes up
    height: u8,
    /// whether the prop blocks walking (anything but `collide: none`)
    collides: bool,
}

impl Footprint {
    fn of(blob: &PropBlob) -> Footprint {
        Footprint {
            floor: blob.def.place.footprint.clone(),
            height: blob.def.place.height,
            collides: !matches!(blob.def.collide, Collide::None),
        }
    }

    /// The cells a prop covers in the world, the same cells the client's prediction uses
    /// (vox_core::body::prop_footprint_cells).
    fn covers(&self, prop: &Prop) -> Vec<[i32; 3]> {
        prop_footprint_cells(prop.at, prop.yaw, &self.floor, self.height)
    }
}

/// The accepted props, by their hash (the version their blob is named by), and the index of the
/// colliding props placed in the world (see `PropIndex`). The index is kept in step by the world
/// wherever an entity changes (`index_change`), and rebuilt when the entities are loaded
/// (`index_all`).
#[derive(Debug, Default)]
pub struct PropLibrary {
    defs: HashMap<[u8; 16], Footprint>,
    index: PropIndex,
    /// Every prop placed in the world (colliding or not, orphans too), by the column its pivot
    /// stands in: a plot's prop cap counts these, not every entity in the world.
    columns: HashMap<[i32; 2], HashSet<EntityId>>,
}

/// The colliding props placed in a world, by the chunks their cells fall in. A footprint check
/// looks only at the props in the chunks the new prop's cells are in, so placing one costs what
/// is near it, not what the world holds. A prop that spans several chunks is in each of them.
#[derive(Debug, Default, PartialEq)]
struct PropIndex {
    chunks: HashMap<ChunkKey, HashSet<EntityId>>,
}

impl PropLibrary {
    /// Read the props folder. Never fails: a problem is logged and the prop (or all of them) left out.
    pub fn load(dir: &Path) -> PropLibrary {
        let mut lib = PropLibrary::default();
        let bytes = match read_capped(&dir.join("catalogue.json"), MAX_CATALOGUE) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("props: no props accepted: {e}");
                return lib;
            }
        };
        let catalogue = match Catalogue::decode(&bytes) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("props: no props accepted: {e}");
                return lib;
            }
        };
        for (id, entry) in &catalogue.props {
            let path = dir.join(format!("{}.voxp", entry.hash.hex()));
            let blob = read_capped(&path, MAX_BLOB)
                .and_then(|b| PropBlob::decode_named(&b, &entry.hash).map_err(|e| e.to_string()));
            match blob {
                Ok(blob) => {
                    lib.defs.insert(entry.hash.0, Footprint::of(&blob));
                }
                Err(e) => eprintln!("props: {id} left out: {e}"),
            }
        }
        lib
    }

    /// Does the world accept this prop (its version)?
    pub fn known(&self, hash: &[u8; 16]) -> bool {
        self.defs.contains_key(hash)
    }

    /// The chunks a prop's cells fall in, when it collides and its hash is accepted: exactly the
    /// props the index holds. Empty otherwise (a rug, or an orphan, takes no part in the index).
    fn chunks_of(&self, prop: &Prop) -> Vec<ChunkKey> {
        let Some(fp) = self.defs.get(&prop.prop).filter(|f| f.collides) else { return Vec::new() };
        let mut keys: Vec<ChunkKey> = fp.covers(prop).into_iter().map(ChunkKey::of).collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// Rebuilds the index from the entities a world holds (at open, once they are loaded).
    pub fn index_all<'a>(&mut self, placed: impl Iterator<Item = (EntityId, &'a Component)>) {
        self.index = PropIndex::default();
        self.columns.clear();
        for (id, c) in placed {
            if let Component::Prop(p) = c {
                self.index_add(id, p);
            }
        }
    }

    /// Keeps the index in step with one entity that changed: it was `old` (None when it is new)
    /// and is now `new` (None when it is removed). Anything but a prop is ignored.
    pub fn index_change(&mut self, id: EntityId, old: Option<&Component>, new: Option<&Component>) {
        if let Some(Component::Prop(p)) = old {
            self.index_remove(id, p);
        }
        if let Some(Component::Prop(p)) = new {
            self.index_add(id, p);
        }
    }

    fn index_add(&mut self, id: EntityId, prop: &Prop) {
        for k in self.chunks_of(prop) {
            self.index.chunks.entry(k).or_default().insert(id);
        }
        self.columns.entry(column_of(prop_cell(prop.at))).or_default().insert(id);
    }

    fn index_remove(&mut self, id: EntityId, prop: &Prop) {
        for k in self.chunks_of(prop) {
            if let Some(set) = self.index.chunks.get_mut(&k) {
                set.remove(&id);
                if set.is_empty() {
                    self.index.chunks.remove(&k);
                }
            }
        }
        let col = column_of(prop_cell(prop.at));
        if let Some(set) = self.columns.get_mut(&col) {
            set.remove(&id);
            if set.is_empty() {
                self.columns.remove(&col);
            }
        }
    }

    /// The cells a prop of an accepted hash covers in the world, the same cells the footprint
    /// check and the client use. Empty for a hash the world does not accept.
    pub fn cells(&self, prop: &Prop) -> Vec<[i32; 3]> {
        self.defs.get(&prop.prop).map(|fp| fp.covers(prop)).unwrap_or_default()
    }

    /// Does a placed colliding prop cover `cell`? `prop_of` gives the entity's prop by id. Looks
    /// only at the props indexed in the cell's chunk, so it costs what is near the cell.
    pub fn covers_cell<'a>(&self, cell: [i32; 3], prop_of: impl Fn(EntityId) -> Option<&'a Prop>) -> bool {
        let Some(ids) = self.index.chunks.get(&ChunkKey::of(cell)) else { return false };
        ids.iter().filter_map(|&id| prop_of(id)).any(|q| {
            self.defs.get(&q.prop).is_some_and(|fp| fp.covers(q).contains(&cell))
        })
    }

    /// How many props have their pivot in the columns `x` and `z` (blocks, lo inclusive, hi
    /// exclusive). Counts the props in the columns those ranges span, `prop_of` giving each by
    /// id, so a plot's cap costs the plot and not the world.
    pub fn count_in<'a>(&self, x: Range<i32>, z: Range<i32>, prop_of: impl Fn(EntityId) -> Option<&'a Prop>) -> usize {
        if x.is_empty() || z.is_empty() {
            return 0;
        }
        let col = |v: i32| ChunkKey::of([v, 0, 0]).0[0];
        let mut n = 0;
        for cx in col(x.start)..=col(x.end - 1) {
            for cz in col(z.start)..=col(z.end - 1) {
                let Some(ids) = self.columns.get(&[cx, cz]) else { continue };
                n += ids
                    .iter()
                    .filter_map(|&id| prop_of(id))
                    .filter(|q| {
                        let c = prop_cell(q.at);
                        x.contains(&c[0]) && z.contains(&c[2])
                    })
                    .count();
            }
        }
        n
    }

    /// The placed colliding props that may stand in `prop`'s way, from the chunks its cells are
    /// in. A superset of what `free` needs: the caller still leaves out the prop being changed.
    pub fn near(&self, prop: &Prop) -> Vec<EntityId> {
        let mut ids: Vec<EntityId> = self
            .chunks_of(prop)
            .iter()
            .filter_map(|k| self.index.chunks.get(k))
            .flatten()
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// The column (chunk of x and z) a cell is in.
fn column_of(cell: [i32; 3]) -> [i32; 2] {
    let k = ChunkKey::of(cell).0;
    [k[0], k[2]]
}

/// Is the prop's footprint free in the world? Every cell it covers must be air (`solid` says
/// which are not). A prop that collides also may not share a cell with another colliding prop:
/// `others` are the other props that may stand in its way: the world passes the ones near `prop`
/// (`PropLibrary::near`), not every prop it holds, and leaves out the prop being changed, so a
/// prop can be turned or moved onto its own old cells. A prop with `collide: none` blocks
/// nothing, so other props may stand on its cells and it on theirs. Props whose definitions are
/// not in the library take no cells here.
pub fn free<'a>(
    lib: &PropLibrary,
    prop: &Prop,
    others: impl Iterator<Item = &'a Prop>,
    solid: impl Fn([i32; 3]) -> bool,
) -> bool {
    let Some(fp) = lib.defs.get(&prop.prop) else { return false };
    let mine = fp.covers(prop);
    if mine.iter().any(|&c| solid(c)) {
        return false;
    }
    if !fp.collides {
        return true;
    }
    !others
        .filter_map(|o| lib.defs.get(&o.prop).filter(|f| f.collides).map(|f| f.covers(o)))
        .any(|cells| cells.iter().any(|c| mine.contains(c)))
}

/// A file's bytes, refused past `cap` (read one byte past it to see that it is over).
fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = Vec::new();
    file.take(cap + 1).read_to_end(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    if buf.len() as u64 > cap {
        return Err(format!("{} is over {cap} bytes", path.display()));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use vox_core::entity::PROP_UNITS_PER_BLOCK;

    use super::*;

    const CRATE: [u8; 16] = [1; 16];
    const RUG: [u8; 16] = [2; 16];

    /// A library of a crate (one cell, two high, collides) and a rug (one cell, does not collide).
    fn library() -> PropLibrary {
        let defs = HashMap::from([
            (CRATE, Footprint { floor: vec![[0, 0]], height: 2, collides: true }),
            (RUG, Footprint { floor: vec![[0, 0]], height: 1, collides: false }),
        ]);
        PropLibrary { defs, index: PropIndex::default(), columns: HashMap::new() }
    }

    /// A prop standing on block (x, z), facing 0.
    fn at(hash: [u8; 16], x: i32, z: i32) -> Prop {
        Prop {
            prop: hash,
            at: [x * PROP_UNITS_PER_BLOCK, 0, z * PROP_UNITS_PER_BLOCK],
            yaw: 0,
            variant: 0,
            state: 0,
            channel: None,
            text: None,
            owner: None,
        }
    }

    /// The world's props, as the full scan (`free` over every other prop) sees them.
    fn free_scan(lib: &PropLibrary, placed: &BTreeMap<EntityId, Component>, p: &Prop, editing: Option<EntityId>) -> bool {
        let others = placed
            .iter()
            .filter(|(id, _)| Some(**id) != editing)
            .filter_map(|(_, c)| match c {
                Component::Prop(q) => Some(q),
                _ => None,
            });
        free(lib, p, others, |_| false)
    }

    /// The same check as the world makes it: only the props `near` returns.
    fn free_near(lib: &PropLibrary, placed: &BTreeMap<EntityId, Component>, p: &Prop, editing: Option<EntityId>) -> bool {
        let others = lib
            .near(p)
            .into_iter()
            .filter(|id| Some(*id) != editing)
            .filter_map(|id| match placed.get(&id) {
                Some(Component::Prop(q)) => Some(q),
                _ => None,
            });
        free(lib, p, others, |_| false)
    }

    #[test]
    fn index_agrees_with_a_full_scan_through_put_move_and_remove() {
        let mut lib = library();
        let mut placed: BTreeMap<EntityId, Component> = BTreeMap::new();
        let mut next_id = 0u32;
        let mut seed: u32 = 12345;
        let mut rand = move |n: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 8) % n
        };
        for _ in 0..1500 {
            let hash = if rand(5) == 0 { RUG } else { CRATE };
            let (x, z) = (rand(40) as i32, rand(40) as i32);
            let p = at(hash, x, z);
            match rand(3) {
                0 => {
                    let id = EntityId(next_id);
                    next_id += 1;
                    if free_near(&lib, &placed, &p, None) {
                        assert!(free_scan(&lib, &placed, &p, None));
                        let c = Component::Prop(p);
                        lib.index_change(id, None, Some(&c));
                        placed.insert(id, c);
                    }
                }
                1 if !placed.is_empty() => {
                    let id = *placed.keys().nth(rand(placed.len() as u32) as usize).unwrap();
                    if free_near(&lib, &placed, &p, Some(id)) {
                        assert!(free_scan(&lib, &placed, &p, Some(id)));
                        let old = placed.insert(id, Component::Prop(p.clone())).unwrap();
                        lib.index_change(id, Some(&old), Some(&Component::Prop(p)));
                    }
                }
                2 if !placed.is_empty() => {
                    let id = *placed.keys().nth(rand(placed.len() as u32) as usize).unwrap();
                    let old = placed.remove(&id).unwrap();
                    lib.index_change(id, Some(&old), None);
                }
                _ => {}
            }
            // the index is what a rebuild from the placed entities makes
            let mut fresh = PropLibrary { defs: lib.defs.clone(), index: PropIndex::default(), columns: HashMap::new() };
            fresh.index_all(placed.iter().map(|(id, c)| (*id, c)));
            assert_eq!(lib.index, fresh.index);
            assert_eq!(lib.columns, fresh.columns);
            // and the plot count agrees with a scan of every placed prop, over ranges that cross
            // chunk edges
            let prop_of = |id: EntityId| match placed.get(&id) {
                Some(Component::Prop(q)) => Some(q),
                _ => None,
            };
            for (x, z) in [(0..10, 0..10), (5..40, 3..33), (31..33, 0..40), (-5..64, -5..64)] {
                let scan = placed
                    .values()
                    .filter(|c| matches!(c, Component::Prop(q) if { let p = prop_cell(q.at); x.contains(&p[0]) && z.contains(&p[2]) }))
                    .count();
                assert_eq!(lib.count_in(x.clone(), z.clone(), prop_of), scan);
            }
        }
        // and the check agrees with the full scan for placements all over the grid
        for x in 0..40 {
            for z in 0..40 {
                let p = at(CRATE, x, z);
                assert_eq!(free_near(&lib, &placed, &p, None), free_scan(&lib, &placed, &p, None));
            }
        }
    }

    #[test]
    fn a_rug_and_an_orphan_take_no_place_in_the_index() {
        let mut lib = library();
        let rug = Component::Prop(at(RUG, 3, 3));
        let orphan = Component::Prop(at([9; 16], 3, 3));
        lib.index_change(EntityId(1), None, Some(&rug));
        lib.index_change(EntityId(2), None, Some(&orphan));
        assert_eq!(lib.index, PropIndex::default());
    }

    #[test]
    fn one_more_prop_among_4000_in_a_zone_is_checked_fast() {
        let mut lib = library();
        let mut placed: BTreeMap<EntityId, Component> = BTreeMap::new();
        for i in 0..4000u32 {
            let c = Component::Prop(at(CRATE, (i % 64) as i32 * 2, (i / 64) as i32 * 2));
            lib.index_change(EntityId(i), None, Some(&c));
            placed.insert(EntityId(i), c);
        }
        let p = at(CRATE, 1, 1);
        let start = std::time::Instant::now();
        let free_now = free_near(&lib, &placed, &p, None);
        let took = start.elapsed();
        assert!(free_now);
        assert_eq!(free_now, free_scan(&lib, &placed, &p, None));
        // the timing is for an optimised build only; debug builds check the answer alone
        if !cfg!(debug_assertions) {
            assert!(took < Duration::from_millis(5), "footprint check took {took:?}");
        }
    }
}
