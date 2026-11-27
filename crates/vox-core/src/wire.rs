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
//!
//! Voice ([`VoicePacket`], protocol 6) is one short ADPCM packet of speech, sent by a player
//! and passed on by the server to the players who may hear it. The server does not decode it.
//! Its messages are the last variants of [`ToServer`] and [`ToClient`].

use crate::world::{Bounds, Cell, ChunkKey, Why};
use serde::{Deserialize, Serialize};

/// Bumped whenever these messages change in a way old players can't read. 6: travel by the
/// server (`Through`, `Travel`; `Notice` came after it); 7: voice (`ToServer::Voice`,
/// `ToClient::Voice`); 8: the ground's water in weather keyframes (`Keyframe::ground`).
pub const PROTOCOL: u32 = 8;

/// Voice channel: heard by players near the speaker.
pub const VOICE_NEARBY: u8 = 0;
/// Voice channel: heard by the speaker's raid party.
pub const VOICE_PARTY: u8 = 1;
/// Samples in one voice packet (20 ms at 16 kHz).
pub const VOICE_SAMPLES: u16 = 320;
/// Bytes of ADPCM data in one voice packet (4 bits per sample).
pub const VOICE_BYTES: usize = 160;

/// One packet of speech: 320 samples of 4-bit ADPCM, with the decoder's starting state.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VoicePacket {
    /// [`VOICE_NEARBY`] or [`VOICE_PARTY`]
    pub channel: u8,
    /// the speaker's packet number, so the listener can spot gaps
    pub seq: u32,
    /// samples in this packet; must be [`VOICE_SAMPLES`]
    pub samples: u16,
    /// the predictor the decoder starts from
    pub pred: i16,
    /// the step index the decoder starts from (0..=88)
    pub index: u8,
    /// the 4-bit codes, two per byte; must be [`VOICE_BYTES`] long
    pub data: Vec<u8>,
}

impl VoicePacket {
    /// Checks the sizes and ranges a packet from the network must have. Decoding and
    /// playing a packet that fails this is never safe, so reject it first.
    pub fn valid(&self) -> bool {
        self.channel <= VOICE_PARTY
            && self.samples == VOICE_SAMPLES
            && self.data.len() == VOICE_BYTES
            && self.index <= 88
    }
}

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
    /// Through the doorway called `portal` (to another world), from `side` (+1 from its +x/+z
    /// side, -1 the other). The world checks you're there and answers [`ToClient::Travel`].
    /// Added last, so older messages keep their numbers.
    Through { portal: String, side: i8 },
    /// A packet of speech from this player. Added last, so older messages keep their numbers.
    Voice(VoicePacket),
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
    /// Go through: connect to `url` (as a portal's url: `":7781"`, `"/w/7781/ws"` or whole),
    /// saying hello with `ticket` if there is one (it's for that world only, once, for a
    /// minute), and arrive at `arrive` (blocks, feet), which is where the world there puts you.
    Travel { url: String, ticket: Option<String>, arrive: [f32; 3] },
    /// Something everyone here should know (a dungeon's objective reached), shown for a moment.
    Notice { text: String },
    /// Speech from `player`, passed on by the server. Added last, so older messages keep
    /// their numbers.
    Voice { player: u32, packet: VoicePacket },
}

