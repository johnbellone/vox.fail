//! Run the relay: `vox-relay [--listen 0.0.0.0:7779]`.
//! Put it behind TLS (a reverse proxy or Cloudflare) for `wss://` in production.
//!
//! For testing a bad network: `--simulate-delay 200` (ms, ± half) and `--simulate-loss 0.05`
//! (share of snapshots and voice packets lost).

use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let listen = args
        .iter()
        .position(|a| a == "--listen")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| "127.0.0.1:7779".into());
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let mut relay = vox_relay::Relay::new(Duration::from_secs(30));
    let delay: u64 = arg("--simulate-delay")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let loss: f32 = arg("--simulate-loss")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    if delay > 0 || loss > 0.0 {
        eprintln!(
            "simulating a bad network: {delay} ms delay, {:.0}% loss",
            loss * 100.0
        );
        relay = relay.simulate(vox_relay::Sim {
            delay: Duration::from_millis(delay),
            loss,
        });
    }
    let reaper = relay.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            reaper.reap();
        }
    });
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| panic!("can't listen on {listen}: {e}"));
    eprintln!("vox-relay listening on {listen}");
    axum::serve(listener, relay.router()).await.expect("server");
}
