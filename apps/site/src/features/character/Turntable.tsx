// The 3D preview in React: starts the turntable once, shows each new pick, and falls back to
// the portrait when WebGL isn't there.

import { Box, Center, Image, Loader, Stack, Text } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import type { CastMember } from "../../api/types";
import type { HeadFit, Preview, Worn } from "./preview";

export function Turntable({ who, worn }: { who: CastMember | undefined; worn: Worn[] }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [view, setView] = useState<Preview>();
  const [state, setState] = useState<"loading" | "ready" | "flat">("loading");

  useEffect(() => {
    let alive = true;
    let made: Preview | undefined;
    (async () => {
      try {
        const [{ Preview }, fits, bare] = await Promise.all([
          import("./preview"),
          api<Record<string, HeadFit>>("/data/head_fit.json").catch(() => ({})),
          api<Record<string, number>>("/data/bare_heads.json").catch(() => ({})),
        ]);
        if (!alive || !canvas.current) return;
        made = new Preview(canvas.current, fits, bare);
        setView(made);
        setState("ready");
      } catch (err) {
        console.warn("no 3D preview:", err);
        if (alive) setState("flat");
      }
    })();
    return () => {
      alive = false;
      made?.dispose();
    };
  }, []);

  useEffect(() => {
    void view?.show(who?.model ?? null, worn);
  }, [view, who?.model, worn]);

  return (
    <Stack gap="xs">
      <Box pos="relative" h={{ base: 380, sm: 520 }} style={{ borderRadius: 18, overflow: "hidden" }} aria-label="Your character, turning">
        <canvas ref={canvas} style={{ width: "100%", height: "100%", display: state === "flat" ? "none" : "block" }} />
        {state === "loading" && (
          <Center pos="absolute" inset={0}>
            <Loader />
          </Center>
        )}
        {state === "flat" && who && (
          <Center h="100%">
            <Image src={`/assets/${who.portrait}`} alt="" maw={260} radius="lg" />
          </Center>
        )}
      </Box>
      {who && (
        <Text ta="center" fw={700}>
          {who.name}
        </Text>
      )}
    </Stack>
  );
}
