// The inventory's rules, apart from how it's drawn: what kinds there are, what each means,
// filtering and ordering.

import type { Item, ItemKind } from "../../api/types";

export const KINDS: [ItemKind | "", string][] = [
  ["", "Everything you've earned"],
  ["coin", "Coins"],
  ["deed", "Deeds"],
  ["key", "Portal keys"],
  ["banner", "Banners"],
  ["trophy", "Trophies"],
  ["wear", "Wardrobe"],
];

export const ABOUT: Partial<Record<ItemKind, string>> = {
  deed: "A plot you claimed: 16 × 16 m (64 × 64 blocks), as high and deep as the world goes. Only you, and a group you share it with, can build or dig in it.",
  key: "A portal you opened from your plot. Everyone who walks through it goes to the world it leads to.",
  banner: "Your party's banner, raised when you led it on a raid.",
  trophy: "From a raid your party won: two or more of you spent ten minutes in the world together.",
  wear: "Part of everyone's wardrobe. Put it on in the character maker.",
};

export type Sort = "new" | "name" | "kind";

export interface Filter {
  kind: ItemKind | "";
  world: string;
  q: string;
  sort: Sort;
}

/** "Everything" means everything earned: the wardrobe is everyone's, so it has its own tab. */
export const inKind = (i: Item, kind: ItemKind | "") => (kind ? i.kind === kind : i.kind !== "wear");

export function shown(items: Item[], f: Filter): Item[] {
  const q = f.q.trim().toLowerCase();
  const v = items.filter(
    (i) =>
      inKind(i, f.kind) &&
      (!f.world || i.world === f.world) &&
      (!q || [i.name, i.world_name, i.code].some((s) => String(s ?? "").toLowerCase().includes(q))),
  );
  const order = KINDS.map((k) => k[0]);
  const byName = (a: Item, b: Item) => String(a.name).localeCompare(String(b.name));
  if (f.sort === "name") v.sort(byName);
  else if (f.sort === "kind") v.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind) || byName(a, b));
  else v.sort((a, b) => (b.acquired ?? 0) - (a.acquired ?? 0) || order.indexOf(a.kind) - order.indexOf(b.kind));
  return v;
}

/** Which picture: a wardrobe piece looks like where it goes. */
export const art = (i: Item) => (i.kind === "wear" && (i.slot === "face" || i.slot === "hand") ? i.slot : i.kind);

export function sub(i: Item): string {
  if (i.kind === "coin") return `${Number(i.amount ?? 0).toLocaleString()} ${i.code ?? ""}`;
  if (i.kind === "wear") return i.worn ? `${i.slot}, wearing` : (i.slot ?? "");
  return i.world_name ?? "";
}
