//! Voice chat in a world: your microphone to nearby players and your raid party, and theirs
//! to you.
//!
//! The pure parts (codec, packets, jitter buffer) live in `vox-voice`; the server decides who
//! hears what. This module is the client side: [`capture`] reads the microphone and
//! [`playback`] mixes the other voices (both native only, the browser has no voice yet),
//! [`mix`] works out how loud and from which side each voice plays, and the plugin ties them
//! to the world link, the input actions, the HUD and the menu.

pub mod lipsync;
pub mod mix;
pub mod plugin;
#[cfg(not(target_arch = "wasm32"))]
pub mod capture;
#[cfg(not(target_arch = "wasm32"))]
pub mod playback;
