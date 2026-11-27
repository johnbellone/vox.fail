// The editor bridge on the page's side: the TypeScript mirror of `vox-world::editor::bridge`
// (Command and Event, serde, `#[serde(tag = "type")]`) and of the components they carry
// (`vox-core::entity`). editor.samples.json is the written record of one sample of every variant;
// editor.test.ts parses it against these guards, so the two can't drift silently.
//
// Numbers: ids and sequence numbers are JSON numbers, exact below 2^53 (they stay well under).
// Optional Rust fields (`Option<T>`) are always sent, as null when empty, so every field here is
// required in the guards.
//
// Strict and lenient: the page's commands are checked strictly (exactly the keys the bridge
// defines, and exactly one tag in a component or destination: the bridge is ours, so an extra
// field is a bug). The client's events are read leniently and normalised: extra fields are
// dropped, and in a component or destination holding several tags the first known one in a fixed
// order is kept and the rest are dropped. A change whose component has no tag this page knows
// (a later phase's Prop, say) is filtered out while the rest of its batch still applies.
//
// `ref` (create, set, remove, and on refused and done): a number the page picks for an edit. The
// client answers with `done` when the server applied it, or `refused` with the same `ref`.
// Sending the same `ref` again after an Unavailable refusal is a retry, never charged twice.
// Options: `ref` may be absent on a command (the edit is then never reported as done), and on
// a refused event (a refusal the client made itself, or for an edit sent without one).

import { editorCommand } from "./wasm";

export type Vec3 = [number, number, number];
export type Tool = "select" | "blocks" | "portal" | "prop" | "node";
export type Axis = "X" | "Z";

/** Where a doorway leads (`vox-core::entity::Dest`, externally tagged). */
export type Dest =
  | { Here: { arrive: Vec3 } }
  | { World: { world: string } }
  | { Pair: { key: number } }
  | { Dungeon: { name: string; arrive: Vec3 } }
  | { Url: { url: string; arrive: Vec3 } };

export interface Portal {
  name: string;
  /** the middle of the sill, blocks */
  at: Vec3;
  axis: Axis;
  /** width and height, blocks */
  size: [number, number];
  /** degrees */
  yaw: number;
  level: string;
  to: Dest;
  key: number | null;
}

export interface Spawn {
  /** feet, blocks */
  at: Vec3;
  /** degrees */
  yaw: number;
}

export interface Objective {
  name: string;
  /** feet, blocks */
  at: Vec3;
  /** how near counts, blocks; more than zero */
  radius: number;
}

export interface Settings {
  /** the level's `world` object as JSON text, without its `portals` and `objective` keys */
  json: string;
}

/** One entity's data (`vox-core::entity::Component`, externally tagged). */
export type Component =
  | { Portal: Portal }
  | { Spawn: Spawn }
  | { Objective: Objective }
  | { Settings: Settings };

/** What the page asks the editor to do (`vox-world::editor::bridge::Command`). */
export type Command =
  | { type: "tool"; tool: Tool }
  | { type: "block"; name: string }
  | { type: "select"; entity: number | null }
  | { type: "create"; component: Component; ref?: number | null }
  | { type: "set"; entity: number; component: Component; expect: number | null; ref?: number | null }
  | { type: "remove"; entity: number; expect: number | null; ref?: number | null }
  | { type: "fly_to"; entity: number }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "build"; node: number };

/** One entity's change: its new component, or null when it was removed. */
export interface Change {
  entity: number;
  seq: number;
  component: Component | null;
}

/** What the editor tells the page (`vox-world::editor::bridge::Event`). */
export type Event =
  | { type: "ready"; world: string; palette: string[] }
  | { type: "entities"; changes: Change[] }
  | { type: "selected"; entity: number | null }
  | { type: "refused"; id: number; ref: number | null; why: string; text: string }
  | { type: "done"; ref: number; entity: number; seq: number }
  | { type: "picked"; cell: [number, number, number] }
  | { type: "here"; players: string[] };

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

