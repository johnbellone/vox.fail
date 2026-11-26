import { useState } from "react";
import { messageOf, post } from "../../api/client";

/** Send something to the directory, then reload; keeps the last problem to show. */
export function useAct(reload: () => Promise<void>) {
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  async function act<T>(path: string, body: unknown): Promise<T | null> {
    setBusy(true);
    setError(undefined);
    try {
      const r = await post<T>(path, body);
      await reload();
      return r;
    } catch (err) {
      setError(messageOf(err));
      return null;
    } finally {
      setBusy(false);
    }
  }
  return { act, error, busy };
}
