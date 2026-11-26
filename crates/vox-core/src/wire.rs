//! The messages between a world server and its players, one per WebSocket binary message,
//! postcard-encoded.
//!
//! Joining: the player sends [`ToServer::Hello`]; the server answers [`ToClient::Welcome`],
//! every chunk with blocks in it, [`ToClient::Ready`], then a [`ToClient::Joined`] for each
//! player already there. From then on edits go out in one order to everyone
//! ([`ToClient::Edits`], numbered by `seq`); a refused edit is answered only to its sender.
//!
//! Poses are opaque to the server (the app's own pose format), apart from where the player
//! is, which edits are checked against.

use crate::world::{Bounds, Cell, ChunkKey, Why};
use serde::{Deserialize, Serialize};

/// Bumped whenever these messages change in a way old players can't read.
pub const PROTOCOL: u32 = 5;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToServer {
    Hello {
        protocol: u32,
        name: String,
        /// what the player looks like (opaque; passed on to the others)
        avatar: Vec<u8>,
        /// a sign-in token from the directory ([`crate::auth`]); a world that needs sign-in
        /// takes the player's name from it
        token: Option<String>,
    },
    /// Set `cell` to `block` (0 = air). `id` is the player's own number for it, echoed back.
    Edit { id: u32, cell: Cell, block: u16 },
    /// Where the player is (in blocks, feet) and their pose (opaque).
    Pose { at: [f32; 3], blob: Vec<u8> },
    /// Change the world's weather (its climate; the simulation takes it from there).
    Climate(crate::weather::Climate),
    /// Instead of `Hello`: watch the world without being in it (the front page's live view).
    /// A watcher gets the world, the edits, the weather and everyone's poses, but no names,
    /// is not a player, and may send nothing more. Added last, so older messages keep their
    /// numbers.
    Watch { protocol: u32 },
}

/// An edit the server made.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Applied {
    /// the world's edit number: every player sees edits in this order
    pub seq: u64,
    pub player: u32,
    /// the player's own number for the edit
    pub id: u32,
    pub cell: Cell,
    pub block: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToClient {
    Welcome {
        you: u32,
        world: String,
        /// block names in id order (id = index + 1)
        palette: Vec<String>,
        block_size: f32,
        bounds: Bounds,
        /// where to start, in blocks (feet)
        spawn: [f32; 3],
        /// the world's own settings as JSON: its sky ("earth" or "void"), time of day, clouds
        settings: String,
        /// the name you go by here (from your sign-in, when the world checks it)
        name: String,
    },
    Chunk { key: ChunkKey, version: u32, data: Vec<u8> },
    /// All the chunks have been sent.
    Ready,
    Edits(Vec<Applied>),
    Refused { id: u32, why: Why },
    Joined { player: u32, name: String, avatar: Vec<u8> },
    Left { player: u32 },
    Pose { player: u32, blob: Vec<u8> },
    /// The world's weather: all of it, on joining, every 15 seconds and when it changes.
    /// Players simulate it themselves in between.
    Weather(Box<crate::weather::Keyframe>),
    /// The server is closing this connection.
    Bye { why: String },
    /// You can't have got there: go back to `at` (blocks, feet). Added last, so older messages
    /// keep their numbers.
    Correct { at: [f32; 3] },
}

impl ToClient {
    /// May be dropped when a player can't keep up (a newer one will follow).
    pub fn droppable(&self) -> bool {
        // (a missed weather keyframe is made up by the next one)
        matches!(self, ToClient::Pose { .. } | ToClient::Weather(_))
    }
}

pub fn encode<T: Serialize>(m: &T) -> Vec<u8> {
    postcard::to_stdvec(m).expect("encode")
}

pub fn decode<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Result<T, String> {
    postcard::from_bytes(b).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let msgs = vec![
            ToClient::Welcome {
                you: 3,
                world: "yard".into(),
                palette: vec!["grass".into(), "stone".into()],
                block_size: 0.5,
                bounds: Bounds::default(),
                spawn: [0.5, 0.0, 0.5],
                settings: r#"{"sky":"void"}"#.into(),
                name: "Sam".into(),
            },
            ToClient::Chunk { key: ChunkKey([-1, 0, 2]), version: 9, data: vec![1, 2, 3] },
            ToClient::Ready,
            ToClient::Edits(vec![Applied { seq: 1 << 40, player: 2, id: 7, cell: [-5, 3, 100], block: 0 }]),
            ToClient::Refused { id: 4, why: Why::TooFar },
            ToClient::Joined { player: 1, name: "Ann".into(), avatar: vec![9] },
            ToClient::Left { player: 1 },
            ToClient::Pose { player: 1, blob: vec![0; 40] },
            ToClient::Bye { why: "full".into() },
        ];
        for m in msgs {
            let back: ToClient = decode(&encode(&m)).unwrap();
            assert_eq!(back, m);
        }
        let m = ToServer::Hello { protocol: PROTOCOL, name: "Sam".into(), avatar: vec![1], token: Some("w1.a.b".into()) };
        assert_eq!(decode::<ToServer>(&encode(&m)).unwrap(), m);
        let m = ToServer::Edit { id: 1, cell: [1, -2, 3], block: 5 };
        assert_eq!(decode::<ToServer>(&encode(&m)).unwrap(), m);
        // an edit is small
        assert!(encode(&m).len() < 12);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode::<ToServer>(&[250, 1, 2]).is_err());
        assert!(decode::<ToClient>(&[]).is_err());
    }
}
