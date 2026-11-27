// The prop catalogue, shown in the left column while the Prop tool is held: the accepted props of
// data/props/catalogue.json, with a text filter and a thumbnail each. Choosing one tells the client
// which prop the Prop tool places; the client checks the hash again. A missing or empty catalogue
// says so, and a thumbnail that is not there hides itself.

import { Box, Button, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useMemo, useState } from "react";
import { messageOf } from "../../api/client";
import type { Command } from "../../game/editor";
import { filterCatalogue, MAX_CATALOGUE_BYTES, type PropEntry, readCatalogue } from "./state";

/** The catalogue as the page holds it: `entries` is undefined while it loads, and `error` says why
 *  it could not be read. The editor page loads it once and gives it to every panel that names props. */
export interface Catalogue {
  entries?: PropEntry[];
  error?: string;
}

const CATALOGUE_URL = "/data/props/catalogue.json";

/** The command that makes the prop with `hash` the one the Prop tool places. */
export function chooseProp(hash: string): Command {
  return { type: "prop", prop: hash };
}

/** Fetches the catalogue, reading at most MAX_CATALOGUE_BYTES. A missing file is an empty catalogue. */
async function loadCatalogue(signal: AbortSignal): Promise<PropEntry[]> {
  const res = await fetch(CATALOGUE_URL, { signal, cache: "no-cache" });
  if (res.status === 404) return [];
  if (!res.ok) throw new Error(`the server answered ${res.status}`);
  const bytes = await res.arrayBuffer();
  if (bytes.byteLength > MAX_CATALOGUE_BYTES) throw new Error("the catalogue is too large");
  let json: unknown;
  try {
    json = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    return [];
  }
  return readCatalogue(json);
}

interface Props {
  /** the prop the Prop tool places, by hash */
  chosen?: string;
  onChoose: (hash: string) => void;
}

/** Loads the catalogue once, for the page to hold. */
export function useCatalogue(): Catalogue {
  const [entries, setEntries] = useState<PropEntry[]>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    const ctrl = new AbortController();
    loadCatalogue(ctrl.signal)
      .then(setEntries)
      .catch((err: unknown) => {
        if (!ctrl.signal.aborted) setError(messageOf(err));
      });
    return () => ctrl.abort();
  }, []);
  return { entries, error };
}

export function PropCatalogue({ entries, error, chosen, onChoose }: Props & Catalogue) {
  const [text, setText] = useState("");

  const shown = useMemo(() => filterCatalogue(entries ?? [], text), [entries, text]);

  return (
    <Box>
      <Text fw={700} fz="sm" mb="xs">
        Props
      </Text>
      {error ? (
        <Text c="ember" fz="sm" role="alert">
          Could not load the props: {error}
        </Text>
      ) : entries === undefined ? (
        <Text c="moon.5" fz="sm">
          Loading the props…
        </Text>
      ) : entries.length === 0 ? (
        <Text c="moon.5" fz="sm">
          No props are accepted in this world yet.
        </Text>
      ) : (
        <Stack gap={4}>
          <TextInput
            size="xs"
            placeholder="Filter by name or tag"
            aria-label="Filter the props"
            value={text}
            onChange={(e) => setText(e.currentTarget.value)}
          />
          <Box mah={260} style={{ overflowY: "auto" }} aria-label="Prop catalogue">
            <Stack gap={4}>
              {shown.map((p) => (
                <Button
                  key={p.id}
                  size="compact-sm"
                  variant={p.hash === chosen ? "filled" : "default"}
                  aria-pressed={p.hash === chosen}
                  justify="flex-start"
                  leftSection={
                    <img
                      src={`/data/props/${p.hash.toLowerCase()}.png`}
                      alt=""
                      width={24}
                      height={24}
                      style={{ objectFit: "contain" }}
                      onError={(e) => {
                        e.currentTarget.style.display = "none";
                      }}
                    />
                  }
                  onClick={() => onChoose(p.hash)}
                >
                  <Box style={{ textAlign: "left", minWidth: 0 }}>
                    <Text fz="sm" truncate="end">
                      {p.name}
                    </Text>
                    {p.tags.length > 0 && (
                      <Text c="moon.5" fz="xs" truncate="end">
                        {p.tags.join(", ")}
                      </Text>
                    )}
                  </Box>
                </Button>
              ))}
              {shown.length === 0 && (
                <Text c="moon.5" fz="sm">
                  No prop matches "{text.trim()}".
                </Text>
              )}
            </Stack>
          </Box>
        </Stack>
      )}
    </Box>
  );
}
