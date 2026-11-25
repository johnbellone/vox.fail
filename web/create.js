import { api, me, $, el, find } from "/vox.js";
import { nav } from "/nav.js";

const GROUPS = { Main: "Vox's own", Cast: "The cast", Roast: "The roast", Animals: "Animals" };
const SLOTS = ["hat", "face", "hand"];

let cat;
let pick = { model: null, wear: {} };
let view = null; // the 3D preview, once it's running

function choose(model) {
  pick.model = model;
  for (const b of document.querySelectorAll(".cast button")) b.setAttribute("aria-pressed", b.dataset.model === model);
  const c = find(cat, model);
  $("#who-name").textContent = c ? c.name : "";
  $("#who-about").textContent = "";
  $("#fallback-img").src = c ? `/assets/${c.portrait}` : "";
  if (view) view.show(model, wornFiles());
}

function wear(slot, id) {
  if (id) pick.wear[slot] = id; else delete pick.wear[slot];
  for (const b of document.querySelectorAll(`#${slot} button`)) b.setAttribute("aria-pressed", (b.dataset.id || "") === (id || ""));
  if (view) view.show(pick.model, wornFiles());
}

function wornFiles() {
  return SLOTS.filter((s) => pick.wear[s]).map((s) => {
    const it = cat.wardrobe.find((w) => w.id === pick.wear[s]);
    return { slot: s, id: it.id, file: it.file };
  });
}

function build() {
  const groups = $("#groups");
  for (const [g, label] of Object.entries(GROUPS)) {
    const people = cat.characters.filter((c) => c.group === g);
    if (!people.length) continue;
    const grid = el("div", { class: "cast", role: "group", "aria-label": label });
    for (const c of people) {
      grid.append(el("button", { type: "button", "data-model": c.model, "aria-pressed": "false", title: c.about || c.name, onclick: () => choose(c.model) },
        el("img", { src: `/assets/${c.portrait}`, alt: "", loading: "lazy" }), c.name));
    }
    groups.append(el("h3", { text: label }), grid);
  }
  for (const s of SLOTS) {
    const box = $(`#${s}`);
    box.append(el("button", { type: "button", "data-id": "", "aria-pressed": "true", onclick: () => wear(s, null), text: "Nothing" }));
    for (const it of cat.wardrobe.filter((w) => w.slot === s)) {
      const label = it.label.replace(/^(a|an|the|some) /, "");
      box.append(el("button", { type: "button", "data-id": it.id, "aria-pressed": "false", onclick: () => wear(s, it.id), text: label }));
    }
  }
}

$("#save").addEventListener("click", async () => {
  $("#error").textContent = "";
  $("#save").disabled = true;
  try {
    await api("/api/character", "PUT", { model: pick.model, wear: SLOTS.filter((s) => pick.wear[s]).map((s) => pick.wear[s]) });
    $("#saved").textContent = "Saved.";
    setTimeout(() => { location.href = "/"; }, 500);
  } catch (err) {
    $("#error").textContent = err.message;
    $("#save").disabled = false;
  }
});

async function start() {
  const who = await me();
  if (!who.name) { location.href = "/"; return; }
  nav($("#top"));
  cat = await api("/api/catalogue");
  build();
  const have = who.character;
  choose(have ? have.model : cat.characters[0].model);
  if (have) for (const id of have.wear) {
    const it = cat.wardrobe.find((w) => w.id === id);
    if (it) wear(it.slot, id);
  }
  // the 3D preview; if WebGL isn't there, the portrait stands in
  try {
    const { Preview } = await import("/preview.js");
    const [fits, bare] = await Promise.all([api("/data/head_fit.json").catch(() => ({})), api("/data/bare_heads.json").catch(() => ({}))]);
    view = new Preview($("#view"), fits, bare);
    $("#loading").hidden = true;
    view.show(pick.model, wornFiles());
  } catch (err) {
    console.warn("no 3D preview:", err);
    $("#loading").hidden = true;
    $("#view").hidden = true;
    $("#fallback").hidden = false;
  }
}

start().catch((err) => { $("#error").textContent = err.message; });
