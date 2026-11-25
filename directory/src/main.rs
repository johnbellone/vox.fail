//! Run the directory: `warren-directory [--listen 127.0.0.1:7700] [--db directory.db]
//! [--key directory.key] [--web web] [--assets assets] [--data data]
//! [--world "The Void=:7780"]... [--download "Windows=https://…"]... [--secure]`.
//!
//! The signing key is made the first time and kept in `--key`; its public half is written
//! next to it (`directory.key.pub`) and served at `/api/key`. Start world servers with
//! `--directory-key directory.key.pub` so they let in the people who sign in here.
//! A world's address `:7780` means the same host as the page, that port.

use std::path::PathBuf;

use warren_directory::{Config, Directory, WorldInfo, load_or_make_key};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let all = |k: &str| -> Vec<String> { args.windows(2).filter(|w| w[0] == k).map(|w| w[1].clone()).collect() };
    let listen = arg("--listen").unwrap_or_else(|| "127.0.0.1:7700".into());
    let root = find_root();
    let key_path = PathBuf::from(arg("--key").unwrap_or_else(|| "directory.key".into()));
    let key = load_or_make_key(&key_path).unwrap_or_else(|e| panic!("{e}"));
    let pub_path = PathBuf::from(format!("{}.pub", key_path.display()));
    let public = warren_core::auth::public_hex(&key.verifying_key());
    let _ = std::fs::write(&pub_path, format!("{public}\n"));
    let pair = |s: &str| s.split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string()));
    let mut worlds: Vec<WorldInfo> =
        all("--world").iter().filter_map(|w| pair(w)).map(|(name, url)| WorldInfo { name, url, about: String::new() }).collect();
    if worlds.is_empty() {
        worlds.push(WorldInfo {
            name: "The Void".into(),
            url: ":7780".into(),
            about: "Where everyone arrives: a bright, endless floor in white fog.".into(),
        });
    }
    let cfg = Config {
        db: arg("--db").unwrap_or_else(|| "directory.db".into()).into(),
        key,
        web: arg("--web").map(PathBuf::from).unwrap_or_else(|| root.join("web")),
        assets: arg("--assets").map(PathBuf::from).unwrap_or_else(|| root.join("assets")),
        data: arg("--data").map(PathBuf::from).unwrap_or_else(|| root.join("data")),
        worlds,
        secure_cookies: args.iter().any(|a| a == "--secure"),
        downloads: all("--download").iter().filter_map(|d| pair(d)).collect(),
    };
    let dir = Directory::open(cfg).unwrap_or_else(|e| panic!("{e}"));
    let listener = tokio::net::TcpListener::bind(&listen).await.unwrap_or_else(|e| panic!("can't listen on {listen}: {e}"));
    eprintln!("warren-directory on http://{listen}/ (public key {public}, in {})", pub_path.display());
    axum::serve(listener, dir.router().into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("serve");
}

/// The project folder (holding web/, assets/ and data/): here, or above the executable.
fn find_root() -> PathBuf {
    let mut tries = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            tries.push(dir.to_path_buf());
        }
    }
    tries.into_iter().find(|p| p.join("data/characters.json").exists()).unwrap_or_else(|| PathBuf::from("."))
}
