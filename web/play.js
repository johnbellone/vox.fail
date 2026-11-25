// Playing in the browser: get a sign-in token, then start the WebAssembly client
// (web/pkg/vox_client.js, built by tools/build_web.sh) on the canvas.

import { api, me, $, worldUrl } from "/vox.js";

let stage = 0;
const STAGES = 5;

// the client reports how it's getting on through these
window.vox_status = (text) => {
  stage = Math.min(stage + 1, STAGES);
  $("#bar").style.width = `${Math.round((stage / STAGES) * 100)}%`;
  $("#status").textContent = text;
};
window.vox_ready = () => {
  $("#bar").style.width = "100%";
  $("#opening").classList.add("gone");
  $("#keys").hidden = false;
  $("#game").focus();
  setTimeout(() => { $("#keys").hidden = true; }, 12000);
};
window.vox_error = (text) => fail(text);

function fail(text) {
  $("#error").textContent = text;
  $("#back").hidden = false;
}

async function start() {
  const who = await me();
  if (!who.name) { location.href = "/"; return; }
  if (!who.character) { location.href = "/create"; return; }
  window.vox_status("Signing you in…");
  const t = await api("/api/token", "POST", {});
  const want = new URLSearchParams(location.search).get("world");
  const world = t.worlds.find((w) => w.name === want) || t.worlds[0];
  if (!world) throw new Error("There are no worlds to go to yet.");
  document.title = `${world.name} · Vox`;
  window.vox_status(`Loading the client…`);
  let client;
  try {
    client = await import("/pkg/vox_client.js");
    await client.default();
  } catch (err) {
    console.error(err);
    throw new Error("The browser client isn't installed on this server yet (tools/build_web.sh builds it).");
  }
  window.vox_status(`Going to ${world.name}…`);
  client.start(JSON.stringify({
    canvas: "#game",
    url: worldUrl(world.url),
    token: t.token,
    name: t.name,
    model: t.character.model,
    wear: t.character.wear,
    base: location.origin,
  }));
}

start().catch((err) => fail(err.message));
