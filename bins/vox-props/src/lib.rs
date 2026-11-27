//! Builds the accepted props from their sources.
//!
//! The source folder holds `palette.json` (a `vox_art::Palette`) and one `<name>.json` per
//! prop: `{ "def": {...}, "recipe": {...} }`. The recipe is evaluated into voxels; the def
//! holds what a recipe cannot say (id, name, tags, pivot, placement, collision); scale, size and
//! parts come from the recipe. A prop that fails any check is refused and nothing is written.
//! The output is a `catalogue.json`, a `<hash>.voxp` blob and a `<hash>.png` thumbnail per prop
//! (lowercase hex hash), the folder `vox-server --props` reads. Output is deterministic: the
//! same sources give the same bytes.
//!
//! It also writes `palette.json`: an object keyed by palette version (`"1": {...}`), each value
//! the `vox_art::Palette` as it serialises. It holds the palette of every `palette_version` the
//! props use, so the client can colour each prop from the palette its def names (vox-tol.4.19).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use vox_art::{
    Palette, PropDef,
    check::{Kind, check},
    prop::{Collide, Part, Place, PropScale},
    recipe::{Recipe, RecipeScale, eval::eval},
    render::{self, View},
};
use vox_format::prop::{Catalogue, CatalogueEntry, PropBlob};

/// Largest source file read, in bytes.
const MAX_SOURCE: u64 = 1 << 20;
/// Widest thumbnail wanted, in pixels; the pixels per voxel are the largest that fit.
const THUMB_WIDTH: u32 = 256;

/// What the source's `def` says; the rest of the `PropDef` comes from the recipe.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceDef {
    id: String,
    name: String,
    #[serde(default)]
    tags: Vec<String>,
    pivot: [u16; 3],
    place: Place,
    collide: Collide,
    palette_version: u16,
}

/// One source file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    def: SourceDef,
    recipe: Recipe,
}

/// What a build did.
#[derive(Debug, Default)]
pub struct Report {
    /// props written
    pub props: usize,
    /// findings that did not stop a prop, as `prop: check: what`
    pub warnings: Vec<String>,
}

/// Reads a source file, capped.
fn read_capped(path: &Path) -> Result<String, String> {
    let len = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len > MAX_SOURCE {
        return Err(format!("{}: over {MAX_SOURCE} bytes", path.display()));
    }
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Builds every prop in `source` into `out`. Nothing is written unless every prop passes.
pub fn build(source: &Path, out: &Path) -> Result<Report, String> {
    let palette = Palette::from_json(&read_capped(&source.join("palette.json"))?)?;

    let mut files: Vec<PathBuf> = fs::read_dir(source)
        .map_err(|e| format!("{}: {e}", source.display()))?
        // an entry that cannot be read is an error, not a prop silently left out of the catalogue
        .map(|e| e.map(|e| e.path()).map_err(|e| format!("{}: {e}", source.display())))
        .collect::<Result<Vec<PathBuf>, String>>()?
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "json") && p.file_name().is_some_and(|n| n != "palette.json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("{}: no prop sources", source.display()));
    }

    let mut report = Report::default();
    let mut catalogue = Catalogue::default();
    // the palette of each palette_version the props use
    let mut palettes: BTreeMap<u16, Palette> = BTreeMap::new();
    // file name -> bytes, written only once everything passed
    let mut outputs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for path in &files {
        let shown = path.display();
        let text = read_capped(path)?;
        let src: Source = serde_json::from_str(&text).map_err(|e| format!("{shown}: {e}"))?;
        let built = eval(&src.recipe, &palette).map_err(|errors| {
            let all: Vec<String> = errors.iter().map(ToString::to_string).collect();
            format!("{shown}: recipe: {}", all.join("; "))
        })?;
        let scale = match src.recipe.scale {
            RecipeScale::Prop => PropScale::Prop,
            RecipeScale::Fine => PropScale::Fine,
            RecipeScale::World => return Err(format!("{shown}: a prop's recipe must be prop or fine scale")),
        };
        let d = src.def;
        // the voxels hold indices into the source palette, so a prop can only name its version
        if d.palette_version != palette.version {
            return Err(format!(
                "{shown}: palette_version {} is not the source palette's version {}",
                d.palette_version, palette.version
            ));
        }
        palettes.entry(d.palette_version).or_insert_with(|| palette.clone());
        let def = PropDef {
            id: d.id.clone(),
            name: d.name.clone(),
            scale,
            size: src.recipe.size,
            pivot: d.pivot,
            tags: d.tags.clone(),
            place: d.place,
            collide: d.collide,
            parts: built.parts.into_iter().map(|(k, voxels)| (k, Part { voxels })).collect(),
            sockets: Vec::new(),
            states: Vec::new(),
            behaviours: Vec::new(),
            variants: Vec::new(),
            palette_version: d.palette_version,
        };
        let mut refused = Vec::new();
        for f in check(&def, &built.grid, &palette) {
            let line = format!("{}: {}: {}", d.id, f.check, f.what);
            match f.kind {
                Kind::Error => refused.push(line),
                Kind::Warning => report.warnings.push(line),
            }
        }
        if !refused.is_empty() {
            return Err(format!("{shown}: refused: {}", refused.join("; ")));
        }

        let blob = PropBlob { def, grid: built.grid };
        let (bytes, hash) = blob.encode().map_err(|e| format!("{shown}: {e}"))?;
        let name = hash.hex().to_ascii_lowercase();
        outputs.insert(format!("{name}.voxp"), bytes);
        outputs.insert(format!("{name}.png"), thumbnail(&blob, &palette).map_err(|e| format!("{shown}: {e}"))?);
        if catalogue
            .props
            .insert(d.id.clone(), CatalogueEntry { hash, name: d.name, tags: d.tags })
            .is_some()
        {
            return Err(format!("{shown}: id {:?} is used twice", d.id));
        }
        report.props += 1;
    }
    outputs.insert("catalogue.json".into(), catalogue.encode().map_err(|e| e.to_string())?);
    outputs.insert("palette.json".into(), encode_palettes(&palettes)?);

    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (name, bytes) in &outputs {
        // names are hex or fixed, so a write cannot leave `out`; checked anyway
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(format!("refusing to write {name:?}"));
        }
        let path = out.join(name);
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(report)
}

/// The palettes as `palette.json`: an object keyed by version, with a final newline. The keys are
/// in order, so the same palettes always give the same bytes.
fn encode_palettes(palettes: &BTreeMap<u16, Palette>) -> Result<Vec<u8>, String> {
    let mut json = serde_json::to_vec_pretty(palettes).map_err(|e| format!("palette.json: {e}"))?;
    json.push(b'\n');
    Ok(json)
}

/// An isometric picture, as big as fits in [`THUMB_WIDTH`] pixels across.
fn thumbnail(blob: &PropBlob, palette: &Palette) -> Result<Vec<u8>, String> {
    for px in (1..=16).rev() {
        let img = render::view(&blob.grid, palette, View::Iso, px)?;
        if img.width <= THUMB_WIDTH {
            return render::encode_png(&img);
        }
    }
    Err("the prop is too big for a thumbnail".into())
}
