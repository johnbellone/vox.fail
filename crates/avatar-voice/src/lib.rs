//! avatar-voice: what Block Avatar hears in a voice, with no engine in it.
//!
//! - [`lipsync`]: the microphone's analysis frames and the lip-sync styles (mouth shapes).
//! - [`voice`]: pitch (for body language) and the first two formants (for vowels).
//! - [`detect`]: laughing and singing, from the same frames.
//! - [`wav`]: reading and writing WAV files, hardened against bad input.

pub mod detect;
pub mod lipsync;
pub mod voice;
pub mod wav;

#[cfg(test)]
mod tests_real;
