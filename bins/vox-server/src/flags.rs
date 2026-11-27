//! The command-line flags every world takes, shared by `vox-server` (one world) and `vox-host`
//! (many worlds in one process), so the two can't drift apart.
//!
//! `--directory-url` (with `VOX_WORLD_SECRET`), `--max-players`, `--max-watchers`,
//! `--trust-moves`, `--directory-key <hex or file>` with `--guests`, `--no-snapshots`,
//! `--oxen [<binary>]` (or `VOX_OXEN`), `--snapshot-every <seconds>`. Snapshots go next to the
//! world's database (`<db>.snapshots/`) unless turned off or the world is in memory.

use std::{path::PathBuf, time::Duration};

use crate::Config;

/// Apply the shared flags in `args` to `cfg`. Call after `cfg.db` is set: the snapshots folder
/// is named after it.
pub fn apply(cfg: &mut Config, args: &[String]) -> Result<(), String> {
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    cfg.directory_url = arg("--directory-url");
    cfg.world_secret = std::env::var("VOX_WORLD_SECRET").ok().filter(|s| !s.trim().is_empty());
    if cfg.directory_url.is_some() && cfg.world_secret.is_none() {
        eprintln!("--directory-url without VOX_WORLD_SECRET: not reporting to the directory");
    }
    if let Some(n) = arg("--max-players").and_then(|v| v.parse().ok()) {
        cfg.max_players = n;
    }
    if let Some(n) = arg("--max-watchers").and_then(|v| v.parse().ok()) {
        cfg.max_watchers = n;
    }
    // test and automation worlds: positions as players report them, unchecked
    cfg.trust_moves = has("--trust-moves");
    // sign-in: the directory's public key (hex, or a file holding it)
    if let Some(k) = arg("--directory-key") {
        let hex = std::fs::read_to_string(&k).unwrap_or(k);
        cfg.directory = Some(vox_core::auth::public_from_hex(&hex).map_err(|e| format!("--directory-key: {e}"))?);
        cfg.guests = has("--guests");
    }
    // snapshots next to the world's database unless told otherwise
    let in_memory = cfg.db.to_string_lossy() == ":memory:";
    cfg.snapshots = if has("--no-snapshots") || in_memory {
        None
    } else {
        let mut d = cfg.db.clone().into_os_string();
        d.push(".snapshots");
        Some(PathBuf::from(d))
    };
    // Oxen: `--oxen` alone finds `oxen` on the PATH; `--oxen /path/to/oxen` names it
    let oxen_flag = args.iter().position(|a| a == "--oxen").map(|i| args.get(i + 1).filter(|v| !v.starts_with("--")).cloned());
    cfg.snapshot_oxen = match (oxen_flag, std::env::var("VOX_OXEN").ok().filter(|v| !v.trim().is_empty())) {
        (Some(Some(path)), _) => Some(PathBuf::from(path)),
        (Some(None), env) => Some(PathBuf::from(env.unwrap_or_else(|| "oxen".into()))),
        (None, Some(path)) => Some(PathBuf::from(path)),
        (None, None) => None,
    };
    if let Some(n) = arg("--snapshot-every").and_then(|v| v.parse().ok()) {
        cfg.snapshot_every = Duration::from_secs(n);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn shared_flags() {
        let mut c = Config::new("/var/lib/vox/yard.db", vec![]);
        apply(&mut c, &args("x --max-players 8 --trust-moves --snapshot-every 60")).unwrap();
        assert_eq!(c.max_players, 8);
        assert!(c.trust_moves);
        assert_eq!(c.snapshot_every, Duration::from_secs(60));
        assert_eq!(c.snapshots, Some(PathBuf::from("/var/lib/vox/yard.db.snapshots")));
        assert!(c.directory.is_none());

        let mut c = Config::new(":memory:", vec![]);
        apply(&mut c, &args("x")).unwrap();
        assert_eq!(c.snapshots, None);
        let mut c = Config::new("a.db", vec![]);
        apply(&mut c, &args("x --no-snapshots")).unwrap();
        assert_eq!(c.snapshots, None);
        let mut c = Config::new("a.db", vec![]);
        assert!(apply(&mut c, &args("x --directory-key abc")).is_err());
    }
}
