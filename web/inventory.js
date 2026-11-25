// The inventory explorer: everything you hold (coins, deeds, keys, banners, trophies, the
// wardrobe), your authority, the feats it buys, and the ledger.

import { api, me, $, el } from "/vox.js";
import { glyph } from "/glyphs.js";
import { nav } from "/nav.js";

const KINDS = [
  ["", "Everything you've earned"],
  ["coin", "Coins"],
  ["deed", "Deeds"],
  ["key", "Portal keys"],
  ["banner", "Banners"],
  ["trophy", "Trophies"],
  ["wear", "Wardrobe"],
];
const ABOUT = {
  deed: "A plot you claimed: 16 × 16 blocks, as high and deep as the world goes. Only you, and a group you share it with, can build or dig in it.",
  key: "A portal you opened from your plot. Everyone who walks through it goes to the world it leads to.",
  banner: "Your party's banner, raised when you led it on a raid.",
  trophy: "From a raid your party won: two or more of you spent ten minutes in the world together.",
  wear: "Part of everyone's wardrobe. Put it on in the character maker.",
};

let state = { items: [], kind: "", world: "", q: "", sort: "new", picked: null };
let groups = [];
let feats = null;

const when = (t) => new Date(t * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
const num = (n) => Number(n).toLocaleString();

function shown() {
  const q = state.q.trim().toLowerCase();
  let v = state.items.filter((i) =>
    (state.kind ? i.kind === state.kind : i.kind !== "wear") &&
    (!state.world || i.world === state.world) &&
    (!q || String(i.name).toLowerCase().includes(q) || String(i.world_name || "").toLowerCase().includes(q) || String(i.code || "").toLowerCase().includes(q)));
  const order = KINDS.map((k) => k[0]);
  if (state.sort === "name") v.sort((a, b) => String(a.name).localeCompare(String(b.name)));
  else if (state.sort === "kind") v.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind) || String(a.name).localeCompare(String(b.name)));
  else v.sort((a, b) => (b.acquired || 0) - (a.acquired || 0) || order.indexOf(a.kind) - order.indexOf(b.kind));
  return v;
}

// a wardrobe piece looks like where it goes
const art = (i) => (i.kind === "wear" && (i.slot === "face" || i.slot === "hand") ? i.slot : i.kind);

function sub(i) {
  if (i.kind === "coin") return `${num(i.amount)} ${i.code}`;
  if (i.kind === "wear") return i.worn ? `${i.slot}, wearing` : i.slot;
  return i.world_name || "";
}

function drawKinds() {
  const box = $("#kinds");
  box.replaceChildren();
  for (const [k, label] of KINDS) {
    const n = state.items.filter((i) => (k ? i.kind === k : i.kind !== "wear")).length;
    if (k && !n && k !== "deed") continue;
    box.append(el("button", { type: "button", "aria-pressed": String(state.kind === k), onclick: () => { state.kind = k; draw(); } },
      el("span", { text: label }), el("span", { class: "n", text: String(n) })));
  }
}

function draw() {
  drawKinds();
  const grid = $("#items");
  grid.replaceChildren();
  const v = shown();
  for (const i of v) {
    const b = el("button", { type: "button", class: "item", "aria-pressed": String(state.picked === i.id), onclick: () => pick(i.id) },
      glyph(art(i), i.code === "AUTH"), el("span", { class: "name", text: i.name }), el("span", { class: "sub", text: sub(i) }));
    grid.append(b);
  }
  $("#empty").hidden = v.length > 0;
  $("#empty").textContent = state.kind === "deed" && !state.q ? "No plots yet. Claim one under Feats, below." : "Nothing here matches.";
}

