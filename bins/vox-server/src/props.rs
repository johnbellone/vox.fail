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

use std::{collections::HashMap, fs, io::Read, path::Path};

use vox_art::prop::Collide;
use vox_core::{body::prop_footprint_cells, entity::Prop};
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

/// The accepted props, by their hash (the version their blob is named by).
#[derive(Debug, Default)]
pub struct PropLibrary {
    defs: HashMap<[u8; 16], Footprint>,
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
}

/// Is the prop's footprint free in the world? Every cell it covers must be air (`solid` says
/// which are not). A prop that collides also may not share a cell with another colliding prop:
/// `others` are the other props in the world, and the caller leaves out the prop being changed,
/// so a prop can be turned or moved onto its own old cells. A prop with `collide: none` blocks
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
