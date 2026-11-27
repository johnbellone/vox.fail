// Tests for the editor page's entity picture: changes apply by seq, a removal leaves a
// tombstone, and a new world (`ready`) starts from nothing.

import { describe, expect, test } from "bun:test";
import type { Change, Component } from "../../game/editor";
import { applyChanges, cleared, sortedRows } from "./state";

const spawn = (x: number): Component => ({ Spawn: { at: [x, 0, 0], yaw: 0 } });
const set = (entity: number, seq: number, x = 0): Change => ({ entity, seq, component: spawn(x) });
const gone = (entity: number, seq: number): Change => ({ entity, seq, component: null });

describe("applyChanges", () => {
  test("a newer change replaces the row, an older one is dropped", () => {
    let e = applyChanges(cleared(), [set(1, 5, 10)]);
    e = applyChanges(e, [set(1, 3, 99)]);
    expect(e.rows.get(1)?.seq).toBe(5);
    expect(e.rows.get(1)?.component).toEqual(spawn(10));
    e = applyChanges(e, [set(1, 7, 20)]);
    expect(e.rows.get(1)?.seq).toBe(7);
  });

  test("a removal drops the row and a stale change after it does not bring it back", () => {
    let e = applyChanges(cleared(), [set(2, 4)]);
    e = applyChanges(e, [gone(2, 6)]);
    expect(e.rows.has(2)).toBe(false);
    expect(e.gone.get(2)).toBe(6);
    e = applyChanges(e, [set(2, 5)]);
    expect(e.rows.has(2)).toBe(false);
    e = applyChanges(e, [gone(2, 6)]);
    expect(e.gone.get(2)).toBe(6);
  });

  test("a change newer than the removal creates the entity again and clears its tombstone", () => {
    let e = applyChanges(cleared(), [gone(3, 2)]);
    e = applyChanges(e, [set(3, 9, 4)]);
    expect(e.rows.get(3)?.seq).toBe(9);
    expect(e.gone.has(3)).toBe(false);
  });

  test("a stale removal does not remove a newer row", () => {
    let e = applyChanges(cleared(), [set(4, 8)]);
    e = applyChanges(e, [gone(4, 3)]);
    expect(e.rows.has(4)).toBe(true);
    expect(e.gone.has(4)).toBe(false);
  });
});

describe("ready", () => {
  test("cleared() forgets rows and tombstones, so a new world's entities apply from any seq", () => {
    let e = applyChanges(cleared(), [set(1, 50), gone(2, 50)]);
    e = cleared();
    expect(e.rows.size).toBe(0);
    expect(e.gone.size).toBe(0);
    e = applyChanges(e, [set(2, 1, 7)]);
    expect(e.rows.get(2)?.seq).toBe(1);
  });
});

describe("sortedRows", () => {
  test("orders by kind, then by id", () => {
    const e = applyChanges(cleared(), [
      { entity: 9, seq: 1, component: { Settings: { json: "{}" } } },
      set(5, 1),
      { entity: 2, seq: 1, component: { Objective: { name: "o", at: [0, 0, 0], radius: 2 } } },
    ]);
    expect(sortedRows(e).map((r) => r.entity)).toEqual([2, 5, 9]);
  });
});
