// vox.fail's front door on Cloudflare. Every request goes on to the server through the tunnel, as
// before; when the server can't be reached, pages get the offline page (what Vox is, and that it's
// away for now) and the API a JSON answer the site already knows how to show.
//
// A Worker's own fetch to its zone goes to the origin, not back through the Worker.

export interface Env {
  /** The offline page's files (dist/, all under /_offline/). */
  ASSETS: { fetch(req: Request | string): Promise<Response> };
}

const OFFLINE_PAGE = "/_offline/offline.html";

// What Cloudflare or cloudflared answer when the server isn't there: 502 (cloudflared can't reach
// the service), 504, the 52x origin errors and 530 (no tunnel, error 1033). Not 503: the directory
// itself says 503 when it's busy, and that should reach the page as it is.
const DOWN = new Set([502, 504, 520, 521, 522, 523, 524, 525, 526, 527, 530]);

export const isDown = (status: number) => DOWN.has(status);

const OFFLINE_HEADERS = {
  "cache-control": "no-store",
  "retry-after": "120",
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY",
  "referrer-policy": "same-origin",
  "content-security-policy":
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'",
};

function wantsPage(req: Request): boolean {
  if (req.method !== "GET" && req.method !== "HEAD") return false;
  if (req.headers.get("sec-fetch-dest") === "document") return true;
  return (req.headers.get("accept") ?? "").includes("text/html");
}

async function offline(req: Request, env: Env, upstream: Response | undefined): Promise<Response> {
  const url = new URL(req.url);
  if (url.pathname.startsWith("/api/")) {
    return Response.json(
      { error: "vox is offline right now; try again later" },
      { status: 503, headers: OFFLINE_HEADERS },
    );
  }
  if (wantsPage(req)) {
    const page = await env.ASSETS.fetch(new URL(OFFLINE_PAGE, url).toString());
    return new Response(req.method === "HEAD" ? null : page.body, {
      status: 503,
      headers: { ...OFFLINE_HEADERS, "content-type": "text/html; charset=utf-8" },
    });
  }
  // a world's socket, a game file: nothing useful to stand in for it
  return upstream ?? new Response("offline", { status: 503, headers: OFFLINE_HEADERS });
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    if (url.pathname.startsWith("/_offline/")) return env.ASSETS.fetch(req);

    let upstream: Response | undefined;
    try {
      upstream = await fetch(req);
    } catch {
      upstream = undefined;
    }
    if (upstream && !isDown(upstream.status)) return upstream;
    return offline(req, env, upstream);
  },
};
