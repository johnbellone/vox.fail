# Decisions
* [Decision: Stay on Bevy 0.19.1](bevy-0-19-1-pin.md) - The owner requires Bevy to stay at 0.19.1; upgrades need explicit approval.
* [Decision: Keep vox-core Free of Bevy](core-without-bevy.md) - Shared game rules live in an engine-free crate so servers, clients and future pocket worlds run identical code.
* [Decision: 120 Hz Fixed Step with Interpolated Drawing](fixed-120hz-simulation.md) - Particles, crowd and effects step at a fixed 120 Hz and are drawn as a blend of the last two ticks, so motion is frame-rate independent.
* [Decision: Read Twitch Chat Anonymously](anonymous-twitch-irc.md) - Chat is read over IRC as the read-only justinfan user so streamers set only a channel name and authorise nothing.
* [Decision: Host vox.fail Behind a Cloudflare Tunnel](cloudflare-tunnel-hosting.md) - Cloudflared carries traffic to localhost services so the server needs no open ports or certificates; worlds answer on /w/<port>/ws.
* [Decision: Signed Tokens Instead of Server Call-backs](ed25519-signed-launch-tokens.md) - The directory signs a player's identity with Ed25519 so worlds verify it offline with only a public key.
* [Decision: Authority Cannot Be Sent](authority-not-transferable.md) - The shared currency can only be earned and spent on feats; tradeable value lives in per-world currencies.
* [Decision: IMA ADPCM for Network Voice (Opus Later)](adpcm-network-voice.md) - Show voice uses 16 kHz IMA ADPCM because it needs no C library; Opus can replace it behind a codec field.
* [Decision: Block Avatar Is Local-First, Voice-First, One Binary](local-first-single-binary-avatar.md) - The design principles of the streaming app: microphone drives everything, no account or cloud, every feature is a text command, failures are visible but not fatal.
* [Decision (done): Restructure into crates/, bins/ and apps/](restructure-into-crates-bins-apps.md) - The repository is a virtual workspace with libraries in crates/, programs in bins/ and Bun web apps in apps/; done and on main.
