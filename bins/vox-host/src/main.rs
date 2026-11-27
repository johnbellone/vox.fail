//! Run many worlds on one port: `vox-host [--listen 127.0.0.1:7790] [--data /var/lib/vox]
//! --world <id>[=<seed level>][@<alias>] … [--blocks data/blocks.json]` plus the flags every
//! world takes (`--directory-key`, `--guests`, `--directory-url`, `--max-players`,
//! `--max-watchers`, `--trust-moves`, `--no-snapshots`, `--oxen`, `--snapshot-every`; see
//! `vox_server::flags`), which apply to every world.
//!
//! `--idle-stop <minutes>` (at least 1): worlds start when someone first connects and are saved and stopped
//! after that long with nobody playing or watching. `--keep <id>` (repeatable) names worlds that
//! start with the host and never stop, such as the Void. Without `--idle-stop` every world
//! starts with the host.
//!
//! Each `--world` is one world: `<id>` is its name (for the directory too) and its path,
//! `/w/<id>/ws`; it is kept in `<data>/<id>.db`, with snapshots in `<data>/<id>.db.snapshots/`,
//! the same files `vox-server --db /var/lib/vox/<id>.db` uses, so worlds move over as they are.
//! A new world is filled from its seed level, `data/levels/<id>.json` unless `=<file>` names
//! another. `@<alias>` is another name for its path, usually its old port, so
//! `/w/7781/ws` still reaches the yard:
//!
//!     vox-host --listen 127.0.0.1:7790 --data /var/lib/vox \
//!       --world void@7780 --world yard@7781 --world islands@7782 --world sea@7783 --world orbit@7784
//!
//! Ctrl-C (or SIGTERM) saves every world and stops.

use std::path::{Path, PathBuf};

use vox_host::{Host, Spec};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let listen = arg("--listen").unwrap_or_else(|| "127.0.0.1:7790".into());
    let data_dir = PathBuf::from(arg("--data").unwrap_or_else(|| ".".into()));
    let game = find_data();
    let blocks_path = arg("--blocks").map(PathBuf::from).unwrap_or_else(|| game.join("blocks.json"));
    let blocks = std::fs::read_to_string(&blocks_path).unwrap_or_else(|e| panic!("can't read {}: {e}", blocks_path.display()));
    let palette = vox_core::palette_names(&blocks).expect("block palette");

    let worlds: Vec<String> = args.windows(2).filter(|w| w[0] == "--world").map(|w| w[1].clone()).collect();
    if worlds.is_empty() {
        eprintln!("vox-host: name at least one world: --world void@7780");
        std::process::exit(2);
    }
    let keep: Vec<String> = args.windows(2).filter(|w| w[0] == "--keep").map(|w| w[1].clone()).collect();
    let idle = arg("--idle-stop").map(|m| {
        // at least a minute: a world remembers which tickets were used only while it runs, and
        // a ticket lasts a minute, so a world can't stop and start again within one's life
        let m: f64 = m.parse().ok().filter(|m: &f64| m.is_finite() && *m >= 1.0).unwrap_or_else(|| panic!("--idle-stop takes minutes, at least 1"));
        std::time::Duration::from_secs_f64(m * 60.0)
    });
    let mut specs = Vec::new();
    for w in &worlds {
        let (id, seed, alias) = parse_world(w).unwrap_or_else(|e| panic!("--world {w}: {e}"));
        let seed = seed.map(PathBuf::from).unwrap_or_else(|| game.join(format!("levels/{id}.json")));
        let mut cfg = vox_server::Config::new(data_dir.join(format!("{id}.db")), palette.clone());
        cfg.block_size = vox_core::block_size(&blocks);
        cfg.name = id.clone();
        cfg.seed = std::fs::read_to_string(&seed).ok();
        if cfg.seed.is_none() {
            eprintln!("vox-host: no seed level at {} for \"{id}\": a new world there starts empty", seed.display());
        }
        cfg.level = seed.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        vox_server::flags::apply(&mut cfg, &args).unwrap_or_else(|e| panic!("{e}"));
        let keep = keep.contains(&id);
        specs.push(Spec { id, aliases: alias.into_iter().collect(), cfg, keep });
    }

    if let Some(k) = keep.iter().find(|k| !worlds.iter().any(|w| parse_world(w).is_ok_and(|(id, ..)| &id == *k))) {
        panic!("--keep {k}: there's no --world {k}");
    }
    let host = Host::start(specs, idle).await.unwrap_or_else(|e| panic!("vox-host: {e}"));
    let listener = tokio::net::TcpListener::bind(&listen).await.unwrap_or_else(|e| panic!("can't listen on {listen}: {e}"));
    eprintln!("vox-host: {} on {listen} (ws://{listen}/w/<id>/ws), saved in {}", host.ids().join(", "), data_dir.display());
    if let Some(idle) = idle {
        eprintln!("vox-host: running {}; the others start when asked for and stop after {} s unused", host.running().await.join(", "), idle.as_secs());
    }
    let stop = host.clone();
    axum::serve(listener, host.router())
        .with_graceful_shutdown(async move {
            stop_asked().await;
            eprintln!("saving…");
            stop.shutdown().await;
        })
        .await
        .expect("server");
}

/// `<id>[=<seed>][@<alias>]`.
fn parse_world(s: &str) -> Result<(String, Option<String>, Option<String>), String> {
    let (rest, alias) = match s.rsplit_once('@') {
        Some((r, a)) => (r, Some(a.to_string())),
        None => (s, None),
    };
    let (id, seed) = match rest.split_once('=') {
        Some((i, f)) if !f.is_empty() => (i, Some(f.to_string())),
        Some(_) => return Err("an empty seed level".into()),
        None => (rest, None),
    };
    if !vox_host::valid_id(id) {
        return Err(format!("\"{id}\" can't name a world: lower-case letters, digits, - and _ only"));
    }
    Ok((id.to_string(), seed, alias))
}

/// Ctrl-C, or SIGTERM (how systemd and Kubernetes ask a program to stop): either way, save
/// and stop.
async fn stop_asked() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// The `data` folder: next to the working directory, or next to the executable.
fn find_data() -> PathBuf {
    let mut tries = vec![PathBuf::from("data")];
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            tries.push(dir.join("data"));
        }
    }
    tries.into_iter().find(|p| Path::new(p).join("blocks.json").exists()).unwrap_or_else(|| PathBuf::from("data"))
}

#[cfg(test)]
mod tests {
    use super::parse_world;

    #[test]
    fn world_flags() {
        let p = |s: &str| parse_world(s).unwrap();
        assert_eq!(p("void"), ("void".into(), None, None));
        assert_eq!(p("yard@7781"), ("yard".into(), None, Some("7781".into())));
        assert_eq!(p("crypt=levels/crypt.json"), ("crypt".into(), Some("levels/crypt.json".into()), None));
        assert_eq!(p("crypt=levels/crypt.json@9000"), ("crypt".into(), Some("levels/crypt.json".into()), Some("9000".into())));
        assert!(parse_world("Void").is_err());
        assert!(parse_world("x=").is_err());
        assert!(parse_world("../x").is_err());
    }
}
