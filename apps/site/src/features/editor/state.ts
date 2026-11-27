// The editor page's picture of the world, built from the client's events: the entity list
// (kept by seq, so a stale change never overwrites a newer one), the selection, who is here,
// and the latest refusal. Pure functions, so the page's state stays easy to reason about.
// A removed entity leaves a tombstone (its id and the seq of the removal), so a stale change
// from before the removal, arriving late, cannot bring it back.

import type { Item } from "../../api/types";
import type { Axis, Change, Command, Component, Dest, Event, Objective, Portal, Spawn, Vec3 } from "../../game/editor";

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

// ---- Edits in flight -------------------------------------------------------------------------
// The page numbers each edit it sends with a `ref`, and keeps the command until the client
// answers. A `done` takes the edit out; a refusal keeps it with the reason, so its form can show
// it. A refusal of `Unavailable` can be retried: the same command, with the same `ref`, so the
// client reuses its edit id and nothing is charged twice.

/** One edit the page has sent. */
export interface Edit {
  ref: number;
  command: Command;
  status: "sent" | "refused";
  /** the client's reason ("Unavailable", "Changed", ...) when refused */
  why?: string;
  /** the text to show the author when refused */
  text?: string;
}

/** The open edits, by `ref`. An edit that is done is not here. */
export type Edits = ReadonlyMap<number, Edit>;

/** Records an edit as sent. The send itself is the caller's. */
export function sentEdit(edits: Edits, ref: number, command: Command): Edits {
  return new Map(edits).set(ref, { ref, command, status: "sent" });
}

/** Records that an edit was refused, with the reason and its text. Unknown refs are ignored. */
export function refuseEdit(edits: Edits, ref: number, why: string, text: string): Edits {
  const cur = edits.get(ref);
  if (!cur) return edits;
  return new Map(edits).set(ref, { ...cur, status: "refused", why, text });
}

/** Applies the client's answer to an edit: `done` takes it out, a refusal records it. Events
 *  for refs that aren't open edits are left alone. */
export function settleEdit(edits: Edits, e: Event): Edits {
  if (e.type === "done") {
    if (!edits.has(e.ref)) return edits;
    const next = new Map(edits);
    next.delete(e.ref);
    return next;
  }
  if (e.type === "refused" && e.ref !== null) return refuseEdit(edits, e.ref, e.why, e.text);
  return edits;
}

/** The command a retry resends (the same command, the same ref), or undefined when the edit
 *  isn't refused as Unavailable. */
export function retryCommand(edits: Edits, ref: number): Command | undefined {
  const e = edits.get(ref);
  return e?.status === "refused" && e.why === "Unavailable" ? e.command : undefined;
}

/** Marks a refused edit as sent again, before its retry goes out. */
export function resendEdit(edits: Edits, ref: number): Edits {
  const cur = edits.get(ref);
  if (!cur) return edits;
  return new Map(edits).set(ref, { ref: cur.ref, command: cur.command, status: "sent" });
}

/** Lets go of an edit (its form was closed or sent again). */
export function dropEdit(edits: Edits, ref: number): Edits {
  if (!edits.has(ref)) return edits;
  const next = new Map(edits);
  next.delete(ref);
  return next;
}

// ---- Portal forms ----------------------------------------------------------------------------
// The inspector and the new-portal form hold their fields as text, and turn them into a Portal
// only when every number is finite. Destinations are the same in both.

/** A portal's yaw is one of the four quarter turns. */
export const YAWS = [0, 90, 180, 270] as const;

/** Where a doorway leads, as the form's kind. */
export type DestKind = "World" | "Pair" | "Here" | "Url" | "Dungeon";

/** The destinations a Basic editor may choose; Advanced adds the rest. */
const BASIC_KINDS: DestKind[] = ["World", "Pair"];
const ADVANCED_KINDS: DestKind[] = ["World", "Pair", "Here", "Url", "Dungeon"];

