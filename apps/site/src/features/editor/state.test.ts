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
  possibleEdges,
  withEdge,
  settingsOf,
  settingsFormOf,
  settingsJsonOf,
  atlasOf,
  spawnFormOf,
  spawnOf,
  objectiveFormOf,
  objectiveOf,
  rowOfKind,
  MAX_SETTINGS_BYTES,
  MAX_RADIUS,
  type Entities,
  type Json,
  type SettingsForm,
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

describe("atlas edges", () => {
  test("a tile has the edges whose next tile is on the map", () => {
    expect(possibleEdges([0, 0], [2, 2])).toEqual(["east", "south"]);
    expect(possibleEdges([1, 1], [3, 3])).toEqual(["east", "west", "south", "north"]);
    expect(possibleEdges([1, 0], [2, 1])).toEqual(["west"]);
    expect(possibleEdges([0, 0], [1, 1])).toEqual([]);
  });

  test("opening and closing keeps the order east, west, south, north", () => {
    const all = ["east", "west", "south", "north"] as const;
    expect(withEdge([], "south", true, all)).toEqual(["south"]);
    expect(withEdge(["south"], "east", true, all)).toEqual(["east", "south"]);
    expect(withEdge(["east", "south"], "east", false, all)).toEqual(["south"]);
    expect(withEdge(["up", "east"], "south", true, all)).toEqual(["east", "south", "up"]);
  });

  test("an edge the tile cannot have is never opened", () => {
    expect(withEdge([], "west", true, ["east", "south"])).toEqual([]);
  });

  test("the atlas of a settings object is its tile and size, or nothing", () => {
    expect(atlasOf({ atlas: { tile: [1, 2], size: [4, 4], edges: [] } })).toEqual({ tile: [1, 2], size: [4, 4] });
    expect(atlasOf({ atlas: { tile: [1], size: [4, 4] } })).toBeNull();
    expect(atlasOf({ sky: "void" })).toBeNull();
  });
});