/** An object with exactly these keys: none missing, none extra (serde's deny_unknown_fields). Commands. */
function exact(v: unknown, keys: readonly string[]): v is Obj {
  return isObj(v) && Object.keys(v).length === keys.length && keys.every((k) => Object.hasOwn(v, k));
}

/** `exact`, where the keys in `optional` may also be absent (serde's `default`). Commands. */
function exactOr(v: unknown, keys: readonly string[], optional: readonly string[]): v is Obj {
  return (
    isObj(v) &&
    keys.every((k) => Object.hasOwn(v, k)) &&
    Object.keys(v).every((k) => keys.includes(k) || optional.includes(k))
  );
}

/** An object with at least these keys; extra keys are ignored. Events. */
function has(v: unknown, keys: readonly string[]): v is Obj {
  return isObj(v) && keys.every((k) => Object.hasOwn(v, k));
}

const isFloat = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isU32 = (v: unknown): v is number => Number.isInteger(v) && (v as number) >= 0 && (v as number) <= 0xffff_ffff;
const isU64 = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const isI32 = (v: unknown): v is number =>
  Number.isInteger(v) && (v as number) >= -0x8000_0000 && (v as number) <= 0x7fff_ffff;
const isI64 = (v: unknown): v is number => Number.isSafeInteger(v);
const isOpt = <T>(v: unknown, ok: (x: unknown) => x is T): v is T | null => v === null || ok(v);
const isStr = (v: unknown): v is string => typeof v === "string";
const isStrs = (v: unknown): v is string[] => Array.isArray(v) && v.every(isStr);

const isVec3 = (v: unknown): v is Vec3 => Array.isArray(v) && v.length === 3 && v.every(isFloat);
/** A command's optional `ref`: absent, null or a u32. */
const isRef = (v: unknown): boolean => v === undefined || v === null || isU32(v);
const isTool = (v: unknown): v is Tool => ["select", "blocks", "portal", "prop", "node"].includes(v as string);
const isAxis = (v: unknown): v is Axis => v === "X" || v === "Z";

/** An externally tagged value: one key, the tag, whose body must have its fields. Commands. */
function tagged(v: unknown): [string, unknown] | null {
  if (!isObj(v)) return null;
  const keys = Object.keys(v);
  return keys.length === 1 ? [keys[0]!, v[keys[0]!]] : null;
}

function isDest(v: unknown): v is Dest {
  const t = tagged(v);
  if (!t) return false;
  const [tag, body] = t;
  switch (tag) {
    case "Here":
      return exact(body, ["arrive"]) && isVec3(body.arrive);
    case "World":
      return exact(body, ["world"]) && isStr(body.world);
    case "Pair":
      return exact(body, ["key"]) && isI64(body.key);
    case "Dungeon":
      return exact(body, ["name", "arrive"]) && isStr(body.name) && isVec3(body.arrive);
    case "Url":
      return exact(body, ["url", "arrive"]) && isStr(body.url) && isVec3(body.arrive);
    default:
      return false;
  }
}

function isPortal(v: unknown): v is Portal {
  return (
    exact(v, ["name", "at", "axis", "size", "yaw", "level", "to", "key"]) &&
    isStr(v.name) &&
    isVec3(v.at) &&
    isAxis(v.axis) &&
    isSize(v.size) &&
    isFloat(v.yaw) &&
    isStr(v.level) &&
    isDest(v.to) &&
    isOpt(v.key, isI64)
  );
}

const isSize = (v: unknown): v is [number, number] => Array.isArray(v) && v.length === 2 && v.every(isFloat);

/** A component as a command carries it: exactly one of the four kinds, exactly its fields. */
export function isComponent(v: unknown): v is Component {
  const t = tagged(v);
  if (!t) return false;
  const [tag, body] = t;
  switch (tag) {
    case "Portal":
      return isPortal(body);
    case "Spawn":
      return exact(body, ["at", "yaw"]) && isVec3(body.at) && isFloat(body.yaw);
    case "Objective":
      return exact(body, ["name", "at", "radius"]) && isStr(body.name) && isVec3(body.at) && isFloat(body.radius);
    case "Settings":
      return exact(body, ["json"]) && isStr(body.json);
    default:
      return false;
  }
}

