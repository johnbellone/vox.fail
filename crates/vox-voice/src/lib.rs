//! vox-voice: the voice codecs shared by the world's voice chat and Block Avatar, with no engine in it.
//!
//! - [`adpcm`]: IMA ADPCM, 4 bits a sample, decoded from data the network sent (so it never panics).
//! - [`resample`]: a linear resampler to the wire rate.
//! - [`jitter`]: the playout buffer that turns packets that arrive unevenly into an even stream.
//! - [`packet`]: cuts audio into 20 ms ADPCM packets behind a silence gate, and checks packets it gets.

pub mod adpcm;
pub mod jitter;
pub mod packet;
pub mod resample;

/// Samples a second on the wire.
pub const RATE: u32 = 16_000;
/// Samples per packet (20 ms).
pub const PACKET: usize = 320;
/// Held before playing starts (and again after running dry): 120 ms.
pub const PREBUFFER: usize = (RATE as usize) * 12 / 100;
/// Most held before old audio is dropped: 400 ms.
pub const MAX_HELD: usize = (RATE as usize) * 4 / 10;

pub use packet::{Packet, Packetizer};
