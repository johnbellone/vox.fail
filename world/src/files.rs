//! The game's files: data (blocks, levels) and assets (block tiles).
//!
//! On the desktop they're read from disk, under [`base`]. In the browser there is no disk:
//! the files a world needs are fetched before it starts ([`put`]), and others on demand
//! ([`want`], then [`read`] again on a later frame).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static BASE: OnceLock<PathBuf> = OnceLock::new();
static USER: OnceLock<PathBuf> = OnceLock::new();
static LOGGER: OnceLock<fn(&str)> = OnceLock::new();
static FETCHED: Mutex<Option<HashMap<String, Vec<u8>>>> = Mutex::new(None);
static WANTED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Where `data/` and `assets/` are (set by the host app; the working directory otherwise).
pub fn base() -> PathBuf {
    BASE.get_or_init(|| {
        // not set (tests, tools): the nearest folder up from here with the game's data
        if cfg!(target_arch = "wasm32") {
            return PathBuf::new();
        }
        let here = std::env::current_dir().unwrap_or_default();
        here.ancestors().find(|d| d.join("data/blocks.json").exists()).map(Path::to_path_buf).unwrap_or_default()
    })
    .clone()
}

pub fn set_base(p: PathBuf) {
    let _ = BASE.set(p);
}

/// The player's own folder (their own levels), if any.
pub fn user_dir() -> PathBuf {
    USER.get().cloned().unwrap_or_else(base)
}

pub fn set_user_dir(p: PathBuf) {
    let _ = USER.set(p);
}

/// Where [`crate::note!`] lines go.
pub fn set_logger(f: fn(&str)) {
    let _ = LOGGER.set(f);
}

pub fn log(s: &str) {
    match LOGGER.get() {
        Some(f) => f(s),
        None => bevy::log::info!("{s}"),
    }
}

/// The key a path is kept under in the browser: relative to the base, with `/`.
fn key(p: &Path) -> String {
    let p = p.strip_prefix(base()).unwrap_or(p);
    p.to_string_lossy().replace('\\', "/").trim_start_matches("./").trim_start_matches('/').to_string()
}

/// Keep a fetched file (the browser client does this before the world starts).
pub fn put(path: &str, bytes: Vec<u8>) {
    FETCHED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(path.trim_start_matches('/').to_string(), bytes);
}

/// A file's bytes.
pub fn read(p: &Path) -> Option<Vec<u8>> {
    if let Some(b) = FETCHED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key(p)).cloned()) {
        return Some(b);
    }
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    std::fs::read(p).ok()
}

pub fn read_string(p: &Path) -> Option<String> {
    read(p).and_then(|b| String::from_utf8(b).ok())
}

pub fn exists(p: &Path) -> bool {
    if FETCHED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.contains_key(&key(p))) {
        return true;
    }
    // in the browser a level may be fetched later: say yes for the built-in ones
    if cfg!(target_arch = "wasm32") {
        return key(p).starts_with("data/levels/");
    }
    p.exists()
}

/// Ask for a file that isn't here yet (in the browser; on the desktop everything is).
pub fn want(p: &Path) {
    if cfg!(target_arch = "wasm32") {
        WANTED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(key(p));
    }
}

/// The files asked for since last time (the browser client fetches them).
pub fn take_wanted() -> Vec<String> {
    WANTED.lock().unwrap_or_else(|e| e.into_inner()).take().map(|s| s.into_iter().collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetched_files_come_first() {
        put("data/test-fetched.txt", b"hello".to_vec());
        assert_eq!(read_string(&base().join("data/test-fetched.txt")).as_deref(), Some("hello"));
        assert!(exists(Path::new("data/test-fetched.txt")));
        assert!(read(Path::new("data/no-such-file.txt")).is_none());
    }
}
