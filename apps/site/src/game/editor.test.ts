import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { type Component, type Event, isCommand, isEvent, onEvent, readEvent } from "./editor";

const samples = JSON.parse(readFileSync(new URL("./editor.samples.json", import.meta.url), "utf8")) as {
  commands: unknown[];
  events: unknown[];
};

describe("editor samples", () => {
  test("every sample command is a Command", () => {
    expect(samples.commands.length).toBeGreaterThan(0);
    for (const c of samples.commands) expect(isCommand(c)).toBe(true);
  });

  test("every sample event is an Event", () => {
    expect(samples.events.length).toBeGreaterThan(0);
    for (const e of samples.events) expect(isEvent(e)).toBe(true);
  });

  test("the samples cover every command and event type", () => {
    const types = (xs: unknown[]) => new Set(xs.map((x) => (x as { type: string }).type));
    expect([...types(samples.commands)].sort()).toEqual(
      ["block", "build", "create", "fly_to", "prop", "redo", "remove", "select", "set", "tool", "undo"],
    );
    expect([...types(samples.events)].sort()).toEqual(
      ["done", "entities", "here", "picked", "prop_spot", "ready", "refused", "selected", "spot"],
    );
  });
});

describe("command guards", () => {
  const select = { type: "select", entity: 3 };

  test("an extra field is refused", () => {
    expect(isCommand({ ...select, extra: 1 })).toBe(false);
    expect(isCommand({ type: "undo", extra: 1 })).toBe(false);
  });

  test("a wrong type is refused", () => {
    expect(isCommand({ type: "select", entity: "3" })).toBe(false);
    expect(isCommand({ type: "select", entity: -1 })).toBe(false);
    expect(isCommand({ type: "select", entity: 1.5 })).toBe(false);
    expect(isCommand({ type: "tool", tool: "hammer" })).toBe(false);
    expect(isCommand({ type: "block" })).toBe(false);
  });

  test("a missing field is refused, but a null entity is not", () => {
    expect(isCommand({ type: "remove", entity: 3 })).toBe(false);
    expect(isCommand({ type: "remove", entity: 3, expect: null })).toBe(true);
    expect(isCommand({ type: "select", entity: null })).toBe(true);
  });

  test("an unknown type or a non-object is refused", () => {
    expect(isCommand({ type: "explode" })).toBe(false);
    expect(isCommand(null)).toBe(false);
    expect(isCommand([])).toBe(false);
    expect(isCommand("undo")).toBe(false);
  });

  test("a component is checked inside the command", () => {
    const portal = samples.commands.find((c) => (c as { type: string }).type === "create");
    expect(isCommand(portal)).toBe(true);
    const bad = structuredClone(portal) as { component: { Portal: { axis: string } } };
    bad.component.Portal.axis = "Y";
    expect(isCommand(bad)).toBe(false);
  });

  test("an extra field inside a component is refused", () => {
    const spawn = { type: "create", component: { Spawn: { at: [0, 0, 0], yaw: 0, extra: 1 } } };
    expect(isCommand(spawn)).toBe(false);
  });

  test("a prop command carries exactly 32 hex characters", () => {
    const hash = "0102030405060708090a0b0c0d0e0f10";
    expect(isCommand({ type: "prop", prop: hash })).toBe(true);
    expect(isCommand({ type: "prop", prop: hash.toUpperCase() })).toBe(true);
    expect(isCommand({ type: "prop", prop: hash.slice(2) })).toBe(false);
    expect(isCommand({ type: "prop", prop: `${hash}00` })).toBe(false);
    expect(isCommand({ type: "prop", prop: `zz${hash.slice(2)}` })).toBe(false);
    expect(isCommand({ type: "prop", prop: 16 })).toBe(false);
    expect(isCommand({ type: "prop", prop: hash, variant: 1 })).toBe(false);
  });
});

