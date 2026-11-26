---
type: Process
title: "Asset Pipeline: MagicaVoxel to glTF"
description: "How characters, props, sets, levels and textures are authored and generated, and which script rebuilds what."
tags: [process, assets, python, tools, glb]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [tools/, source/vox/, data/]
---

# Asset Pipeline

Sources: MagicaVoxel `.vox` files in `source/vox/` (characters, props, sets, early versions). Outputs: `.glb` (glTF 2.0) in `assets/`. All GLBs pass the Khronos glTF validator with no errors or warnings. Python 3 scripts in `tools/` (about 4.3k lines; PIL, plus Playwright for tests).

| Script | Produces |
| --- | --- |
| `build_animals.py` | Blue, dog, cat, rhino, donkey, peanut on the shared rig (e.g. `python3 tools/build_animals.py`) |
| `build_roasted.py` | The six roasted-peanut outfits and their props |
| `repack.py`, `make_stepped.py` | Re-pack rigs; make stepped-timing twins (`make_stepped.py in.glb out.glb data/lead_manifest.json`) |
| `build_wardrobe.py` | Hats and face pieces; measures each head into `data/head_fit.json` |
| `build_bare_heads.py` | Second, capless head for ten characters (`data/bare_heads.json`) |
| `build_sets.py` | The six stage sets (greedy box merge) |
| `build_blocks.py` | Block tiles from noise (colour, height, glow) |
| `build_void.py`, `build_islands.py` | Level JSON for the Void and the islands |
| `make_portraits.py` | 38 front orthographic portraits |
| `guess_surfaces.py` | First-guess surface map for stubble/hair/cloth (`data/surfaces.json`) |
| `build_catalogue.py` | Props catalogue image |
| `eval_voice.py` | Voice-analysis accuracy against Praat |
| `visual_test.py`, `web_e2e.py`, `build_web.sh` | Tests and the browser build |

## Rig rule
`Character > Rig > Hips > Head, UpperArm_L > LowerArm_L, ...`; each node's origin is its joint; 5 cm blocks; facing +Z. Swappable parts are children toggled by scale 0/1. A new character that follows this rig gets all 100+ shared clips, faces and props for free.

## Manifests
`data/lead_manifest.json` and `cast_manifest.json` give clip timing, props used and dialogue lines; `characters.json` is the catalogue shared by the app, the directory and the web pages.

## Rules
All assets must satisfy `convention/hard-constraints`. Sound effects stay real CC0 recordings. Package sizes: assets are 110 MB but compress to a ~2.5 MB installer.

# Related Concepts
- [Characters, Props, Sets and Content Licences](../project/characters-and-content.md): what the pipeline produces
