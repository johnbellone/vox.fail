# Architecture
* [System Overview and Crate Dependencies](system-overview.md) - The processes, ports and Rust crates of vox.fail and how they depend on each other.
* [Block Avatar Runtime: Bevy Schedule, Threads and State](avatar-app-runtime.md) - How the streaming app is organised: the ordered Bevy system chains, worker threads, shared state, and the 120 Hz fixed simulation.
* [Voice Analysis and Lip Sync](voice-and-lipsync.md) - How the microphone signal becomes mouth shapes, vowels, pitch-based body language and laugh/sing events, with measured accuracy.
* [Rendering, Cameras and Outputs (Avatar App)](rendering-and-output.md) - How the avatar app renders to the window, the virtual webcam and the portrait shorts camera, plus lighting, bevels and retro mode.
* [Avatar Control Surfaces: Control API, Commands, Settings and Files](control-surfaces.md) - The local control page and HTTP API, the single text command language shared by keys, hotkeys and bots, the settings file and the user folder.
* [Audience Interaction: Chat, Dress-up, Polls, Crowd and Moments](audience-interaction.md) - How chat from Twitch, YouTube and Kick reaches the avatar and what viewers can do, with the limits that keep spam watchable.
* [Viewer Questions: Ask Page, Queue and Moderation](viewer-questions.md) - How viewers record a question on the public page, how the streamer reviews and airs it in the viewer's own voice, and the security controls on that page.
* [Shows over the Network: Relay, Host, Guest and Viewers](show-networking.md) - How the avatar app streams a live show through vox-relay so viewers and a remote guest see and hear the same scene.
* [vox-core: Shared Rules, Wire Format and Tokens](vox-core.md) - The engine-free crate that holds block chunks, edit checks, ray casts, body movement, levels, weather, the world wire format and token signing.
* [vox-world: Drawing a World (Blocks, Sky, Portals, Players)](vox-world.md) - The Bevy crate that shows a Vox world on screen and talks to a world server; shared by the streaming app and the player client, native and WebAssembly.
* [vox-server: One Persistent World](vox-server.md) - A single-world game server: WebSocket in, one thread owns the blocks and the edit order, SQLite for persistence, optional directory reporting.
* [vox-directory: Accounts, Characters, Economy and Web Pages](vox-directory.md) - The account service and website: sign-in, characters, signed tokens, the world list, the economy and social API, and the static pages.
* [vox-client and the Web Front End](vox-client-and-web.md) - The player client (desktop and WebAssembly) and the plain HTML/JS pages that the directory serves around it.
* [World Wire Protocol (PROTOCOL = 5)](wire-protocol.md) - The message set between a world server and its players, how it is encoded, and what each protocol version added.
* [Identity, Sessions and Signed Launch Tokens](identity-and-tokens.md) - How people sign in, how the directory proves who they are to world servers without a call-back, and what secrets exist.
* [Authority, World Currencies, Feats, Plots, Portals and Raids](economy-and-social.md) - The player economy and social graph in the directory: the shared Authority currency, what it buys, world coins, the ledger, friends, groups and raid parties.
* [Weather Simulation](weather-simulation.md) - A small server-side atmosphere model whose keyframes drive haze, fog, clouds, rain and wind in every client.
* [Worlds, Levels and Blocks](worlds-catalogue.md) - The level file format, the block palette, the three shipped worlds and how portals connect them.
* [Asset Pipeline: MagicaVoxel to glTF](asset-pipeline.md) - How characters, props, sets, levels and textures are authored and generated, and which script rebuilds what.
* [Deployment: vox.fail on Cloudflare Tunnel, and Windows Packaging](deployment.md) - How vox.fail is hosted (one Linux box, systemd, Cloudflare Tunnel with no open ports) and how Block Avatar is packaged for Windows.
* [Testing and Continuous Integration](testing-and-ci.md) - What the tests cover, how to run them (unit, integration, visual, browser end-to-end) and what CI does.
* [Design (proposed): Worlds at 60 km x 60 km](world-scaling-design.md) - The unbuilt plan to scale worlds to about 55,000 times today's size with regions, generated terrain, streaming, simulation owners and caches.
* [Design (phase 1 built): The World Format](world-format-design.md) - Regions stored as content-addressed zstd blobs with snapshots in a folder or an Oxen repository (squashed daily) and structure entitlements; phase 1 and the Oxen store are built.
* [Design (proposed): World Graph and Generated Art](world-graph-and-art-design.md) - A plan where a language model edits a graph of nodes and typed recipes that a Rust interpreter turns into voxels in the project's style.
* [Design (phases 1-4 built): Block-Game Controls, HUD and Menus](controls-and-hud-design.md) - Action-based input with gamepad hints per brand, first-person view, pixel HUD and controller-navigable menus for Vox worlds.
* [Design (proposed): Props and the Prop Pipeline](prop-pipeline-design.md) - A prop is a small authored object with behaviours, made as a recipe or an imported MagicaVoxel file, checked, reviewed, and placed in worlds as a versioned entity; with a seventy-prop catalogue.
