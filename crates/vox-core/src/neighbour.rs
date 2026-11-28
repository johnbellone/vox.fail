//! The link between two neighbouring world servers (ADR 0005, step 2): its messages and the
//! proofs that both ends know the world secret.
//!
//! Worlds that meet (atlas tiles side by side, the two ends of a doorway) keep a WebSocket
//! between them. The world whose id sorts lower dials ([`Hail`]); the other listens
//! ([`Answer`]). The handshake:
//!
//! 1. dialler: `Hello { protocol, from, nonce }`
//! 2. listener: `Challenge { world, nonce, proof }`, the proof made over the dialler's nonce, so
//!    a listener that doesn't know the secret can't pass and the dialler never offers its own
//!    proof to it;
//! 3. dialler: `Proof { proof }`, made over the listener's nonce (so an old one can't be replayed);
//! 4. listener: `Linked`. Then `Ping` / `Pong` every few seconds.
//!
//! The secret itself never goes over the link: each proof is a keyed BLAKE3 hash of the side,
//! the nonce and both world ids, under a key derived from the secret.
//!
//! Postcard like [`crate::wire`]. Everything that arrives is untrusted: messages are capped in
//! size, world ids in length and characters, and decoding errors are answers, never panics.

use serde::{Deserialize, Serialize};

/// The link protocol's version; a hello with another is refused.
pub const LINK_PROTOCOL: u32 = 1;
/// The longest world id a link names (as vox-host's ids).
pub const MAX_WORLD_ID: usize = 64;
/// The largest link message taken (a hello is under 120 bytes; a goodbye's reason is capped).
pub const MAX_LINK_MESSAGE: usize = 1024;
/// The longest reason a goodbye gives.
pub const MAX_WHY: usize = 200;

/// What the dialling world sends.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Hail {
    /// the first message: the link protocol, the dialler's world id and a fresh nonce
    Hello { protocol: u32, from: String, nonce: [u8; 32] },
    /// the dialler's proof, over the listener's nonce
    Proof { proof: [u8; 32] },
    /// still here (`t` is the dialler's own clock, echoed back)
    Ping { t: u64 },
}

/// What the listening world answers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Answer {
    /// the listener's world id, its own fresh nonce, and its proof over the dialler's nonce
    Challenge { world: String, nonce: [u8; 32], proof: [u8; 32] },
    /// both proofs checked: the link is up
    Linked,
    /// the answer to a ping, with its `t`
    Pong { t: u64 },
    /// refused or closing, and why; the link is closed after it
    Bye { why: String },
}

/// Which end a proof is made by: the two ends' proofs differ even over the same nonce, so one
/// can't be sent back as the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Listener,
    Dialler,
}

