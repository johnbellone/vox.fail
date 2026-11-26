import { describe, expect, test } from "bun:test";
import type { Item } from "../../api/types";
import { art, shown, sub } from "./model";

const item = (over: Partial<Item>): Item => ({ id: "x", kind: "coin", name: "x", detail: {}, ...over });

const items: Item[] = [
  item({ id: "a", kind: "coin", name: "Authority", code: "AUTH", amount: 1200, acquired: 1 }),
  item({ id: "b", kind: "deed", name: "Plot", world: "yard", world_name: "The yard", acquired: 3 }),
  item({ id: "c", kind: "wear", name: "Cowboy hat", slot: "hat" }),
  item({ id: "d", kind: "trophy", name: "Raided the yard", world: "yard", world_name: "The yard", acquired: 2 }),
];

describe("inventory filter", () => {
  test("everything leaves the wardrobe out, newest first", () => {
    expect(shown(items, { kind: "", world: "", q: "", sort: "new" }).map((i) => i.id)).toEqual(["b", "d", "a"]);
  });
  test("the wardrobe has its own tab", () => {
    expect(shown(items, { kind: "wear", world: "", q: "", sort: "new" }).map((i) => i.id)).toEqual(["c"]);
  });
  test("by world and by text", () => {
    expect(shown(items, { kind: "", world: "yard", q: "", sort: "name" }).map((i) => i.id)).toEqual(["b", "d"]);
    expect(shown(items, { kind: "", world: "", q: "auth", sort: "new" }).map((i) => i.id)).toEqual(["a"]);
  });
  test("by kind, then name", () => {
    expect(shown(items, { kind: "", world: "", q: "", sort: "kind" }).map((i) => i.id)).toEqual(["a", "b", "d"]);
  });
});

describe("labels", () => {
  test("coins show their amount, wardrobe its slot", () => {
    expect(sub(items[0]!)).toBe(`${(1200).toLocaleString()} AUTH`);
    expect(sub(item({ kind: "wear", slot: "face", worn: true }))).toBe("face, wearing");
  });
  test("face and hand pieces get their own pictures", () => {
    expect(art(item({ kind: "wear", slot: "hand" }))).toBe("hand");
    expect(art(item({ kind: "wear", slot: "hat" }))).toBe("wear");
  });
});
