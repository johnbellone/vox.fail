//! Run a world: `vox-server [--listen 127.0.0.1:7780] [--db world.db] [--name void]
//! [--seed data/levels/void.json] [--blocks data/blocks.json] [--max-players 100] [--max-watchers 200] [--trust-moves]
//! [--voice-radius <blocks, default 32>]
//! [--directory-key <hex or file> [--guests]] [--directory-url http://127.0.0.1:7700]
//! [--snapshots <dir> | --no-snapshots] [--snapshot-every <seconds>] [--oxen [<binary>]]`.
//!
//! Snapshots (vox-format) go to `<db>.snapshots/` by default: the zones that changed (and their regions), every
//! five minutes and on closing, kept a week. With `--oxen` (or `VOX_OXEN=<binary>`; the
//! `oxen` command line must be installed) that folder is an Oxen repository with a commit per
//! snapshot, and history older than the week is squashed away daily; a folder of plain
//! snapshots from before is taken in as the first commit. With the world closed:
//! `vox-server --db world.db --list-snapshots`, `--restore <id|latest>`, or `--restore-at <unix
//! time>` (the snapshot before it plus the edits up to it). A restore first snapshots the world
//! as it is, so it can be undone.
//!
//! With `--directory-url` and `VOX_WORLD_SECRET` (the directory's too), the world reports who
//! is in it each minute (so players earn authority, and raids are won) and takes the plots and
//! portals players made at the directory. `--name` must be the world's id there.
//!
//! With `--directory-key`, players must sign in at the directory (vox-directory) and bring
//! its token; they go by the name in it. `--guests` lets people in without one too.
//!
//! A new database is filled from the seed level; after that the world is whatever its
//! players built. Ctrl-C (or SIGTERM) saves and stops. Put it behind TLS (a reverse proxy or Cloudflare)
//! for `wss://` on the internet.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let listen = arg("--listen").unwrap_or_else(|| "127.0.0.1:7780".into());
    let data = find_data();
    let blocks_path = arg("--blocks").map(PathBuf::from).unwrap_or_else(|| data.join("blocks.json"));
    let blocks = std::fs::read_to_string(&blocks_path)
        .unwrap_or_else(|e| panic!("can't read {}: {e}", blocks_path.display()));
    let palette = vox_core::palette_names(&blocks).expect("block palette");
    let mut cfg = vox_server::Config::new(arg("--db").unwrap_or_else(|| "world.db".into()), palette);
    cfg.block_size = vox_core::block_size(&blocks).expect("block palette");
    cfg.name = arg("--name").unwrap_or_else(|| "void".into());
    // a new world starts as the void, the first world everyone enters
    let seed = arg("--seed").map(PathBuf::from).unwrap_or_else(|| data.join("levels/void.json"));
    cfg.seed = std::fs::read_to_string(&seed).ok();
    cfg.level = seed.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if let Err(e) = vox_server::flags::apply(&mut cfg, &args) {
        panic!("{e}");
    }
    // one world alone may put its snapshots anywhere
    if let (Some(dir), Some(_)) = (arg("--snapshots"), &cfg.snapshots) {
        cfg.snapshots = Some(PathBuf::from(dir));
    }
    // the world closed: list or restore snapshots, then stop
    if args.iter().any(|a| a == "--list-snapshots") {
        let dir = cfg.snapshots.clone().unwrap_or_else(|| panic!("no snapshots for an in-memory world"));
        for (id, at, holds) in vox_server::list_snapshots(&dir, cfg.snapshot_oxen.as_deref()).unwrap_or_else(|e| panic!("{e}")) {
            println!("{id}  taken {at}  {holds}");
        }
        return;
    }
    let restore = arg("--restore")
        .map(vox_server::Restore::Snapshot)
        .or_else(|| arg("--restore-at").map(|t| vox_server::Restore::At(t.parse().unwrap_or_else(|_| panic!("--restore-at takes a unix time")))));
    if let Some(which) = restore {
        match vox_server::restore(cfg, which) {
            Ok(what) => println!("{what}"),
            Err(e) => {
                eprintln!("can't restore: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let db = cfg.db.clone();
    let server = vox_server::Server::start(cfg).unwrap_or_else(|e| panic!("can't open {}: {e}", db.display()));
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| panic!("can't listen on {listen}: {e}"));
    if let Some(s) = server.status().await {
        eprintln!("vox-server: world \"{}\" ({} blocks) on ws://{listen}/ws, saved in {}", s.world, s.blocks, db.display());
    }
    let stop = server.clone();
    axum::serve(listener, server.router())
        .with_graceful_shutdown(async move {
            stop_asked().await;
            eprintln!("saving…");
            stop.shutdown().await;
        })
        .await
        .expect("server");
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
    tries.into_iter().find(|p| p.join("blocks.json").exists()).unwrap_or_else(|| PathBuf::from("data"))
}
