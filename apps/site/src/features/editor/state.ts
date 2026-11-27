// The editor page's picture of the world, built from the client's events: the entity list
// (kept by seq, so a stale change never overwrites a newer one), the selection, who is here,
// and the latest refusal. Pure functions, so the page's state stays easy to reason about.
// A removed entity leaves a tombstone (its id and the seq of the removal), so a stale change
// from before the removal, arriving late, cannot bring it back.

import type { Change, Component } from "../../game/editor";

export interface Row {
  entity: number;
  seq: number;
  component: Component;
}

export interface Entities {
  /** the live entities, by id */
  readonly rows: ReadonlyMap<number, Row>;
  /** removed entities, by id, with the seq of the removal: a change at or below it is stale */
  readonly gone: ReadonlyMap<number, number>;
}

/** An empty picture: what the page starts from, and what a new world (`ready`) starts from. */
export function cleared(): Entities {
  return { rows: new Map(), gone: new Map() };
}

/** Applies a batch of changes. A change older than the row it would replace, or older than the
 *  removal of its entity, is dropped. */
export function applyChanges(entities: Entities, changes: Change[]): Entities {
  const rows = new Map(entities.rows);
  const gone = new Map(entities.gone);
  for (const c of changes) {
    const have = rows.get(c.entity);
    if (have && have.seq > c.seq) continue;
    const removed = gone.get(c.entity);
    if (removed !== undefined && removed >= c.seq) continue;
    if (c.component === null) {
      rows.delete(c.entity);
      gone.set(c.entity, c.seq);
    } else {
      rows.set(c.entity, { entity: c.entity, seq: c.seq, component: c.component });
      gone.delete(c.entity);
    }
  }
  return { rows, gone };
}

/** The kind of an entity's component: "Portal", "Spawn", "Objective" or "Settings". */
export function kindOf(component: Component): string {
  return Object.keys(component)[0] ?? "Unknown";
}

/** A short label for the entity list and the inspector heading. */
export function labelOf(row: Row): string {
  const c = row.component;
  if ("Portal" in c) return `Portal "${c.Portal.name}"`;
  if ("Objective" in c) return `Objective "${c.Objective.name}"`;
  if ("Spawn" in c) return "Spawn";
  return "Settings";
}

/** Sorts rows by kind, then by entity id, so the list keeps its order between batches. */
export function sortedRows(entities: Entities): Row[] {
  const order = ["Portal", "Objective", "Spawn", "Settings"];
  return [...entities.rows.values()].sort(
    (a, b) => order.indexOf(kindOf(a.component)) - order.indexOf(kindOf(b.component)) || a.entity - b.entity,
  );
}
