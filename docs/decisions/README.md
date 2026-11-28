# Decisions

Architecture decision records: one file per decision that is hard to reverse or surprising
without its context, numbered in order. Each says what was decided, why, what else was
considered, and what follows. A later record that changes one says so at the top of both.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-simulation-in-the-voxel-octree.md) | Simulation state in the sparse voxel octree: the octree as simulation's index (activity, summaries, sampling points), fields in octree-indexed bricks, sparse per-block state | proposed |
| [0002](0002-bsn-scenes-rooms-and-openings.md) | BSN (`bsn!`) for how the world's components are drawn on the client, components staying engine-free; authored rooms and openings for indoor (cells-and-portals) culling | proposed |

Larger plans live in [../design/](../design/); decisions are also recorded in the project's
knowledge base (`knowledge/decisions/`, through the `okf_*` tools).
