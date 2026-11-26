//! Run a world: `vox-server [--listen 127.0.0.1:7780] [--db world.db] [--name void]
//! [--seed data/levels/void.json] [--blocks data/blocks.json] [--max-players 100] [--max-watchers 200]
//! [--directory-key <hex or file> [--guests]] [--directory-url http://127.0.0.1:7700]`.
//!
//! With `--directory-url` and `VOX_WORLD_SECRET` (the directory's too), the world reports who
//! is in it each minute (so players earn authority, and raids are won) and takes the plots and
//! portals players made at the directory. `--name` must be the world's id there.
//!
//! With `--directory-key`, players must sign in at the directory (vox-directory) and bring
//! its token; they go by the name in it. `--guests` lets people in without one too.
//!
//! A new database is filled from the seed level; after that the world is whatever its
//! players built. Ctrl-C saves and stops. Put it behind TLS (a reverse proxy or Cloudflare)
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
    cfg.block_size = vox_core::block_size(&blocks);
    cfg.name = arg("--name").unwrap_or_else(|| "void".into());
    // a new world starts as the void, the first world everyone enters
    let seed = arg("--seed").map(PathBuf::from).unwrap_or_else(|| data.join("levels/void.json"));
    cfg.seed = std::fs::read_to_string(&seed).ok();
    cfg.level = seed.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
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
    // sign-in: the directory's public key (hex, or a file holding it)
    if let Some(k) = arg("--directory-key") {
        let hex = std::fs::read_to_string(&k).unwrap_or(k);
        cfg.directory = Some(vox_core::auth::public_from_hex(&hex).unwrap_or_else(|e| panic!("--directory-key: {e}")));
        cfg.guests = args.iter().any(|a| a == "--guests");
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
            let _ = tokio::signal::ctrl_c().await;
            eprintln!("saving…");
            stop.shutdown().await;
        })
        .await
        .expect("server");
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
