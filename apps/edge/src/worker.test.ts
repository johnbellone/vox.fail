// The Worker with a stand-in for the server: passes things through while it's up, and stands in
// for it (offline page, JSON for the API) only when Cloudflare says it can't be reached.

import { afterEach, describe, expect, test } from "bun:test";
import worker, { type Env } from "./worker";

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

/** The server answers every request with `status` (or can't be reached at all, with "throw"). */
function server(status: number | "throw") {
  const seen: string[] = [];
  globalThis.fetch = (async (req: Request) => {
    seen.push(new URL(req.url).pathname);
    if (status === "throw") throw new Error("unreachable");
    return new Response(`server ${status}`, { status });
  }) as typeof fetch;
  return seen;
}

const env: Env = {
  ASSETS: {
    async fetch(req) {
      const path = new URL(typeof req === "string" ? req : req.url).pathname;
      return new Response(`asset ${path}`, { status: 200 });
    },
  },
};

const page = (path: string) => new Request(`https://vox.fail${path}`, { headers: { accept: "text/html" } });
const api = (path: string) => new Request(`https://vox.fail${path}`, { headers: { accept: "application/json" } });

describe("while the server is up", () => {
  test("pages, API answers and errors pass through unchanged", async () => {
    for (const status of [200, 302, 404, 503]) {
      server(status);
      const r = await worker.fetch(page("/play"), env);
      expect(r.status).toBe(status);
      expect(await r.text()).toBe(`server ${status}`);
    }
    server(503); // the directory's own "busy"
    expect(await (await worker.fetch(api("/api/me"), env)).text()).toBe("server 503");
  });
});

describe("while the server is down", () => {
  for (const status of [530, 502, 522, "throw"] as const) {
    test(`a page gets the offline page (${status})`, async () => {
      server(status);
      const r = await worker.fetch(page("/"), env);
      expect(r.status).toBe(503);
      expect(r.headers.get("content-type")).toContain("text/html");
      expect(r.headers.get("cache-control")).toBe("no-store");
      expect(await r.text()).toBe("asset /_offline/offline.html");
    });
  }

  test("the API answers JSON the site can show", async () => {
    server(530);
    const r = await worker.fetch(api("/api/me"), env);
    expect(r.status).toBe(503);
    expect(((await r.json()) as { error: string }).error).toContain("offline");
  });

  test("a game file keeps the server's answer", async () => {
    server(530);
    const r = await worker.fetch(new Request("https://vox.fail/pkg/vox_client_bg.wasm"), env);
    expect(r.status).toBe(530);
  });
});

test("the offline page's own files never go to the server", async () => {
  const seen = server(200);
  const r = await worker.fetch(page("/_offline/shots/void.png"), env);
  expect(await r.text()).toBe("asset /_offline/shots/void.png");
  expect(seen).toEqual([]);
});
