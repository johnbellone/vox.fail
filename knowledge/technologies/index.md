# Technologies
* [Technology Stack Overview](stack-overview.md) - One table of every technology in the project, what it is used for, and where its details live.
* [Rust and the Cargo Workspace](rust-workspace.md) - Edition, workspace members, build profiles, features, and target-specific dependencies.
* [Bevy 0.20.1: How and Why It Is Used](bevy.md) - What the project uses Bevy for, the version rule, and the engine quirks that have shaped the code.
* [Server Stack: axum, tokio, tower-http, reqwest, tiny_http](server-stack.md) - Which HTTP and WebSocket libraries each service uses and why the avatar app has no async runtime.
* [SQLite Persistence (rusqlite, bundled)](sqlite-persistence.md) - Where SQLite is used, the table sets, and the operational notes (backups, ignored files).
* [WebAssembly and WebGPU Browser Client](webassembly-webgpu.md) - How the Rust client is compiled for the browser, its size, browser requirements and the verified test setup.
* [Audio and Speech Technology](audio-and-speech.md) - Microphone capture, playback, hand-written voice DSP, network voice coding and optional Whisper speech-to-text.
* [Serialization, Cryptography and Randomness](serialization-and-crypto.md) - Libraries for the binary wire format, JSON, signing, password hashing and random numbers.
* [Media, Imaging and Asset Authoring Tools](media-and-asset-tools.md) - ffmpeg, image codecs, font rendering, MagicaVoxel and glTF as used for shorts, comics, cards and models.
* [Windows Integration: Hotkeys, OBS Virtual Camera, Installer](windows-integration.md) - The Windows-only pieces, how they work, and the fact that none of them has run on Windows.
* [Developer and Agent Tooling: okf, beads, pi, MCP](dev-and-agent-tooling.md) - The agent-facing tools set up around the repository and where their configuration lives.
* [Bevy Ecosystem Crates: Use, Copy, Watch or Skip](bevy-ecosystem.md) - Verdicts on 42 Bevy ecosystem crates for vox.fail as of 2026-11-26, the Bevy 0.20 upgrade question, and what belongs in the world format and a pack.
