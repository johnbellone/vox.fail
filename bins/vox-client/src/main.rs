//! The desktop client: `vox-client --token <launch code> [--world ws://host:7780/ws]`.
//!
//! The launch code comes from the directory's page ("Copy launch code"); it says who you are
//! and what you look like. Without one, `--model models/dog.glb --name Sam` joins a world
//! that lets guests in. `--world` defaults to the Void at vox.fail (`--world
//! ws://127.0.0.1:7780/ws` for one on this machine). `--detail browser` or `--detail mobile`
//! draws and keeps as little as a browser or a phone would (`vox_world::detail`), to try them.

use std::path::PathBuf;

/// Where the Void is.
const DEFAULT_WORLD: &str = "wss://vox.fail/w/7780/ws";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let root = find_root();
    vox_world::files::set_base(root.clone());
    let token = arg("--token").or_else(|| std::env::var("VOX_TOKEN").ok()).filter(|t| !t.trim().is_empty());
    let start = vox_client::Start {
        url: vox_world::client::ws_url(&arg("--world").unwrap_or_else(|| DEFAULT_WORLD.into())),
        token,
        name: arg("--name").unwrap_or_default(),
        model: arg("--model").unwrap_or_default(),
        wear: arg("--wear").map(|w| w.split(',').map(String::from).collect()).unwrap_or_default(),
        canvas: None,
        // --watch: look at the world without joining it
        watch: args.iter().any(|a| a == "--watch"),
        // the in-world editor is a page feature: only the browser client starts it
        edit: false,
        detail: arg("--detail"),
    }
    .from_token();
    vox_client::app(start, root.join("assets").to_string_lossy().into_owned()).run();
}

/// The folder holding data/ and assets/: here, or above the executable.
fn find_root() -> PathBuf {
    let mut tries = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            tries.push(dir.to_path_buf());
        }
    }
    tries
        .into_iter()
        .find(|p| p.join("data/blocks.json").exists() && p.join("assets").is_dir())
        .map(|p| p.canonicalize().unwrap_or(p))
        .unwrap_or_else(|| PathBuf::from("."))
}