function pick(id) {
  state.picked = id;
  draw();
  const i = state.items.find((x) => x.id === id);
  const d = $("#detail");
  d.replaceChildren();
  if (!i) return;
  d.append(glyph(art(i), i.code === "AUTH"), el("h2", { text: i.name }));
  const dl = el("dl");
  const row = (k, v) => dl.append(el("dt", { text: k }), el("dd", { text: v }));
  if (i.kind === "coin") {
    row("You hold", `${num(i.amount)} ${i.code}`);
    if (i.world_name) row("Issued by", i.world_name);
    d.append(el("p", { class: "quiet", text: i.detail.about }), dl);
    if (i.code !== "AUTH") d.append(sendForm(i));
    return;
  }
  if (i.world_name) row("World", i.world_name);
  if (i.acquired) row("Since", when(i.acquired));
  if (i.kind === "deed") {
    const a = i.detail.area;
    row("Where", `x ${a.x0} to ${a.x1 - 1}, z ${a.z0} to ${a.z1 - 1}`);
    row("Builders", i.detail.shared_with ? `you and ${i.detail.shared_with.name}` : "you");
  }
  if (i.kind === "key") row("Leads to", i.detail.to_name);
  if (i.kind === "banner") row("Raid on", i.detail.raid_on);
  if (i.kind === "trophy") { row("Party", i.detail.party); row("Members", String(i.detail.with)); }
  if (i.kind === "wear") row("Worn on", i.slot);
  d.append(el("p", { class: "quiet", text: ABOUT[i.kind] || "" }), dl);
  if (i.kind === "deed") d.append(deedActions(i));
  if (i.kind === "wear") d.append(el("a", { class: "button plain", href: "/create", text: "Change what you wear" }));
}

function sendForm(i) {
  const f = el("form", { class: "feat" });
  const msg = el("p", { class: "error", role: "alert" });
  f.append(
    el("h3", { text: `Send ${i.code}` }),
    el("label", { for: "to", text: "To" }), el("input", { type: "text", id: "to", required: "", placeholder: "Their name", autocomplete: "off" }),
    el("label", { for: "amount", text: "How many" }), el("input", { type: "number", id: "amount", min: "1", max: String(i.amount), required: "" }),
    el("label", { for: "memo", text: "Note (they'll see it)" }), el("input", { type: "text", id: "memo", maxlength: "80" }),
    el("button", { type: "submit", text: "Send" }), msg);
  f.addEventListener("submit", async (e) => {
    e.preventDefault();
    msg.className = "error";
    try {
      const r = await api("/api/send", "POST", { to: $("#to", f).value, code: i.code, amount: Number($("#amount", f).value), memo: $("#memo", f).value });
      await load();
      pick(i.id);
      const m = $("#detail .error") || msg;
      m.className = "ok";
      m.textContent = `Sent ${num(r.sent)} ${r.code} to ${r.to}.`;
    } catch (err) { msg.textContent = sentence(err.message); }
  });
  return f;
}

function deedActions(i) {
  const box = el("div", { class: "feat" });
  const msg = el("p", { class: "error", role: "alert" });
  const sel = el("select", { id: "share" }, el("option", { value: "", text: "Just you" }));
  for (const g of groups) sel.append(el("option", { value: String(g.id), text: g.name }));
  sel.value = i.detail.shared_with ? String(i.detail.shared_with.id) : "";
  const share = el("button", { type: "button", text: "Share", onclick: async () => {
    try {
      await api("/api/plots/share", "POST", { plot: i.ref, group: sel.value ? Number(sel.value) : null });
      await load(); pick(i.id);
    } catch (err) { msg.textContent = sentence(err.message); }
  } });
  let sure = false;
  const release = el("button", { type: "button", class: "plain", text: "Give up this plot", onclick: async () => {
    if (!sure) { sure = true; release.textContent = "Give it up for good (its portals close too)"; return; }
    try { await api("/api/plots/release", "POST", { plot: i.ref }); state.picked = null; await load(); $("#detail").replaceChildren(el("p", { class: "quiet", text: "The plot is free again." })); }
    catch (err) { msg.textContent = sentence(err.message); }
  } });
  box.append(el("h3", { text: "Who can build here" }), el("label", { for: "share", text: "Share with a group" }),
    el("div", { class: "inline" }, sel, share), groups.length ? "" : el("p", { class: "note", text: "Make a group on Friends and raids to build here together." }),
    el("p", { style: "margin-top:14px" }, release), msg);
  return box;
}