describe("event guards", () => {
  test("an unknown event or a wrong type is refused", () => {
    expect(isEvent({ type: "explode" })).toBe(false);
    expect(isEvent({ type: "selected", entity: "3" })).toBe(false);
    expect(isEvent({ type: "here", players: "Sam" })).toBe(false);
    expect(isEvent({ type: "picked", cell: [1, 2] })).toBe(false);
  });

  test("an event with an extra field passes, and the extra field is dropped", () => {
    expect(isEvent({ type: "selected", entity: 3, extra: 1 })).toBe(true);
    expect(readEvent({ type: "selected", entity: 3, extra: 1 })).toEqual({ type: "selected", entity: 3 });
    expect(isEvent({ type: "ready", world: "void", palette: [], extra: 1 })).toBe(true);
  });

  test("a spot is a doorway's place, axis and size; a bad axis or size is refused", () => {
    const spot = { type: "spot", at: [4.5, 3, -2.5], axis: "X", size: [4, 6] };
    expect(readEvent(spot)).toEqual({ type: "spot", at: [4.5, 3, -2.5], axis: "X", size: [4, 6] });
    expect(isEvent({ ...spot, axis: "Y" })).toBe(false);
    expect(isEvent({ ...spot, size: [4] })).toBe(false);
    expect(isEvent({ type: "spot", at: [0, 0, 0], axis: "Z" })).toBe(false);
  });

  test("an extra field inside a component passes and is dropped", () => {
    const batch = {
      type: "entities",
      changes: [{ entity: 2, seq: 2, component: { Spawn: { at: [0, 0, 0], yaw: 0, extra: 1 } } }],
    };
    expect(isEvent(batch)).toBe(true);
    expect(readEvent(batch)).toEqual({
      type: "entities",
      changes: [{ entity: 2, seq: 2, component: { Spawn: { at: [0, 0, 0], yaw: 0 } } }],
    });
  });

  const portalSample = (): Record<string, unknown> =>
    structuredClone(
      (samples.commands.find((c) => (c as { type: string }).type === "create") as { component: { Portal: object } })
        .component.Portal,
    ) as Record<string, unknown>;

  test("a component with two tags is normalised to the first known one", () => {
    const two = { Settings: { json: "{}" }, Spawn: { at: [1, 2, 3], yaw: 4 }, Prop: { name: "lamp" } };
    const e = readEvent({ type: "entities", changes: [{ entity: 1, seq: 1, component: two }] });
    // Spawn comes before Settings in the fixed order; Prop is unknown
    expect(e).toEqual({
      type: "entities",
      changes: [{ entity: 1, seq: 1, component: { Spawn: { at: [1, 2, 3], yaw: 4 } } }],
    });
    // a command component with two tags is still refused
    expect(isCommand({ type: "create", component: { Spawn: { at: [1, 2, 3], yaw: 4 }, Settings: { json: "{}" } } })).toBe(false);
  });

  test("a destination with two tags is normalised to the first known one", () => {
    const portal = portalSample();
    portal.to = { Url: { url: ":7780", arrive: [0, 0, 0] }, Prop: {}, World: { world: "yard" } };
    const e = readEvent({ type: "entities", changes: [{ entity: 1, seq: 1, component: { Portal: portal } }] }) as Extract<
      Event,
      { type: "entities" }
    >;
    // World comes before Url in the fixed order
    expect(e.changes[0]!.component as unknown).toEqual({ Portal: { ...portal, to: { World: { world: "yard" } } } });
  });

  test("an extra key inside a destination is dropped from the result", () => {
    const portal = portalSample();
    portal.to = { Url: { url: ":7780", arrive: [0, 0, 0], extra: true } };
    portal.surplus = 1;
    const e = readEvent({ type: "entities", changes: [{ entity: 1, seq: 1, component: { Portal: portal } }] }) as Extract<
      Event,
      { type: "entities" }
    >;
    const got = (e.changes[0]!.component as unknown as { Portal: Record<string, unknown> }).Portal;
    expect(got.to).toEqual({ Url: { url: ":7780", arrive: [0, 0, 0] } });
    expect("surplus" in got).toBe(false);
  });

  test("a component or destination with only unknown tags filters the change", () => {
    const portal = portalSample();
    portal.to = { Teleporter: { x: 1 } };
    const bad = [
      { entity: 1, seq: 1, component: { Prop: {} } },
      { entity: 2, seq: 2, component: { Portal: portal } },
      { entity: 3, seq: 3, component: { Spawn: { at: [0, 0] , yaw: 0 } } },
    ];
    expect(readEvent({ type: "entities", changes: bad })).toEqual({ type: "entities", changes: [] });
  });

  test("done and refused with a ref round trip", () => {
    expect(readEvent({ type: "done", ref: 4, entity: 3, seq: 12, extra: 1 })).toEqual({ type: "done", ref: 4, entity: 3, seq: 12 });
    expect(readEvent({ type: "done", ref: 4, entity: 3 })).toBeNull();
    expect(readEvent({ type: "refused", id: 7, ref: 4, why: "TooFar", text: "x" })).toEqual({
      type: "refused", id: 7, ref: 4, why: "TooFar", text: "x",
    });
    expect(readEvent({ type: "refused", id: 7, ref: null, why: "TooFar", text: "x" })).toEqual({
      type: "refused", id: 7, ref: null, why: "TooFar", text: "x",
    });
    // older clients send no ref at all
    expect(readEvent({ type: "refused", id: 7, why: "TooFar", text: "x" })).toEqual({
      type: "refused", id: 7, ref: null, why: "TooFar", text: "x",
    });
    expect(readEvent({ type: "refused", id: 7, ref: "4", why: "TooFar", text: "x" })).toBeNull();
  });

  test("a command may carry a ref, or none", () => {
    const base = { type: "remove", entity: 3, expect: null };
    expect(isCommand(base)).toBe(true);
    expect(isCommand({ ...base, ref: 5 })).toBe(true);
    expect(isCommand({ ...base, ref: null })).toBe(true);
    expect(isCommand({ ...base, ref: -1 })).toBe(false);
    expect(isCommand({ ...base, ref: 5, extra: 1 })).toBe(false);
  });

  test("a prop spot is a prop's hash, its place in 5 cm units and its turn; a bad one is refused", () => {
    const hash = "0102030405060708090a0b0c0d0e0f10";
    const spot: Event = { type: "prop_spot", prop: hash, at: [25, 30, -12], yaw: 3 };
    expect(readEvent(spot)).toEqual(spot);
    expect(readEvent({ ...spot, extra: 1 })).toEqual(spot);
    expect(readEvent({ ...spot, at: [25.5, 30, -12] })).toBeNull();
    expect(readEvent({ ...spot, yaw: 256 })).toBeNull();
    expect(readEvent({ ...spot, prop: "nothex" })).toBeNull();
    expect(readEvent({ type: "prop_spot", prop: hash, at: [0, 0, 0] })).toBeNull();
  });

  test("an entities batch with a kind this page does not know and a Portal yields only the Portal change", () => {
    const portal = (samples.commands.find((c) => (c as { type: string }).type === "create") as {
      component: Component;
    }).component;
    const unknown = { entity: 1, seq: 1, component: { Node: { kind: "door" } } };
    const kept = { entity: 2, seq: 2, component: portal };
    const batch = { type: "entities", changes: [unknown, kept] };
    expect(isEvent(batch)).toBe(true);
    expect(readEvent(batch)).toEqual({ type: "entities", changes: [kept] });
  });

  test("a Prop change is read, and an owner that is not a safe integer is read as null", () => {
    const prop = {
      prop: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
      at: [25, 30, -12] as [number, number, number],
      yaw: 3,
      variant: 1,
      state: 0,
      channel: null,
      text: "Keep out",
      owner: 9007199254740993,
    };
    const batch = { type: "entities", changes: [{ entity: 4, seq: 7, component: { Prop: prop } }] };
    expect(readEvent(batch)).toEqual({
      type: "entities",
      changes: [{ entity: 4, seq: 7, component: { Prop: { ...prop, owner: null } } }],
    });
    expect(readEvent({ ...batch, changes: [{ entity: 4, seq: 7, component: { Prop: { ...prop, owner: 12 } } }] }))
      .toEqual({ type: "entities", changes: [{ entity: 4, seq: 7, component: { Prop: { ...prop, owner: 12 } } }] });
  });

  test("a Prop with a bad field is filtered out of the batch", () => {
    const good = {
      prop: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
      at: [0, 0, 0],
      yaw: 0,
      variant: 0,
      state: 0,
      channel: null,
      text: null,
      owner: null,
    };
    const bad = [
      { ...good, yaw: 24 },
      { ...good, variant: 8 },
      { ...good, state: 8 },
      { ...good, channel: 65536 },
      { ...good, text: "a\u0007b" },
      { ...good, text: "a".repeat(141) },
      { ...good, at: [0.5, 0, 0] },
      { ...good, prop: [1, 2, 3] },
    ];
    for (const b of bad) {
      expect(readEvent({ type: "entities", changes: [{ entity: 1, seq: 1, component: { Prop: b } }] })).toEqual({
        type: "entities",
        changes: [],
      });
    }
  });
});