impl ToClient {
    /// May be dropped when a player can't keep up (a newer one will follow).
    pub fn droppable(&self) -> bool {
        // (a missed weather keyframe is made up by the next one)
        matches!(self, ToClient::Pose { .. } | ToClient::Weather(_) | ToClient::Voice { .. })
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
            ToClient::Correct { at: [1.0, 2.0, 3.0] },
            ToClient::Travel { url: ":7781".into(), ticket: Some("t1.a.b".into()), arrive: [0.0, 0.0, 11.5] },
            ToClient::Notice { text: "The altar: reached".into() },
        ];
        for m in msgs {
            let back: ToClient = decode(&encode(&m)).unwrap();
            assert_eq!(back, m);
        }
        let m = ToServer::Hello { protocol: PROTOCOL, name: "Sam".into(), avatar: vec![1], token: Some("w1.a.b".into()) };
        assert_eq!(decode::<ToServer>(&encode(&m)).unwrap(), m);
        let m = ToServer::Through { portal: "yard".into(), side: -1 };
        assert_eq!(decode::<ToServer>(&encode(&m)).unwrap(), m);
        let m = ToServer::Edit { id: 1, cell: [1, -2, 3], block: 5 };
        assert_eq!(decode::<ToServer>(&encode(&m)).unwrap(), m);
        // an edit is small
        assert!(encode(&m).len() < 12);
    }

    fn voice() -> VoicePacket {
        VoicePacket {
            channel: VOICE_PARTY,
            seq: 42,
            samples: VOICE_SAMPLES,
            pred: -300,
            index: 88,
            data: vec![0x5a; VOICE_BYTES],
        }
    }

    #[test]
    fn voice_round_trips_and_is_small() {
        let m = ToServer::Voice(voice());
        let bytes = encode(&m);
        assert_eq!(decode::<ToServer>(&bytes).unwrap(), m);
        assert!(bytes.len() < 200, "voice packet is {} bytes", bytes.len());

        let m = ToClient::Voice { player: 9, packet: voice() };
        let bytes = encode(&m);
        assert_eq!(decode::<ToClient>(&bytes).unwrap(), m);
        assert!(bytes.len() < 200, "voice packet is {} bytes", bytes.len());
        assert!(m.droppable());
    }

    #[test]
    fn voice_valid_rejects_bad_packets() {
        assert!(voice().valid());
        let mut p = voice();
        p.channel = 2;
        assert!(!p.valid());
        let mut p = voice();
        p.samples = VOICE_SAMPLES - 1;
        assert!(!p.valid());
        let mut p = voice();
        p.data.pop();
        assert!(!p.valid());
        let mut p = voice();
        p.index = 89;
        assert!(!p.valid());
    }

    #[test]
    fn old_variants_keep_their_first_byte() {
        // the variant index is the first byte postcard writes; old players rely on it
        assert_eq!(encode(&ToServer::Hello { protocol: PROTOCOL, name: String::new(), avatar: vec![], token: None })[0], 0);
        assert_eq!(encode(&ToServer::Edit { id: 0, cell: [0, 0, 0], block: 0 })[0], 1);
        assert_eq!(encode(&ToServer::Watch { protocol: PROTOCOL })[0], 4);
        assert_eq!(encode(&ToServer::Through { portal: String::new(), side: 1 })[0], 5);
        assert_eq!(encode(&ToServer::Voice(voice()))[0], 6);
        assert_eq!(encode(&ToClient::Ready)[0], 2);
        assert_eq!(encode(&ToClient::Correct { at: [0.0; 3] })[0], 10);
        assert_eq!(encode(&ToClient::Travel { url: String::new(), ticket: None, arrive: [0.0; 3] })[0], 11);
        assert_eq!(encode(&ToClient::Notice { text: String::new() })[0], 12);
        assert_eq!(encode(&ToClient::Voice { player: 0, packet: voice() })[0], 13);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode::<ToServer>(&[250, 1, 2]).is_err());
        assert!(decode::<ToClient>(&[]).is_err());
    }

    #[test]
    fn truncated_voice_is_an_error() {
        // a cut-off packet must fail to decode, not come out as a short packet
        let bytes = encode(&ToServer::Voice(voice()));
        assert!(bytes.len() > 50);
        assert!(decode::<ToServer>(&bytes[..50]).is_err());

        let bytes = encode(&ToClient::Voice { player: 9, packet: voice() });
        assert!(bytes.len() > 50);
        assert!(decode::<ToClient>(&bytes[..50]).is_err());
    }

    #[test]
    fn voice_with_the_wrong_length_decodes_but_is_not_valid() {
        // the length is checked after decoding: the packet itself is well formed
        let mut p = voice();
        p.data = vec![0x5a; VOICE_BYTES + 100];
        let bytes = encode(&ToServer::Voice(p.clone()));
        let back: ToServer = decode(&bytes).unwrap();
        assert_eq!(back, ToServer::Voice(p.clone()));
        assert!(!p.valid());
    }
}