describe("world settings", () => {
  const base: Json = {
    sky: "void",
    time: 12,
    clouds: 0.45,
    relief: { amount: 1, seed: 7 },
    hotbar: ["pearl", "lumen"],
    portals: [{ name: "yard", at: [0, 0, 0] }],
    cameras: [{ yaw: 0 }],
    floor: 2,
    unknown: { kept: true },
  };

  test("a form edits a copy: keys it does not show come back as they were", () => {
    const form = settingsFormOf(base);
    const out = settingsJsonOf(base, { ...form, time: "6" });
    expect(out.ok).toBe(true);
    if (!out.ok) return;
    const json = JSON.parse(out.value);
    expect(json.time).toBe(6);
    expect(json.portals).toEqual(base.portals);
    expect(json.cameras).toEqual(base.cameras);
    expect(json.floor).toBe(2);
    expect(json.unknown).toEqual({ kept: true });
    expect(json.relief).toEqual({ amount: 1, seed: 7 });
  });

  test("an empty field leaves its key out, and an unset weather is left out", () => {
    const form = settingsFormOf(base);
    const out = settingsJsonOf(base, { ...form, sky: " ", clouds: "", relief: "" });
    expect(out.ok).toBe(true);
    if (!out.ok) return;
    const json = JSON.parse(out.value);
    expect("sky" in json).toBe(false);
    expect("clouds" in json).toBe(false);
    expect(json.relief).toEqual({ seed: 7 });
    expect("weather" in json).toBe(false);
  });

  test("numbers out of range or not finite are refused", () => {
    const form = settingsFormOf(base);
    const refused = (f: Partial<SettingsForm>) => !settingsJsonOf(base, { ...form, ...f }).ok;
    expect(refused({ time: "25" })).toBe(true);
    expect(refused({ time: "-1" })).toBe(true);
    expect(refused({ time: "Infinity" })).toBe(true);
    expect(refused({ time: "noon" })).toBe(true);
    expect(refused({ clouds: "1.5" })).toBe(true);
    expect(refused({ relief: "2" })).toBe(true);
    expect(refused({ time: "24" })).toBe(false);
  });

  test("stored edges are kept once, and kept as they are when the tile is unknown", () => {
    const tiled = { atlas: { tile: [0, 0], size: [2, 2], edges: ["east", "east", "south"] } };
    expect(settingsFormOf(tiled).edges).toEqual(["east", "south"]);
    const untiled = { atlas: { edges: ["east", "north"] } };
    expect(settingsFormOf(untiled).edges).toEqual(["east", "north"]);
  });

  test("a custom weather is a JSON object, and a preset is a string", () => {
    const form = settingsFormOf(base);
    expect(settingsJsonOf(base, { ...form, weather: "custom", weatherJson: "[1]" }).ok).toBe(false);
    expect(settingsJsonOf(base, { ...form, weather: "custom", weatherJson: "nope" }).ok).toBe(false);
    const custom = settingsJsonOf(base, { ...form, weather: "custom", weatherJson: '{"kind":"snow"}' });
    expect(custom.ok && JSON.parse(custom.value).weather).toEqual({ kind: "snow" });
    const preset = settingsJsonOf(base, { ...form, weather: "luminous" });
    expect(preset.ok && JSON.parse(preset.value).weather).toBe("luminous");
  });

  test("water is an object with three colour numbers, a swell and an opacity; off removes it", () => {
    const withWater: Json = { ...base, water: { colour: [0.1, 0.2, 0.3], swell: 1, opacity: 0.5, extra: 1 } };
    const form = settingsFormOf(withWater);
    expect(form.water).toEqual({ colour: ["0.1", "0.2", "0.3"], swell: "1", opacity: "0.5" });
    const out = settingsJsonOf(withWater, { ...form, water: { colour: ["0", "0.5", "1"], swell: "3", opacity: "0" } });
    expect(out.ok && JSON.parse(out.value).water).toEqual({ colour: [0, 0.5, 1], swell: 3, opacity: 0, extra: 1 });
    expect(settingsJsonOf(withWater, { ...form, water: { colour: ["0", "0", "2"], swell: "1", opacity: "1" } }).ok).toBe(false);
    expect(settingsJsonOf(withWater, { ...form, water: { colour: ["0", "0", "0"], swell: "4", opacity: "1" } }).ok).toBe(false);
    expect(settingsJsonOf(withWater, { ...form, water: { colour: ["0", "0", "0"], swell: "1", opacity: "" } }).ok).toBe(false);
    const off = settingsJsonOf(withWater, { ...form, water: null });
    expect(off.ok && "water" in JSON.parse(off.value)).toBe(false);
  });

  test("the hotbar holds nine blocks at most, and an empty one is left out", () => {
    const form = settingsFormOf(base);
    expect(settingsJsonOf(base, { ...form, hotbar: Array(10).fill("stone") }).ok).toBe(false);
    const nine = settingsJsonOf(base, { ...form, hotbar: Array(9).fill("stone") });
    expect(nine.ok && JSON.parse(nine.value).hotbar).toEqual(Array(9).fill("stone"));
    const none = settingsJsonOf(base, { ...form, hotbar: [] });
    expect(none.ok && "hotbar" in JSON.parse(none.value)).toBe(false);
  });

  test("on an atlas tile, the edges written are the ones the form has, in order", () => {
    const tile: Json = { sky: "earth", atlas: { name: "a", tile: [0, 0], size: [2, 2], edges: ["east"] } };
    const form = settingsFormOf(tile);
    expect(form.edges).toEqual(["east"]);
    const possible = possibleEdges([0, 0], [2, 2]);
    const opened = withEdge(form.edges, "south", true, possible);
    const out = settingsJsonOf(tile, { ...form, edges: opened });
    expect(out.ok && JSON.parse(out.value).atlas).toEqual({ name: "a", tile: [0, 0], size: [2, 2], edges: ["east", "south"] });
  });

  test("the settings text is at most MAX_SETTINGS_BYTES", () => {
    const out = settingsJsonOf(base, { ...settingsFormOf(base), sky: "x".repeat(MAX_SETTINGS_BYTES) });
    expect(out.ok).toBe(false);
    expect(settingsJsonOf({}, { ...settingsFormOf({}), sky: "x".repeat(1000) }).ok).toBe(true);
  });

  test("the settings must be an object", () => {
    expect(settingsOf("[]").ok).toBe(false);
    expect(settingsOf("nope").ok).toBe(false);
    expect(settingsOf('{"sky":"void"}').ok).toBe(true);
  });
});

