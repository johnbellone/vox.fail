//! The desktop client: `vox-client --token <launch code> [--world ws://host:7780/ws]`.
//!
//! The launch code comes from the directory's page ("Copy launch code"); it says who you are
//! and what you look like. Without one, `--model models/dog.glb --name Sam` joins a world
//! that lets guests in. `--world` defaults to the Void at vox.fail (`--world
//! ws://127.0.0.1:7780/ws` for one on this machine).
//!
//! `--version` prints the version and exits. `--check` prints the version, the folder with the
//! game's data and the player's own folder, then exits 0 if the game's files are all there, or
//! 1 and names the missing ones. Both run before the window opens.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

/// Where the Void is.
const DEFAULT_WORLD: &str = "wss://vox.fail/w/7780/ws";

/// The version in the release (`VOX_VERSION`, set by the packaging scripts), else the crate's.
const VERSION: &str = match option_env!("VOX_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// What `--check` needs under the root to start the game.
const REQUIRED: [&str; 3] = ["data/blocks.json", "assets/models/reporter.glb", "assets/shaders/block.wgsl"];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    if args.iter().any(|a| a == "--version" || a == "--check") {
        attach_console();
    }
    if args.iter().any(|a| a == "--version") {
        println!("vox-client {VERSION}");
        return;
    }
    let root = find_root();
    vox_world::files::set_base(root.clone());
    // an installed copy keeps the player's levels in their own data folder, not beside the program
    let user = installed_user_dir(&root).filter(|d| std::fs::create_dir_all(d).is_ok());
    if let Some(d) = &user {
        vox_world::files::set_user_dir(d.clone());
    }
    if args.iter().any(|a| a == "--check") {
        std::process::exit(check(&root));
    }
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
    }
    .from_token();
    vox_client::app(start, root.join("assets").to_string_lossy().into_owned()).run();
}

/// Release Windows builds have no console, so the output of --version and --check would vanish
/// when started from a terminal. Borrow the terminal's console, as block-avatar does.
fn attach_console() {
    #[cfg(windows)]
    {
        // SAFETY: plain Win32 call with a constant argument
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
}

/// `--check`: says where things are and whether the game's files are all there.
fn check(root: &Path) -> i32 {
    println!("vox-client {VERSION}");
    println!("root: {}", root.display());
    println!("user dir: {}", vox_world::files::user_dir().display());
    let missing: Vec<&str> = REQUIRED.into_iter().filter(|f| !root.join(f).exists()).collect();
    if missing.is_empty() {
        return 0;
    }
    for f in missing {
        eprintln!("missing: {}", root.join(f).display());
    }
    1
}

/// The player's own folder for an installed copy. A source checkout (a Cargo.toml at its root)
/// keeps its levels in the checkout, so there is none.
fn installed_user_dir(root: &Path) -> Option<PathBuf> {
    if root.join("Cargo.toml").exists() {
        return None;
    }
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env("APPDATA").map(|d| d.join("Vox"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/Vox"))
    } else {
        // the XDG spec says a relative XDG_DATA_HOME must be ignored
        env("XDG_DATA_HOME").filter(|p| p.is_absolute()).or_else(|| env("HOME").map(|h| h.join(".local/share"))).map(|d| d.join("vox"))
    }
}

/// The folder holding data/ and assets/: here, or above the executable. An installed copy has
/// them in the app bundle's Resources (macOS) or in usr/share/vox beside usr/bin (AppImage).
fn find_root() -> PathBuf {
    let mut dirs = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs.into_iter()
        .flat_map(|d| [d.clone(), d.join("Resources"), d.join("share/vox")])
        .find(|p| p.join("data/blocks.json").exists() && p.join("assets").is_dir())
        .map(|p| p.canonicalize().unwrap_or(p))
        .unwrap_or_else(|| PathBuf::from("."))
}