const sentence = (s) => s.charAt(0).toUpperCase() + s.slice(1) + (/[.!?]$/.test(s) ? "" : ".");

function featForm(title, cost, about, fields, run, note) {
  const f = el("form", { class: "feat" });
  const msg = el("p", { class: "error", role: "alert" });
  const can = feats.authority >= cost;
  const go = el("button", { type: "submit", text: title });
  f.append(el("h3", {}, el("span", { text: title }), el("span", { class: "cost", text: `${cost} authority` })), el("p", { class: "note", text: about }));
  for (const x of fields) f.append(x);
  f.append(go, msg);
  if (note) f.append(el("p", { class: "note", text: note }));
  if (!can) f.append(el("p", { class: "note", text: `You have ${feats.authority}: keep playing to earn the rest.` }));
  f.addEventListener("submit", async (e) => {
    e.preventDefault();
    msg.className = "error";
    msg.textContent = "";
    go.disabled = true;
    try {
      const done = await run();
      await load();
      msg.className = "ok";
      msg.textContent = done;
    } catch (err) { msg.textContent = sentence(err.message); } finally { go.disabled = false; }
  });
  return f;
}

function pickWorld(id, label, list, empty) {
  const s = el("select", { id, required: "" });
  for (const w of list) s.append(el("option", { value: w.value, text: w.text }));
  if (!list.length) s.append(el("option", { value: "", text: empty }));
  return [el("label", { for: id, text: label }), s];
}

function drawFeats() {
  const box = $("#feats");
  box.replaceChildren();
  const c = feats.costs;
  const places = feats.places.map((p) => ({ value: p.world, text: `${p.world_name}, where you were on ${when(p.seen)}` }));
  // claim a plot
  box.append(featForm("Claim a plot", c.plot,
    "16 × 16 blocks around where you last stood, at least 24 blocks from where people arrive. Only you (and a group you choose) can build in it.",
    pickWorld("plot-world", "Where", places, "Go into a world first"),
    async () => {
      const r = await api("/api/feats/plot", "POST", { world: $("#plot-world").value });
      return `Claimed: x ${r.area.x0} to ${r.area.x1 - 1}, z ${r.area.z0} to ${r.area.z1 - 1}. The deed is in your inventory.`;
    }));
  // open a portal
  const inPlot = feats.places.filter((p) => p.in_your_plot).map((p) => ({ value: p.world, text: p.world_name }));
  const dests = feats.worlds.map((w) => ({ value: w.id, text: w.name }));
  box.append(featForm("Open a portal", c.portal,
    "A doorway, where you last stood in a plot of yours, to another world. Players arriving after it opens see it.",
    [...pickWorld("portal-from", "From", inPlot, "Stand in a plot of yours first"), ...pickWorld("portal-to", "To", dests, "")],
    async () => {
      const r = await api("/api/feats/portal", "POST", { world: $("#portal-from").value, to: $("#portal-to").value });
      return `Opened. Its key is in your inventory (${r.at.join(", ")}).`;
    }));
  // lead somewhere else by default
  const to = $("#portal-to");
  const other = dests.find((d) => d.value !== $("#portal-from").value);
  if (other) to.value = other.value;
  // issue a currency: for a world's owner
  const issuable = feats.owned_worlds.filter((w) => !w.currency).map((w) => ({ value: w.world, text: w.world_name }));
  if (issuable.length) {
    box.append(featForm("Issue a currency", c.currency,
      "Give a world you own its own coin. You set the supply once; all of it is yours to give out.",
      [...pickWorld("cur-world", "For", issuable, "Your worlds have their coins"),
        el("label", { for: "cur-name", text: "Name" }), el("input", { type: "text", id: "cur-name", required: "", maxlength: "24", placeholder: "Acorns" }),
        el("label", { for: "cur-code", text: "Code (3 to 5 letters)" }), el("input", { type: "text", id: "cur-code", required: "", maxlength: "5", placeholder: "ACRN" }),
        el("label", { for: "cur-supply", text: "Supply" }), el("input", { type: "number", id: "cur-supply", required: "", min: "1", max: "1000000000", value: "10000" })],
      async () => {
        const r = await api("/api/feats/currency", "POST", { world: $("#cur-world").value, name: $("#cur-name").value, code: $("#cur-code").value, supply: Number($("#cur-supply").value) });
        return `${r.name} (${r.code}) issued: all ${num(r.supply)} are yours.`;
      }));
  } else {
    box.append(el("div", { class: "feat" }, el("h3", {}, el("span", { text: "Issue a currency" }), el("span", { class: "cost", text: `${c.currency} authority` })),
      el("p", { class: "note", text: feats.owned_worlds.length
        ? `Your worlds have their coins: ${feats.owned_worlds.map((w) => `${w.currency} for ${w.world_name}`).join(", ")}.`
        : "A world's owner can give it its own coin, which anyone can then send and receive. You don't own a world yet." })));
  }
  box.append(el("div", { class: "feat" }, el("h3", {}, el("span", { text: "Start a raid" }), el("span", { class: "cost", text: `${c.raid} authority` })),
    el("p", { class: "note" }, "Lead your party into another world. Raids start from ", el("a", { href: "/friends", text: "Friends and raids" }), ".")));
}