describe("spawn and objective", () => {
  const spot = { at: [4, 1, -2] as [number, number, number], axis: "X" as const, size: [4, 6] as [number, number] };

  test("a new spawn starts at the marked spot; an existing one at its own place", () => {
    expect(spawnFormOf(undefined, spot).at).toEqual(["4", "1", "-2"]);
    expect(spawnFormOf({ at: [1, 2, 3], yaw: 90 }, spot)).toEqual({ at: ["1", "2", "3"], yaw: "90" });
  });

  test("a spawn needs three position numbers and a finite turn", () => {
    expect(spawnOf({ at: ["1", "2", "3"], yaw: "180" })).toEqual({ ok: true, value: { at: [1, 2, 3], yaw: 180 } });
    expect(spawnOf({ at: ["1", "", "3"], yaw: "0" }).ok).toBe(false);
    expect(spawnOf({ at: ["1", "2", "3"], yaw: "Infinity" }).ok).toBe(false);
  });

  test("an objective needs a name, a position and a radius from more than 0 to MAX_RADIUS", () => {
    const form = objectiveFormOf(undefined, spot);
    expect(form.radius).toBe("");
    const good = { name: "the altar", at: ["0", "0", "-9"] as [string, string, string], radius: "3" };
    expect(objectiveOf(good)).toEqual({ ok: true, value: { name: "the altar", at: [0, 0, -9], radius: 3 } });
    expect(objectiveOf({ ...good, name: "  " }).ok).toBe(false);
    expect(objectiveOf({ ...good, name: "a".repeat(33) }).ok).toBe(false);
    expect(objectiveOf({ ...good, radius: "0" }).ok).toBe(false);
    expect(objectiveOf({ ...good, radius: String(MAX_RADIUS + 1) }).ok).toBe(false);
    expect(objectiveOf({ ...good, radius: String(MAX_RADIUS) }).ok).toBe(true);
  });

  test("rowOfKind finds the first entity of a kind, typed", () => {
    const entities: Entities = {
      rows: new Map([
        [3, { entity: 3, seq: 1, component: spawn(0) }],
        [4, { entity: 4, seq: 2, component: { Settings: { json: "{}" } } }],
      ]),
      gone: new Map(),
    };
    expect(rowOfKind(entities, "Settings")?.component.Settings.json).toBe("{}");
    expect(rowOfKind(entities, "Objective")).toBeUndefined();
    expect(rowOfKind(entities, "Spawn")?.component.Spawn.yaw).toBe(0);
  });
});

describe("atlas edges left as the world has them", () => {
  test("an atlas with no edges key starts with every edge checked, and Apply leaves the key out", () => {
    const tile: Json = { sky: "earth", atlas: { name: "a", tile: [0, 0], size: [2, 2] } };
    const form = settingsFormOf(tile);
    expect(form.edges).toEqual(possibleEdges([0, 0], [2, 2]));
    const out = settingsJsonOf(tile, form);
    expect(out.ok && JSON.parse(out.value).atlas).toEqual({ name: "a", tile: [0, 0], size: [2, 2] });
  });

  test("once an edge is toggled, the edges are written: the closed one is left out, the rest kept", () => {
    const tile: Json = { sky: "earth", atlas: { name: "a", tile: [0, 0], size: [2, 2] } };
    const form = settingsFormOf(tile);
    const closed = withEdge(form.edges, "east", false, possibleEdges([0, 0], [2, 2]));
    const out = settingsJsonOf(tile, { ...form, edges: closed });
    expect(out.ok && JSON.parse(out.value).atlas.edges).toEqual(["south"]);
  });

  test("toggling an edge off and back on leaves the key out again", () => {
    const tile: Json = { atlas: { name: "a", tile: [0, 0], size: [2, 2] } };
    const possible = possibleEdges([0, 0], [2, 2]);
    const form = settingsFormOf(tile);
    const back = withEdge(withEdge(form.edges, "east", false, possible), "east", true, possible);
    const out = settingsJsonOf(tile, { ...form, edges: back });
    expect(out.ok && "edges" in JSON.parse(out.value).atlas).toBe(false);
  });

  test("a stored edge the tile cannot have is dropped from the form, and not written back", () => {
    // the corner tile (0,0) of a 2x2 map has no west or north edge
    const tile: Json = { atlas: { name: "a", tile: [0, 0], size: [2, 2], edges: ["east", "west", "up"] } };
    const form = settingsFormOf(tile);
    expect(form.edges).toEqual(["east"]);
    const out = settingsJsonOf(tile, form);
    expect(out.ok && JSON.parse(out.value).atlas.edges).toEqual(["east"]);
  });
});
