# The editor

The editor changes a world while you stand in it. Go to `/edit?world=` and the world's id, as
in `/edit?world=yard`. You sign in as you do to play, and the editor starts the world in edit
mode on the same canvas.

## Opening the editor

- The editor shows the world's name, the tools on the left, and the list of what is in the world.
- **Undo** and **Redo** are at the top. They undo your own changes only (see below).
- The badge shows how many other people are in the world with you, and that the page is live.
- Everyone starts in **Basic**. The switch to **Advanced** appears only for the world's owner and
  its editors; everyone else sees just "Basic".

## Basic tools

Basic lets you change your own plot. A plot is a square of 16 × 16 blocks you have claimed
(see [Building](building.md)).

- **Select** picks and moves things. Pick a thing in the world or in the list to open its panel.
- **Blocks** shows the block palette. Choose a block, then build with it.
- **Portal** marks a spot in your plot; a form opens there. A portal is 4 to 8 blocks wide and
  6 to 16 blocks tall. A plot holds two of your portals at most, and each portal costs 30
  Authority.
- **Prop** shows the prop catalogue while it is held. Choose a prop, then mark a spot. A prop
  needs something to stand on (the floor, or a wall, a ceiling, water or a table, as the prop
  allows), and nobody can put a block where a prop stands; if what holds a prop up is broken, the
  prop goes. A plot holds 64 props. If the catalogue is empty, the world accepts no props yet.

### Pairing portals

A portal leads to a world, or to another of your portals. To make a pair, choose "my other
portal" where the form asks where it leads, and pick one of your keys: the key names the other portal. Pairs run both ways: the
other end is turned to lead back. The keys you can pick are the portals you have made, listed
in your inventory.

Your own portals can be moved, turned and resized in your plot (the form keeps a portal 4 to 8
wide and 6 to 16 tall). The name of a portal you made
is fixed once it is made.

## Moving and turning things

Select a portal or a prop and its handles appear on the ground and at the top of a portal.

- Drag the disc at the sill (or at a prop's foot) to move it.
- Drag the ring around it to turn it.
- Drag the knob at the top corner of a portal to resize it.

Right click drops a drag without changing anything.

## Undo and redo

Undo takes back your last change, and redo puts it back. The editor keeps the last 100 changes.
Undo only ever takes back your own changes. If someone else has changed the same thing since,
the undo is refused and nothing is overwritten.

## Advanced

Advanced is for the world's owner and its editors (the owner may add up to 32 editors). It adds:

- **Any destination** for a portal: a world, your other portal, a spot in this world ("here"),
  an address ("url"), or a dungeon ("dungeon").
- **World settings**: the sky, the time of day, the weather, the clouds, the relief of the
  ground, the water, the hotbar, and on an atlas tile the edges that lead on.
- **Spawn** and **Objective**: where players start, and a place with a radius to reach.
- **Add node**: a graph node, with its kind, size, recipe and seed, and a **Build** button that
  asks the server to build it. Nodes show as outlines, and the selected one as a preview of what
  it builds, with its block count. Undo takes a build back too, block by block, leaving any block
  someone has changed since; a message in the world says how many were left.

Advanced does not change a player's own portal, which is read-only there. The world's settings
cannot be removed, and the last spawn cannot be removed.
