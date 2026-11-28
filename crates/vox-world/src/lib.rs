//! vox-world: a Vox world on screen, shared by the desktop app (`block_avatar --world`)
//! and the player client (`vox-client`, on the desktop and in the browser).
//!
//! - [`blocks`]: block definitions, the texture atlas, meshing, and the block, water and flame
//!   materials.
//! - [`client`]: the link to a world server, chunks and their meshes, walking and building,
//!   the camera (Diablo-style or behind the shoulder, and fixed shots), other players, the
//!   weather between keyframes, the HUD.
//! - [`outdoors`]: the sky, the sun or moons, haze, fog, rain, the endless floor.
//! - [`portals`]: doorways that show an echo of another place and lead there.
//! - [`pose`] and [`proto`]: avatars over the network (the pose format and playing it back).
//! - [`files`]: reading the game's files from disk, or (in the browser) from what was fetched.
//! - [`detail`]: how much a desktop, a browser or a phone draws and keeps.
//! - [`wheel`] and [`emote`]: the block wheel and the emote wheel, held open by the bumpers.
//!
//! What the host app provides: a camera marked [`WorldCamera`], a [`LocalAvatar`] describing
//! the player's own character (and acting on its `walking`), and a [`WorldRender`] with the
//! quality to draw at. Lights it wants the sun to drive are marked [`KeyLight`].

pub mod blocks;
pub mod client;
pub mod detail;
pub mod editor;
pub mod emote;
pub mod entities;
pub mod files;
pub mod fluid;
pub mod hud;
pub mod input;
pub mod menu;
pub mod outdoors;
pub mod portals;
pub mod pose;
pub mod props;
pub mod proto;
pub mod voice;
pub mod wheel;

use bevy::{gltf::Gltf, prelude::*};

/// Log a line (the host app can route these to its own log with [`files::set_logger`]).
#[macro_export]
macro_rules! note {
    ($($t:tt)*) => { $crate::files::log(&format!($($t)*)) };
}

/// The camera the world is seen through.
#[derive(Component, Default)]
pub struct WorldCamera;

/// Meshes the host app's bevel pass should leave alone (blocks are already square).
#[derive(Component, Default, Clone, Copy)]
pub struct NoBevel;

/// The light the sun or moon drives; fill and rim lights are turned off in a world.
#[derive(Component, Default)]
pub struct KeyLight;
#[derive(Component, Default)]
pub struct FillLight;
#[derive(Component, Default)]
pub struct RimLight;

/// How much to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Level {
    Low,
    #[default]
    Medium,
    High,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "low" => Level::Low,
            "medium" | "med" => Level::Medium,
            "high" => Level::High,
            _ => return None,
        })
    }
}

/// The host's rendering choices, and whether a world is lighting the scene.
#[derive(Resource, Clone, Copy, Debug)]
pub struct WorldRender {
    pub level: Level,
    pub shadows: bool,
    /// set while a world draws its own sky and light
    pub outdoors: bool,
    /// light from the whole sky through generated environment maps (needs compute shaders,
    /// which some browsers' WebGPU lacks); without them, flat ambient light stands in
    pub env_maps: bool,
}

impl Default for WorldRender {
    fn default() -> Self {
        WorldRender { level: Level::Medium, shadows: true, outdoors: false, env_maps: true }
    }
}

/// The player's own character, as the host app has it.
#[derive(Resource, Default, Clone, Debug)]
pub struct LocalAvatar {
    /// its model file under the assets folder (others load it to see you)
    pub model: String,
    /// shown for players whose model can't be loaded
    pub fallback: String,
    /// the character's scene root, once spawned
    pub root: Option<Entity>,
    pub gltf: Option<Handle<Gltf>>,
    /// its animation is running (the nodes are there)
    pub ready: bool,
    /// it has a Walk clip
    pub can_walk: bool,
    /// set by the world: walking (play Walk) or standing
    pub walking: bool,
    /// set by the emote wheel: play this clip once (the host takes it); walking stops it
    pub emote: Option<String>,
    /// the character's clips by name, as the host has them: the Emotes screen offers these
    pub clips: Vec<String>,
}
