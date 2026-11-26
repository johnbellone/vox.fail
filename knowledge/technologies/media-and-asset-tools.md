---
type: Technology
title: "Media, Imaging and Asset Authoring Tools"
description: "ffmpeg, image codecs, font rendering, MagicaVoxel and glTF as used for shorts, comics, cards and models."
tags: [technology, ffmpeg, media, gltf, magicavoxel]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Media and Asset Tools

- **ffmpeg** (external, optional): joins frames and sound into 1080x1920 H.264 + AAC MP4 and highlight reels (concat, no re-encode). Without it, the app writes frames and a `MAKE_VIDEO.txt` with the one command. Install with `winget install Gyan.FFmpeg`.
- **Frames**: kept as JPEGs in memory (`jpeg-encoder` 0.6, `jpeg-decoder` 0.3). Title/end cards use quality **88**, because at 90+ the encoder switches to 4:4:4 colour and ffmpeg's concat demuxer then drops the first image.
- **PNG**: `png` 0.18 for comic strips and texture reading. **Text**: `ab_glyph` 0.2 with Press Start 2P, drawn by the app (`cards.rs`), no ffmpeg filters.
- **MagicaVoxel** `.vox` sources to **glTF 2.0** `.glb` through Python; Khronos validator clean.
- **Textures**: 128x128 PNG tiles (colour, `.height.png`, `.glow.png`); 64x64 per face surface detail generated at load.
- **Formats of choice**: GLB for models; WAV for audio; JSON for data and levels; plain text for settings.

See `architecture/rendering-and-output`, `architecture/asset-pipeline`.
