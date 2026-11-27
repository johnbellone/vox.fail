// What the directory's API sends back (directory/src/lib.rs, game.rs, people.rs).

export type Site = "steam" | "discord" | (string & {});

export interface Character {
  model: string;
  wear: string[];
}

export interface Me {
  name?: string;
  character?: Character | null;
  sign_in_with?: Site[];
  linked?: Site[];
}

export interface CastMember {
  model: string;
  name: string;
  group: string;
  portrait: string;
  about?: string;
  /** one character in several outfits (build_catalogue.py FAMILIES): its id, its name, this outfit */
  family?: string;
  family_name?: string;
  outfit?: string;
}

export type Slot = "hat" | "face" | "hand";

export interface WardrobeItem {
  id: string;
  slot: Slot;
  label: string;
  file: string;
}

export interface Catalogue {
  characters: CastMember[];
  wardrobe: WardrobeItem[];
}

export interface Download {
  label: string;
  url: string;
}

export interface WorldEntry {
  name: string;
  url: string;
}

export interface Worlds {
  worlds?: WorldEntry[];
  downloads?: Download[];
}

export interface Token {
  token: string;
  name: string;
  character: Character;
  worlds: WorldEntry[];
}

export interface Balance {
  code: string;
  amount: number;
}

export interface LedgerLine {
  code: string;
  amount: number;
  memo: string;
  at: number;
  with?: string | null;
}

export interface Wallet {
  balances: Balance[];
  earning: { today: number; daily: number; minutes_per: number; edits_per: number };
  ledger: LedgerLine[];
}

export type ItemKind = "coin" | "deed" | "key" | "banner" | "trophy" | "wear";

export interface Item {
  id: string;
  kind: ItemKind;
  name: string;
  code?: string;
  amount?: number;
  slot?: Slot;
  worn?: boolean;
  world?: string;
  world_name?: string;
  acquired?: number;
  ref?: number;
  // what else the directory says about it, by kind
  detail: {
    about?: string;
    area?: { x0: number; x1: number; z0: number; z1: number };
    shared_with?: { id: number; name: string } | null;
    to_name?: string;
    raid_on?: string;
    party?: string;
    with?: number;
  };
}

export interface Inventory {
  items: Item[];
}

export interface Feats {
  authority: number;
  costs: { plot: number; portal: number; currency: number; raid: number };
  places: { world: string; world_name: string; seen: number; in_your_plot: boolean }[];
  worlds: { id: string; name: string }[];
  owned_worlds: { world: string; world_name: string; currency?: string | null }[];
}

export interface Friend {
  name: string;
  model: string;
}

export interface CrewMember {
  name: string;
  leads: boolean;
}

export interface Raid {
  state: "on" | "won" | "lost" | (string & {});
  world_name: string;
  together: number;
  needed: number;
  ends: number;
}

export interface Crew {
  id: number;
  kind: "group" | "party";
  name: string;
  members: CrewMember[];
  invited: string[];
  you_lead: boolean;
  raid?: Raid | null;
}

export interface People {
  asking: string[];
  asked: string[];
  friends: Friend[];
  invites: { crew: number; kind: "group" | "party"; name: string; by: string }[];
  crews: Crew[];
}
