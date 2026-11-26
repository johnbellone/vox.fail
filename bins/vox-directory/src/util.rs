//! Small helpers: cookies, the time, random tokens, hashing, the signing key, checking names.

#[allow(unused_imports)]
use super::*;

/// The value of cookie `name` in a request.
pub(super) fn cookie(h: &HeaderMap, name: &str) -> Option<String> {
    h.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

pub(super) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub(super) fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("random");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub(super) fn salt() -> SaltString {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("random");
    SaltString::encode_b64(&b).expect("salt")
}

pub(super) fn sha(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|x| format!("{x:02x}")).collect()
}

/// A new signing key, from the system's randomness.
pub fn new_key() -> SigningKey {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("random");
    auth::key_from_seed(seed)
}

/// The key in `path` (64 hex digits), made and saved there the first time.
pub fn load_or_make_key(path: &std::path::Path) -> Result<SigningKey, String> {
    if let Ok(s) = std::fs::read_to_string(path) {
        let s = s.trim();
        let mut seed = [0u8; 32];
        if s.len() != 64 {
            return Err(format!("{} should hold 64 hex digits", path.display()));
        }
        for (i, b) in seed.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| format!("{} isn't hex", path.display()))?;
        }
        return Ok(auth::key_from_seed(seed));
    }
    let key = new_key();
    let hex: String = key.to_bytes().iter().map(|x| format!("{x:02x}")).collect();
    std::fs::write(path, hex + "\n").map_err(|e| format!("can't write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// A name people go by: 3-20 letters, digits, spaces, `_`, `-` or `.`, starting with a letter
/// or digit.
pub fn check_name(n: &str) -> Result<String, String> {
    let n = n.trim();
    let len = n.chars().count();
    if !(3..=20).contains(&len) {
        return Err("names are 3 to 20 characters".into());
    }
    if !n.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return Err("names start with a letter or a digit".into());
    }
    if !n.chars().all(|c| c.is_alphanumeric() || " _-.".contains(c)) || n.contains("  ") {
        return Err("names use letters, digits, single spaces, _ - and .".into());
    }
    Ok(n.to_string())
}
