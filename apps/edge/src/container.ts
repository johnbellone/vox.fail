// The Durable Object that runs vox.fail's servers in one container (docs/design/cloudflare-hosting.md).
// It starts the container on the first request (deploy/docker/start.sh: restore from R2, then the
// directory and vox-host under Litestream), waits until both servers answer, and passes requests
// on: /w/ to vox-host (7790), the rest to the directory (7700). Cloudflare stops the container
// after IDLE_MS with no requests; it saves and syncs to R2 on the way out.
//
// World sockets are relayed here, so it knows how many are open: while any are, an alarm keeps
// the container awake, since a quiet game may send nothing Cloudflare counts as a request.
//
// When it can't start the servers it answers 530, which the Worker turns into its offline answers.

import { DurableObject } from "cloudflare:workers";
import { DIRECTORY_PORT as DIRECTORY, HOST_PORT as HOST, portFor } from "./ports";

/** The Worker's secrets and settings the container needs (set by deploy/cloudflare-setup.sh). */
export const CONTAINER_ENV = [
  "VOX_WORLD_SECRET",
  "VOX_DISCORD_CLIENT_ID",
  "VOX_DISCORD_CLIENT_SECRET",
  "VOX_STEAM_KEY",
  "VOX_DIRECTORY_KEY",
  "LITESTREAM_ACCESS_KEY_ID",
  "LITESTREAM_SECRET_ACCESS_KEY",
  "VOX_R2_BUCKET",
  "VOX_R2_ENDPOINT",
] as const;
export type ContainerEnv = Partial<Record<(typeof CONTAINER_ENV)[number], string>>;

const IDLE_MS = 15 * 60 * 1000; // asleep after a quarter of an hour without requests or players
const READY_MS = 120 * 1000; // restoring from R2 and starting both servers
const BOOT_MS = 10 * 1000; // how long `running` may stay false right after start()
const KEEPALIVE_MS = 5 * 60 * 1000; // while sockets are open, renew the timeout this often

const isUpgrade = (req: Request) => req.headers.get("upgrade")?.toLowerCase() === "websocket";

export class VoxServers extends DurableObject<ContainerEnv> {
  /** The current start, and which one it is (a late result from an older one is ignored). */
  private ready: Promise<void> | undefined;
  private generation = 0;
  private settled = false;
  /** Open world sockets relayed through here. */
  private sockets = 0;

  constructor(ctx: DurableObjectState, env: ContainerEnv) {
    super(ctx, env);
    // after a deploy restarts this object, a running container still needs its timeout
    const c = ctx.container;
    if (c?.running) void ctx.blockConcurrencyWhile(() => c.setInactivityTimeout(IDLE_MS));
  }

  override async fetch(req: Request): Promise<Response> {
    const upgrade = isUpgrade(req);
    const ready = this.ensureReady();
    if (upgrade && !this.settled) {
      // a world socket mustn't hang through a cold start: start in the background, ask to retry
      this.ctx.waitUntil(ready.catch(() => {}));
      return new Response("the worlds are starting; try again in a few seconds", {
        status: 503,
        headers: { "retry-after": "5" },
      });
    }
    try {
      await ready;
    } catch (e) {
      console.error("vox: the servers didn't start:", e);
      return new Response("the servers didn't start", { status: 530 });
    }
    try {
      const res = await this.ctx.container!.getTcpPort(portFor(new URL(req.url).pathname)).fetch(req);
      return upgrade ? this.relay(res) : res;
    } catch (e) {
      console.error("vox: the servers can't be reached:", e);
      return new Response("the servers can't be reached", { status: 502 });
    }
  }

  /** While world sockets are open, keep the container from its idle stop. */
  override async alarm(): Promise<void> {
    const c = this.ctx.container;
    if (this.sockets > 0 && c?.running) {
      await c.setInactivityTimeout(IDLE_MS);
      await this.ctx.storage.setAlarm(Date.now() + KEEPALIVE_MS);
    }
  }

  /** Pass a world socket through, counting it while it's open. */
  private relay(res: Response): Response {
    const upstream = res.webSocket;
    if (!upstream) return res; // refused (a bad ticket, say): the answer as it is
    const pair = new WebSocketPair();
    const [client, server] = [pair[0], pair[1]];
    upstream.accept();
    server.accept();
    this.sockets++;
    void this.ctx.storage.setAlarm(Date.now() + KEEPALIVE_MS);
    let open = true;
    const close = (code?: number, reason?: string) => {
      if (!open) return;
      open = false;
      this.sockets--;
      const c = code === 1005 || code === 1006 || code === undefined ? 1000 : code;
      for (const ws of [upstream, server]) {
        try {
          ws.close(c, reason ?? "");
        } catch {
          // already closed
        }
      }
    };
    server.addEventListener("message", (e) => upstream.send(e.data));
    upstream.addEventListener("message", (e) => server.send(e.data));
    for (const ws of [upstream, server]) {
      ws.addEventListener("close", (e) => close(e.code, e.reason));
      ws.addEventListener("error", () => close(1011, "relay error"));
    }
    return new Response(null, { status: 101, webSocket: client });
  }

  /** One start at a time; every request waits on the same one until it settles. */
  private ensureReady(): Promise<void> {
    const running = this.ctx.container!.running;
    // a settled start whose container has since stopped is over: begin a new one
    if (this.ready && this.settled && !running) this.ready = undefined;
    if (!this.ready) {
      const gen = ++this.generation;
      this.settled = false;
      this.ready = this.start(gen).then(
        () => {
          if (gen === this.generation) this.settled = true;
        },
        (e) => {
          if (gen === this.generation) this.ready = undefined;
          throw e;
        },
      );
    }
    return this.ready;
  }

  private async start(gen: number): Promise<void> {
    const c = this.ctx.container!;
    const began = Date.now();
    if (!c.running) {
      const env: Record<string, string> = {};
      for (const k of CONTAINER_ENV) {
        const v = this.env[k];
        if (v) env[k] = v;
      }
      try {
        c.start({
          image: c.images.vox,
          instance: "basic",
          enableInternet: true, // R2, and Steam and Discord for sign-in
          entrypoint: ["/usr/bin/tini", "--", "/opt/vox/bin/start.sh"],
          env,
        });
      } catch (e) {
        // started by someone else meanwhile: wait for it like everyone else
        if (!c.running) throw e;
      }
    }
    // every start (or adoption of a running container) sets the timeout and watches the exit
    await c.setInactivityTimeout(IDLE_MS);
    this.ctx.waitUntil(
      c
        .monitor()
        .then(() => console.log("vox: the container stopped"))
        .catch((e) => console.error("vox: the container failed:", e))
        .finally(() => {
          // only this start's: a newer one has its own
          if (gen === this.generation) {
            this.ready = undefined;
            this.settled = false;
          }
        }),
    );
    // start.sh restores from R2 first, so the servers come up after a few seconds
    for (const port of [DIRECTORY, HOST]) {
      for (;;) {
        try {
          const r = await c.getTcpPort(port).fetch("http://container/");
          if (r.status < 500) break;
        } catch {
          // not listening yet
        }
        const waited = Date.now() - began;
        if (!c.running && waited > BOOT_MS) throw new Error("the container stopped while starting");
        if (waited > READY_MS) throw new Error(`port ${port} didn't answer within ${READY_MS / 1000} s`);
        await new Promise((r) => setTimeout(r, 500));
      }
    }
  }
}
