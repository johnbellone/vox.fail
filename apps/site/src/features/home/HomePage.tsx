// The start: what Vox is, and the door: sign in, or go in.

import { Container, Loader, Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import { useEffect } from "react";
import { useApi } from "../../api/hooks";
import type { Me } from "../../api/types";
import { LiveWorld } from "../../components/LiveWorld";
import { Problem } from "../../components/Problem";
import { Wordmark } from "../../components/Wordmark";
import { SignIn } from "./SignIn";
import { SignedIn } from "./SignedIn";

export function HomePage() {
  const me = useApi<Me>("/api/me");
  // signed in but no character yet: make one first
  useEffect(() => {
    if (me.data?.name && !me.data.character) location.href = "/create";
  }, [me.data]);

  return (
    <>
    <LiveWorld />
    <Container size="lg" component="main" py={{ base: "xl", sm: 80 }}>
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing={{ base: "xl", md: 64 }} style={{ alignItems: "center" }}>
        <Stack gap="md">
          <Wordmark />
          <Text fz="xl" fw={700} c="moon.9">
            A world built from blocks, shared with everyone in it. What you build stays.
          </Text>
          <Text c="moon.5">
            Everyone arrives in the Void: a bright floor that goes on for ever, a circle of pale pillars, and doorways
            that open onto other places as you walk up to them.
          </Text>
          <Text c="moon.5">
            Time spent building earns authority, the standing every world recognises. Spend it on feats: claim a plot
            that only you and your group can build in, open a portal to another world, give a world you own its own coin,
            or lead your friends on a raid.
          </Text>
        </Stack>
        <Paper shadow="sm" aria-live="polite">
          {me.loading && !me.data ? (
            <Loader mx="auto" display="block" />
          ) : me.error ? (
            <Problem error={me.error} />
          ) : me.data?.name ? (
            <SignedIn me={me.data} onSignOut={() => void me.reload()} />
          ) : (
            <SignIn offered={me.data?.sign_in_with ?? []} onDone={() => void me.reload()} />
          )}
        </Paper>
      </SimpleGrid>
    </Container>
    </>
  );
}
