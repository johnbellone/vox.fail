// The editor page's picture of the world, built from the client's events: the entity list
// (kept by seq, so a stale change never overwrites a newer one), the selection, who is here,
// and the latest refusal. Pure functions, so the page's state stays easy to reason about.
// A removed entity leaves a tombstone (its id and the seq of the removal), so a stale change
// from before the removal, arriving late, cannot bring it back.

import type { Item } from "../../api/types";
import type { Axis, Change, Command, Component, Dest, Event, Portal, Vec3 } from "../../game/editor";

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

const textOf = (v: Vec3): [string, string, string] => [String(v[0]), String(v[1]), String(v[2])];

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
 *  before anything is sent; an Advanced builder's size is left to the world's own checks. */
export function portalOf(f: PortalForm, base: { level: string; key: number | null }, advanced: boolean): Built<Portal> {
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
  if (yaw === null || !(YAWS as readonly number[]).includes(yaw)) return fail("Turn it to 0, 90, 180 or 270 degrees.");
  const to = destOf(f.dest);
  if (!to.ok) return to;
  return {
    ok: true,
    value: { name, at: at.value, axis: f.axis, size: [w, h], yaw, level: base.level, to: to.value, key: base.key },
  };
}

/** What an editor may change on a portal. A player's portal is read-only to an Advanced editor
 *  (they cannot change other people's portals); its name is read-only to everyone. */
export function rightsFor(advanced: boolean, keyed: boolean): { editable: boolean; nameEditable: boolean } {
  if (keyed && advanced) return { editable: false, nameEditable: false };
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

