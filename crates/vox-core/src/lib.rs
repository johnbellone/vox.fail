//! vox-core: the rules every part of Vox shares, with no engine in them.
//!
//! - [`world`]: blocks in 32³ chunks, the world's bounds, edits and the checks on them.
//! - [`ray`]: which block the crosshair is on (a voxel walk).
//! - [`body`]: moving a box through the blocks (walking, falling, bumping into walls).
//! - [`level`]: reading a level file (boxes of blocks) into a world.
//! - [`wire`]: the messages between a world server and its players.
//! - [`auth`]: sign-in tokens from the directory, checked by world servers.
//! - [`moves`]: how far a player can really move (the server's check on reported positions).
//! - [`portal`]: doorways to other places: where they are and where going through takes you.
//! - [`predict`]: a player's view of the world: confirmed blocks plus their own edits that
//!   the server hasn't answered yet.
//!
//! Positions in here are in blocks: block (x, y, z) spans x..x+1 etc. The app scales by the
//! palette's block size (0.5 m) to get metres.

pub mod auth;
pub mod body;
pub mod entitle;
pub mod level;
pub mod moves;
pub mod portal;
pub mod predict;
pub mod ray;
pub mod weather;
pub mod wire;
pub mod world;

pub use world::{Bounds, CHUNK, Cell, Chunk, ChunkKey, Why, World};

/// The palette's block names from `data/blocks.json`, in id order (id = index + 1; 0 is air).
pub fn palette_names(blocks_json: &str) -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct File {
        blocks: Vec<Def>,
    }
    #[derive(serde::Deserialize)]
    struct Def {
        name: String,
    }
    let f: File = serde_json::from_str(blocks_json).map_err(|e| e.to_string())?;
    Ok(f.blocks.into_iter().map(|d| d.name).collect())
}

/// The block size in metres from `data/blocks.json`.
pub fn block_size(blocks_json: &str) -> f32 {
    serde_json::from_str::<serde_json::Value>(blocks_json)
        .ok()
        .and_then(|v| v.get("block_size").and_then(|s| s.as_f64()))
        .unwrap_or(0.5) as f32
}
