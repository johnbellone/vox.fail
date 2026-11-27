---
type: Fact
title: "Prop Catalogue: Seventy Props and the Twelve-Prop First Wave"
description: "The proposed props by world and purpose, with sizes and behaviours, and the twelve first-wave props chosen to exercise every part of the pipeline."
tags: [props, catalogue, content, first-wave, worlds]
generated: { by: agent/mcp, at: "2026-11-27T03:35:07Z" }
status: draft
stale_after: 2026-12-26
---

# Prop Catalogue: Seventy Props and the Twelve-Prop First Wave

Proposed, not built; source `docs/design/props.md`; design in `architecture/prop-pipeline-design`. Size: S under 30 cm (fine), M to 1.5 m (fine), L to 3 m (10 cm), XL to 6 m (10 cm). Behaviours are those of `architecture/prop-definition`. Star marks the first wave.

## Light (8)
Street lamp (L, `light` at night) star; Lantern, hanging (S, light, swing) star; Candle (S, flicker); Wall torch (M, flicker); Brazier (M, flicker, lit/out switch) star; String lights (M, light x4); Floor lamp (M); Moon lamp (M, cold light, bob).

## Home and village (14)
Bench (M, seat x2) star; Chair (M, seat); Table (M, `top` socket); Bed (M, lie, seeded from `bed.vox`); Door, wood (M, door swing) star; Window shutters (M, door x2); Mailbox (M, readable, door); Fence section (M, boxes, gate variant is a door); Well (L, bucket bob); Signpost (M, readable) star; Flower pot (S, sway); Rug (S, no collision, a thin-prop test); Bookshelf (M, later container); Wardrobe (L, door).

## Nature (9)
Pine (L, sway, snow variant) star; Oak (XL, sway); Bush (M, sway, berries); Boulder (M); Reeds (M, sway); Mushrooms (S, glowing variant for the islands); Stump (S, seat); Driftwood (M); Kelp (L, sways with the swell).

## Dark sea (9)
Pier lantern (M, light, swing, salted); Barrel (M, `top` socket, the `lathe` tutorial) star; Crate (M, stackable, lid state) star; Fishing net (M, sway); Anchor (M); Ship's wheel (M, spin on use); Rowboat (L, seat x2, on water, imported from `boat.vox`) star; Buoy (S, bob, blinking light); Lighthouse lamp (L, rotating light).

## Station / orbit (10)
Console (M, readable status, indicator lights) star; Airlock hatch (L, sliding door, switch); Handrail (M); Oxygen tank (M, colour bands); Cargo container (L, door, stackable); Viewport shutter (L, slide); Small solar panel (L, slow turn with the clock); Antenna (L, spin, blink); Cryopod (L, door, light, frost); Tether reel (S).

## Islands, dark fantasy (8)
Crystal cluster (M, light, pulse, cold-blue and ember-red variants); Banner (M, sway); Chain (M, sway); Rune stone (M, light, readable lore); Fire bowl (M, flicker); Iron gate (L, sliding door on a switch channel); Altar (M, marker); Bone pile (S, never a figure of a person).

## Dungeon and play (8)
Lever (M, switch on a channel) star; Pressure plate (S, switch by standing); Portcullis (L, sliding door); Chest (M, door, later container); Objective beacon (M, marker, light); Sconce (M, flicker); Cobweb (S, translucent); Teleport pad (S, light, marker).

## Void, ethereal (4)
Floating pearl (S, bob, light); Echo frame (L, glowing doorframe); Monolith marker (XL); Door in the fog (L, opens onto nothing).

## The first wave and what each proves
| Prop | Proves |
| --- | --- |
| Barrel | `lathe`, the simplest recipe; `top` socket |
| Bench | symmetry, `legs`, `seat` with two sockets |
| Signpost | `part` and `readable` text |
| Street lamp | a light tied to night; variants by `paint` |
| Brazier | flicker, a two-state switch, fire colours |
| Door (wood) | `door` with a moving part and collision that changes |
| Lever | a `switch` on a channel, used with the door |
| Pine | `scatter`, wind `sway`, seeds, the snow variant |
| Console | emissive indicators, `readable` status |
| Lantern (hanging) | a `hang` socket, `swing`, attaching to another prop |
| Rowboat | Lane B import from `boat.vox`, the `water` surface, two seats |
| Crate | stacking by `top` sockets, a lid state, `weather` |

When these twelve pass the checks, look right on the contact sheet, and behave in the yard, the dark sea and the station, the pipeline is proven and the rest is work, not research.