/** The tags a destination may have, in the order an event is read: the first present wins. */
const DEST_TAGS = ["Here", "World", "Pair", "Dungeon", "Url"] as const;
/** The tags a component may have, in the order an event is read: the first present wins. */
const COMPONENT_TAGS = ["Portal", "Spawn", "Objective", "Settings"] as const;

/** Reads a destination from an event: the first known tag in `DEST_TAGS`, with only its known
 *  fields, or null when there is no known tag or that tag's body is malformed. */
function readDest(v: unknown): Dest | null {
  if (!isObj(v)) return null;
  const tag = DEST_TAGS.find((t) => Object.hasOwn(v, t));
  if (!tag) return null;
  const body = v[tag];
  switch (tag) {
    case "Here":
      return has(body, ["arrive"]) && isVec3(body.arrive) ? { Here: { arrive: body.arrive } } : null;
    case "World":
      return has(body, ["world"]) && isStr(body.world) ? { World: { world: body.world } } : null;
    case "Pair":
      return has(body, ["key"]) && isI64(body.key) ? { Pair: { key: body.key } } : null;
    case "Dungeon":
      return has(body, ["name", "arrive"]) && isStr(body.name) && isVec3(body.arrive)
        ? { Dungeon: { name: body.name, arrive: body.arrive } }
        : null;
    case "Url":
      return has(body, ["url", "arrive"]) && isStr(body.url) && isVec3(body.arrive)
        ? { Url: { url: body.url, arrive: body.arrive } }
        : null;
  }
}

function readPortal(v: unknown): Portal | null {
  if (!has(v, ["name", "at", "axis", "size", "yaw", "level", "to", "key"])) return null;
  if (!isStr(v.name) || !isVec3(v.at) || !isAxis(v.axis) || !isSize(v.size) || !isFloat(v.yaw)) return null;
  if (!isStr(v.level) || !isOpt(v.key, isI64)) return null;
  const to = readDest(v.to);
  if (!to) return null;
  return { name: v.name, at: v.at, axis: v.axis, size: v.size, yaw: v.yaw, level: v.level, to, key: v.key };
}

/** Reads a component from an event: the first known tag in `COMPONENT_TAGS`, with only its known
 *  fields, or null when there is no known tag or that tag's body is malformed. */
export function readComponent(v: unknown): Component | null {
  if (!isObj(v)) return null;
  const tag = COMPONENT_TAGS.find((t) => Object.hasOwn(v, t));
  if (!tag) return null;
  const body = v[tag];
  switch (tag) {
    case "Portal": {
      const portal = readPortal(body);
      return portal ? { Portal: portal } : null;
    }
    case "Spawn":
      return has(body, ["at", "yaw"]) && isVec3(body.at) && isFloat(body.yaw)
        ? { Spawn: { at: body.at, yaw: body.yaw } }
        : null;
    case "Objective":
      return has(body, ["name", "at", "radius"]) && isStr(body.name) && isVec3(body.at) && isFloat(body.radius)
        ? { Objective: { name: body.name, at: body.at, radius: body.radius } }
        : null;
    case "Settings":
      return has(body, ["json"]) && isStr(body.json) ? { Settings: { json: body.json } } : null;
  }
}

