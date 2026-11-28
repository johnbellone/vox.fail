# ADR 0004: Seamless transitions between worlds: falling from orbit into the sea

- **Status:** proposed
- **Date:** 2026-11-28
- **Decider:** the owner
- **Builds on:** [world-network.md](../design/world-network.md) (tickets, atlas edges);
  [chunk-streaming.md](../design/chunk-streaming.md) (streaming, the far-field octree);
  [ADR 0001](0001-simulation-in-the-voxel-octree.md) (gravity as a world setting)
- **Beads:** epic `vox-81l`

## Context

The owner asked how a player can go from orbit down into the sea, onto the planet, **by
falling**: no doorway and no loading screen.

What exists today:

- **Orbit** (`data/levels/orbit.json`) is a station 250 km above the planet. The planet is drawn
  by the sky shader (`outdoors::orbit_limb`, `orbit_ground`, a fixed `ORBIT_KM`); it is not
  blocks. The Down Gate is a doorway to the sea.
- **The sea** (`sea.json`) is a surface world: water, stacks and a lighthouse, with the default
  height (-16 m to +48 m). Its Sky Door leads back up.
- **Going between worlds is a hand-off with a gap.** The client sends `Through`. The world asks
  the directory for a ticket, answers `Travel { url, ticket, arrive }`, and the client drops the
  connection, dials the new world, says hello with the ticket and starts at `arrive`. A ticket
  carries a position and a yaw: **no velocity**. An echo (a picture through the doorway) shows
  the far side first. Atlas edges (`edge:east` and so on) work the same way.
- **Falling has no top speed on the client and a hard one on the server.** The client
  accelerates at 22 m/s² without limit (`client/mod.rs` `GRAVITY`). The server's move check
  refuses falls faster than 70 m/s (`moves::MAX_FALL`). Below a world's floor you are put back
  at the spawn.
- Streaming by view radius and the far-field octree are planned, not built (vox-cti).

Real numbers to design against: a skydiver falls at about 55 m/s at terminal velocity. From
250 km, a real fall would take over an hour, and re-entry speeds are kilometres a second. The
fall has to be a designed experience, not a simulation.

## Decision

### The general mechanism: make before break

Every crossing between worlds (doorways, atlas edges, and the new vertical edges below) becomes
**seamless**, built from four parts:

1. **Prepare early.** When a player is close to a crossing (near a doorway or an edge, or falling
   towards one within a few seconds), the server asks for the ticket at once and sends
   `ToClient::Prepare { url, ticket, frame }`. `frame` says how the two worlds' coordinates
   meet: an offset in blocks, and later a turn.
2. **Connect and stream before crossing.** The client opens the second connection in the
   background as an *arriving* player: not yet in that world, no avatar. It asks for chunks and
   far tiles round the expected arrival point (streaming, `View`). It draws them **in the
   current scene**, placed through `frame`, as part of the world below or beyond. They are not
   an echo on a quad.
3. **Cross with state.** The crossing itself is decided by the origin world as now (`Through`,
   or `edge:down`). The ticket gains **velocity** and **facing**, so the destination starts you
   moving as you were. The client switches its poses to the new world, which already has its
   chunks on screen, and closes the old connection once the new world has accepted the ticket.
   No frame is drawn without a world.
4. **Fail safe.** If the destination isn't ready, because it's still connecting or the ticket was
   refused, the crossing waits rather than blanks. A walker is held at the edge; a faller slows
   and hangs in the haze. If it fails, they stay where they are, as today.

Tickets, `Travel` and the directory's travel API gain velocity and the frame, which means a
protocol bump. The new connection proves itself with the ticket exactly as now: one world, once,
for a minute.

### Orbit to sea: two worlds, one fall

5. **A shared planet frame.** A world may say where it is on its planet: `"frame": { "planet":
   "home", "surface": [x, z], "altitude_m": a }`. The sea is at the surface (sea level at y = 0).
   The station is 250 km above the same place. Crossings between worlds that share a planet take
   their `frame` from this, so falling straight down from the station lands you in the sea below
   it.
6. **The sea gets a tall sky.** Its level's `bounds` reach 2 km up (8,000 blocks). Empty air
   costs nothing to store, since only built chunks are kept. The top of that sky is the sea
   world's **upper edge**.
