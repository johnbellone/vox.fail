// Talking to the directory. Same origin, cookies for the session, JSON both ways.

import type { Catalogue, CastMember } from "./types";

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

/**
 * The servers are asleep or down and couldn't be woken: the edge Worker (apps/edge) says so
 * with a 503 starting "vox is offline". Show its offline page, which comes back here after.
 */
export function offlineAddress(said: string, status: number, here: string): string | undefined {
  if (status !== 503 || !said.startsWith("vox is offline")) return undefined;
  return `/_offline/offline.html?back=${encodeURIComponent(here)}`;
}

export async function api<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const init: RequestInit = { method, credentials: "same-origin", headers: {} };
  if (body !== undefined) {
    init.headers = { "Content-Type": "application/json" };
    init.body = JSON.stringify(body);
  }
  const r = await fetch(path, init);
  let data: unknown = null;
  try {
    data = await r.json();
  } catch {
    data = null;
  }
  if (!r.ok) {
    const said = data && typeof data === "object" && "error" in data ? String((data as { error: unknown }).error) : "";
    const away = offlineAddress(said, r.status, location.pathname + location.search);
    // never from the offline page itself (it asks nothing today; this keeps it from looping)
    if (away && !location.pathname.startsWith("/_offline/")) location.replace(away);
    throw new ApiError(said || `the directory answered ${r.status}`, r.status);
  }
  return data as T;
}

export const post = <T>(path: string, body: unknown = {}) => api<T>(path, "POST", body);

/** The directory's messages are lower-case fragments: make one a sentence. */
export function sentence(s: string): string {
  if (!s) return s;
  return s.charAt(0).toUpperCase() + s.slice(1) + (/[.!?]$/.test(s) ? "" : ".");
}

export const messageOf = (err: unknown) => sentence(err instanceof Error ? err.message : String(err));

/** A character's catalogue entry, by its model. */
export const findCast = (cat: Catalogue | undefined, model: string | undefined): CastMember | undefined =>
  cat?.characters.find((c) => c.model === model);

/** A world's address: ":7780" is this host, that port; "/w/7780/ws" a path on this host. */
export function worldUrl(url: string): string {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  if (url.startsWith(":")) return `${scheme}://${location.hostname}${url}/ws`;
  if (url.startsWith("/")) return `${scheme}://${location.host}${url}`;
  return url;
}

export const when = (t: number) =>
  new Date(t * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });

export const num = (n: number) => Number(n).toLocaleString();
