// Tests for the editor page's entity picture: changes apply by seq, a removal leaves a
// tombstone, and a new world (`ready`) starts from nothing.

import { describe, expect, test } from "bun:test";
import type { Change, Component } from "../../game/editor";
import type { Dest, Event, Portal } from "../../game/editor";
import { isCommand } from "../../game/editor";
import type { Item } from "../../api/types";
import {
  applyChanges,
  cleared,
  createPortal,
  destFormOf,
  destKinds,
  destOf,
  type Edits,
  newPortalForm,
  numberOf,
  type PortalForm,
  portalKeys,
  portalOf,
  reloaded,
  refusalLine,
  rightsFor,
  refuseEdit,
  removePortal,
  resendEdit,
  retryCommand,
  sentEdit,
  settleEdit,
  setPortal,
  dropEdit,
  sortedRows,
} from "./state";

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

// Portal forms and edits in flight: the destination choices by mode, the commands a form sends
// (checked against the bridge's own guards), refs matched to their answers, and a retry.


const portal = (over: Partial<Portal> = {}): Portal => ({
  name: "home",
  at: [12, 0, -4],
  axis: "Z",
  size: [8, 16],
  yaw: 0,
  level: "yard",
  to: { World: { world: "void" } },
  key: null,
  ...over,
});

describe("destination choices", () => {
  test("Basic offers a world and one of your portals; Advanced adds here, url and dungeon", () => {
    expect(destKinds(false, "World")).toEqual(["World", "Pair"]);
    expect(destKinds(true, "World")).toEqual(["World", "Pair", "Here", "Url", "Dungeon"]);
  });

  test("an existing doorway's kind is still offered when the mode lacks it", () => {
    expect(destKinds(false, "Url")).toEqual(["World", "Pair", "Url"]);
  });

  test("each kind round-trips through its form, and a form's own fields are checked", () => {
    const dests: Dest[] = [
      { World: { world: "void" } },
      { Pair: { key: 42 } },
      { Here: { arrive: [1, 2, 3] } },
      { Url: { url: ":7781", arrive: [0, 0, 0] } },
      { Dungeon: { name: "crypt", arrive: [4, 5, 6] } },
    ];
    for (const d of dests) {
      const back = destOf(destFormOf(d, [0, 0, 0]));
      expect(back).toEqual({ ok: true, value: d });
    }
    expect(destOf(destFormOf({ World: { world: "" } }, [0, 0, 0])).ok).toBe(false);
    expect(destOf({ ...destFormOf({ Pair: { key: 1 } }, [0, 0, 0]), key: "" }).ok).toBe(false);
    expect(destOf({ ...destFormOf({ Url: { url: "x", arrive: [0, 0, 0] } }, [0, 0, 0]), url: "  " }).ok).toBe(false);
  });
});

describe("rights on a portal", () => {
  test("a player's portal is read-only to an Advanced editor", () => {
    expect(rightsFor(true, true)).toEqual({ editable: false, nameEditable: false });
  });

  test("a player's name is locked for a Basic builder, the rest is theirs to move", () => {
    expect(rightsFor(false, true)).toEqual({ editable: true, nameEditable: false });
    expect(rightsFor(false, false)).toEqual({ editable: true, nameEditable: true });
    expect(rightsFor(true, false)).toEqual({ editable: true, nameEditable: true });
  });
});

describe("numbers and portal forms", () => {
  test("numberOf takes finite numbers only", () => {
    expect(numberOf(" 3.5 ")).toBe(3.5);
    expect(numberOf("")).toBeNull();
    expect(numberOf("abc")).toBeNull();
    expect(numberOf("Infinity")).toBeNull();
  });

  test("a portal is built from its form with the level and key it was given", () => {
    const form = { ...newPortalForm({ at: [1, 2, 3], axis: "X", size: [4, 6] }), name: " gate " };
    const built = portalOf({ ...form, dest: { ...form.dest, kind: "World", world: "void" } }, { level: "yard", key: null }, true);
    expect(built).toEqual({
      ok: true,
      value: {
        name: "gate",
        at: [1, 2, 3],
        axis: "X",
        size: [4, 6],
        yaw: 0,
        level: "yard",
        to: { World: { world: "void" } },
        key: null,
      },
    });
  });

  test("a form with a missing name, a non-finite number or a turn that isn't a quarter is refused", () => {
    const base = newPortalForm({ at: [0, 0, 0], axis: "Z", size: [4, 6] });
    const world = { ...base.dest, kind: "World" as const, world: "void" };
    expect(portalOf({ ...base, name: "", dest: world }, { level: "yard", key: null }, true).ok).toBe(false);
    expect(portalOf({ ...base, name: "a", at: ["1", "Infinity", "0"], dest: world }, { level: "yard", key: null }, true).ok).toBe(false);
    expect(portalOf({ ...base, name: "a", yaw: "45", dest: world }, { level: "yard", key: null }, true).ok).toBe(false);
  });

  test("a create and a set are commands the bridge accepts, with their ref and expect", () => {
    const p = portal();
    const create = createPortal(p, 3);
    expect(create).toEqual({ type: "create", component: { Portal: p }, ref: 3 });
    expect(isCommand(create)).toBe(true);
    const set = setPortal(7, p, 5, 4);
    expect(set).toEqual({ type: "set", entity: 7, component: { Portal: p }, expect: 5, ref: 4 });
    expect(isCommand(set)).toBe(true);
    const remove = removePortal(7, 5, 6);
    expect(remove).toEqual({ type: "remove", entity: 7, expect: 5, ref: 6 });
    expect(isCommand(remove)).toBe(true);
  });
});

