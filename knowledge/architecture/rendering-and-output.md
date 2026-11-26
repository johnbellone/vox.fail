---
type: Architecture
title: "Rendering, Cameras and Outputs (Avatar App)"
description: "How the avatar app renders to the window, the virtual webcam and the portrait shorts camera, plus lighting, bevels and retro mode."
tags: [architecture, rendering, webcam, shorts, lighting]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [bins/block-avatar/src/scene/stage.rs, bins/block-avatar/src/scene/lighting.rs, bins/block-avatar/src/character/bevel/, bins/block-avatar/src/output/webcam.rs, bins/block-avatar/src/output/clips.rs, bins/block-avatar/src/output/cards.rs, bins/block-avatar/src/scene/camera.rs]
---

# Rendering, Cameras and Outputs

## Three cameras
| Camera | Output | Notes |
| --- | --- | --- |
| Main window | OBS window capture, optional transparent window | 3D camera plus a 2D overlay camera drawing the same final picture. The overlay carries speech bubbles and chat labels so they show in the webcam too. |
| Webcam | 1280x720 at 60 fps (configurable) | Off-screen render, GPU read-back, converted to NV12 on a worker thread, written to OBS Virtual Camera's shared-memory queue (`OBSVirtualCamVideo`) byte-for-byte like OBS's writer. On 144 Hz displays it picks every 2nd/3rd frame to average 60. The last frame repeats if rendering pauses. |
| Shorts (portrait 9:16) | 1080x1920 H.264+AAC MP4, 4-panel comic PNG | Renders the show with the same bubbles and credits; keeps the last **45 s** in memory as JPEG frames plus the program audio mix. Export runs on a background thread through ffmpeg. |

Windows only: webcam driver, hotkeys. Linux: window and shorts.

## Stages and retro mode (`stage.rs`)
Each output has "view roots". Retro mode (`Y`) renders the 3D camera to a small image (final size / pixel size, default 4, range 2-12), snaps every pixel to the character's own palette (3 shades per colour) and adds a 1-pixel dark outline; bubbles stay crisp on top.

## Lighting (`lighting.rs`)
Three-point studio rig: warm key light (soft shadows), cool fill, rim from behind. Baked corner shading (ambient occlusion) in the blocks. Plastic sheen on characters, matte sets. Each set has its own preset (warm diner, blue night, bright rally, flat newsroom, green hospital). Bloom only on bright emissive props (an HDR main camera with a non-HDR overlay froze the window once). Event lights: raid spotlight sweep, party colours on sub, red flash on a big hit, camera flashes at press conferences, follow spot when singing. Depth-of-field blur on sets with the f-number solved per camera. Plain backgrounds stay one flat colour so green-screen keying is unaffected.

**Quality** `low|medium|high|auto`: High has everything; Medium drops glow and uses smaller shadow maps; Low also drops shadows and bevels. Auto starts High and steps down when frame rate stays below 45.

## Block bevels (`bevel.rs`, 1,962 lines)
No high-poly model exists, so detail is hand-painted: a normal-map atlas with one tile per voxel face, rounded bevels on real outside edges, faint seams, per-block wobble, and 64x64 per-face surface detail (stubble, hair, cloth) from one height field that drives colour and bump. `data/surfaces.json` maps palette colours to surfaces; `tools/assets/guess_surfaces.py` suggests first guesses. "Blend blocks" removes seams between same-surface neighbours. Merged glTF quads are cut back into voxel faces at load. A NaN pixel bug on the night set was fixed by orthogonalising tangents and welding block corners; `bevel::real_models` tests every model.

## Motion and feel
`jiggle.rs`: damped springs for hats, face pieces and animal tails, stepped in 1/120 s substeps in PostUpdate (they follow animated pose, so cannot live in FixedUpdate). `fx.rs`: head squash on hit, camera shake, dust, hop crouch/stretch/squash, six sound effects. `particles.rs`: voxel weather per set, bursts for subs/raids/hits, hearts, Zs, sweat.

## Known rendering limits
Reflections are sky-only (no screen-space reflections: the show's materials do not use deferred rendering). Only the nearest 12 lamps/fires cast light. Parallax can shimmer at grazing angles. Background blur is off at Low quality and never over a green screen.

See `architecture/avatar-app-runtime` for system order and `technologies/bevy`.

# Related Concepts
- [Media, Imaging and Asset Authoring Tools](../technologies/media-and-asset-tools.md): ffmpeg and image libraries
- [Windows Integration: Hotkeys, OBS Virtual Camera, Installer](../technologies/windows-integration.md): the Windows-only webcam driver
