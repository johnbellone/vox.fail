// Friends, groups and raid parties.

import { api, me, $, el } from "/vox.js";
import { nav } from "/nav.js";

let worlds = [];
let catalogue = null;
const sentence = (s) => s.charAt(0).toUpperCase() + s.slice(1) + (/[.!?]$/.test(s) ? "" : ".");
const minutes = (s) => Math.max(0, Math.round(s / 60));

async function act(path, body, msgEl) {
  try {
    const r = await api(path, "POST", body);
    await load();
    return r;
  } catch (err) {
    if (msgEl) msgEl.textContent = sentence(err.message);
    return null;
  }
}

function portrait(model) {
  const c = catalogue && catalogue.characters.find((x) => x.model === model);
  return el("img", { src: c ? `/assets/${c.portrait}` : "/assets/portraits/blue.png", alt: "" });
}

function drawInvites(p) {
  const list = $("#inv-list");
  list.replaceChildren();
  for (const name of p.asking) {
    list.append(el("li", {}, el("span", { text: `${name} wants to be your friend` }),
      el("span", { class: "row" },
        el("button", { type: "button", text: "Accept", onclick: () => act("/api/friends/answer", { name, accept: true }) }),
        el("button", { type: "button", class: "plain", text: "Decline", onclick: () => act("/api/friends/answer", { name, accept: false }) }))));
  }
  for (const i of p.invites) {
    const what = i.kind === "party" ? "raid party" : "group";
    list.append(el("li", {}, el("span", { text: `${i.by} invites you to the ${what} ${i.name}` }),
      el("span", { class: "row" },
        el("button", { type: "button", text: "Join", onclick: () => act("/api/crews/answer", { crew: i.crew, accept: true }) }),
        el("button", { type: "button", class: "plain", text: "No thanks", onclick: () => act("/api/crews/answer", { crew: i.crew, accept: false }) }))));
  }
  $("#invites").hidden = !(p.asking.length + p.invites.length);
}

function drawFriends(p) {
  const ul = $("#friends");
  ul.replaceChildren();
  for (const f of p.friends) {
    let sure = false;
    const b = el("button", { type: "button", class: "plain", text: "Remove", onclick: () => {
      if (!sure) { sure = true; b.textContent = `Remove ${f.name}?`; return; }
      act("/api/friends/remove", { name: f.name });
    } });
    ul.append(el("li", {}, el("span", { class: "who" }, portrait(f.model), el("span", { text: f.name })), b));
  }
  if (!p.friends.length) ul.append(el("li", { class: "quiet", text: "No friends here yet. Ask someone by name." }));
  $("#asked").textContent = p.asked.length ? `Waiting to hear from ${p.asked.join(", ")}.` : "";
}

function inviteRow(c, friends) {
  const inside = new Set(c.members.map((m) => m.name).concat(c.invited));
  const can = friends.filter((f) => !inside.has(f.name));
  if (!can.length) return el("p", { class: "note", text: friends.length ? "All your friends are in it or invited." : "Make friends to invite them." });
  const sel = el("select", { "aria-label": `Invite a friend to ${c.name}` });
  for (const f of can) sel.append(el("option", { value: f.name, text: f.name }));
  const msg = el("p", { class: "error", role: "alert" });
  return el("div", {}, el("div", { class: "inline" }, sel,
    el("button", { type: "button", class: "plain", text: "Invite", onclick: () => act("/api/crews/invite", { crew: c.id, name: sel.value }, msg) })), msg);
}

function members(c) {
  const box = el("div", { class: "members" });
  for (const m of c.members) box.append(el("span", { class: m.leads ? "leads" : "", text: m.leads && c.kind === "party" ? `${m.name}, leading` : m.name }));
  for (const n of c.invited) box.append(el("span", { class: "quiet", text: `${n}, invited` }));
  return box;
}

function leaveButton(c) {
  let sure = false;
  const b = el("button", { type: "button", class: "plain", text: `Leave ${c.name}`, onclick: () => {
    if (!sure) { sure = true; b.textContent = c.members.length === 1 ? `Leave and end ${c.name}?` : `Leave ${c.name}?`; return; }
    act("/api/crews/leave", { crew: c.id });
  } });
  return b;
}

function raidBox(c) {
  const box = el("div", { class: "raid" });
  const r = c.raid;
  const now = Date.now() / 1000;
  if (r && r.state === "on") {
    box.append(el("strong", { text: `Raiding ${r.world_name}` }),
      el("p", { class: "note", text: `${r.together} of ${r.needed} minutes together so far; ${minutes(r.ends - now)} minutes left.` }),
      el("div", { class: "meter", "aria-hidden": "true" }, el("i", { style: `width:${Math.min(100, (r.together / r.needed) * 100)}%` })),
      el("p", { style: "margin:10px 0 0" }, el("a", { class: "button", href: `/play?world=${encodeURIComponent(r.world_name)}`, text: `Go to ${r.world_name}` })));
    return box;
  }
  if (r) box.append(el("p", { class: "note", text: r.state === "won" ? `Won the raid on ${r.world_name}.` : `The raid on ${r.world_name} ran out of time.` }));
  if (!c.you_lead) {
    box.append(el("p", { class: "note", text: "The leader starts the next raid." }));
    return box;
  }
  const sel = el("select", { "aria-label": "World to raid" });
  for (const w of worlds) sel.append(el("option", { value: w.id, text: w.name }));
  const msg = el("p", { class: "error", role: "alert" });
  box.append(el("div", { class: "inline" }, sel,
    el("button", { type: "button", text: "Start a raid (10 authority)", onclick: () => act("/api/feats/raid", { party: c.id, world: sel.value }, msg) })), msg);
  return box;
}

function drawCrews(p) {
  const groups = $("#groups");
  const parties = $("#parties");
  groups.replaceChildren();
  parties.replaceChildren();
  for (const c of p.crews) {
    const box = el("div", { class: "crew" }, el("h3", { text: c.name }), members(c));
    if (c.kind === "party") box.append(raidBox(c));
    if (c.kind === "group" || c.you_lead) box.append(inviteRow(c, p.friends));
    box.append(leaveButton(c));
    (c.kind === "party" ? parties : groups).append(box);
  }
  if (!groups.children.length) groups.append(el("p", { class: "quiet", text: "You're not in a group." }));
  if (!parties.children.length) parties.append(el("p", { class: "quiet", text: "You're not in a raid party." }));
}

async function load() {
  const p = await api("/api/people");
  drawInvites(p);
  drawFriends(p);
  drawCrews(p);
}

function form(id, input, msg, run) {
  $(id).addEventListener("submit", async (e) => {
    e.preventDefault();
    $(msg).textContent = "";
    const r = await act(...run($(input).value.trim()), $(msg));
    if (r) $(input).value = "";
  });
}

async function start() {
  const who = await me();
  if (!who.name) { location.href = "/"; return; }
  nav($("#top"));
  const [f, cat] = await Promise.all([api("/api/feats"), api("/api/catalogue")]);
  worlds = f.worlds;
  catalogue = cat;
  form("#add", "#add-name", "#add-msg", (name) => ["/api/friends", { name }]);
  form("#new-group", "#group-name", "#group-msg", (name) => ["/api/crews", { kind: "group", name }]);
  form("#new-party", "#party-name", "#party-msg", (name) => ["/api/crews", { kind: "party", name }]);
  await load();
}

start().catch((err) => { $("#add-msg").textContent = err.message; });