function drawWallet(w) {
  const auth = w.balances.find((b) => b.code === "AUTH");
  $("#authority").textContent = num(auth.amount);
  const e = w.earning;
  $("#today").textContent = `Earned today: ${e.today} of ${e.daily}`;
  $("#today-bar").style.width = `${Math.min(100, (e.today / e.daily) * 100)}%`;
  $("#how").textContent = `One for every ${e.minutes_per} minutes in a world and every ${e.edits_per} blocks you place.`;
  const ul = $("#ledger");
  ul.replaceChildren();
  for (const l of w.ledger.slice(0, 25)) {
    const code = l.code === "AUTH" ? "authority" : l.code;
    const amt = `${l.amount > 0 ? "+" : "−"}${num(Math.abs(l.amount))} ${code}`;
    ul.append(el("li", {}, el("span", { text: l.memo }), el("span", { class: l.amount > 0 ? "amt" : "amt out", text: amt }),
      el("span", { class: "when", text: `${when(l.at)}${l.with ? (l.amount > 0 ? `, from ${l.with}` : `, to ${l.with}`) : ""}` })));
  }
  if (!w.ledger.length) ul.append(el("li", { class: "quiet", text: "Nothing yet." }));
}

async function load() {
  const [inv, w, f, p] = await Promise.all([api("/api/inventory"), api("/api/wallet"), api("/api/feats"), api("/api/people")]);
  state.items = inv.items;
  feats = f;
  groups = p.crews.filter((c) => c.kind === "group");
  const ws = $("#world");
  const keep = ws.value;
  ws.replaceChildren(el("option", { value: "", text: "Every world" }));
  const seen = new Map();
  for (const i of state.items) if (i.world) seen.set(i.world, i.world_name || i.world);
  for (const [id, name] of seen) ws.append(el("option", { value: id, text: name }));
  ws.value = seen.has(keep) ? keep : "";
  state.world = ws.value;
  drawWallet(w);
  drawFeats();
  draw();
}

async function start() {
  const who = await me();
  if (!who.name) { location.href = "/"; return; }
  nav($("#top"));
  $("#world").addEventListener("change", (e) => { state.world = e.target.value; draw(); });
  $("#q").addEventListener("input", (e) => { state.q = e.target.value; draw(); });
  $("#sort").addEventListener("change", (e) => { state.sort = e.target.value; draw(); });
  await load();
}

start().catch((err) => { $("#detail").replaceChildren(el("p", { class: "error", text: err.message })); });