describe("Prop command guard", () => {
  const prop = {
    prop: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    at: [25, 30, -12],
    yaw: 23,
    variant: 7,
    state: 7,
    channel: 65535,
    text: "a".repeat(140),
    owner: null,
  };

  test("a create with a Prop is a command; its fields must be exact and in range", () => {
    expect(isCommand({ type: "create", component: { Prop: prop }, ref: 1 })).toBe(true);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, text: null, channel: null, owner: 5 } } })).toBe(true);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, extra: 1 } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, yaw: 24 } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, variant: 8 } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, state: 8 } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, channel: 65536 } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, text: "a".repeat(141) } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, text: "tab\there" } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, owner: 9007199254740993 } } })).toBe(false);
    expect(isCommand({ type: "create", component: { Prop: { ...prop, prop: [1, 2] } } })).toBe(false);
  });
});

describe("onEvent", () => {
  type Win = { vox_editor?: (json: string) => void };
  const g = globalThis as unknown as { window?: Win };

  test("delivers events, ignores junk, and the disposer clears only its own handler", () => {
    const saved = g.window;
    g.window = {};
    try {
      const first: Event[] = [];
      const second: Event[] = [];
      const dispose1 = onEvent((e) => first.push(e));
      g.window.vox_editor!(JSON.stringify({ type: "selected", entity: 3 }));
      g.window.vox_editor!("not json");
      g.window.vox_editor!(JSON.stringify({ type: "explode" }));
      expect(first).toEqual([{ type: "selected", entity: 3 }]);

      // a later handler replaces the first; the first's disposer must not undo it
      const dispose2 = onEvent((e) => second.push(e));
      dispose1();
      expect(g.window.vox_editor).toBeDefined();
      g.window.vox_editor!(JSON.stringify({ type: "done", ref: 1, entity: 2, seq: 3 }));
      expect(second).toEqual([{ type: "done", ref: 1, entity: 2, seq: 3 }]);
      expect(first.length).toBe(1);

      dispose2();
      expect(g.window.vox_editor).toBeUndefined();
    } finally {
      g.window = saved;
    }
  });
});