7. **Orbit's empty space below the station is the high descent, compressed.** Below the
   station, the orbit world maps its height to a true altitude. Falling 8 km in the orbit world
   carries you from 250 km down to 2 km, on a log scale: fast where the air is thin, slower as
   it thickens. The sky, the planet's curve, the horizon's dip and the atmosphere's glow are
   drawn for the **true** altitude: `ORBIT_KM` becomes a value that changes, and the orbit and
   sea skies blend through 20 to 100 km. A re-entry glow (plasma and heat) wraps the player
   through the middle of it, which also hides the moment the planet's surface switches from the
   sky shader to real far-field tiles of the sea.
8. **The crossing is `edge:down`.** Reaching the bottom of orbit's descent (2 km true altitude)
   is a vertical edge, like the atlas's side edges. The sea world was prepared and has been
   streaming since the fall began (decision 1), so the player arrives at the top of the sea's
   tall sky, still falling, on the clouds and water they were already watching. From there it is
   real: 2 km at about 55 m/s is about 36 s, into the swell. The Sky Door's way up stays a
   doorway, and an *upper edge* back into orbit is possible later, for a lift or a rocket.
9. **Falling becomes a world setting.** Gravity, terminal velocity and the fastest fall the
   server accepts are per-world settings (ADR 0001 decision 6), shared by the client's body and
   the server's move check, so they can't disagree as 22 m/s² and 70 m/s do now. The sea: 9.81
   m/s² and 55 m/s. Orbit's descent: a fall speed set by the altitude mapping, with the move
   check reading the same mapping. Water slows a fall. Whether a fall onto solid ground hurts is
   left to a health decision.
10. **What you see on the way down** comes from the destination's own data, once streaming and
    the far field exist: the sea's far tiles and clouds from its weather grid, with
    simulation summaries (ADR 0001) colouring them. Before that, the planet stays the sky
    shader's, and the prepared sea world's chunks appear as the fall nears them.

## Options considered

| | Option | Verdict |
| --- | --- | --- |
| A | Keep doorways and loading gaps (today) | Rejected for this: the owner asked for falling, without a break. |
| B | One world from orbit to sea floor, 250 km tall, sparse | Rejected: different skies, gravity and weather in one world; positions far from 0 lose precision (needing a floating origin everywhere); one server for all of it; worlds can't be owned separately. |
| C | Three worlds stacked: orbit, an empty "sky" world, sea | Kept as a variant: a sky world is worth having only if the descent needs content of its own (weather to fly through, encounters). Two worlds first. |
| D | A cutscene or a fade between orbit and sea | Rejected: not falling. |
| E | Two worlds, a compressed high descent in orbit, a tall real sky over the sea, seamless crossing | **Chosen.** |

## Consequences

- **Good:**
  - The same make-before-break crossing makes doorways and atlas edges seamless too.
  - The fall from the station is one continuous experience into the sea below it.
  - Falling rules stop disagreeing between client and server.
- **Costs:**
  - Two connections at once for a few seconds per crossing: more memory, and a second join.
  - The protocol bump for `Prepare`, velocity and the frame.
  - The sky shader generalised to any altitude.
  - The descent mapping must be the same on the client, on the server's move check, and in the
    shader.
  - The fall is only as seamless as streaming makes it, so this depends on vox-cti.7 and .8 for
    the sea's chunks and .12 and .13 for its far tiles.
- **Open:**
  - The descent's numbers (8 km, the log curve, 2 km): tune by feel.
  - Whether you steer while falling, and how much.
  - Fall damage (with health).
  - Several stations or seas on one planet: the frame allows it, and the atlas could hang under
    the station.
  - What other players see: a falling player shows in the sea's sky only after their crossing,
    unless the sea also gets their poses while they're prepared.

## Follow-up beads (epic `vox-81l`)

1. `vox-81l.1` Per-world falling settings: gravity, terminal velocity, and the fastest fall the
   server accepts.
2. `vox-81l.2` Tickets and travel carry velocity, facing and a frame; `Prepare` (protocol bump).
3. `vox-81l.3` Client: make-before-break crossings, doorways and atlas edges first (after
   streaming, vox-cti.8).
4. `vox-81l.4` The planet frame, the sea's tall sky, orbit's compressed descent, and `edge:down`.
5. `vox-81l.5` The sky at any altitude, and the re-entry glow.
6. `vox-81l.6` Review, and a recorded playtest of the fall from the station.