/// A world id a link may name: lower-case letters, digits, `-` and `_`, 1 to 64 of them.
pub fn valid_world(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_WORLD_ID && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The proof `side` gives: a keyed hash, under a key derived from the world secret, of the side,
/// the other end's nonce, the prover's world id and the other's. Each id is preceded by its
/// length, so ("ab", "c") and ("a", "bc") hash apart.
pub fn proof(secret: &str, side: Side, nonce: &[u8; 32], own: &str, other: &str) -> [u8; 32] {
    let key = blake3::derive_key("vox.fail neighbour link v1", secret.as_bytes());
    let mut m = Vec::with_capacity(16 + 32 + own.len() + other.len());
    m.extend_from_slice(match side {
        Side::Listener => b"listener",
        Side::Dialler => b"dialler\0",
    });
    m.extend_from_slice(nonce);
    for id in [own, other] {
        m.extend_from_slice(&(id.len() as u64).to_le_bytes());
        m.extend_from_slice(id.as_bytes());
    }
    *blake3::keyed_hash(&key, &m).as_bytes()
}

/// Is `given` the proof expected? Compared in time that doesn't depend on where they differ.
pub fn same(given: &[u8; 32], expected: &[u8; 32]) -> bool {
    given.iter().zip(expected).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

pub fn encode<T: Serialize>(m: &T) -> Vec<u8> {
    crate::wire::encode(m)
}

/// A dialler's message, from the network: size-capped, and the hello's world id checked.
pub fn decode_hail(b: &[u8]) -> Result<Hail, String> {
    if b.len() > MAX_LINK_MESSAGE {
        return Err("too long for a link message".into());
    }
    let h: Hail = crate::wire::decode(b)?;
    if let Hail::Hello { from, .. } = &h
        && !valid_world(from)
    {
        return Err("not a world id".into());
    }
    Ok(h)
}

/// A listener's message, from the network: size-capped, the challenge's world id checked and a
/// goodbye's reason cut to [`MAX_WHY`] without control characters.
pub fn decode_answer(b: &[u8]) -> Result<Answer, String> {
    if b.len() > MAX_LINK_MESSAGE {
        return Err("too long for a link message".into());
    }
    match crate::wire::decode(b)? {
        Answer::Challenge { world, .. } if !valid_world(&world) => Err("not a world id".into()),
        Answer::Bye { why } => Ok(Answer::Bye { why: why.chars().filter(|c| !c.is_control()).take(MAX_WHY).collect() }),
        a => Ok(a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let hails = [Hail::Hello { protocol: LINK_PROTOCOL, from: "atlas-earth-0-0".into(), nonce: [7; 32] }, Hail::Proof { proof: [1; 32] }, Hail::Ping { t: 12345 }];
        for h in hails {
            assert_eq!(decode_hail(&encode(&h)).unwrap(), h);
        }
        let answers = [
            Answer::Challenge { world: "yard".into(), nonce: [2; 32], proof: [3; 32] },
            Answer::Linked,
            Answer::Pong { t: 9 },
            Answer::Bye { why: "not a neighbour".into() },
        ];
        for a in answers {
            assert_eq!(decode_answer(&encode(&a)).unwrap(), a);
        }
    }

    #[test]
    fn the_handshake_checks_out_only_with_the_secret() {
        let (dn, ln) = ([5u8; 32], [6u8; 32]);
        // the listener proves over the dialler's nonce; the dialler checks with its own secret
        let p = proof("s3cret", Side::Listener, &dn, "yard", "void");
        assert!(same(&p, &proof("s3cret", Side::Listener, &dn, "yard", "void")));
        // another secret, another nonce, the other side, or the ids swapped: no match
        assert!(!same(&p, &proof("s3creT", Side::Listener, &dn, "yard", "void")));
        assert!(!same(&p, &proof("s3cret", Side::Listener, &ln, "yard", "void")));
        assert!(!same(&p, &proof("s3cret", Side::Dialler, &dn, "yard", "void")));
        assert!(!same(&p, &proof("s3cret", Side::Listener, &dn, "void", "yard")));
        // ids are length-prefixed, so their boundary counts
        assert!(!same(&proof("s", Side::Dialler, &ln, "ab", "c"), &proof("s", Side::Dialler, &ln, "a", "bc")));
        let mut wrong = p;
        wrong[31] ^= 1;
        assert!(!same(&wrong, &p));
    }

    #[test]
    fn untrusted_messages_are_refused() {
        let long = "a".repeat(MAX_WORLD_ID + 1);
        assert!(decode_hail(&encode(&Hail::Hello { protocol: 1, from: long.clone(), nonce: [0; 32] })).is_err());
        assert!(decode_hail(&encode(&Hail::Hello { protocol: 1, from: "Void/..".into(), nonce: [0; 32] })).is_err());
        assert!(decode_hail(&encode(&Hail::Hello { protocol: 1, from: String::new(), nonce: [0; 32] })).is_err());
        assert!(decode_answer(&encode(&Answer::Challenge { world: long, nonce: [0; 32], proof: [0; 32] })).is_err());
        assert!(decode_hail(&vec![0u8; MAX_LINK_MESSAGE + 1]).is_err());
        assert!(decode_hail(&[0xff, 0xff, 0xff]).is_err());
        assert!(decode_answer(&[]).is_err());
        // a goodbye's reason is cut short and cleaned
        let why = format!("{}\n{}", "x".repeat(10), "y".repeat(500));
        let Answer::Bye { why } = decode_answer(&encode(&Answer::Bye { why })).unwrap() else { panic!() };
        assert_eq!(why.chars().count(), MAX_WHY);
        assert!(!why.contains('\n'));
    }
}