describe("no raw HTML in the editor", () => {
  const folder = fileURLToPath(new URL("../features/editor", import.meta.url));
  const RAW_HTML = /innerHTML|outerHTML|insertAdjacentHTML|document\.write|srcdoc|dangerouslySetInnerHTML/;

  function files(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name);
      return entry.isDirectory() ? files(path) : [path];
    });
  }

  /** The files under a folder that set HTML from a string. */
  function rawHtmlHits(dir: string): string[] {
    return files(dir).filter((path) => RAW_HTML.test(readFileSync(path, "utf8")));
  }

  test("nothing in features/editor sets HTML from a string", () => {
    // vox-tol.3.8 creates the folder; until then there is nothing to check
    if (!existsSync(folder)) return;
    expect(rawHtmlHits(folder)).toEqual([]);
  });

  test("the scanner catches each raw-HTML pattern", () => {
    for (const s of ["x.innerHTML = a", "x.outerHTML", "el.insertAdjacentHTML(", "document.write(", "<iframe srcdoc=", "dangerouslySetInnerHTML"]) {
      expect(RAW_HTML.test(s)).toBe(true);
    }
  });

  test("the scanner finds a planted file and passes a clean one", () => {
    const dir = mkdtempSync(join(tmpdir(), "editor-scan-"));
    try {
      writeFileSync(join(dir, "bad.tsx"), 'el.innerHTML = "<b>x</b>";\n');
      mkdirSync(join(dir, "nested"));
      writeFileSync(join(dir, "nested", "ok.tsx"), 'el.textContent = "x";\n');
      expect(rawHtmlHits(dir).map((p) => basename(p))).toEqual(["bad.tsx"]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
