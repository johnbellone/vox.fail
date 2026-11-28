# Decisions

Architecture decision records: one file per decision that is hard to reverse or surprising
without its context, numbered in order. Each says what was decided, why, what else was
considered, and what follows. A later record that changes one says so at the top of both.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-simulation-in-the-voxel-octree.md) | Simulation state in the sparse voxel octree: the octree as simulation's index (activity, summaries, sampling points), fields in octree-indexed bricks, sparse per-block state | proposed |
| [0002](0002-bsn-scenes-rooms-and-openings.md) | BSN (`bsn!`) for how the world's components are drawn on the client, components staying engine-free; authored rooms and openings for indoor (cells-and-portals) culling | proposed |
| [0003](0003-lua-triggers.md) | Lua (Luau) triggers in the world format: scripts as hashed blobs and a `Trigger` component, run only by the world server through a checked host API, with budgets | proposed |
| [0004](0004-seamless-transitions.md) | Seamless transitions: make-before-break crossings between worlds (prepared by time, the destination woken, a live echo); falling from orbit into the sea through a shared planet frame and a compressed descent | proposed, amended |
| [0005](0005-traversing-servers.md) | Traversing servers without seams: make before break everywhere, neighbouring servers share a border margin with one owner per cell, a gateway holding each player's one connection, moving boundaries later | proposed |

Larger plans live in [../design/](../design/); decisions are also recorded in the project's
knowledge base (`knowledge/decisions/`, through the `okf_*` tools).
