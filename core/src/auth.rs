//! Sign-in tokens: who a player is, signed by the directory.
//!
//! The directory (where people sign in and make their character) hands a signed-in player a
//! token: their account, the name they go by, their character and when it runs out, signed
//! with the directory's Ed25519 key. World servers hold only the directory's public key, so
//! they check a token with no call to the directory.
//!
//! A token is `v1.<claims>.<signature>`, both parts base64url (no padding); the claims are
//! postcard. Tokens last a day by default.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::{Signature, Signer, Verifier};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

/// How long a token lasts, s.
pub const LIFETIME: u64 = 24 * 3600;

/// What someone looks like in a world.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Character {
    /// the model, under the assets folder (`models/reporter.glb`)
    pub model: String,
    /// what they wear, by wardrobe item id (`cowboy`, `shades`), at most one per slot
    pub wear: Vec<String>,
}

/// Who a token says the player is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    /// the directory's account number
    pub account: u64,
    /// the name they go by
    pub name: String,
    pub character: Character,
    /// seconds since 1970
    pub issued: u64,
    pub expires: u64,
}

/// Make a token for `claims`.
pub fn sign(claims: &Claims, key: &SigningKey) -> String {
    let body = postcard::to_stdvec(claims).expect("claims");
    let sig = key.sign(&body);
    format!("v1.{}.{}", B64.encode(&body), B64.encode(sig.to_bytes()))
}

/// What a token says, if `key` signed it and it hasn't run out at `now` (seconds since 1970).
pub fn verify(token: &str, key: &VerifyingKey, now: u64) -> Result<Claims, String> {
    let mut parts = token.trim().split('.');
    let (Some("v1"), Some(body), Some(sig), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err("not a sign-in token".into());
    };
    let body = B64.decode(body).map_err(|_| "bad token")?;
    let sig = B64.decode(sig).map_err(|_| "bad token")?;
    let sig = Signature::from_slice(&sig).map_err(|_| "bad token")?;
    key.verify(&body, &sig).map_err(|_| "this token wasn't signed by the directory")?;
    let claims: Claims = postcard::from_bytes(&body).map_err(|_| "bad token")?;
    if now >= claims.expires {
        return Err("your sign-in has run out: sign in again".into());
    }
    if claims.issued > now + 300 {
        return Err("this token is from the future".into());
    }
    Ok(claims)
}

/// What a token says, without checking who signed it (for a client showing its own name).
pub fn peek(token: &str) -> Option<Claims> {
    let body = token.trim().split('.').nth(1)?;
    postcard::from_bytes(&B64.decode(body).ok()?).ok()
}

/// A key from 32 random bytes.
pub fn key_from_seed(seed: [u8; 32]) -> SigningKey {
    SigningKey::from_bytes(&seed)
}

/// A public key as hex, and back.
pub fn public_hex(key: &VerifyingKey) -> String {
    key.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn public_from_hex(hex: &str) -> Result<VerifyingKey, String> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err("a public key is 64 hex digits".into());
    }
    let mut b = [0u8; 32];
    for (i, byte) in b.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| "not hex")?;
    }
    VerifyingKey::from_bytes(&b).map_err(|_| "not a public key".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(now: u64) -> Claims {
        Claims {
            account: 7,
            name: "Sam".into(),
            character: Character { model: "models/reporter.glb".into(), wear: vec!["cowboy".into()] },
            issued: now,
            expires: now + LIFETIME,
        }
    }

    #[test]
    fn signed_tokens_check_out() {
        let key = key_from_seed([3; 32]);
        let t = sign(&claims(1000), &key);
        assert!(t.starts_with("v1."));
        assert_eq!(verify(&t, &key.verifying_key(), 2000).unwrap(), claims(1000));
        assert_eq!(peek(&t).unwrap().name, "Sam");
        // the public key travels as hex
        let hex = public_hex(&key.verifying_key());
        assert_eq!(public_from_hex(&hex).unwrap(), key.verifying_key());
        assert!(public_from_hex("abc").is_err());
    }

    #[test]
    fn forged_old_and_broken_tokens_dont() {
        let key = key_from_seed([3; 32]);
        let other = key_from_seed([4; 32]);
        let t = sign(&claims(1000), &key);
        // another key
        assert!(verify(&t, &other.verifying_key(), 2000).is_err());
        // run out
        assert!(verify(&t, &key.verifying_key(), 1000 + LIFETIME).is_err());
        // from the future
        assert!(verify(&t, &key.verifying_key(), 100).is_err());
        // a changed name with the old signature
        let mut c = claims(1000);
        c.name = "Admin".into();
        let forged = sign(&c, &other);
        let (_, sig) = t.rsplit_once('.').unwrap();
        let (head, _) = forged.rsplit_once('.').unwrap();
        assert!(verify(&format!("{head}.{sig}"), &key.verifying_key(), 2000).is_err());
        for junk in ["", "v1", "v1.a.b", "w1.a.b", "v1.a.b.c", "hello"] {
            assert!(verify(junk, &key.verifying_key(), 2000).is_err(), "{junk}");
        }
    }
}
