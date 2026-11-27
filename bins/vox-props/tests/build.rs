//! The tool's output is what a world accepts, and the committed `data/props` is its output.

use std::{fs, path::{Path, PathBuf}};

use vox_format::prop::{Catalogue, PropBlob};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vox-props-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn every_entry_decodes_and_data_props_is_in_step() {
    let out = temp("build");
    let report = vox_props::build(&root().join("source/props"), &out).expect("build");
    assert_eq!(report.props, 3);

    let bytes = fs::read(out.join("catalogue.json")).unwrap();
    let catalogue = Catalogue::decode(&bytes).expect("catalogue decodes");
    assert_eq!(catalogue.props.len(), 3);
    for entry in catalogue.props.values() {
        let name = entry.hash.hex().to_ascii_lowercase();
        let blob = fs::read(out.join(format!("{name}.voxp"))).unwrap();
        let prop = PropBlob::decode_named(&blob, &entry.hash).expect("blob decodes under its name");
        assert_eq!(prop.def.name, entry.name);
        assert!(out.join(format!("{name}.png")).is_file());
    }

    // the committed folder holds exactly these files, byte for byte
    let committed = root().join("data/props");
    let mut made: Vec<String> = fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    made.sort();
    assert_eq!(made.len(), 3 * 2 + 1);
    for name in &made {
        let want = fs::read(out.join(name)).unwrap();
        let have = fs::read(committed.join(name)).unwrap_or_else(|_| panic!("data/props/{name} is missing: run vox-props"));
        assert!(want == have, "data/props/{name} is out of date: run vox-props");
    }
    let committed_files = fs::read_dir(&committed)
        .unwrap()
        .filter(|e| {
            let n = e.as_ref().unwrap().file_name();
            !n.to_string_lossy().ends_with(".md")
        })
        .count();
    assert_eq!(committed_files, made.len(), "data/props holds files the tool did not make");
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn a_bad_recipe_writes_nothing() {
    let src = temp("src");
    let out = temp("bad");
    fs::create_dir_all(&src).unwrap();
    fs::copy(root().join("source/props/palette.json"), src.join("palette.json")).unwrap();
    let bad = fs::read_to_string(root().join("source/props/crate.json")).unwrap().replace("\"wood\"", "\"nothing\"");
    fs::write(src.join("crate.json"), bad).unwrap();
    assert!(vox_props::build(&src, &out).is_err());
    assert!(!out.exists());
    let _ = fs::remove_dir_all(&src);
}
