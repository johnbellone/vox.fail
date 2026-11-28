// vox.fail's front door on Cloudflare (docs/design/cloudflare-hosting.md). The Worker serves the
// site itself: the pages, the browser client (/pkg/), /assets/ and /data/, from its static files,
// with the headers the directory used to set. Only /api/, /auth/ and /w/ go on to the servers,
// which run in a container behind the Durable Object SERVERS (src/container.ts). When they can't
// be started, the API answers a JSON 503 that the site turns into the offline page, and a page
// request gets that page.

export interface Env {
  /** The site's files (dist/: the pages, pkg/, assets/, data/, _offline/). */
  ASSETS: { fetch(req: Request | string): Promise<Response> };
  /** The Durable Object that runs the servers' container (one, named "vox"). */
  SERVERS: { idFromName(name: string): unknown; get(id: unknown): { fetch(req: Request): Promise<Response> } };
}

const OFFLINE_PAGE = "/_offline/offline.html";

/** What goes on to the servers. */
const DYNAMIC = ["/api/", "/auth/", "/w/"];
/** For the servers on localhost only (world reports, travel, dungeon instances): never from outside. */
const INTERNAL = ["/api/world/", "/api/host/"];
/** A world's link to its neighbours (`/w/<id>/link`, ADR 0005): between worlds inside the container only. */
const LINK = /^\/w\/[^/]+\/link\/?$/;
const internal = (path: string) => starts(path, INTERNAL) || LINK.test(path) || path === "/link";

// What says the servers aren't there: 530 (the container didn't start), 502 (it can't be
// reached), 504 and the 52x errors. Not 503: the directory itself says 503 when it's busy, and
// that should reach the page as it is.
const DOWN = new Set([502, 504, 520, 521, 522, 523, 524, 525, 526, 527, 530]);
export const isDown = (status: number) => DOWN.has(status);

// The directory's headers (bins/vox-directory/src/routes.rs), now set here for the files.
const CSP =
  "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; " +
  "img-src 'self' data: blob:; connect-src 'self' blob: data: ws: wss: http: https:; frame-ancestors 'none'";
const SECURITY: Record<string, string> = {
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY",
  "referrer-policy": "same-origin",
  "content-security-policy": CSP,
};
const KEPT = ["/pkg/", "/assets/", "/data/"]; // the game's files: five minutes in caches

const OFFLINE_HEADERS = { ...SECURITY, "cache-control": "no-store", "retry-after": "120" };

const starts = (path: string, prefixes: string[]) => prefixes.some((p) => path.startsWith(p));

/** The headers a file from the site gets, as the directory gave them. */
export function siteHeaders(path: string, status: number): Record<string, string> {
  const kept = starts(path, KEPT) && status >= 200 && status < 300;
  return { ...SECURITY, "cache-control": kept ? "public, max-age=300" : "no-cache" };
}

function withHeaders(res: Response, extra: Record<string, string>): Response {
  const out = new Response(res.body, res);
  for (const [k, v] of Object.entries(extra)) out.headers.set(k, v);
  return out;
}

function wantsPage(req: Request): boolean {
  if (req.method !== "GET" && req.method !== "HEAD") return false;
  if (req.headers.get("sec-fetch-dest") === "document") return true;
  return (req.headers.get("accept") ?? "").includes("text/html");
}

const TYPES: Record<string, string> = {
  wasm: "application/wasm",
  js: "text/javascript; charset=utf-8",
  json: "application/json",
};

/**
 * /pkg/: the browser client. Its .wasm is bigger than a Worker's file limit, so the build keeps
 * only its brotli and gzip copies (tools/dev/build_web.sh makes them); this sends whichever the
 * browser takes, marked as such.
 */
/** The encodings an Accept-Encoding header takes, leaving out any it refuses with q=0. */
export function encodings(header: string): Set<string> {
  const out = new Set<string>();
  for (const part of header.split(",")) {
    const [name, ...params] = part.trim().toLowerCase().split(";");
    const q = params.map((p) => p.trim()).find((p) => p.startsWith("q="));
    if (name && !(q && Number(q.slice(2)) === 0)) out.add(name.trim());
  }
  return out;
}

