# Worlds and portals

A Vox world is a shared block space. What people build in it stays. You can walk between
worlds through portals.

## The Void

The Void is the first world. It is an ethereal, flat, luminous space: a pale polished floor,
25 km across, going on into a bright haze, with three moons faint in the light. There is room
to build almost anywhere. You arrive in a circle of pale pillars with glowing tops.

- To the north stands the **First Gate**, a lone doorway. It leads to the yard.
- To the west is the black **Ember Door**. It leads to the islands.

Far off in the haze there is a stair to nowhere, some monoliths and empty doorframes.

## The yard

The yard is the world on port 7781. Its sky is the earth sky: sun, clouds and stars. On earth
the time of day can change. An echo of the yard shows through the First Gate in the Void.

## The islands

The islands are the old floating islands in dark fog, on port 7782. The sky has three veiled
moons. The Ember Door in the Void leads here.

## Portals

Walk into a portal to cross to another world. A portal can show its destination before you
step in: look for an echo, faint at first and solid up close. Walk through and you are there.

If nobody answers on the other side, you are put back where you were.

You can also open portals of your own inside a plot you own (30 authority; see below).

## Weather

Each world has its own air, shared by everyone in it. Weather sets how far you can see, the
fog round you, the clouds, rain or snow, and which way flames and water move. The bar at the
bottom of the screen shows the air where you stand.

The weather commands below are typed into the Block Avatar app's control page when it is
joined to a world (see [the avatar manual](../avatar/manual.md)); the Vox client has no
command box.

Commands:

- `world:weather:<name>`, where `<name>` is one of: clear, haze, mist, fog, luminous, rain,
  storm, windy, snow
- `world:wind:8:270` (speed in m/s, and the direction it blows from: 270 is west)
- `world:humidity:0.9`
- `world:temperature:-3`
- `world:time:18.5` or `world:time:auto` (on earth skies; `auto` is a day every 24 minutes)
- `world:clouds:0.7`

## Plots, authority and raids

Authority is the currency every world shares. You start with 25, and you earn more by staying
in worlds and building. A plot (20 authority) is 64 × 64 blocks, which only you, or a group you
share it with, can build in. A raid is for a raid party: its leader pays 10 authority; when two or more party members spend
ten minutes in a world together within the hour, each member who is there gets a trophy and 15
authority (up to twice a day).

## Running your own world

To run a world of your own, see [Vox: playing and running worlds](../vox/README.md).
