//! vox-core: the rules every part of Vox shares, with no engine in them.
//!
//! - [`world`]: blocks in 32³ chunks, the world's bounds, edits and the checks on them.
//! - [`ray`]: which block the crosshair is on (a voxel walk).
//! - [`body`]: moving a box through the blocks (walking, falling, bumping into walls).
//! - [`level`]: reading a level file (boxes of blocks) into a world.
//! - [`wire`]: the messages between a world server and its players.
//! - [`auth`]: sign-in tokens from the directory, checked by world servers.
//! - [`moves`]: how far a player can really move (the server's check on reported positions).
//! - [`terrain`]: generated ground for an atlas (a big map of tiles), the same on every machine.
//! - [`relief`]: the shallow dips in natural ground, from a seed, the same on every machine.
//! - [`portal`]: doorways to other places: where they are and where going through takes you.
//! - [`entity`]: the things placed in a world beside its blocks (doorways, spawns, settings).
//! - [`predict`]: a player's view of the world: confirmed blocks plus their own edits that
//!   the server hasn't answered yet.
//!
//! Positions in here are in blocks: block (x, y, z) spans x..x+1 etc. A block is [`BLOCK_M`]
//! metres (a quarter metre); sizes that are really metres (a body, a reach, a plot) are written
//! in metres and turned into blocks with [`in_blocks`], so the block size lives in one place.

pub mod auth;
pub mod body;
pub mod edit;
pub mod entitle;
pub mod entity;
pub mod level;
pub mod moves;
pub mod portal;
pub mod predict;
pub mod terrain;
pub mod ray;
pub mod relief;
pub mod weather;
pub mod wire;
pub mod world;

pub use world::{Bounds, CHUNK, Cell, Chunk, ChunkKey, Why, World};

/// A block's side in metres, in every world (docs/design/block-scale.md). `data/blocks.json`
/// says the same (`block_size`), and [`block_size`] checks that it does.
pub const BLOCK_M: f32 = 0.25;

/// `metres` as a number of blocks.
pub const fn in_blocks(metres: f32) -> f32 {
    metres / BLOCK_M
}

/// `metres` as a whole number of blocks (rounded to the nearest).
pub const fn whole_blocks(metres: f32) -> i32 {
    (metres / BLOCK_M + 0.5) as i32
}

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

/// The block size in metres from `data/blocks.json`, which must be [`BLOCK_M`]: every rule
/// measured in blocks is worked out for that size, so a palette that says otherwise is refused
/// rather than played at the wrong scale.
pub fn block_size(blocks_json: &str) -> Result<f32, String> {
    let size = serde_json::from_str::<serde_json::Value>(blocks_json)
        .map_err(|e| e.to_string())?
        .get("block_size")
        .and_then(|s| s.as_f64())
        .ok_or("the palette has no block_size")? as f32;
    if (size - BLOCK_M).abs() > 1e-6 {
        return Err(format!("the palette's blocks are {size} m, but this build is made for {BLOCK_M} m blocks"));
    }
    Ok(size)
}
