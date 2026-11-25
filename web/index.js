import { api, me, $, el, find } from "/vox.js";

let mode = "in";

function setMode(m) {
  mode = m;
  $("#tab-in").setAttribute("aria-selected", m === "in");
  $("#tab-up").setAttribute("aria-selected", m === "up");
  $("#go").textContent = m === "in" ? "Sign in" : "Make my account";
  $("#name-hint").hidden = m === "in";
  $("#pass-hint").hidden = m === "in";
  $("#password").autocomplete = m === "in" ? "current-password" : "new-password";
  $("#error").textContent = "";
}

async function show() {
  const who = await me();
  if (!who.name) {
    $("#out").hidden = false;
    $("#in").hidden = true;
    $("#name").focus();
    return;
  }
  // signed in but no character yet: make one first
  if (!who.character) {
    location.href = "/create";
    return;
  }
  const cat = await api("/api/catalogue");
  const c = find(cat, who.character.model);
  $("#out").hidden = true;
  $("#in").hidden = false;
  $("#who").textContent = who.name;
  $("#as").textContent = c ? `as ${c.name}` : "";
  if (c) $("#portrait").src = `/assets/${c.portrait}`;
  const w = await api("/api/worlds");
  const links = $("#dl-links");
  links.replaceChildren();
  for (const d of w.downloads || []) links.append(el("a", { class: "button plain", href: d.url, text: `Download for ${d.label}` }));
  if (!(w.downloads || []).length) {
    $("#dl-text").textContent = "Running the desktop app from source? Copy your launch code and start it with: vox-client --token <code>";
  }
}

$("#tab-in").addEventListener("click", () => setMode("in"));
$("#tab-up").addEventListener("click", () => setMode("up"));

$("#form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const name = $("#name").value.trim();
  const password = $("#password").value;
  $("#go").disabled = true;
  $("#error").textContent = "";
  try {
    await api(mode === "in" ? "/api/signin" : "/api/signup", "POST", { name, password });
    await show();
  } catch (err) {
    $("#error").textContent = err.message.charAt(0).toUpperCase() + err.message.slice(1) + ".";
  } finally {
    $("#go").disabled = false;
  }
});

$("#signout").addEventListener("click", async () => {
  await api("/api/signout", "POST", {});
  setMode("in");
  await show();
});

$("#code-btn").addEventListener("click", async () => {
  try {
    const t = await api("/api/token", "POST", {});
    await navigator.clipboard.writeText(t.token);
    $("#code-note").textContent = "Copied. It works for a day.";
  } catch (err) {
    $("#code-note").textContent = err.message;
  }
});

setMode("in");
show().catch((err) => { $("#error").textContent = err.message; $("#out").hidden = false; });