/** The destination kinds to offer: the mode's, and the current one even when the mode lacks it
 *  (so an existing doorway shows what it is). */
export function destKinds(advanced: boolean, current: DestKind): DestKind[] {
  const kinds = advanced ? ADVANCED_KINDS : BASIC_KINDS;
  return kinds.includes(current) ? kinds : [...kinds, current];
}

export interface DestForm {
  kind: DestKind;
  /** a world's id (World) */
  world: string;
  /** another portal's key, as text (Pair) */
  key: string;
  /** the address (Url) */
  url: string;
  /** the dungeon's name (Dungeon) */
  dungeon: string;
  /** where arrivals land (Here, Url, Dungeon), as text */
  arrive: [string, string, string];
}

export interface PortalForm {
  name: string;
  /** the middle of the sill, as text */
  at: [string, string, string];
  axis: Axis;
  /** degrees, as the select's text */
  yaw: string;
  /** width and height, as text */
  size: [string, string];
  dest: DestForm;
}

/** A new portal's place, from the Portal tool's `spot` event. */
export interface Spot {
  at: Vec3;
  axis: Axis;
  size: [number, number];
}

/** A check's answer: the value, or a sentence to show the author. */
export type Built<T> = { ok: true; value: T } | { ok: false; error: string };

const fail = (error: string): { ok: false; error: string } => ({ ok: false, error });

