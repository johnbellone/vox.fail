## 2026-11-27
* **Update** [2026-11-27T14:07:57Z]: Updated concept `architecture/ground-relief-design.md`. — Phases 1-4 built: relief, wet ground, puddles, rings, ice; protocol 8
* **Relate** [2026-11-27T14:07:57Z]: Linked `architecture/weather-simulation.md` to `architecture/ground-relief-design.md` (Ground water per column fills the puddles drawn in the generated relief.).
* **Update** [2026-11-27T14:07:57Z]: Updated concept `architecture/weather-simulation.md`. — Ground water per column (puddles), in keyframes from protocol 8; the fog bake and the client's step moved off the main thread
* **Update** [2026-11-27T14:07:43Z]: Updated concept `architecture/wire-protocol.md`. — Protocol 8: weather keyframes carry the ground's water (puddles)
* **Relate** [2026-11-27T13:28:19Z]: Linked `architecture/weather-simulation.md` to `architecture/ground-relief-design.md` (Proposed: ground water per column, filling puddles in the generated relief.).
* **Relate** [2026-11-27T13:28:19Z]: Linked `architecture/ground-relief-design.md` to `architecture/world-scaling-design.md` (Relief is part of the generated terrain and follows its integer-only rules.).
* **Relate** [2026-11-27T13:28:19Z]: Linked `architecture/ground-relief-design.md` to `architecture/weather-simulation.md` (Ground water that fills the puddles is a new field of the weather simulation.).
* **Creation** [2026-11-27T13:28:19Z]: Documented concept `architecture/ground-relief-design.md` (Design (proposed): Ground Relief and Puddles). — Design for micro relief in block tops and rain pooling, asked for 2026-11-27
* **Relate** [2026-11-27T12:47:43Z]: Linked `decisions/zones-and-regions.md` to `architecture/world-format-design.md` (The format the zones and regions are stored in.).
* **Relate** [2026-11-27T12:47:43Z]: Linked `architecture/world-scaling-design.md` to `decisions/zones-and-regions.md` (The unit names and sizes this design uses.).
* **Relate** [2026-11-27T12:47:43Z]: Linked `architecture/world-scaling-design.md` to `architecture/world-format-design.md` (storage format that zones and regions need).
* **Relate** [2026-11-27T12:47:43Z]: Linked `architecture/world-format-design.md` to `decisions/zones-and-regions.md` (Zones are the 256 m storage unit; regions are 32 x 32 zones listed by a snapshot.).
* **Relate** [2026-11-27T12:47:43Z]: Linked `architecture/world-format-design.md` to `technologies/bevy-ecosystem.md` (Why entities use our own blob instead of DynamicWorld, and what belongs in a pack.).
* **Relate** [2026-11-27T12:47:43Z]: Linked `architecture/world-format-design.md` to `decisions/structure-entitlements.md` (Structures have an owner and use, copy and edit rights.).
* **Relate** [2026-11-27T12:47:43Z]: Linked `architecture/world-format-design.md` to `decisions/oxen-snapshot-store.md` (Oxen is the snapshot store when asked for, with history squashed daily.).
* **Update** [2026-11-27T12:47:32Z]: Updated concept `architecture/world-scaling-design.md`. — The 256 m unit is now a zone; regions are 32 x 32 zones (decisions/zones-and-regions)
* **Update** [2026-11-27T12:47:32Z]: Updated concept `architecture/world-format-design.md`. — Zones are the old regions; regions are 32 x 32 zones (manifest format 2); see decisions/zones-and-regions
* **Creation** [2026-11-27T12:47:08Z]: Documented concept `decisions/zones-and-regions.md` (Decision: Zones Are 256 m, Regions Are 32 x 32 Zones). — The world data model: zones are the old regions, regions are 32 x 32 zones (manifest level, old snapshots still read)
* **Update** [2026-11-27T11:53:58Z]: Updated concept `decisions/world-voice-chat.md`. — Tried on real devices: speakers matched (0.974); the mic's DC offset and hiss held the open-mic gate open, fixed with a DC blocker and a noise-floor gate in the packetizer
* **Update** [2026-11-27T11:17:07Z]: Updated concept `decisions/world-voice-chat.md`. — Voice landed on main as PROTOCOL 7 (after travel's 6); radius from the world, immediate mute, non-blocking device start; wire-protocol and related concepts updated
* **Relate** [2026-11-27T04:40:31Z]: Linked `decisions/bevy-0-20-upgrade.md` to `technologies/bevy-ecosystem.md` (The analysis that preceded the upgrade, and the crates still waiting on 0.20 releases.).
* **Relate** [2026-11-27T04:40:31Z]: Linked `decisions/bevy-0-20-upgrade.md` to `decisions/bevy-0-19-1-pin.md` (The rule this supersedes.).
* **Relate** [2026-11-27T04:40:31Z]: Linked `technologies/bevy.md` to `technologies/bevy-ecosystem.md` (Which ecosystem crates to use, copy, watch or skip.).
* **Relate** [2026-11-27T04:40:31Z]: Linked `technologies/bevy.md` to `decisions/bevy-0-20-upgrade.md` (The version rule, and what the upgrade from 0.19.1 took.).
* **Update** [2026-11-27T04:40:31Z]: Updated concept `convention/hard-constraints.md`. — Constraint 5 now Bevy 0.20.1, upgraded on the owner's say-so (2026-11-27)
* **Update** [2026-11-27T04:40:12Z]: Updated concept `technologies/bevy.md`. — Upgraded to Bevy 0.20.1; shaders are WESL
* **Deprecation** [2026-11-27T04:40:12Z]: Deprecated concept `decisions/bevy-0-19-1-pin.md`. — Superseded by decisions/bevy-0-20-upgrade (owner approved the upgrade)
* **Creation** [2026-11-27T04:40:12Z]: Documented concept `decisions/bevy-0-20-upgrade.md` (Decision: Upgrade to Bevy 0.20.1, and Stay There).
* **Relate** [2026-11-27T03:55:18Z]: Linked `architecture/world-format-design.md` to `technologies/bevy-ecosystem.md` (Why entities use our own blob instead of DynamicWorld, and what belongs in a pack.).
* **Relate** [2026-11-27T03:55:18Z]: Linked `decisions/bevy-0-19-1-pin.md` to `technologies/bevy-ecosystem.md` (The analysis behind staying on 0.19.1 for now.).
* **Relate** [2026-11-27T03:55:18Z]: Linked `technologies/bevy.md` to `technologies/bevy-ecosystem.md` (Which ecosystem crates to use, copy, watch or skip, and the 0.20 upgrade question.).
* **Creation** [2026-11-27T03:55:11Z]: Documented concept `technologies/bevy-ecosystem.md` (Bevy Ecosystem Crates: Use, Copy, Watch or Skip); set stale_after 2027-01-26.
* **Update** [2026-11-27T03:55:11Z]: Updated concept `decisions/bevy-0-19-1-pin.md`. — Owner said an upgrade is not ruled out (2026-11-26); analysis recommends staying for now
* **Relate** [2026-11-27T03:35:47Z]: Linked `project/characters-and-content.md` to `project/prop-catalogue.md` (The proposed world props, which seed from the avatar props listed here.).
* **Relate** [2026-11-27T03:35:47Z]: Linked `status/roadmap.md` to `status/props-phases-and-open-questions.md` (The proposed prop pipeline's phases and open questions.).
* **Relate** [2026-11-27T03:35:47Z]: Linked `architecture/prop-definition.md` to `decisions/props-third-kind-of-thing.md` (Why props are neither blocks nor structures.).
* **Relate** [2026-11-27T03:35:41Z]: Linked `architecture/prop-definition.md` to `decisions/props-closed-behaviours.md` (Why behaviours are a closed set.).
* **Relate** [2026-11-27T03:35:41Z]: Linked `architecture/prop-authoring-lanes.md` to `decisions/props-hybrid-authoring.md` (Why there are two lanes.).
* **Relate** [2026-11-27T03:35:41Z]: Linked `architecture/props-in-worlds.md` to `decisions/structure-entitlements.md` (Player-made props use the same owner and grants rule as structures.).
* **Relate** [2026-11-27T03:35:41Z]: Linked `architecture/prop-pipeline-design.md` to `decisions/props-third-kind-of-thing.md` (Props beside blocks and structures.).
* **Relate** [2026-11-27T03:35:41Z]: Linked `architecture/prop-pipeline-design.md` to `decisions/props-closed-behaviours.md` (Behaviours are a closed vocabulary, never code.).
* **Relate** [2026-11-27T03:35:41Z]: Linked `architecture/prop-pipeline-design.md` to `decisions/props-hybrid-authoring.md` (Recipes and imported MagicaVoxel files, the owner's choice.).
* **Relate** [2026-11-27T03:35:34Z]: Linked `architecture/prop-pipeline-design.md` to `status/props-phases-and-open-questions.md` (Build phases and the open questions.).
* **Relate** [2026-11-27T03:35:34Z]: Linked `architecture/prop-pipeline-design.md` to `project/prop-catalogue.md` (Seventy proposed props and the twelve-prop first wave.).
* **Relate** [2026-11-27T03:35:34Z]: Linked `architecture/prop-pipeline-design.md` to `architecture/props-in-worlds.md` (How worlds store, place, draw, collide with and use prop instances.).
* **Relate** [2026-11-27T03:35:34Z]: Linked `architecture/prop-pipeline-design.md` to `architecture/prop-checks-and-review.md` (Budgets, structured checks, contact-sheet review and the style guide.).
* **Relate** [2026-11-27T03:35:34Z]: Linked `architecture/prop-pipeline-design.md` to `architecture/prop-authoring-lanes.md` (Recipes, MagicaVoxel import and the touch-up loop.).
* **Relate** [2026-11-27T03:35:34Z]: Linked `architecture/prop-pipeline-design.md` to `architecture/prop-definition.md` (What a prop holds and the closed set of behaviours.).
* **Creation** [2026-11-27T03:35:24Z]: Documented concept `decisions/props-third-kind-of-thing.md` (Decision: Props Are a Third Kind of Thing, Beside Blocks and Structures).
* **Creation** [2026-11-27T03:35:24Z]: Documented concept `decisions/props-closed-behaviours.md` (Decision: Prop Behaviours Are a Closed Vocabulary, Never Code).
* **Creation** [2026-11-27T03:35:24Z]: Documented concept `decisions/props-hybrid-authoring.md` (Decision: Props Come from Recipes and Imported MagicaVoxel Files).
* **Creation** [2026-11-27T03:35:07Z]: Documented concept `status/props-phases-and-open-questions.md` (Props: Phases and Open Questions); set stale_after 2026-12-26.
* **Creation** [2026-11-27T03:35:07Z]: Documented concept `project/prop-catalogue.md` (Prop Catalogue: Seventy Props and the Twelve-Prop First Wave); set stale_after 2026-12-26.
* **Creation** [2026-11-27T03:34:48Z]: Documented concept `architecture/props-in-worlds.md` (Props in Worlds: Entity, Placement, Drawing, Collision and Use); set stale_after 2026-12-26.
* **Creation** [2026-11-27T03:34:48Z]: Documented concept `architecture/prop-checks-and-review.md` (Prop Checks, Contact-Sheet Review and the Style Guide); set stale_after 2026-12-26.
* **Creation** [2026-11-27T03:34:30Z]: Documented concept `architecture/prop-authoring-lanes.md` (Prop Authoring: Recipes, MagicaVoxel Import and the Touch-up Loop); set stale_after 2026-12-26.
* **Creation** [2026-11-27T03:34:30Z]: Documented concept `architecture/prop-definition.md` (Prop Definition: Fields, Scales and Behaviours); set stale_after 2026-12-26.
* **Relate** [2026-11-27T02:46:21Z]: Linked `architecture/asset-pipeline.md` to `architecture/prop-pipeline-design.md` (Proposed replacement for the Python prop builders: recipes and .vox import in Rust.).
* **Relate** [2026-11-27T02:46:21Z]: Linked `architecture/world-graph-and-art-design.md` to `architecture/prop-pipeline-design.md` (The prop scale made concrete: what a prop is, its pipeline and a first catalogue.).
* **Creation** [2026-11-27T02:46:20Z]: Documented concept `architecture/prop-pipeline-design.md` (Design (proposed): Props and the Prop Pipeline); set stale_after 2026-12-26.

## 2026-11-26
* **Update** [2026-11-26T23:26:41Z]: Updated concept `decisions/world-voice-chat.md`. — Built and verified
* **Update** [2026-11-26T23:26:41Z]: Updated concept `decisions/world-voice-chat.md`. — As built
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/voice-and-lipsync.md`. — lip sync in worlds
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/show-networking.md`. — pointer to world voice
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/vox-client-and-web.md`. — Voice chat in the client
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/vox-core.md`. — Voice: protocol 6 and relay
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/system-overview.md`. — Voice: protocol 6 and relay
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/vox-server.md`. — Voice: protocol 6 and relay
* **Update** [2026-11-26T23:26:41Z]: Updated concept `architecture/wire-protocol.md`. — PROTOCOL 6: voice messages
* **Relate** [2026-11-26T22:12:53Z]: Linked `architecture/economy-and-social.md` to `decisions/structure-entitlements.md` (Who may use, copy and edit a structure; plots' builders are meant to edit their plot's structure.).
* **Relate** [2026-11-26T22:12:53Z]: Linked `architecture/world-format-design.md` to `decisions/structure-entitlements.md` (Structures have an owner and use, copy and edit rights.).
* **Relate** [2026-11-26T22:12:53Z]: Linked `architecture/world-format-design.md` to `decisions/oxen-snapshot-store.md` (Oxen is the snapshot store when asked for, with history squashed daily.).
* **Update** [2026-11-26T22:11:57Z]: Updated concept `architecture/world-format-design.md`. — Oxen store built and squashing decided; structure entitlements built in the directory
* **Creation** [2026-11-26T22:11:34Z]: Documented concept `decisions/structure-entitlements.md` (Decision: Structures Have an Owner and Use, Copy and Edit Rights).
* **Creation** [2026-11-26T22:11:34Z]: Documented concept `decisions/oxen-snapshot-store.md` (Decision: Oxen Is the Snapshot Store, Squashed Daily).
* **Creation** [2026-11-26T22:44:32Z]: Documented concept `decisions/world-voice-chat.md` (Decision: Voice Chat in Worlds (Nearby and Raid Party), Relayed by vox-server). — Design for world voice chat (goal mwy52zqr-4grzwa)
* **Creation** [2026-11-26T21:54:59Z]: Documented concept `decisions/player-help-single-source.md` (Decision: Player Help Is Markdown in docs/help, Rendered by the Site). — Record help-docs single-source design
* **Creation** [2026-11-26T21:54:59Z]: Documented concept `decisions/haiku-writes-sonnet-reviews.md` (Decision: Haiku Writes, Sonnet Reviews, Beads Plan the Work). — Record model-role policy adopted with the AGENTS.md restructure
* **Update** [2026-11-26T21:54:51Z]: Updated concept `convention/work-tracking.md`.
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/repository-hygiene.md` to `status/restructure-and-follow-on-work.md` (where the large worktree came from).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/test-results.md` to `architecture/testing-and-ci.md` (how tests and CI are set up).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/documentation-drift.md` to `status/restructure-and-follow-on-work.md` (docs also changed with the restructure).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/roadmap.md` to `convention/hard-constraints.md` (rules for the backlog Capitol world).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/roadmap.md` to `architecture/world-format-design.md` (next world-format phases).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/roadmap.md` to `architecture/world-scaling-design.md` (planned scaling work).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/risks-and-open-issues.md` to `status/verification-gaps.md` (untested areas).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/controls-and-hud-design.md` (controls and HUD, phases 1-4 built).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/world-graph-and-art-design.md` (generated art, proposed).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/world-scaling-design.md` (60 km worlds, proposed).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/world-format-design.md` (world format, phase 1 built).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/restructure-and-follow-on-work.md` to `decisions/restructure-into-crates-bins-apps.md` (the restructure decision).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/repository-hygiene.md` (housekeeping findings).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/documentation-drift.md` (which docs are stale).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/roadmap.md` (what comes next).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/risks-and-open-issues.md` (ranked risks).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/verification-gaps.md` (what has never been run).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/test-results.md` (measured test results).
* **Relate** [2026-11-26T17:38:54Z]: Linked `status/current-state.md` to `status/restructure-and-follow-on-work.md` (the restructure and follow-on commits).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/testing-and-ci.md` to `status/test-results.md` (the latest measured results).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-client-and-web.md` to `project/vox-online.md` (the player journey).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-client-and-web.md` to `technologies/webassembly-webgpu.md` (how the browser build works).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-directory.md` to `technologies/serialization-and-crypto.md` (argon2, ed25519, sha2).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-directory.md` to `technologies/server-stack.md` (axum and friends).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-directory.md` to `architecture/economy-and-social.md` (authority, feats, friends).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-directory.md` to `architecture/identity-and-tokens.md` (sign-in and tokens).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-server.md` to `architecture/world-format-design.md` (snapshots and restore).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-server.md` to `architecture/vox-directory.md` (reports and plots flow through the directory).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-server.md` to `technologies/sqlite-persistence.md` (its storage).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-server.md` to `architecture/wire-protocol.md` (the protocol it speaks).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-world.md` to `architecture/controls-and-hud-design.md` (its input, HUD and menus).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-world.md` to `technologies/webassembly-webgpu.md` (browser build).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-world.md` to `architecture/weather-simulation.md` (the weather it displays).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-world.md` to `architecture/worlds-catalogue.md` (the levels and blocks it draws).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-core.md` to `decisions/core-without-bevy.md` (why it has no engine).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-core.md` to `architecture/identity-and-tokens.md` (its token code).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-core.md` to `architecture/weather-simulation.md` (its weather model).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/vox-core.md` to `architecture/wire-protocol.md` (its wire messages).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `decisions/restructure-into-crates-bins-apps.md` (why the app is plugins and folders).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `technologies/bevy.md` (the engine).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `decisions/fixed-120hz-simulation.md` (why effects step at 120 Hz).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/audience-interaction.md` (chat features).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/control-surfaces.md` (control page and commands).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/rendering-and-output.md` (cameras and outputs).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/voice-and-lipsync.md` (voice analysis feeding the avatar).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/system-overview.md` to `architecture/world-format-design.md` (how worlds are stored (vox-format)).
* **Relate** [2026-11-26T17:38:54Z]: Linked `architecture/system-overview.md` to `architecture/testing-and-ci.md` (tests and CI).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/asset-pipeline.md` (how assets are made).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/identity-and-tokens.md` (sign-in and tokens).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/wire-protocol.md` (the world protocol).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/deployment.md` (how it is hosted and packaged).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/show-networking.md` (relay, host, guest, viewers).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/vox-client-and-web.md` (player client and site).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/vox-directory.md` (accounts and economy service).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/vox-server.md` (world server).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/vox-world.md` (world rendering crate).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/vox-core.md` (shared rules crate).
* **Relate** [2026-11-26T17:38:53Z]: Linked `architecture/system-overview.md` to `architecture/avatar-app-runtime.md` (how the avatar app runs).
* **Relate** [2026-11-26T17:38:53Z]: Linked `project/repository-layout.md` to `decisions/restructure-into-crates-bins-apps.md` (why the layout is crates/bins/apps).
* **Relate** [2026-11-26T17:38:53Z]: Linked `project/repository-layout.md` to `status/repository-hygiene.md` (untracked and oversized directories).
* **Relate** [2026-11-26T17:38:53Z]: Linked `project/repository-layout.md` to `status/restructure-and-follow-on-work.md` (the restructure that produced this layout).
* **Relate** [2026-11-26T17:38:53Z]: Linked `project/history.md` to `status/restructure-and-follow-on-work.md` (the 29 later commits).
* **Update** [2026-11-26T17:38:31Z]: Updated concept `project/history.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:38:31Z]: Updated concept `status/roadmap.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:38:31Z]: Updated concept `decisions/restructure-into-crates-bins-apps.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:38:31Z]: Updated concept `architecture/world-format-design.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:38:31Z]: Updated concept `architecture/controls-and-hud-design.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:25:19Z]: Updated concept `architecture/avatar-app-runtime.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:25:19Z]: Updated concept `architecture/vox-world.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:25:19Z]: Updated concept `architecture/vox-core.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:25:19Z]: Updated concept `architecture/vox-client-and-web.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:25:19Z]: Updated concept `architecture/vox-directory.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:25:19Z]: Updated concept `architecture/vox-server.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:24:20Z]: Updated concept `architecture/testing-and-ci.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:24:20Z]: Updated concept `status/repository-hygiene.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:24:20Z]: Updated concept `status/documentation-drift.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:24:20Z]: Updated concept `status/roadmap.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:24:20Z]: Updated concept `status/risks-and-open-issues.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:24:20Z]: Updated concept `status/test-results.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:23:36Z]: Updated concept `status/restructure-and-follow-on-work.md` (set stale_after 2026-12-26). — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:23:36Z]: Updated concept `status/current-state.md` (set stale_after 2026-12-26). — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:23:36Z]: Updated concept `architecture/system-overview.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Update** [2026-11-26T17:23:36Z]: Updated concept `project/repository-layout.md`. — Re-based on main 7c4e517 (restructured layout) on 2026-11-26
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/documentation-drift.md` to `status/restructure-and-follow-on-work.md` (docs also differ across the branch).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/roadmap.md` to `convention/hard-constraints.md` (rules for the backlog Capitol world).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/roadmap.md` to `architecture/world-scaling-design.md` (planned scaling work).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/risks-and-open-issues.md` to `status/verification-gaps.md` (untested areas).
* **Relate** [2026-11-26T15:57:59Z]: Linked `architecture/world-scaling-design.md` to `architecture/world-format-design.md` (storage format that regions need).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/controls-and-hud-design.md` (controls and HUD, phases 1-4 built).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/world-graph-and-art-design.md` (generated art, proposed).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/world-scaling-design.md` (60 km worlds, proposed).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/restructure-and-follow-on-work.md` to `architecture/world-format-design.md` (world format, phase 1 built).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/restructure-and-follow-on-work.md` to `decisions/restructure-into-crates-bins-apps.md` (the restructure decision).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/current-state.md` to `status/repository-hygiene.md` (housekeeping findings).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/current-state.md` to `status/documentation-drift.md` (which docs are stale).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/current-state.md` to `status/roadmap.md` (what comes next).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/current-state.md` to `status/risks-and-open-issues.md` (ranked risks).
* **Relate** [2026-11-26T15:57:59Z]: Linked `status/current-state.md` to `status/verification-gaps.md` (what has never been run).
* **Relate** [2026-11-26T15:57:58Z]: Linked `status/current-state.md` to `status/test-results.md` (measured test results).
* **Relate** [2026-11-26T15:57:58Z]: Linked `status/current-state.md` to `status/restructure-and-follow-on-work.md` (the newer unmerged line of work).
* **Relate** [2026-11-26T15:57:58Z]: Linked `convention/work-tracking.md` to `technologies/dev-and-agent-tooling.md` (the tools involved).
* **Relate** [2026-11-26T15:57:58Z]: Linked `convention/hard-constraints.md` to `project/characters-and-content.md` (content the rules apply to).
* **Relate** [2026-11-26T15:57:58Z]: Linked `convention/hard-constraints.md` to `decisions/bevy-0-19-1-pin.md` (constraint 5 in detail).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/rust-workspace.md` to `status/restructure-and-follow-on-work.md` (the branch turns the root into a virtual workspace).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/bevy.md` to `decisions/bevy-0-19-1-pin.md` (the version rule).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/dev-and-agent-tooling.md` (okf, beads, pi, MCP).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/windows-integration.md` (Windows integration).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/media-and-asset-tools.md` (media and asset tools).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/serialization-and-crypto.md` (serialization and crypto).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/audio-and-speech.md` (audio and speech).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/webassembly-webgpu.md` (WebAssembly and WebGPU).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/sqlite-persistence.md` (SQLite).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/server-stack.md` (HTTP and WebSocket libraries).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/bevy.md` (Bevy).
* **Relate** [2026-11-26T15:57:58Z]: Linked `technologies/stack-overview.md` to `technologies/rust-workspace.md` (Rust and Cargo).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/worlds-catalogue.md` to `project/vox-online.md` (how players reach the worlds).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/asset-pipeline.md` to `project/characters-and-content.md` (what the pipeline produces).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/testing-and-ci.md` to `status/test-results.md` (the latest measured results).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/deployment.md` to `technologies/windows-integration.md` (installer and Windows pieces).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/deployment.md` to `decisions/cloudflare-tunnel-hosting.md` (why a tunnel).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/economy-and-social.md` to `decisions/authority-not-transferable.md` (rationale for non-transferable authority).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/identity-and-tokens.md` to `decisions/ed25519-signed-launch-tokens.md` (rationale for signed tokens).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-client-and-web.md` to `project/vox-online.md` (the player journey).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-client-and-web.md` to `technologies/webassembly-webgpu.md` (how the browser build works).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-directory.md` to `technologies/serialization-and-crypto.md` (argon2, ed25519, sha2).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-directory.md` to `technologies/server-stack.md` (axum and friends).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-directory.md` to `architecture/economy-and-social.md` (authority, feats, friends).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-directory.md` to `architecture/identity-and-tokens.md` (sign-in and tokens).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-server.md` to `architecture/vox-directory.md` (reports and plots flow through the directory).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-server.md` to `technologies/sqlite-persistence.md` (its storage).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-server.md` to `architecture/wire-protocol.md` (the protocol it speaks).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-world.md` to `technologies/webassembly-webgpu.md` (browser build).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-world.md` to `architecture/weather-simulation.md` (the weather it displays).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-world.md` to `architecture/worlds-catalogue.md` (the levels and blocks it draws).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-core.md` to `decisions/core-without-bevy.md` (why it has no engine).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-core.md` to `architecture/identity-and-tokens.md` (its token code).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-core.md` to `architecture/weather-simulation.md` (its weather model).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/vox-core.md` to `architecture/wire-protocol.md` (its wire messages).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/rendering-and-output.md` to `technologies/windows-integration.md` (the Windows-only webcam driver).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/rendering-and-output.md` to `technologies/media-and-asset-tools.md` (ffmpeg and image libraries).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/control-surfaces.md` to `decisions/local-first-single-binary-avatar.md` (design principles behind one command path).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/control-surfaces.md` to `project/block-avatar-features.md` (features reachable by commands).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/voice-and-lipsync.md` to `technologies/audio-and-speech.md` (audio libraries and Whisper).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/show-networking.md` to `architecture/wire-protocol.md` (the separate world protocol).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/show-networking.md` to `decisions/adpcm-network-voice.md` (why voice uses ADPCM).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/viewer-questions.md` to `status/risks-and-open-issues.md` (known weaknesses of the ask page).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/audience-interaction.md` to `decisions/anonymous-twitch-irc.md` (why chat is read anonymously).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/audience-interaction.md` to `architecture/viewer-questions.md` (the public question page).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/avatar-app-runtime.md` to `technologies/bevy.md` (the engine).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/avatar-app-runtime.md` to `decisions/fixed-120hz-simulation.md` (why effects step at 120 Hz).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/audience-interaction.md` (chat features).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/control-surfaces.md` (control page and commands).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/rendering-and-output.md` (cameras and outputs).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/avatar-app-runtime.md` to `architecture/voice-and-lipsync.md` (voice analysis feeding the avatar).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/testing-and-ci.md` (tests and CI).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/asset-pipeline.md` (how assets are made).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/identity-and-tokens.md` (sign-in and tokens).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/wire-protocol.md` (the world protocol).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/deployment.md` (how it is hosted and packaged).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/show-networking.md` (relay, host, guest, viewers).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/vox-client-and-web.md` (player client and web pages).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/vox-directory.md` (accounts and economy service).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/vox-server.md` (world server).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/vox-world.md` (world rendering crate).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/vox-core.md` (shared rules crate).
* **Relate** [2026-11-26T15:57:58Z]: Linked `architecture/system-overview.md` to `architecture/avatar-app-runtime.md` (how the avatar app runs).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/repository-layout.md` to `status/repository-hygiene.md` (untracked and oversized directories).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/repository-layout.md` to `status/restructure-and-follow-on-work.md` (the restructured layout on the branch).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/history.md` to `status/restructure-and-follow-on-work.md` (29 newer commits live on an unmerged branch).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `convention/work-tracking.md` (how work is tracked).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `project/characters-and-content.md` (the characters, props, sets and licences).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `project/vox-online.md` (what the online world does).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `project/block-avatar-features.md` (what the streaming app does).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `project/history.md` (how the project got here).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `project/repository-layout.md` (where things live in the repository).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `convention/hard-constraints.md` (rules nobody may break).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `technologies/stack-overview.md` (technologies used).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `status/current-state.md` (where the project stands today).
* **Relate** [2026-11-26T15:57:58Z]: Linked `project/overview.md` to `architecture/system-overview.md` (architecture of both products and how the parts connect).
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `decisions/restructure-into-crates-bins-apps.md` (Decision (done on branch): Restructure into crates/, bins/ and apps/); set stale_after 2027-01-25. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `architecture/controls-and-hud-design.md` (Design (phases 1-4 built on branch): Block-Game Controls, HUD and Menus); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `architecture/world-graph-and-art-design.md` (Design (proposed): World Graph and Generated Art); set stale_after 2027-01-25. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `architecture/world-format-design.md` (Design (phase 1 built on branch): The World Format); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `architecture/world-scaling-design.md` (Design (proposed): Worlds at 60 km x 60 km); set stale_after 2027-01-25. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/repository-hygiene.md` (Repository and Disk Hygiene Findings); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/documentation-drift.md` (Documentation Drift: Where the Docs Disagree with the Code); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/roadmap.md` (Roadmap and Next Steps); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/risks-and-open-issues.md` (Risks and Open Issues (re-checked 2026-11-26)); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/verification-gaps.md` (Verification Gaps: What Has Never Been Run); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/test-results.md` (Test Results on main (2026-11-26 run)); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/restructure-and-follow-on-work.md` (Active Development Branch: claude/project-build-daed3d); set stale_after 2026-12-10. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:57:12Z]: Documented concept `status/current-state.md` (Current State of main (as of 2026-11-26)); set stale_after 2026-12-26. — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/local-first-single-binary-avatar.md` (Decision: Block Avatar Is Local-First, Voice-First, One Binary). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/adpcm-network-voice.md` (Decision: IMA ADPCM for Network Voice (Opus Later)). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/authority-not-transferable.md` (Decision: Authority Cannot Be Sent). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/ed25519-signed-launch-tokens.md` (Decision: Signed Tokens Instead of Server Call-backs). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/cloudflare-tunnel-hosting.md` (Decision: Host vox.fail Behind a Cloudflare Tunnel). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/anonymous-twitch-irc.md` (Decision: Read Twitch Chat Anonymously). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/fixed-120hz-simulation.md` (Decision: 120 Hz Fixed Step with Interpolated Drawing). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/core-without-bevy.md` (Decision: Keep vox-core Free of Bevy). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `decisions/bevy-0-19-1-pin.md` (Decision: Stay on Bevy 0.19.1). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/dev-and-agent-tooling.md` (Developer and Agent Tooling: okf, beads, pi, MCP). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/windows-integration.md` (Windows Integration: Hotkeys, OBS Virtual Camera, Installer). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/media-and-asset-tools.md` (Media, Imaging and Asset Authoring Tools). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/serialization-and-crypto.md` (Serialization, Cryptography and Randomness). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/audio-and-speech.md` (Audio and Speech Technology). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/webassembly-webgpu.md` (WebAssembly and WebGPU Browser Client). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/sqlite-persistence.md` (SQLite Persistence (rusqlite, bundled)). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/server-stack.md` (Server Stack: axum, tokio, tower-http, reqwest, tiny_http). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/bevy.md` (Bevy 0.19.1: How and Why It Is Used). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/rust-workspace.md` (Rust and the Cargo Workspace). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:54:16Z]: Documented concept `technologies/stack-overview.md` (Technology Stack Overview). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/testing-and-ci.md` (Testing and Continuous Integration). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/deployment.md` (Deployment: vox.fail on Cloudflare Tunnel, and Windows Packaging). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/asset-pipeline.md` (Asset Pipeline: MagicaVoxel to glTF). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/worlds-catalogue.md` (Worlds, Levels and Blocks). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/weather-simulation.md` (Weather Simulation). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/economy-and-social.md` (Authority, World Currencies, Feats, Plots, Portals and Raids). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/identity-and-tokens.md` (Identity, Sessions and Signed Launch Tokens). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/wire-protocol.md` (World Wire Protocol (PROTOCOL = 5)). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/vox-client-and-web.md` (vox-client and the Web Front End). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/vox-directory.md` (vox-directory: Accounts, Characters, Economy and Web Pages). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/vox-server.md` (vox-server: One Persistent World). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/vox-world.md` (vox-world: Drawing a World (Blocks, Sky, Portals, Players)). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:52:35Z]: Documented concept `architecture/vox-core.md` (vox-core: Shared Rules, Wire Format and Tokens). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/show-networking.md` (Shows over the Network: Relay, Host, Guest and Viewers). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/viewer-questions.md` (Viewer Questions: Ask Page, Queue and Moderation). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/audience-interaction.md` (Audience Interaction: Chat, Dress-up, Polls, Crowd and Moments). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/control-surfaces.md` (Avatar Control Surfaces: Control API, Commands, Settings and Files). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/rendering-and-output.md` (Rendering, Cameras and Outputs (Avatar App)). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/voice-and-lipsync.md` (Voice Analysis and Lip Sync). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/avatar-app-runtime.md` (Block Avatar Runtime: Bevy Schedule, Threads and State). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:51:01Z]: Documented concept `architecture/system-overview.md` (System Overview and Crate Dependencies). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `convention/work-tracking.md` (Work Tracking: GitHub Issues, beads and okf). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `convention/hard-constraints.md` (Hard Constraints Set by the Owner). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `project/vox-online.md` (Vox Online: Player Journey and Worlds). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `project/characters-and-content.md` (Characters, Props, Sets and Content Licences). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `project/block-avatar-features.md` (Block Avatar: Feature Inventory). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `project/repository-layout.md` (Repository Layout). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `project/history.md` (Project History and Timeline). — Initial analysis of the repository (2026-11-26)
* **Creation** [2026-11-26T15:49:37Z]: Documented concept `project/overview.md` (vox.fail: Project Overview). — Initial analysis of the repository (2026-11-26)
* **Creation**: Initialized OKF v0.3 project memory.
