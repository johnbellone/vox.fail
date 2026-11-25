// What every Warren page shares: talking to the directory.

export async function api(path, method = "GET", body) {
  const opts = { method, credentials: "same-origin", headers: {} };
  if (body !== undefined) {
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  const r = await fetch(path, opts);
  let data = null;
  try { data = await r.json(); } catch { data = null; }
  if (!r.ok) {
    const e = new Error((data && data.error) || `the directory answered ${r.status}`);
    e.status = r.status;
    throw e;
  }
  return data;
}

export const me = () => api("/api/me");

/** A character's catalogue entry, by its model. */
export function find(catalogue, model) {
  return catalogue.characters.find((c) => c.model === model);
}

/** A world's address: ":7780" is this host, that port. */
export function worldUrl(url) {
  if (!url.startsWith(":")) return url;
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${location.hostname}${url}/ws`;
}

export function $(sel, root = document) { return root.querySelector(sel); }

export function el(tag, attrs = {}, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "text") e.textContent = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v);
  }
  for (const k of kids) e.append(k);
  return e;
}