/** A number from a text field, or null when it isn't a finite number. */
export function numberOf(s: string): number | null {
  const t = s.trim();
  if (t === "") return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

/** A position as three text fields. */
export const textOf = (v: Vec3): [string, string, string] => [String(v[0]), String(v[1]), String(v[2])];

function vecOf(v: readonly string[], what: string): Built<Vec3> {
  const n = v.map(numberOf);
  const [x, y, z] = n;
  if (x === null || x === undefined || y === null || y === undefined || z === null || z === undefined) {
    return fail(`${what} needs three numbers.`);
  }
  return { ok: true, value: [x, y, z] };
}

/** The form for a destination. `arrive` is the default for the kinds that need one. */
export function destFormOf(d: Dest, arrive: Vec3): DestForm {
  const base: DestForm = { kind: "World", world: "", key: "", url: "", dungeon: "", arrive: textOf(arrive) };
  if ("World" in d) return { ...base, kind: "World", world: d.World.world };
  if ("Pair" in d) return { ...base, kind: "Pair", key: String(d.Pair.key) };
  if ("Here" in d) return { ...base, kind: "Here", arrive: textOf(d.Here.arrive) };
  if ("Url" in d) return { ...base, kind: "Url", url: d.Url.url, arrive: textOf(d.Url.arrive) };
  return { ...base, kind: "Dungeon", dungeon: d.Dungeon.name, arrive: textOf(d.Dungeon.arrive) };
}

/** A destination from its form. */
export function destOf(f: DestForm): Built<Dest> {
  switch (f.kind) {
    case "World":
      return f.world ? { ok: true, value: { World: { world: f.world } } } : fail("Choose a world.");
    case "Pair": {
      const key = numberOf(f.key);
      return key !== null && Number.isSafeInteger(key) && key >= 0
        ? { ok: true, value: { Pair: { key } } }
        : fail("Choose one of your portals.");
    }
    case "Here": {
      const arrive = vecOf(f.arrive, "Arrival");
      return arrive.ok ? { ok: true, value: { Here: { arrive: arrive.value } } } : arrive;
    }
    case "Url": {
      // the address is checked by the world (':port' or '/w/<id>/ws'), which says why it refuses
      const url = f.url.trim();
      if (!url) return fail("Give the address to lead to.");
      const arrive = vecOf(f.arrive, "Arrival");
      return arrive.ok ? { ok: true, value: { Url: { url, arrive: arrive.value } } } : arrive;
    }
    case "Dungeon": {
      const name = f.dungeon.trim();
      if (!name) return fail("Give the dungeon's name.");
      if (!DUNGEON_NAME.test(name)) return fail("A dungeon's name is 1 to 64 of a-z, 0-9, _ and -.");
      const arrive = vecOf(f.arrive, "Arrival");
      return arrive.ok ? { ok: true, value: { Dungeon: { name, arrive: arrive.value } } } : arrive;
    }
  }
}

/** A dungeon's name, as the server checks it (`vox-core::edit`): 1 to 64 of a-z, 0-9, _ and -. */
const DUNGEON_NAME = /^[a-z0-9_-]{1,64}$/;

/** The longest portal name, in characters (`vox-core::entity::MAX_NAME`). */
export const MAX_PORTAL_NAME = 32;

/** The size a Basic builder may give a portal: width and height, each a range (inclusive). */
export const BASIC_WIDTH: readonly [number, number] = [4, 8];
export const BASIC_HEIGHT: readonly [number, number] = [6, 16];

/** The size bounds as a hint for the form. */
export const BASIC_SIZE_HINT = `${BASIC_WIDTH[0]} to ${BASIC_WIDTH[1]} wide, ${BASIC_HEIGHT[0]} to ${BASIC_HEIGHT[1]} tall`;

/** The form for an existing portal. */
export function portalFormOf(p: Portal): PortalForm {
  return {
    name: p.name,
    at: textOf(p.at),
    axis: p.axis,
    yaw: String(p.yaw),
    size: [String(p.size[0]), String(p.size[1])],
    dest: destFormOf(p.to, p.at),
  };
}

/** The form for a new portal at a spot, with no destination chosen yet. */
export function newPortalForm(spot: Spot): PortalForm {
  return {
    name: "",
    at: textOf(spot.at),
    axis: spot.axis,
    yaw: "0",
    size: [String(spot.size[0]), String(spot.size[1])],
    dest: destFormOf({ World: { world: "" } }, spot.at),
  };
}

/** A portal from its form. `level` and `key` come from what the portal already is (or the new
 *  portal's world, and no key). A Basic builder's size is checked against the Basic bounds here,
 *  before anything is sent; an Advanced builder's size is left to the world's own checks. `yaw` is
 *  the turn the portal has now: Apply keeps it even when it is not a quarter turn, so an edit that
 *  does not touch the turn does not change it. */
export function portalOf(
  f: PortalForm,
  base: { level: string; key: number | null; yaw?: number },
  advanced: boolean,
): Built<Portal> {
  const name = f.name.trim();
  if (!name) return fail("Give the portal a name.");
  if ([...name].length > MAX_PORTAL_NAME) return fail(`The name is at most ${MAX_PORTAL_NAME} characters.`);
  const at = vecOf(f.at, "Position");
  if (!at.ok) return at;
  const w = numberOf(f.size[0]);
  const h = numberOf(f.size[1]);
  if (w === null || h === null) return fail("Size needs two numbers.");
  if (!advanced && (w < BASIC_WIDTH[0] || w > BASIC_WIDTH[1] || h < BASIC_HEIGHT[0] || h > BASIC_HEIGHT[1])) {
    return fail(`In Basic a portal is ${BASIC_SIZE_HINT}.`);
  }
  const yaw = numberOf(f.yaw);
  // a finite number only (numberOf); a quarter turn, or the turn the portal already has
  const kept = yaw !== null && yaw === base.yaw;
  if (yaw === null || (!(YAWS as readonly number[]).includes(yaw) && !kept)) {
    return fail("Turn it to 0, 90, 180 or 270 degrees.");
  }
  const to = destOf(f.dest);
  if (!to.ok) return to;
  return {
    ok: true,
    value: { name, at: at.value, axis: f.axis, size: [w, h], yaw, level: base.level, to: to.value, key: base.key },
  };
}

/** What an editor may change on a portal. A player's portal (keyed) is read-only to an Advanced
 *  editor, who may not change other people's portals; its name is read-only to everyone. A level
 *  file's portal (keyless) is read-only to a Basic builder: the server refuses Builders any keyless
 *  portal (`vox-core::edit`). */
export function rightsFor(advanced: boolean, keyed: boolean): { editable: boolean; nameEditable: boolean } {
  if (advanced ? keyed : !keyed) return { editable: false, nameEditable: false };
  return { editable: true, nameEditable: !keyed };
}

/** A portal the player may pair with: one of their keys, without this portal's own key. */
export interface PortalKey {
  key: number;
  label: string;
}

/** The player's portal keys from the inventory, leaving out `exclude` (this portal's own key). */
export function portalKeys(items: readonly Item[], exclude: number | null): PortalKey[] {
  const out: PortalKey[] = [];
  for (const i of items) {
    if (i.kind !== "key" || typeof i.ref !== "number" || i.ref === exclude) continue;
    out.push({ key: i.ref, label: i.world_name ? `${i.name} (in ${i.world_name})` : i.name });
  }
  return out;
}

/** An inspector's form after a reload: its fields from the portal as it now is, and the seq it
 *  now expects (the seq the entity has now, so the next edit is not refused as Changed). */
export function reloaded(portal: Portal, seq: number): { form: PortalForm; expect: number } {
  return { form: portalFormOf(portal), expect: seq };
}

/** The line a refused edit shows: a Changed refusal asks for a reload; any other shows the
 *  client's text. */
export function refusalLine(e: Edit): string {
  return e.why === "Changed" ? "Changed by someone else — reload to see it." : (e.text ?? "");
}

/** The command that creates a portal. */
export const createPortal = (portal: Portal, ref: number): Command => ({
  type: "create",
  component: { Portal: portal },
  ref,
});

/** The command that changes an entity's portal, expecting the seq the author saw. */
export const setPortal = (entity: number, portal: Portal, expect: number, ref: number): Command => ({
  type: "set",
  entity,
  component: { Portal: portal },
  expect,
  ref,
});

/** The command that removes an entity, expecting the seq the author saw. */
export const removePortal = (entity: number, expect: number, ref: number): Command => ({
  type: "remove",
  entity,
  expect,
  ref,
});

// ---- Rows by kind ----------------------------------------------------------------------------

/** The kinds an advanced panel edits. */
export type PanelKind = "Spawn" | "Objective" | "Settings";

/** A row whose component is of kind K, so its data is typed. */
export type RowOf<K extends "Portal" | PanelKind> = Row & { component: Extract<Component, Record<K, unknown>> };

/** The first row of a kind (a world has at most one Settings and, by the editor's forms, one
 *  Spawn and one Objective per click), or undefined. */
export function rowOfKind<K extends "Portal" | PanelKind>(entities: Entities, kind: K): RowOf<K> | undefined {
  for (const r of entities.rows.values()) if (kind in r.component) return r as RowOf<K>;
  return undefined;
}

/** The command that adds an entity. */
export const createEntity = (component: Component, ref: number): Command => ({ type: "create", component, ref });

/** The command that replaces an entity's component, expecting the seq the author saw. */
export const setEntity = (entity: number, component: Component, expect: number, ref: number): Command => ({
  type: "set",
  entity,
  component,
  expect,
  ref,
});

/** The command that removes an entity, expecting the seq the author saw. */
export const removeEntity = (entity: number, expect: number, ref: number): Command => ({
  type: "remove",
  entity,
  expect,
  ref,
});

// ---- World settings, spawn and objective -----------------------------------------------------
// Forms for the Settings entity (the world's level JSON), the Spawn and the Objective. As the
// portal forms, each turns its text into a value only when every number is finite and in range.
// The settings form edits a copy of the JSON it opened on, so every key it does not show (portals,
// cameras, anything unknown) is written back as it was.

/** A Settings entity's JSON, at most this many bytes (`vox-core::entity::MAX_SETTINGS`). */
export const MAX_SETTINGS_BYTES = 16 * 1024;
/** An objective's radius, in blocks, at most this (`vox-core::entity::MAX_RADIUS`). */
export const MAX_RADIUS = 256;
/** The hotbar holds this many blocks at most. */
export const MAX_HOTBAR = 9;
/** The weather value that means "an object written in the text field". */
export const CUSTOM_WEATHER = "custom";

/** A level's settings: a JSON object. */
export type Json = Record<string, unknown>;

const isJsonObj = (v: unknown): v is Json => typeof v === "object" && v !== null && !Array.isArray(v);
const isFiniteNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isStr = (v: unknown): v is string => typeof v === "string";
/** A number as a text field shows it, or empty when it is not one. */
const numText = (v: unknown): string => (isFiniteNum(v) ? String(v) : "");
const colourText = (v: unknown): [string, string, string] => {
  const c: string[] = Array.isArray(v) && v.length === 3 ? v.map(numText) : ["", "", ""];
  return [c[0] ?? "", c[1] ?? "", c[2] ?? ""];
};

/** A Settings entity's JSON as an object, or a sentence to show when it is not one. */
export function settingsOf(json: string): Built<Json> {
  let v: unknown;
  try {
    v = JSON.parse(json);
  } catch {
    return fail("The world's settings are not valid JSON.");
  }
  return isJsonObj(v) ? { ok: true, value: v } : fail("The world's settings are not an object.");
}

/** The four ways off an atlas tile, in the order the form lists them (`vox-core::terrain::EDGES`). */
export type Edge = "east" | "west" | "south" | "north";
const EDGE_STEPS: readonly [Edge, number, number][] = [
  ["east", 1, 0],
  ["west", -1, 0],
  ["south", 0, 1],
  ["north", 0, -1],
];

/** The edges an atlas tile can have: those whose next tile lies inside the map. */
export function possibleEdges(tile: readonly [number, number], size: readonly [number, number]): Edge[] {
  return EDGE_STEPS.filter(([, dx, dz]) => {
    const i = tile[0] + dx;
    const j = tile[1] + dz;
    return i >= 0 && j >= 0 && i < size[0] && j < size[1];
  }).map(([e]) => e);
}

/** Opens or closes an edge of a tile. Opening an edge the tile cannot have changes nothing. The
 *  list keeps the order east, west, south, north, and any other names it holds stay after them. */
export function withEdge(edges: readonly string[], edge: Edge, open: boolean, possible: readonly Edge[]): string[] {
  if (open && !possible.includes(edge)) return [...edges];
  const names = new Set(edges);
  if (open) names.add(edge);
  else names.delete(edge);
  const known = EDGE_STEPS.map(([e]) => e).filter((e) => names.has(e));
  return [...known, ...edges.filter((e) => !EDGE_STEPS.some(([k]) => k === e))];
}

const isPair = (v: unknown): v is [number, number] => Array.isArray(v) && v.length === 2 && v.every(isFiniteNum);

/** The atlas tile a settings object names (its place and the map's size), or null when it names none. */
export function atlasOf(obj: Json): { tile: [number, number]; size: [number, number] } | null {
  const a = obj.atlas;
  if (!isJsonObj(a) || !isPair(a.tile) || !isPair(a.size)) return null;
  return { tile: a.tile, size: a.size };
}

/** The settings form: what the panel shows and edits. Every field is text, read by `settingsJsonOf`. */
export interface SettingsForm {
  sky: string;
  /** hours, as text */
  time: string;
  /** a preset's name, CUSTOM_WEATHER (the JSON below), or "" when the world sets none */
  weather: string;
  weatherJson: string;
  clouds: string;
  /** the relief amount, as text (the rest of `relief` is kept) */
  relief: string;
  /** null when the world has no water */
  water: { colour: [string, string, string]; swell: string; opacity: string } | null;
  hotbar: string[];
  /** the atlas edges that lead on (checked in the panel only on an atlas tile) */
  edges: string[];
}

/** The form for a settings object. */
export function settingsFormOf(obj: Json): SettingsForm {
  const weather = obj.weather;
  const water = obj.water;
  const relief = isJsonObj(obj.relief) ? obj.relief : {};
  const atlas = isJsonObj(obj.atlas) ? obj.atlas : {};
  // the edges the tile can have; a stored name it cannot have is left out (the server refuses it)
  const tile = atlasOf(obj);
  const possible: readonly string[] = tile ? possibleEdges(tile.tile, tile.size) : [];
  // an atlas with no `edges` key has every edge open (the server reads it so), so the form shows them all
  return {
    sky: isStr(obj.sky) ? obj.sky : "",
    time: numText(obj.time),
    weather: isStr(weather) ? weather : isJsonObj(weather) ? CUSTOM_WEATHER : "",
    weatherJson: isJsonObj(weather) ? JSON.stringify(weather, null, 2) : "",
    clouds: numText(obj.clouds),
    relief: numText(relief.amount),
    water: isJsonObj(water)
      ? { colour: colourText(water.colour), swell: numText(water.swell), opacity: numText(water.opacity) }
      : null,
    hotbar: Array.isArray(obj.hotbar) ? obj.hotbar.filter(isStr) : [],
    // without a tile and size the possible edges are unknown, so stored ones are kept as they are;
    // a repeated name is kept once (the server refuses repeats)
    edges: Array.isArray(atlas.edges)
      ? [...new Set(atlas.edges.filter((e): e is string => isStr(e) && (tile === null || possible.includes(e))))]
      : [...possible],
  };
}

/** Whether two edge lists hold the same names (order aside). */
const sameNames = (a: readonly string[], b: readonly string[]): boolean =>
  a.length === b.length && a.every((x) => b.includes(x));

/** A setting's number from its text: null when empty (its key is then left out), or a sentence
 *  when it is not a number in range. */
function optNumber(s: string, what: string, lo: number, hi: number): Built<number | null> {
  if (s.trim() === "") return { ok: true, value: null };
  const n = numberOf(s);
  if (n === null) return fail(`${what} needs a number.`);
  if (n < lo || n > hi) return fail(`${what} is ${lo} to ${hi}.`);
  return { ok: true, value: n };
}

/** The settings JSON after the form's edits: a copy of `base` with the form written over it, every
 *  key the form does not show kept as it was, and an empty field leaving its key out. Refused when
 *  the text is over MAX_SETTINGS_BYTES. The ranges are the world's own: it clamps the water's swell
 *  to 0 to 3 and its opacity to 0 to 1 (`vox-world::outdoors::water_look`). */
export function settingsJsonOf(base: Json, f: SettingsForm): Built<string> {
  const out: Json = { ...base };
  const put = (key: string, v: unknown): void => {
    if (v === null) delete out[key];
    else out[key] = v;
  };

  const sky = f.sky.trim();
  put("sky", sky === "" ? null : sky);

  const time = optNumber(f.time, "Time", 0, 24);
  if (!time.ok) return time;
  put("time", time.value);

  if (f.weather === "") {
    put("weather", null);
  } else if (f.weather === CUSTOM_WEATHER) {
    let w: unknown;
    try {
      w = JSON.parse(f.weatherJson);
    } catch {
      return fail("The custom weather must be JSON.");
    }
    if (!isJsonObj(w)) return fail("The custom weather must be a JSON object.");
    put("weather", w);
  } else {
    put("weather", f.weather);
  }

  const clouds = optNumber(f.clouds, "Clouds", 0, 1);
  if (!clouds.ok) return clouds;
  put("clouds", clouds.value);

  const amount = optNumber(f.relief, "Relief", 0, 1);
  if (!amount.ok) return amount;
  if (amount.value === null) {
    if (isJsonObj(base.relief)) {
      const r = { ...base.relief };
      delete r.amount;
      out.relief = r;
    }
  } else {
    out.relief = { ...(isJsonObj(base.relief) ? base.relief : {}), amount: amount.value };
  }

  if (f.water === null) {
    put("water", null);
  } else {
    const colour: number[] = [];
    for (const s of f.water.colour) {
      const n = numberOf(s);
      if (n === null || n < 0 || n > 1) return fail("The water's colour is three numbers from 0 to 1.");
      colour.push(n);
    }
    const swell = numberOf(f.water.swell);
    if (swell === null || swell < 0 || swell > 3) return fail("The water's swell is 0 to 3 metres.");
    const opacity = numberOf(f.water.opacity);
    if (opacity === null || opacity < 0 || opacity > 1) return fail("The water's opacity is 0 to 1.");
    out.water = { ...(isJsonObj(base.water) ? base.water : {}), colour, swell, opacity };
  }

  if (f.hotbar.length > MAX_HOTBAR) return fail(`The hotbar holds ${MAX_HOTBAR} blocks at most.`);
  put("hotbar", f.hotbar.length === 0 ? null : [...f.hotbar]);

  // an atlas with no `edges` key keeps none unless the form changed the edges: a written [] would
  // close every edge, where the server reads a missing key as all open
  if (isJsonObj(base.atlas)) {
    const atlas: Json = { ...base.atlas };
    if (Array.isArray(base.atlas.edges) || !sameNames(f.edges, settingsFormOf(base).edges)) atlas.edges = [...f.edges];
    out.atlas = atlas;
  }

  const text = JSON.stringify(out);
  const bytes = new TextEncoder().encode(text).length;
  if (bytes > MAX_SETTINGS_BYTES) return fail(`The settings are ${bytes} bytes; the most is ${MAX_SETTINGS_BYTES}.`);
  return { ok: true, value: text };
}

/** A spawn's form: where a player starts (feet, blocks) and the turn in degrees. */
export interface SpawnForm {
  at: [string, string, string];
  yaw: string;
}

/** The form for a spawn; a new one starts at the marked spot, or at the origin. */
export function spawnFormOf(s: Spawn | undefined, spot: Spot | undefined): SpawnForm {
  if (s) return { at: textOf(s.at), yaw: String(s.yaw) };
  return { at: textOf(spot?.at ?? [0, 0, 0]), yaw: "0" };
}

/** A spawn from its form. */
export function spawnOf(f: SpawnForm): Built<Spawn> {
  const at = vecOf(f.at, "Position");
  if (!at.ok) return at;
  const yaw = numberOf(f.yaw);
  if (yaw === null) return fail("Turn needs a number of degrees.");
  return { ok: true, value: { at: at.value, yaw } };
}

/** An objective's form: its name, where it is (feet, blocks) and how near counts (blocks). */
export interface ObjectiveForm {
  name: string;
  at: [string, string, string];
  radius: string;
}

/** The form for an objective; a new one starts at the marked spot, with no radius chosen. */
export function objectiveFormOf(o: Objective | undefined, spot: Spot | undefined): ObjectiveForm {
  if (o) return { name: o.name, at: textOf(o.at), radius: String(o.radius) };
  return { name: "", at: textOf(spot?.at ?? [0, 0, 0]), radius: "" };
}

/** An objective from its form. The name is as long as a portal's name (MAX_PORTAL_NAME). */
export function objectiveOf(f: ObjectiveForm): Built<Objective> {
  const name = f.name.trim();
  if (!name) return fail("Give the objective a name.");
  if ([...name].length > MAX_PORTAL_NAME) return fail(`The name is at most ${MAX_PORTAL_NAME} characters.`);
  const at = vecOf(f.at, "Position");
  if (!at.ok) return at;
  const radius = numberOf(f.radius);
  if (radius === null || radius <= 0 || radius > MAX_RADIUS) {
    return fail(`The radius is more than 0 and at most ${MAX_RADIUS} blocks.`);
  }
  return { ok: true, value: { name, at: at.value, radius } };
}