async function pkg(req: Request, env: Env, url: URL): Promise<Response> {
  const accepts = encodings(req.headers.get("accept-encoding") ?? "");
  const ext = url.pathname.split(".").pop() ?? "";
  for (const [enc, suffix] of [
    ["br", ".br"],
    ["gzip", ".gz"],
  ] as const) {
    if (!accepts.has(enc)) continue;
    const r = await env.ASSETS.fetch(new URL(url.pathname + suffix, url).toString());
    if (!r.ok) continue;
    const headers = new Headers(r.headers);
    headers.set("content-encoding", enc);
    headers.set("content-type", TYPES[ext] ?? "application/octet-stream");
    headers.set("vary", "accept-encoding");
    for (const [k, v] of Object.entries(siteHeaders(url.pathname, 200))) headers.set(k, v);
    // the body is already compressed: Workers mustn't compress it again
    return new Response(req.method === "HEAD" ? null : r.body, { status: 200, headers, encodeBody: "manual" } as ResponseInit);
  }
  // A browser taking neither gets the file as it is; the .wasm itself isn't among the Worker's
  // files (too big), so that's a 404, but every browser that runs the client takes gzip.
  const plain = await env.ASSETS.fetch(req);
  return withHeaders(plain, siteHeaders(url.pathname, plain.status));
}

async function offline(req: Request, env: Env, url: URL, upstream: Response | undefined): Promise<Response> {
  if (url.pathname.startsWith("/api/")) {
    return Response.json({ error: "vox is offline right now; try again later" }, { status: 503, headers: OFFLINE_HEADERS });
  }
  if (wantsPage(req)) {
    const page = await env.ASSETS.fetch(new URL(OFFLINE_PAGE, url).toString());
    return new Response(req.method === "HEAD" ? null : page.body, {
      status: 503,
      headers: { ...OFFLINE_HEADERS, "content-type": "text/html; charset=utf-8" },
    });
  }
  // a world's socket: nothing useful to stand in for it
  return upstream ?? new Response("offline", { status: 503, headers: OFFLINE_HEADERS });
}

/**
 * On to the servers; when they can't be reached, the offline answers. The visitor's address goes
 * with it: the directory counts sign-in tries by cf-connecting-ip (--behind-proxy).
 */
async function toServers(req: Request, env: Env, url: URL): Promise<Response> {
  const forward = new Request(req);
  const ip = req.headers.get("cf-connecting-ip");
  if (ip) forward.headers.set("cf-connecting-ip", ip);
  // the directory's fallback when cf-connecting-ip is missing; never let a visitor choose it
  forward.headers.delete("x-forwarded-for");
  let upstream: Response | undefined;
  try {
    upstream = await env.SERVERS.get(env.SERVERS.idFromName("vox")).fetch(forward);
  } catch {
    upstream = undefined;
  }
  if (upstream && !isDown(upstream.status)) return upstream;
  return offline(req, env, url, upstream);
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    const path = url.pathname;
    if (internal(path)) {
      return Response.json({ error: "not found" }, { status: 404, headers: { ...SECURITY, "cache-control": "no-store" } });
    }
    if (starts(path, DYNAMIC)) return toServers(req, env, url);

    // the site's files
    if (req.method !== "GET" && req.method !== "HEAD") {
      return new Response("method not allowed", { status: 405, headers: { ...SECURITY, allow: "GET, HEAD" } });
    }
    if (path.startsWith("/_offline/")) {
      const res = await env.ASSETS.fetch(req);
      return withHeaders(res, siteHeaders(path, res.status));
    }
    if (path.startsWith("/pkg/")) return pkg(req, env, url);
    const res = await env.ASSETS.fetch(req);
    return withHeaders(res, siteHeaders(path, res.status));
  },
};
