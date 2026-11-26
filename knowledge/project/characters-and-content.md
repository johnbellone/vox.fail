---
type: Fact
title: "Characters, Props, Sets and Content Licences"
description: "The 38 original rigged characters, the props, sets and sounds, how they share one rig, and where every third-party asset comes from."
tags: [assets, characters, content, licences]
generated: { by: agent/mcp, at: "2026-11-26T15:49:37Z" }
status: stable
---

# Characters, Props, Sets and Content

All art is original voxel art made in this project, with MagicaVoxel sources in `source/vox/`. Third-party content is limited to a font, real audio recordings and test speech, all under open licences.

## Characters (data/characters.json: 38)

| Group | Count | Notes |
| --- | --- | --- |
| Main | 1 | **Blue**: an American pit bull terrier (blue nose, blue-grey coat, white blaze, rose ears, orange collar). First in the list, the default for new players and first runs. |
| Cast | 26 | White House staff, Secret Service agents, senators, representatives, police, riot officers, judge, reporter, rally-goers, executive, and the original "lead" (red cap, overalls, beard). All fictional. |
| Animals | 5 | dog, cat, rhino, donkey, peanut |
| Roast | 6 | Six "roasted peanut" outfits: army, medic, Miami detective, cook, garbage, riot |

Every character shares **one rig** (`Character > Rig > Hips > Head, UpperArm/LowerArm L/R, UpperLeg/LowerLeg L/R`), one face kit (8 eye shapes, 4 brow shapes, 11 mouth shapes, 3 hand shapes) and the lead's 100 body clips, so any animation, face, hat or prop works on any character. About 1.85 m tall, 5 cm blocks. Cast members add their own clips from index 100.

## Other content
- **Props and effects**: 143 files in `assets/props/` (lead, cast, roasted, wardrobe). Wardrobe: 15 hats, 4 face pieces, 12 held items, fitted per head by `data/head_fit.json`. Ten characters have a second "bare head" so hats sit on hair, not on the cap.
- **Sets**: six voxel stages, 14 m wide, 6 m tall, 10 cm voxels.
- **Block textures**: 63 PNGs (colour, height, glow) for 19+ block types, drawn procedurally by `tools/assets/build_blocks.py`.
- **Portraits**: 38 head-and-shoulders pictures, used on the viewer page and title cards.
- **Sound effects**: 6 real recordings from ESC-50 (CC0), trimmed and levelled.
- **Font**: Press Start 2P (SIL Open Font License).

## Test and demo audio (all real, unmodified human recordings)
Free Spoken Digit Dataset (CC BY-SA 4.0), LJ Speech (public domain), ESC-50 laughs (CC0/CC BY), vocadito singing (CC BY 4.0), LibriSpeech demos (CC BY 4.0), CMU ARCTIC questions, and a Praat pitch reference CSV.

## Rules
See `convention/hard-constraints`: no real-person likenesses, no trademarked characters, no synthetic voices, green medical cross, no police-assault poses.

## Open product question (from docs/PROJECT.md)
Whether the lead's political-slogan cap suits a public store page and screenshots. It was kept off the app icon.
