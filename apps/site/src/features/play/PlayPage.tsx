// Playing in the browser: get a sign-in token, then start the WebAssembly client (game/wasm.ts)
// on the canvas. It reports how it's getting on through window.vox_status, vox_ready and
// vox_error.

import { Anchor, Box, Center, Progress, Stack, Text, Transition } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import { api, messageOf, post, worldUrl } from "../../api/client";
import type { Me, Token } from "../../api/types";
import { Problem } from "../../components/Problem";
import { Wordmark } from "../../components/Wordmark";
import { loadClient } from "../../game/wasm";

const STAGES = 5;

export function PlayPage() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [stage, setStage] = useState(0);
  const [status, setStatus] = useState("Opening the Void…");
  const [error, setError] = useState<string>();
  const [ready, setReady] = useState(false);
  const [keys, setKeys] = useState(false);

  useEffect(() => {
    const step = (text: string) => {
      setStage((s) => Math.min(s + 1, STAGES));
      setStatus(text);
    };
    window.vox_status = step;
    window.vox_ready = () => {
      setStage(STAGES);
      setReady(true);
      setKeys(true);
      canvas.current?.focus();
      setTimeout(() => setKeys(false), 12000);
    };
    window.vox_error = (text) => setError(text);

    (async () => {
      const me = await api<Me>("/api/me");
      if (!me.name) return void (location.href = "/");
      if (!me.character) return void (location.href = "/create");
      step("Signing you in…");
      const t = await post<Token>("/api/token");
      const want = new URLSearchParams(location.search).get("world");
      const world = t.worlds.find((w) => w.name === want) ?? t.worlds[0];
      if (!world) throw new Error("there are no worlds to go to yet");
      document.title = `${world.name} · Vox`;
      step("Loading the client…");
      const start = await loadClient();
      step(`Going to ${world.name}…`);
      start({
        canvas: "#game",
        url: worldUrl(world.url),
        token: t.token,
        name: t.name,
        model: t.character.model,
        wear: t.character.wear,
      });
    })().catch((err) => setError(messageOf(err)));

    return () => {
      delete window.vox_status;
      delete window.vox_ready;
      delete window.vox_error;
    };
  }, []);

  return (
    <>
      <canvas id="game" ref={canvas} className="vox-game" tabIndex={0} aria-label="The world" />
      <Transition mounted={!ready || !!error} transition="fade" duration={400}>
        {(style) => (
          <Center pos="fixed" inset={0} bg="#f3f5f9" style={style} aria-live="polite">
            <Stack align="center" gap="md" w="min(360px, 80vw)">
              <Wordmark />
              <Progress value={(stage / STAGES) * 100} w="100%" size="sm" transitionDuration={300} />
              <Text c="moon.5">{status}</Text>
              {error && (
                <>
                  <Problem error={error} />
                  <Anchor href="/">Back to the start</Anchor>
                </>
              )}
            </Stack>
          </Center>
        )}
      </Transition>
      <Box pos="fixed" top={14} left={16}>
        <Wordmark small />
      </Box>
      <Transition mounted={keys} transition="slide-up">
        {(style) => (
          <Text
            pos="fixed"
            bottom={12}
            left="50%"
            fz="sm"
            c="moon.5"
            px="md"
            py={6}
            bg="rgba(251,252,254,0.85)"
            style={{ ...style, transform: "translateX(-50%)", borderRadius: 999, whiteSpace: "nowrap" }}
          >
            Click to look · WASD to walk · left click breaks, right click places · E for blocks · hold Tab to quick-select · Esc for the menu · a controller works too
          </Text>
        )}
      </Transition>
    </>
  );
}
