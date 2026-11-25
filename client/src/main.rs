//! The desktop client: `warren-client --token <launch code> [--world ws://host:7780/ws]`.
//!
//! The launch code comes from the directory's page ("Copy launch code"); it says who you are
//! and what you look like. Without one, `--model models/dog.glb --name Sam` joins a world
//! that lets guests in. `--world` defaults to a Void on this machine.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let root = find_root();
    warren_world::files::set_base(root.clone());
    let token = arg("--token").or_else(|| std::env::var("WARREN_TOKEN").ok()).filter(|t| !t.trim().is_empty());
    let start = warren_client::Start {
        url: warren_world::client::ws_url(&arg("--world").unwrap_or_else(|| "ws://127.0.0.1:7780/ws".into())),
        token,
        name: arg("--name").unwrap_or_default(),
        model: arg("--model").unwrap_or_default(),
        wear: arg("--wear").map(|w| w.split(',').map(String::from).collect()).unwrap_or_default(),
        canvas: None,
    }
    .from_token();
    warren_client::app(start, root.join("assets").to_string_lossy().into_owned()).run();
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
