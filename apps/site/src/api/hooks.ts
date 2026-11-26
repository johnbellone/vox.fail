// Loading from the directory in components: data, error, and a way to load again.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./client";
import type { Me } from "./types";

export interface Loaded<T> {
  data: T | undefined;
  error: Error | undefined;
  loading: boolean;
  reload: () => Promise<void>;
}

/** Load with `fetcher` now and whenever `reload` is called; stale answers are dropped. */
export function useLoad<T>(fetcher: () => Promise<T>, deps: unknown[] = []): Loaded<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<Error>();
  const [loading, setLoading] = useState(true);
  const ticket = useRef(0);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const run = useCallback(fetcher, deps);
  const reload = useCallback(async () => {
    const mine = ++ticket.current;
    setLoading(true);
    try {
      const d = await run();
      if (mine === ticket.current) {
        setData(d);
        setError(undefined);
      }
    } catch (e) {
      if (mine === ticket.current) setError(e instanceof Error ? e : new Error(String(e)));
    } finally {
      if (mine === ticket.current) setLoading(false);
    }
  }, [run]);
  useEffect(() => {
    void reload();
  }, [reload]);
  return { data, error, loading, reload };
}

export const useApi = <T>(path: string) => useLoad(() => api<T>(path), [path]);

/** Who is signed in; sends anyone who isn't back to the start (and anyone without a
 *  character to the character maker, when `needCharacter`). */
export function useSignedIn({ needCharacter = false } = {}) {
  const me = useApi<Me>("/api/me");
  useEffect(() => {
    if (!me.data) return;
    if (!me.data.name) location.href = "/";
    else if (needCharacter && !me.data.character) location.href = "/create";
  }, [me.data, needCharacter]);
  return me;
}
