# Decisions

Architecture decision records: one file per decision that is hard to reverse or surprising
without its context, numbered in order. Each says what was decided, why, what else was
considered, and what follows. A later record that changes one says so at the top of both.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-simulation-in-the-voxel-octree.md) | Simulation state in the sparse voxel octree: fields on grids, sparse per-block state, the octree for activity and summaries | proposed |

Larger plans live in [../design/](../design/); decisions are also recorded in the project's
knowledge base (`knowledge/decisions/`, through the `okf_*` tools).