describe("portal keys to pair with", () => {
  const key = (ref: number | undefined, kind: Item["kind"] = "key"): Item => ({
    id: `i${ref}`,
    kind,
    name: `Portal to ${ref}`,
    world: "yard",
    world_name: "The Yard",
    ref,
    detail: {},
  });

  test("lists the player's keys, leaving out this portal's own", () => {
    const items = [key(10), key(11), key(undefined), key(12, "coin")];
    expect(portalKeys(items, 11).map((k) => k.key)).toEqual([10]);
    expect(portalKeys(items, null).map((k) => k.key)).toEqual([10, 11]);
  });
});

describe("edits in flight", () => {
  const cmd = (ref: number) => removePortal(1, 1, ref);

  test("a done takes out its own edit and leaves the others", () => {
    let e: Edits = new Map();
    e = sentEdit(e, 1, cmd(1));
    e = sentEdit(e, 2, cmd(2));
    const done: Event = { type: "done", ref: 2, entity: 1, seq: 4 };
    e = settleEdit(e, done);
    expect([...e.keys()]).toEqual([1]);
  });

  test("a refusal keeps its edit with the reason, and a refusal with no ref leaves edits alone", () => {
    let e: Edits = sentEdit(new Map(), 3, cmd(3));
    e = settleEdit(e, { type: "refused", id: 1, ref: 3, why: "Changed", text: "It changed since you looked." });
    expect(e.get(3)?.status).toBe("refused");
    expect(e.get(3)?.text).toBe("It changed since you looked.");
    const same = settleEdit(e, { type: "refused", id: 2, ref: null, why: "Invalid", text: "bad" });
    expect(same).toBe(e);
    expect(settleEdit(e, { type: "done", ref: 99, entity: 1, seq: 1 })).toBe(e);
  });

  test("only an edit refused as Unavailable is retried, with the same command and ref", () => {
    const c = setPortal(2, portal(), 8, 5);
    let e: Edits = sentEdit(new Map(), 5, c);
    e = settleEdit(e, { type: "refused", id: 1, ref: 5, why: "Unavailable", text: "not available right now" });
    const again = retryCommand(e, 5);
    expect(again).toEqual(c);
    expect(again && "ref" in again && again.ref).toBe(5);
    e = resendEdit(e, 5);
    expect(e.get(5)?.status).toBe("sent");
    expect(retryCommand(e, 5)).toBeUndefined();

    const changed = refuseEdit(sentEdit(new Map(), 6, c), 6, "Changed", "changed");
    expect(retryCommand(changed, 6)).toBeUndefined();
  });

  test("letting go of an edit removes it", () => {
    const e = dropEdit(sentEdit(new Map(), 9, cmd(9)), 9);
    expect(e.size).toBe(0);
  });
});

describe("reload after a Changed refusal", () => {
  test("a reload takes the portal as it now is, and the seq the entity now has", () => {
    const moved = { ...portal(), name: "moved on" };
    const r = reloaded(moved, 7);
    expect(r.expect).toBe(7);
    expect(r.form.name).toBe("moved on");
    // an edit built after the reload expects the reloaded seq
    expect(setPortal(1, moved, r.expect, 2)).toMatchObject({ expect: 7 });
  });

  test("a Changed refusal shows the reload line; any other shows the client's text", () => {
    const changed = refuseEdit(sentEdit(new Map(), 6, removePortal(1, 5, 6)), 6, "Changed", "that changed since you looked");
    expect(refusalLine(changed.get(6)!)).toBe("Changed by someone else — reload to see it.");
    const other = refuseEdit(sentEdit(new Map(), 7, removePortal(1, 5, 7)), 7, "Invalid", "bad name");
    expect(refusalLine(other.get(7)!)).toBe("bad name");
  });
});

describe("bounds checked before sending", () => {
  const base = newPortalForm({ at: [0, 0, 0], axis: "Z", size: [4, 6] });
  const world = { ...base.dest, kind: "World" as const, world: "void" };
  const built = (over: Partial<PortalForm>, advanced: boolean) =>
    portalOf({ ...base, name: "gate", dest: world, ...over }, { level: "yard", key: null }, advanced).ok;

  test("in Basic a size is 4 to 8 wide and 6 to 16 tall; Advanced leaves it to the world", () => {
    expect(built({ size: ["4", "6"] }, false)).toBe(true);
    expect(built({ size: ["8", "16"] }, false)).toBe(true);
    expect(built({ size: ["3", "6"] }, false)).toBe(false);
    expect(built({ size: ["9", "6"] }, false)).toBe(false);
    expect(built({ size: ["4", "5"] }, false)).toBe(false);
    expect(built({ size: ["4", "17"] }, false)).toBe(false);
    expect(built({ size: ["3", "6"] }, true)).toBe(true);
  });

  test("a name is 1 to 32 characters", () => {
    expect(built({ name: "a".repeat(32) }, true)).toBe(true);
    expect(built({ name: "a".repeat(33) }, true)).toBe(false);
    expect(built({ name: "é".repeat(32) }, true)).toBe(true);
    expect(built({ name: "   " }, true)).toBe(false);
  });

  test("a dungeon's name is 1 to 64 of a-z, 0-9, _ and -", () => {
    const dungeon = (name: string) =>
      destOf({ ...destFormOf({ Dungeon: { name: "x", arrive: [0, 0, 0] } }, [0, 0, 0]), dungeon: name }).ok;
    expect(dungeon("crypt_2-b")).toBe(true);
    expect(dungeon("a".repeat(64))).toBe(true);
    expect(dungeon("a".repeat(65))).toBe(false);
    expect(dungeon("Crypt")).toBe(false);
    expect(dungeon("cr ypt")).toBe(false);
    expect(dungeon("  ")).toBe(false);
  });
});