/** Whether a value is a command the bridge defines, field by field. */
export function isCommand(v: unknown): v is Command {
  if (!isObj(v)) return false;
  switch (v.type) {
    case "tool":
      return exact(v, ["type", "tool"]) && isTool(v.tool);
    case "block":
      return exact(v, ["type", "name"]) && isStr(v.name);
    case "select":
      return exact(v, ["type", "entity"]) && isOpt(v.entity, isU32);
    case "create":
      return exactOr(v, ["type", "component"], ["ref"]) && isComponent(v.component) && isRef(v.ref);
    case "set":
      return (
        exactOr(v, ["type", "entity", "component", "expect"], ["ref"]) &&
        isU32(v.entity) &&
        isComponent(v.component) &&
        isOpt(v.expect, isU64) &&
        isRef(v.ref)
      );
    case "remove":
      return (
        exactOr(v, ["type", "entity", "expect"], ["ref"]) && isU32(v.entity) && isOpt(v.expect, isU64) && isRef(v.ref)
      );
    case "fly_to":
      return exact(v, ["type", "entity"]) && isU32(v.entity);
    case "undo":
    case "redo":
      return exact(v, ["type"]);
    case "build":
      return exact(v, ["type", "node"]) && isU32(v.node);
    default:
      return false;
  }
}

/** Reads one change of an entities batch, normalised, or null when it is malformed or its
 *  component has no kind this page knows (so the batch filters it out and keeps the others). */
function readChange(v: unknown): Change | null {
  if (!has(v, ["entity", "seq", "component"]) || !isU32(v.entity) || !isU64(v.seq)) return null;
  if (v.component === null) return { entity: v.entity, seq: v.seq, component: null };
  const component = readComponent(v.component);
  return component ? { entity: v.entity, seq: v.seq, component } : null;
}

/** Reads one event the client sent, or null when it isn't one the bridge defines. Lenient and
 *  normalising: the result holds only the known fields, a component or destination keeps only its
 *  first known tag, and an entities batch keeps its well-formed changes and drops the rest. */
export function readEvent(v: unknown): Event | null {
  if (!isObj(v)) return null;
  switch (v.type) {
    case "ready":
      return has(v, ["world", "palette"]) && isStr(v.world) && isStrs(v.palette)
        ? { type: "ready", world: v.world, palette: v.palette }
        : null;
    case "entities": {
      if (!has(v, ["changes"]) || !Array.isArray(v.changes)) return null;
      const changes = v.changes.map(readChange).filter((c): c is Change => c !== null);
      return { type: "entities", changes };
    }
    case "selected":
      return has(v, ["entity"]) && isOpt(v.entity, isU32) ? { type: "selected", entity: v.entity } : null;
    case "refused":
      // `ref` is null (or absent) when the command had none
      return has(v, ["id", "why", "text"]) && isU32(v.id) && isStr(v.why) && isStr(v.text) && isRef(v.ref)
        ? { type: "refused", id: v.id, ref: (v.ref as number | null | undefined) ?? null, why: v.why, text: v.text }
        : null;
    case "done":
      return has(v, ["ref", "entity", "seq"]) && isU32(v.ref) && isU32(v.entity) && isU64(v.seq)
        ? { type: "done", ref: v.ref, entity: v.entity, seq: v.seq }
        : null;
    case "picked":
      return has(v, ["cell"]) && Array.isArray(v.cell) && v.cell.length === 3 && v.cell.every(isI32)
        ? { type: "picked", cell: v.cell as [number, number, number] }
        : null;
    case "here":
      return has(v, ["players"]) && isStrs(v.players) ? { type: "here", players: v.players } : null;
    default:
      return null;
  }
}

/** Whether a value is an event the bridge defines (lenient and normalising, as `readEvent`). */
export function isEvent(v: unknown): v is Event {
  return readEvent(v) !== null;
}

/** Sends one command to the running client. Throws when the editor isn't running. */
export function sendCommand(c: Command): void {
  editorCommand(JSON.stringify(c));
}

/** Installs the handler for the client's events and returns a disposer. The disposer clears
 *  `window.vox_editor` only if it is still this handler, so a later `onEvent` is not undone.
 *  Anything that isn't a known event is ignored. */
export function onEvent(handler: (e: Event) => void): () => void {
  const receive = (json: string): void => {
    let v: unknown;
    try {
      v = JSON.parse(json);
    } catch {
      return;
    }
    const e = readEvent(v);
    if (e) handler(e);
  };
  window.vox_editor = receive;
  return () => {
    if (window.vox_editor === receive) delete window.vox_editor;
  };
}
