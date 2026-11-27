// The Worker with stand-ins for its files and the servers: the site comes from its files with the
// directory's headers, the browser client in its compressed copies, /api/ /auth/ /w/ go on to the
// servers, the internal routes never do, and the offline answers stand in only when the servers
// can't be reached.

import { afterEach, describe, expect, test } from "bun:test";
import { backTo } from "./back";
import { portFor } from "./ports";
import worker, { encodings, type Env } from "./worker";

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

/** The servers answer every request with `status` (or can't be reached at all, with "throw"). */
function servers(status: number | "throw") {
  const seen: string[] = [];
  globalThis.fetch = (async (req: Request) => {
    seen.push(new URL(req.url).pathname);
    if (status === "throw") throw new Error("unreachable");
    return new Response(`server ${status}`, { status });
  }) as typeof fetch;
  return seen;
}

/** The servers' Durable Object, which hands requests to the stand-in servers (fetch above). */
const named: string[] = [];
const forwarded: Request[] = [];
const SERVERS: Env["SERVERS"] = {
  idFromName(name) {
    named.push(name);
    return name;
  },
  get() {
    return {
      fetch(req: Request) {
        forwarded.push(req);
        return globalThis.fetch(req);
      },
    };
  },
};

/** The Worker's files: `files` by path; anything else is a 404. */
function files(have: Record<string, string>): Env {
  return {
    SERVERS,
    ASSETS: {
      async fetch(req) {
        const p = new URL(typeof req === "string" ? req : req.url).pathname;
        const body = have[p];
        return body === undefined ? new Response("not found", { status: 404 }) : new Response(body, { status: 200 });
      },
    },
  };
}

const env = files({
  "/": "index",
  "/play": "play page",
  "/assets/models/dog.glb": "dog",
  "/pkg/vox_client_bg.wasm.br": "wasm-br",
  "/pkg/vox_client_bg.wasm.gz": "wasm-gz",
  "/pkg/vox_client.js": "js",
  "/_offline/offline.html": "offline page",
});

const get = (path: string, headers: Record<string, string> = {}) => new Request(`https://vox.fail${path}`, { headers });
const page = (path: string) => get(path, { accept: "text/html" });

describe("the site's files", () => {
  test("pages come from the files, with the directory's headers, never from the servers", async () => {
    const seen = servers(200);
    for (const [path, body] of [
      ["/", "index"],
      ["/play", "play page"],
    ] as const) {
      const r = await worker.fetch(page(path), env);
      expect(r.status).toBe(200);
      expect(await r.text()).toBe(body);
      expect(r.headers.get("cache-control")).toBe("no-cache");
      expect(r.headers.get("x-frame-options")).toBe("DENY");
      expect(r.headers.get("x-content-type-options")).toBe("nosniff");
      expect(r.headers.get("content-security-policy")).toContain("'wasm-unsafe-eval'");
    }
    expect(seen).toEqual([]);
  });

  test("game files are kept five minutes; a missing file isn't", async () => {
    servers(200);
    const r = await worker.fetch(get("/assets/models/dog.glb"), env);
    expect(r.headers.get("cache-control")).toBe("public, max-age=300");
    const missing = await worker.fetch(get("/assets/nothing.glb"), env);
    expect(missing.status).toBe(404);
    expect(missing.headers.get("cache-control")).toBe("no-cache");
  });

  test("the browser client goes out in the compressed copy the browser takes", async () => {
    servers(200);
    const br = await worker.fetch(get("/pkg/vox_client_bg.wasm", { "accept-encoding": "gzip, deflate, br" }), env);
    expect(await br.text()).toBe("wasm-br");
    expect(br.headers.get("content-encoding")).toBe("br");
    expect(br.headers.get("content-type")).toBe("application/wasm");
    expect(br.headers.get("cache-control")).toBe("public, max-age=300");
    const gz = await worker.fetch(get("/pkg/vox_client_bg.wasm", { "accept-encoding": "gzip" }), env);
    expect(await gz.text()).toBe("wasm-gz");
    expect(gz.headers.get("content-encoding")).toBe("gzip");
    // a file without compressed copies comes as it is
    const js = await worker.fetch(get("/pkg/vox_client.js", { "accept-encoding": "br" }), env);
    expect(await js.text()).toBe("js");
    expect(js.headers.get("content-encoding")).toBeNull();
  });

  test("the offline page's own files get the site's headers", async () => {
    servers(200);
    const r = await worker.fetch(page("/_offline/offline.html"), env);
    expect(await r.text()).toBe("offline page");
    expect(r.headers.get("content-security-policy")).toContain("frame-ancestors 'none'");
  });

  test("only GET and HEAD for files", async () => {
    servers(200);
    const r = await worker.fetch(new Request("https://vox.fail/play", { method: "POST" }), env);
    expect(r.status).toBe(405);
  });
});

