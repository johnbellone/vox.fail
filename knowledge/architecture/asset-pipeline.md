---
type: Process
title: "Asset Pipeline: MagicaVoxel to glTF"
description: "How characters, props, sets, levels and textures are authored and generated, and which script rebuilds what."
tags: [process, assets, python, tools, glb]
generated: { by: agent/mcp, at: "2026-11-27T20:45:40Z" }
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
| `visual_test.py`, `web_e2e.py`, `editor_e2e.py`, `build_web.sh` | Tests and the browser build |

## World props (Rust, not Python)
The props that worlds accept and the editor places are built by `bins/vox-props` (`cargo run -p vox-props -- --source source/props --out data/props`) from `source/props/`: a `palette.json` (a `vox_art::Palette`) and one `<name>.json` per prop (`{ "def": {...}, "recipe": {...} }`; the recipe is evaluated into voxels through `vox-art`, the def holds id, name, tags, pivot, placement and collision). A prop that fails any check is refused and nothing is written. Output in `data/props/` is deterministic (same sources, same bytes): `catalogue.json` (name, tags, hash), one `<hash>.voxp` blob per prop (`vox_format::prop::PropBlob`, hash in lowercase hex), a `<hash>.png` thumbnail for the editor's prop list, and `palette.json` keyed by palette version so the client can colour each prop. `vox-server --props` and the client read this folder; do not edit it by hand. First props: barrel, crate, stool.

## Rig rule
`Character > Rig > Hips > Head, UpperArm_L > LowerArm_L, ...`; each node's origin is its joint; 5 cm blocks; facing +Z. Swappable parts are children toggled by scale 0/1. A new character that follows this rig gets all 100+ shared clips, faces and props for free.

## Manifests
`data/lead_manifest.json` and `cast_manifest.json` give clip timing, props used and dialogue lines; `characters.json` is the catalogue shared by the app, the directory and the web pages.

## Rules
All assets must satisfy `convention/hard-constraints`. Sound effects stay real CC0 recordings. Package sizes: assets are 110 MB but compress to a ~2.5 MB installer.

# Related Concepts
- [Characters, Props, Sets and Content Licences](../project/characters-and-content.md): what the pipeline produces
- [Design (proposed): Props and the Prop Pipeline](prop-pipeline-design.md): the design behind the Rust prop builder
- [Decision: The World Editor](../decisions/world-editor.md): the editor that places these props