describe("the servers", () => {
  test("/api/, /auth/ and /w/ go on to them, answers unchanged (the directory's 503 too)", async () => {
    for (const status of [200, 302, 404, 503]) {
      const seen = servers(status);
      for (const path of ["/api/me", "/auth/discord", "/w/void/ws"]) {
        const r = await worker.fetch(get(path), env);
        expect(r.status).toBe(status);
        expect(await r.text()).toBe(`server ${status}`);
      }
      expect(seen).toEqual(["/api/me", "/auth/discord", "/w/void/ws"]);
    }
  });

  test("they're one Durable Object, told the visitor's address", async () => {
    servers(200);
    named.length = 0;
    forwarded.length = 0;
    await worker.fetch(get("/api/me", { "cf-connecting-ip": "203.0.113.7" }), env);
    expect(named).toEqual(["vox"]);
    expect(forwarded[0]?.headers.get("cf-connecting-ip")).toBe("203.0.113.7");
  });

  test("worlds go to vox-host, the rest to the directory", () => {
    expect(portFor("/w/void/ws")).toBe(7790);
    expect(portFor("/w/void/")).toBe(7790);
    expect(portFor("/api/me")).toBe(7700);
    expect(portFor("/auth/discord")).toBe(7700);
  });

  test("the internal routes never reach them", async () => {
    const seen = servers(200);
    for (const path of ["/api/world/report", "/api/world/void/rules", "/api/host/instance"]) {
      const r = await worker.fetch(new Request(`https://vox.fail${path}`, { method: "POST" }), env);
      expect(r.status).toBe(404);
    }
    expect(seen).toEqual([]);
  });

  for (const status of [530, 502, 522, "throw"] as const) {
    test(`when they can't be reached (${status}), the API says vox is offline`, async () => {
      servers(status);
      const r = await worker.fetch(get("/api/me", { accept: "application/json" }), env);
      expect(r.status).toBe(503);
      expect(((await r.json()) as { error: string }).error).toStartWith("vox is offline");
      expect(r.headers.get("cache-control")).toBe("no-store");
    });
  }

  test("a sign-in page while they're down gets the offline page", async () => {
    servers(530);
    const r = await worker.fetch(page("/auth/discord"), env);
    expect(r.status).toBe(503);
    expect(await r.text()).toBe("offline page");
  });

  test("a world's socket keeps Cloudflare's answer", async () => {
    servers(530);
    const r = await worker.fetch(get("/w/void/ws"), env);
    expect(r.status).toBe(530);
  });
});

test("the offline page's Try again goes back only to a page of this site", () => {
  const here = "https://vox.fail";
  expect(backTo("?back=%2Fplay%3Fworld%3Dvoid", here)).toBe("/play?world=void");
  expect(backTo("", here)).toBe("/");
  const bad = ["https://evil.example/", "//evil.example/", "/\\evil.example", "/\\/evil.example", "javascript:alert(1)",
    "/\t/evil.example", "/\n/evil.example", "/ /evil.example", "/_offline/offline.html"];
  for (const b of bad) expect(backTo(`?back=${encodeURIComponent(b)}`, here)).toBe("/");
  // an encoded backslash stays a harmless part of a path here
  expect(backTo("?back=%2F%255C%2Fx", here)).toBe("/%5C/x");
});

test("Accept-Encoding: q=0 refuses an encoding", () => {
  expect([...encodings("gzip, deflate, br")]).toEqual(["gzip", "deflate", "br"]);
  expect(encodings("br;q=0, gzip").has("br")).toBe(false);
  expect(encodings("br;q=0.5").has("br")).toBe(true);
  expect(encodings("").size).toBe(0);
});
